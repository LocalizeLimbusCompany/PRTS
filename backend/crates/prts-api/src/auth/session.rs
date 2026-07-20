//! DB-authoritative 会话令牌与 durable Redis cache outbox。

use prts_auth::{jwt, token};
use prts_common::Error;
use prts_db::audit::{
    AuditActor, AuditActorKind, AuditEvent, AuthFailureMethod, AuthFailureReason,
};
use prts_db::auth_sessions::{
    self, AuthIntentPayload, AuthSessionState, NewAuthSession, RefreshTokenHash,
};
use sqlx::PgConnection;

use crate::dto::UserDto;
use crate::state::AppState;

const AUTH_INTENT_LEASE_DURATION: std::time::Duration = std::time::Duration::from_secs(30);
const REDIS_OPERATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const AUTH_INTENT_MAX_ATTEMPTS: i32 = i32::MAX;

/// 一次签发的令牌对。
pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// access token 剩余有效秒数。
    pub access_expires_in: i64,
}

/// Refresh rotation 的完整提交结果；用户字段已转换为无密对外 DTO。
pub struct RefreshedSession {
    pub tokens: IssuedTokens,
    pub user: UserDto,
}

/// 令牌签发来源，用于 action-specific 审计。
#[derive(Debug, Clone)]
pub enum IssueKind {
    Register,
    Login,
    #[cfg(feature = "zoot-oauth")]
    OAuth {
        provider: String,
        new_user: bool,
    },
}

fn session_cache_key(session_handle: &str) -> String {
    format!("auth_session:{session_handle}")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn database_error(error: sqlx::Error) -> Error {
    Error::internal(format!("db error: {error}"))
}

#[cfg(feature = "zoot-oauth")]
fn redis_err(error: redis::RedisError) -> Error {
    Error::internal(format!("redis error: {error}"))
}

fn auth_outbox_db_error_category(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::Tls(_) => "tls",
        sqlx::Error::Protocol(_) => "protocol",
        sqlx::Error::PoolTimedOut => "pool_timeout",
        sqlx::Error::PoolClosed => "pool_closed",
        _ => "other",
    }
}

fn refresh_hash(raw_refresh_token: &str) -> RefreshTokenHash {
    RefreshTokenHash::parse(token::sha256_hex(raw_refresh_token))
        .expect("sha256_hex always returns validated lowercase SHA-256")
}

fn build_tokens(
    state: &AppState,
    user_id: i64,
    session_handle: String,
    refresh_token: String,
) -> IssuedTokens {
    let auth = &state.settings.auth;
    let iat = now();
    let claims = jwt::Claims {
        sub: user_id,
        iat,
        exp: iat + auth.access_ttl_secs,
        typ: "access".to_string(),
        sid: Some(session_handle.clone()),
    };
    let access_token = jwt::encode(&claims, state.jwt_secret());
    IssuedTokens {
        access_token,
        refresh_token,
        access_expires_in: auth.access_ttl_secs,
    }
}

/// 在独立事务中签发令牌；DB commit 后才把 raw token 返回调用方。
///
/// 当前注册、登录和 OAuth route 为了把用户 mutation 纳入同一事务而调用
/// [`issue_tx`]；保留本入口供无需组合其它 mutation 的签发调用方复用。
#[allow(dead_code)]
pub async fn issue(state: &AppState, user_id: i64, kind: IssueKind) -> Result<IssuedTokens, Error> {
    let mut tx = state.db.begin().await.map_err(database_error)?;
    let tokens = issue_tx(&mut tx, state, user_id, kind).await?;
    tx.commit().await.map_err(database_error)?;
    Ok(tokens)
}

/// 把 pending→active session、认证结果、token-issued audit 与 Redis outbox 写入调用方事务。
pub async fn issue_tx(
    conn: &mut PgConnection,
    state: &AppState,
    user_id: i64,
    kind: IssueKind,
) -> Result<IssuedTokens, Error> {
    let session_handle = token::random_token(24);
    let family_handle = token::random_token(24);
    let raw_refresh_token = format!("{session_handle}.{}", token::random_token(48));
    let expires_at =
        chrono::Utc::now() + chrono::Duration::seconds(state.settings.auth.refresh_ttl_secs.max(1));
    let pending = auth_sessions::create_pending_tx(
        &mut *conn,
        NewAuthSession {
            session_handle: session_handle.clone(),
            family_handle,
            user_id,
            refresh_token_hash: refresh_hash(&raw_refresh_token),
            expires_at,
            predecessor_id: None,
        },
    )
    .await
    .map_err(database_error)?;
    let active = auth_sessions::activate_pending_tx(&mut *conn, pending.id)
        .await
        .map_err(database_error)?
        .ok_or_else(|| Error::internal("failed to activate pending auth session"))?;

    let actor = AuditActor {
        id: Some(user_id),
        kind: AuditActorKind::User,
        ip: None,
    };
    match &kind {
        IssueKind::Register => {
            prts_db::audit::append_event_tx(
                &mut *conn,
                actor,
                AuditEvent::AuthRegistered {
                    user_id,
                    method: "password",
                    status: "active",
                },
            )
            .await
            .map_err(|_| Error::AuditUnavailable)?;
        }
        IssueKind::Login => {
            prts_db::audit::append_event_tx(
                &mut *conn,
                actor,
                AuditEvent::AuthLoginSucceeded {
                    user_id,
                    method: "password",
                },
            )
            .await
            .map_err(|_| Error::AuditUnavailable)?;
        }
        #[cfg(feature = "zoot-oauth")]
        IssueKind::OAuth { provider, new_user } => {
            prts_db::audit::append_event_tx(
                &mut *conn,
                actor,
                AuditEvent::AuthOAuthSucceeded {
                    user_id,
                    provider,
                    new_user: *new_user,
                },
            )
            .await
            .map_err(|_| Error::AuditUnavailable)?;
        }
    }
    let method = match &kind {
        IssueKind::Register | IssueKind::Login => "password",
        #[cfg(feature = "zoot-oauth")]
        IssueKind::OAuth { provider, .. } => provider.as_str(),
    };
    prts_db::audit::append_event_tx(
        &mut *conn,
        actor,
        AuditEvent::AuthTokenIssued {
            session_id: active.id,
            session_handle: &active.session_handle,
            method,
            expires_at: active.expires_at,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    auth_sessions::enqueue_intent_tx(
        conn,
        active.id,
        AuthIntentPayload::RedisPopulate {
            session_handle: active.session_handle.clone(),
            expires_at: active.expires_at,
        },
        AUTH_INTENT_MAX_ATTEMPTS,
    )
    .await
    .map_err(database_error)?;

    Ok(build_tokens(
        state,
        user_id,
        active.session_handle,
        raw_refresh_token,
    ))
}

/// 用 refresh token 轮换出新令牌对；Redis 只作 cache hint，权威校验始终锁 DB hash。
pub async fn refresh(state: &AppState, refresh_token: &str) -> Result<RefreshedSession, Error> {
    let predecessor_hash = refresh_hash(refresh_token);
    let known_user_id = auth_sessions::find_user_id_by_refresh_hash(&state.db, &predecessor_hash)
        .await
        .map_err(database_error)?;
    let mut tx = state.db.begin().await.map_err(database_error)?;
    let Some(predecessor) =
        auth_sessions::lock_active_unexpired_by_refresh_hash_tx(&mut tx, &predecessor_hash)
            .await
            .map_err(database_error)?
    else {
        tx.rollback().await.map_err(database_error)?;
        record_auth_failure(
            state,
            known_user_id.unwrap_or(0),
            AuthFailureMethod::Refresh,
            AuthFailureReason::InvalidRefresh,
        )
        .await?;
        return Err(Error::Unauthorized);
    };
    let rotating = auth_sessions::begin_rotation_tx(&mut tx, predecessor.id)
        .await
        .map_err(database_error)?
        .ok_or_else(|| Error::internal("locked auth session could not enter rotation"))?;
    let successor_handle = token::random_token(24);
    let raw_successor = format!("{successor_handle}.{}", token::random_token(48));
    let expires_at =
        chrono::Utc::now() + chrono::Duration::seconds(state.settings.auth.refresh_ttl_secs.max(1));
    let successor = auth_sessions::create_pending_tx(
        &mut tx,
        NewAuthSession {
            session_handle: successor_handle,
            family_handle: rotating.family_handle.clone(),
            user_id: rotating.user_id,
            refresh_token_hash: refresh_hash(&raw_successor),
            expires_at,
            predecessor_id: Some(rotating.id),
        },
    )
    .await
    .map_err(database_error)?;
    let (revoked, active) = auth_sessions::complete_rotation_tx(&mut tx, rotating.id, successor.id)
        .await
        .map_err(database_error)?
        .ok_or_else(|| Error::internal("auth session rotation invariant failed"))?;
    let user = prts_db::users::find_by_id_for_update_tx(&mut tx, active.user_id)
        .await
        .map_err(database_error)?
        .ok_or_else(|| Error::internal("issued auth session user no longer exists"))?;
    let user = UserDto::from(&user);
    let actor = AuditActor {
        id: Some(active.user_id),
        kind: AuditActorKind::User,
        ip: None,
    };
    prts_db::audit::append_event_tx(
        &mut tx,
        actor,
        AuditEvent::AuthRefreshRotated {
            session_id: active.id,
            session_handle: &active.session_handle,
            predecessor_handle: &revoked.session_handle,
            expires_at: active.expires_at,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        actor,
        AuditEvent::AuthTokenIssued {
            session_id: active.id,
            session_handle: &active.session_handle,
            method: "refresh",
            expires_at: active.expires_at,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    auth_sessions::enqueue_intent_tx(
        &mut tx,
        revoked.id,
        AuthIntentPayload::RedisInvalidate {
            session_handle: revoked.session_handle.clone(),
        },
        AUTH_INTENT_MAX_ATTEMPTS,
    )
    .await
    .map_err(database_error)?;
    auth_sessions::enqueue_intent_tx(
        &mut tx,
        active.id,
        AuthIntentPayload::RedisPopulate {
            session_handle: active.session_handle.clone(),
            expires_at: active.expires_at,
        },
        AUTH_INTENT_MAX_ATTEMPTS,
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(RefreshedSession {
        tokens: build_tokens(state, active.user_id, active.session_handle, raw_successor),
        user,
    })
}

/// 吊销 refresh token。DB commit 即失效；Redis invalidate 由 durable outbox 重放。
pub async fn revoke(state: &AppState, refresh_token: &str) -> Result<(), Error> {
    let hash = refresh_hash(refresh_token);
    let known_user_id = auth_sessions::find_user_id_by_refresh_hash(&state.db, &hash)
        .await
        .map_err(database_error)?;
    let mut tx = state.db.begin().await.map_err(database_error)?;
    let Some(active) = auth_sessions::lock_active_unexpired_by_refresh_hash_tx(&mut tx, &hash)
        .await
        .map_err(database_error)?
    else {
        tx.rollback().await.map_err(database_error)?;
        record_auth_failure(
            state,
            known_user_id.unwrap_or(0),
            AuthFailureMethod::Refresh,
            AuthFailureReason::InvalidRefresh,
        )
        .await?;
        return Ok(());
    };
    let revoked = auth_sessions::revoke_unexpired_tx(&mut tx, active.id)
        .await
        .map_err(database_error)?
        .ok_or_else(|| Error::internal("failed to revoke active auth session"))?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(revoked.user_id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::AuthLoggedOut {
            session_id: revoked.id,
            session_handle: &revoked.session_handle,
            revoked_sessions: 1,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    auth_sessions::enqueue_intent_tx(
        &mut tx,
        revoked.id,
        AuthIntentPayload::RedisInvalidate {
            session_handle: revoked.session_handle,
        },
        AUTH_INTENT_MAX_ATTEMPTS,
    )
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(())
}

/// 同步记录失败认证；审计本身失败时返回 `AuditUnavailable`，不得暴露原认证结论。
pub async fn record_failed_authentication(
    state: &AppState,
    user_id: i64,
    method: &str,
    reason_code: &str,
) -> Result<(), Error> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| Error::AuditUnavailable)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: None,
            kind: AuditActorKind::Anonymous,
            ip: None,
        },
        AuditEvent::AuthLoginFailed {
            user_id,
            method,
            reason_code,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(|_| Error::AuditUnavailable)
}

/// 同步记录已提供凭证的通用认证失败；审计链路任一步失败都优先返回 503。
pub async fn record_auth_failure(
    state: &AppState,
    user_id: i64,
    method: AuthFailureMethod,
    reason: AuthFailureReason,
) -> Result<(), Error> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|_| Error::AuditUnavailable)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: None,
            kind: AuditActorKind::Anonymous,
            ip: None,
        },
        AuditEvent::AuthFailed {
            user_id,
            method,
            reason,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(|_| Error::AuditUnavailable)
}

/// 启动 durable Redis cache outbox worker；租约过期的 intent 会由其它实例接管。
pub fn spawn_outbox_worker(db: prts_db::Db, cache: prts_db::Cache) -> tokio::task::JoinHandle<()> {
    let worker_id = format!("auth-outbox-{}", token::random_token(12).to_lowercase());
    tokio::spawn(async move {
        loop {
            match process_one_outbox_intent(&db, &cache, &worker_id, None).await {
                Ok(OutboxProcessOutcome::Idle) => {}
                Ok(OutboxProcessOutcome::LostLease) => {
                    tracing::warn!(worker = %worker_id, "auth outbox worker lost intent lease");
                    continue;
                }
                Ok(
                    OutboxProcessOutcome::Completed
                    | OutboxProcessOutcome::Rescheduled
                    | OutboxProcessOutcome::PermanentlyFailed,
                ) => continue,
                Err(error) => tracing::error!(
                    worker = %worker_id,
                    category = auth_outbox_db_error_category(&error),
                    "auth outbox iteration failed"
                ),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    })
}

/// 单次 outbox 处理结果；任何 fencing 更新失败都显式报告 LostLease。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutboxProcessOutcome {
    Idle,
    Completed,
    Rescheduled,
    PermanentlyFailed,
    LostLease,
}

/// 处理一个 durable cache intent；可选 session 限定供分片 worker 与确定性恢复验证复用。
pub(crate) async fn process_one_outbox_intent(
    db: &prts_db::Db,
    cache: &prts_db::Cache,
    worker_id: &str,
    session_id: Option<i64>,
) -> Result<OutboxProcessOutcome, sqlx::Error> {
    let claimed = match session_id {
        Some(session_id) => {
            auth_sessions::claim_intent_for_session(
                db,
                worker_id,
                AUTH_INTENT_LEASE_DURATION.as_secs() as i64,
                session_id,
            )
            .await?
        }
        None => {
            auth_sessions::claim_intent(db, worker_id, AUTH_INTENT_LEASE_DURATION.as_secs() as i64)
                .await?
        }
    };
    let Some(intent) = claimed else {
        return Ok(OutboxProcessOutcome::Idle);
    };
    let Some(session_handle) = intent
        .payload
        .get("session_handle")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
    else {
        let updated = auth_sessions::fail_intent_permanently(
            db,
            intent.id,
            worker_id,
            "invalid_auth_intent",
            "auth intent payload is invalid",
        )
        .await?;
        return Ok(if updated.is_some() {
            OutboxProcessOutcome::PermanentlyFailed
        } else {
            OutboxProcessOutcome::LostLease
        });
    };
    if !matches!(intent.kind.as_str(), "redis_populate" | "redis_invalidate") {
        let updated = auth_sessions::fail_intent_permanently(
            db,
            intent.id,
            worker_id,
            "invalid_auth_intent",
            "auth intent kind is invalid",
        )
        .await?;
        return Ok(if updated.is_some() {
            OutboxProcessOutcome::PermanentlyFailed
        } else {
            OutboxProcessOutcome::LostLease
        });
    }

    recover_incomplete_session(db, intent.session_id).await?;
    let session = auth_sessions::find_by_id(db, intent.session_id).await?;
    let redis_operation = async {
        match intent.kind.as_str() {
            "redis_populate" => {
                if let Some(session) = session.filter(|session| {
                    session.state == AuthSessionState::Active
                        && session.expires_at > chrono::Utc::now()
                }) {
                    let ttl = (session.expires_at - chrono::Utc::now())
                        .num_seconds()
                        .max(1);
                    let mut connection = cache.clone();
                    redis::cmd("SET")
                        .arg(session_cache_key(&session_handle))
                        .arg(session.user_id)
                        .arg("EX")
                        .arg(ttl)
                        .query_async::<()>(&mut connection)
                        .await
                } else {
                    let mut connection = cache.clone();
                    redis::cmd("DEL")
                        .arg(session_cache_key(&session_handle))
                        .query_async::<()>(&mut connection)
                        .await
                }
            }
            "redis_invalidate" => {
                let mut connection = cache.clone();
                redis::cmd("DEL")
                    .arg(session_cache_key(&session_handle))
                    .query_async::<()>(&mut connection)
                    .await
            }
            _ => unreachable!("intent kind validated before Redis operation"),
        }
    };
    let redis_result = tokio::time::timeout(REDIS_OPERATION_TIMEOUT, redis_operation).await;
    match redis_result {
        Ok(Ok(())) => {
            let updated = auth_sessions::complete_intent(db, intent.id, worker_id).await?;
            Ok(if updated.is_some() {
                OutboxProcessOutcome::Completed
            } else {
                OutboxProcessOutcome::LostLease
            })
        }
        Ok(Err(_)) | Err(_) => {
            if intent.attempts >= intent.max_attempts {
                let updated = auth_sessions::fail_intent_permanently(
                    db,
                    intent.id,
                    worker_id,
                    "auth_intent_attempts_exhausted",
                    "auth intent exhausted all attempts",
                )
                .await?;
                Ok(if updated.is_some() {
                    OutboxProcessOutcome::PermanentlyFailed
                } else {
                    OutboxProcessOutcome::LostLease
                })
            } else {
                let updated = auth_sessions::reschedule_intent(
                    db,
                    intent.id,
                    worker_id,
                    "redis_unavailable",
                    "redis cache operation failed or timed out",
                    prts_core::jobs::retry_backoff_seconds(intent.attempts),
                )
                .await?;
                Ok(if updated.is_some() {
                    OutboxProcessOutcome::Rescheduled
                } else {
                    OutboxProcessOutcome::LostLease
                })
            }
        }
    }
}

/// crash 后若发现持久化 pending/rotating，会话先 fail closed 到终态再处理 cache。
async fn recover_incomplete_session(db: &prts_db::Db, session_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    let Some(session) = auth_sessions::find_by_id_for_update_tx(&mut tx, session_id).await? else {
        tx.rollback().await?;
        return Ok(());
    };
    if !matches!(
        session.state,
        AuthSessionState::Pending | AuthSessionState::Rotating
    ) {
        tx.rollback().await?;
        return Ok(());
    }
    let previous_state = session.state.as_str();
    let recovered = if session.expires_at <= chrono::Utc::now() {
        auth_sessions::expire_due_tx(&mut tx, session.id).await?
    } else {
        auth_sessions::revoke_unexpired_tx(&mut tx, session.id).await?
    };
    if let Some(recovered) = recovered {
        prts_db::audit::append_auth_session_state_tx(
            &mut tx,
            AuditActor {
                id: None,
                kind: AuditActorKind::System,
                ip: None,
            },
            recovered.id,
            &recovered.session_handle,
            previous_state,
            recovered.state.as_str(),
        )
        .await?;
    }
    tx.commit().await
}

/// 暂存 OAuth 流程的 `state → code_verifier`（带 TTL，默认 10 分钟）。
#[cfg(feature = "zoot-oauth")]
pub async fn store_oauth_state(
    state: &AppState,
    oauth_state: &str,
    code_verifier: &str,
) -> Result<(), Error> {
    let mut conn = state.cache.clone();
    let _: () = redis::cmd("SET")
        .arg(format!("oauth_state:{oauth_state}"))
        .arg(code_verifier)
        .arg("EX")
        .arg(600)
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;
    Ok(())
}

/// 取回并删除 OAuth 流程暂存的 `code_verifier`（一次性）。
#[cfg(feature = "zoot-oauth")]
pub async fn take_oauth_state(
    state: &AppState,
    oauth_state: &str,
) -> Result<Option<String>, Error> {
    let key = format!("oauth_state:{oauth_state}");
    let mut conn = state.cache.clone();
    let verifier: Option<String> = redis::cmd("GETDEL")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .map_err(redis_err)?;
    Ok(verifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_worker_exposes_its_join_handle_for_supervision() {
        let _spawn: fn(prts_db::Db, prts_db::Cache) -> tokio::task::JoinHandle<()> =
            spawn_outbox_worker;
    }

    #[test]
    fn redis_timeout_is_strictly_shorter_than_the_intent_lease() {
        assert!(REDIS_OPERATION_TIMEOUT < AUTH_INTENT_LEASE_DURATION);
    }

    #[test]
    fn outbox_database_errors_are_classified_without_source_text() {
        let secret = "Authorization: Bearer RAW_SECRET";
        let error = sqlx::Error::Protocol(secret.to_string());
        let category = auth_outbox_db_error_category(&error);
        assert_eq!(category, "protocol");
        assert!(!category.contains(secret));
    }

    #[test]
    fn outbox_worker_log_does_not_format_raw_database_errors() {
        let source = include_str!("session.rs");
        let worker = source
            .split_once("pub fn spawn_outbox_worker")
            .expect("worker exists")
            .1
            .split_once("pub(crate) enum OutboxProcessOutcome")
            .expect("worker block ends")
            .0;
        assert!(!worker.contains("%error"));
    }
}
