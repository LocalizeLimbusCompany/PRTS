//! PostgreSQL 权威认证会话与 durable cache intent/outbox 仓储。
//!
//! 公开接口只接收强类型 refresh hash，并且永不返回持久化 hash。权威读取、锁定、激活
//! 与 rotation SQL 自身都要求 `expires_at > now()`；Redis 只能缓存这里确认过的 active 行。

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgConnection, PgPool};

/// 已验证的 SHA-256 小写十六进制 refresh token hash。
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshTokenHash(String);

impl std::fmt::Debug for RefreshTokenHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RefreshTokenHash([REDACTED])")
    }
}

/// refresh hash 格式错误；raw/base64 token 不得进入 repository。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRefreshTokenHash;

impl std::fmt::Display for InvalidRefreshTokenHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("refresh token hash must be 64 lowercase hexadecimal characters")
    }
}

impl std::error::Error for InvalidRefreshTokenHash {}

impl RefreshTokenHash {
    /// 验证并包装 hash。构造失败时调用方必须拒绝，而不能回退存 raw token。
    pub fn parse(value: String) -> Result<Self, InvalidRefreshTokenHash> {
        if value.len() == 64
            && value
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            Ok(Self(value))
        } else {
            Err(InvalidRefreshTokenHash)
        }
    }

    /// 参数化 SQL 绑定使用的已验证文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 权威认证会话状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSessionState {
    Pending,
    Active,
    Rotating,
    Revoked,
    Expired,
}

impl AuthSessionState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "rotating" => Some(Self::Rotating),
            "revoked" => Some(Self::Revoked),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }

    /// 稳定数据库状态字符串，供审计与 worker 编排使用。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Rotating => "rotating",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        }
    }
}

/// 对上层公开的会话快照。持久化 refresh hash 刻意不在该类型中。
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub id: i64,
    pub session_handle: String,
    pub family_handle: String,
    pub user_id: i64,
    pub state: AuthSessionState,
    pub expires_at: DateTime<Utc>,
    pub predecessor_id: Option<i64>,
    pub successor_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 仅仓储内部可见的完整持久化行。
#[derive(Debug, FromRow)]
struct AuthSessionRow {
    id: i64,
    session_handle: String,
    family_handle: String,
    user_id: i64,
    refresh_token_hash: String,
    state: String,
    expires_at: DateTime<Utc>,
    predecessor_id: Option<i64>,
    successor_id: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<AuthSessionRow> for AuthSession {
    type Error = sqlx::Error;

    fn try_from(row: AuthSessionRow) -> Result<Self, Self::Error> {
        let state = AuthSessionState::parse(&row.state)
            .ok_or_else(|| sqlx::Error::Protocol("unknown auth session state".to_string()))?;
        // 明确消费但不复制 hash，防止未来自动转换意外将其暴露。
        drop(row.refresh_token_hash);
        Ok(Self {
            id: row.id,
            session_handle: row.session_handle,
            family_handle: row.family_handle,
            user_id: row.user_id,
            state,
            expires_at: row.expires_at,
            predecessor_id: row.predecessor_id,
            successor_id: row.successor_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

fn into_session(row: Option<AuthSessionRow>) -> Result<Option<AuthSession>, sqlx::Error> {
    row.map(AuthSession::try_from).transpose()
}

/// 按 id 读取任意状态的会话，供 durable outbox worker 判定恢复动作。
pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<AuthSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionRow>("SELECT * FROM auth_sessions WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    into_session(row)
}

/// 在调用方事务内锁定任意状态的会话。
pub async fn find_by_id_for_update_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<AuthSession>, sqlx::Error> {
    let row =
        sqlx::query_as::<_, AuthSessionRow>("SELECT * FROM auth_sessions WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(conn)
            .await?;
    into_session(row)
}

/// 新建 pending 会话所需的非秘密字段。
#[derive(Debug, Clone)]
pub struct NewAuthSession {
    pub session_handle: String,
    pub family_handle: String,
    pub user_id: i64,
    pub refresh_token_hash: RefreshTokenHash,
    pub expires_at: DateTime<Utc>,
    pub predecessor_id: Option<i64>,
}

/// 创建不可认证的 pending 会话；调用方提交 active 状态后才可返回 raw token。
pub async fn create_pending_tx(
    conn: &mut PgConnection,
    input: NewAuthSession,
) -> Result<AuthSession, sqlx::Error> {
    let row: AuthSessionRow = sqlx::query_as(
        "INSERT INTO auth_sessions (
             session_handle, family_handle, user_id, refresh_token_hash,
             state, expires_at, predecessor_id
         ) VALUES ($1, $2, $3, $4, 'pending', $5, $6)
         RETURNING *",
    )
    .bind(input.session_handle)
    .bind(input.family_handle)
    .bind(input.user_id)
    .bind(input.refresh_token_hash.as_str())
    .bind(input.expires_at)
    .bind(input.predecessor_id)
    .fetch_one(conn)
    .await?;
    row.try_into()
}

/// 按 refresh hash 锁定 active 且未过期的权威会话。
pub async fn lock_active_unexpired_by_refresh_hash_tx(
    conn: &mut PgConnection,
    refresh_token_hash: &RefreshTokenHash,
) -> Result<Option<AuthSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionRow>(
        "SELECT * FROM auth_sessions
         WHERE refresh_token_hash = $1 AND state = 'active' AND expires_at > now()
         FOR UPDATE",
    )
    .bind(refresh_token_hash.as_str())
    .fetch_optional(conn)
    .await?;
    into_session(row)
}

/// 仅返回 refresh hash 对应的用户 id，供失败认证写脱敏 target；不暴露持久化 hash。
pub async fn find_user_id_by_refresh_hash(
    pool: &PgPool,
    refresh_token_hash: &RefreshTokenHash,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT user_id FROM auth_sessions WHERE refresh_token_hash = $1")
        .bind(refresh_token_hash.as_str())
        .fetch_optional(pool)
        .await
}

/// Redis cache miss 只可回源 active 且未过期的 opaque handle。
pub async fn find_active_unexpired_by_handle(
    pool: &PgPool,
    session_handle: &str,
) -> Result<Option<AuthSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionRow>(
        "SELECT * FROM auth_sessions
         WHERE session_handle = $1 AND state = 'active' AND expires_at > now()",
    )
    .bind(session_handle)
    .fetch_optional(pool)
    .await?;
    into_session(row)
}

/// 激活没有 predecessor 的普通 pending 会话；rotation successor 必须走 complete_rotation。
pub async fn activate_pending_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<AuthSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionRow>(
        "UPDATE auth_sessions
         SET state = 'active', updated_at = now()
         WHERE id = $1 AND state = 'pending' AND predecessor_id IS NULL
           AND expires_at > now()
         RETURNING *",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    into_session(row)
}

/// 将 active 且未过期的会话置为 rotating。
pub async fn begin_rotation_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<AuthSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionRow>(
        "UPDATE auth_sessions
         SET state = 'rotating', updated_at = now()
         WHERE id = $1 AND state = 'active' AND expires_at > now()
         RETURNING *",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    into_session(row)
}

/// 吊销尚未过期的 pending/active/rotating 会话。
pub async fn revoke_unexpired_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<AuthSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionRow>(
        "UPDATE auth_sessions
         SET state = 'revoked', updated_at = now()
         WHERE id = $1 AND state IN ('pending', 'active', 'rotating')
           AND expires_at > now()
         RETURNING *",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    into_session(row)
}

/// 在改密/封禁等安全操作中批量吊销用户全部未过期会话。
pub async fn revoke_all_unexpired_for_user_tx(
    conn: &mut PgConnection,
    user_id: i64,
) -> Result<Vec<AuthSession>, sqlx::Error> {
    let rows = sqlx::query_as::<_, AuthSessionRow>(
        "UPDATE auth_sessions
         SET state = 'revoked', updated_at = now()
         WHERE user_id = $1 AND state IN ('pending', 'active', 'rotating')
           AND expires_at > now()
         RETURNING *",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await?;
    rows.into_iter().map(AuthSession::try_from).collect()
}

/// 把已经到期的非终态会话物化为 expired；未到期行不会被误终结。
pub async fn expire_due_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<AuthSession>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionRow>(
        "UPDATE auth_sessions
         SET state = 'expired', updated_at = now()
         WHERE id = $1 AND state IN ('pending', 'active', 'rotating')
           AND expires_at <= now()
         RETURNING *",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    into_session(row)
}

/// 原子完成 rotation：锁定并验证两行，先撤销 predecessor，再激活精确 successor。
pub async fn complete_rotation_tx(
    conn: &mut PgConnection,
    predecessor_id: i64,
    successor_id: i64,
) -> Result<Option<(AuthSession, AuthSession)>, sqlx::Error> {
    if predecessor_id == successor_id {
        return Ok(None);
    }
    let ids = [predecessor_id, successor_id];
    let rows: Vec<AuthSessionRow> = sqlx::query_as(
        "SELECT * FROM auth_sessions
         WHERE id = ANY($1::BIGINT[]) AND expires_at > now()
         ORDER BY id
         FOR UPDATE",
    )
    .bind(&ids[..])
    .fetch_all(&mut *conn)
    .await?;
    if rows.len() != 2 {
        return Ok(None);
    }
    let mut predecessor = None;
    let mut successor = None;
    for row in rows {
        if row.id == predecessor_id {
            predecessor = Some(row);
        } else if row.id == successor_id {
            successor = Some(row);
        }
    }
    let (predecessor, successor) = match (predecessor, successor) {
        (Some(predecessor), Some(successor))
            if predecessor.state == "rotating"
                && successor.state == "pending"
                && successor.predecessor_id == Some(predecessor.id)
                && predecessor
                    .successor_id
                    .is_none_or(|linked_id| linked_id == successor.id)
                && predecessor.user_id == successor.user_id
                && predecessor.family_handle == successor.family_handle =>
        {
            (predecessor, successor)
        }
        _ => return Ok(None),
    };

    let predecessor = sqlx::query_as::<_, AuthSessionRow>(
        "UPDATE auth_sessions
         SET state = 'revoked', successor_id = $2, updated_at = now()
         WHERE id = $1 AND state = 'rotating' AND expires_at > now()
           AND (successor_id IS NULL OR successor_id = $2)
         RETURNING *",
    )
    .bind(predecessor.id)
    .bind(successor.id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(predecessor) = predecessor else {
        return Ok(None);
    };
    let successor = sqlx::query_as::<_, AuthSessionRow>(
        "UPDATE auth_sessions
         SET state = 'active', updated_at = now()
         WHERE id = $1 AND state = 'pending' AND predecessor_id = $2
           AND expires_at > now()
         RETURNING *",
    )
    .bind(successor.id)
    .bind(predecessor.id)
    .fetch_optional(conn)
    .await?;
    let Some(successor) = successor else {
        return Ok(None);
    };
    Ok(Some((predecessor.try_into()?, successor.try_into()?)))
}

/// auth intent 的允许载荷；只包含 opaque handle 与公开过期时间。
#[derive(Debug, Clone)]
pub enum AuthIntentPayload {
    RedisPopulate {
        session_handle: String,
        expires_at: DateTime<Utc>,
    },
    RedisInvalidate {
        session_handle: String,
    },
}

impl AuthIntentPayload {
    fn kind(&self) -> &'static str {
        match self {
            Self::RedisPopulate { .. } => "redis_populate",
            Self::RedisInvalidate { .. } => "redis_invalidate",
        }
    }

    fn to_json(&self) -> serde_json::Value {
        match self {
            Self::RedisPopulate {
                session_handle,
                expires_at,
            } => serde_json::json!({
                "session_handle": session_handle,
                "expires_at": expires_at,
            }),
            Self::RedisInvalidate { session_handle } => {
                serde_json::json!({"session_handle": session_handle})
            }
        }
    }
}

/// Durable intent 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthIntentState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Redis 暂时失败的重试延迟上限，防止异常调用方写入无界 interval。
const AUTH_INTENT_MAX_RETRY_DELAY_SECONDS: i64 = 300;

impl AuthIntentState {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// 对 worker 公开的 intent 快照；持久化 row/state 字符串保持模块私有。
#[derive(Debug, Clone)]
pub struct AuthSessionIntent {
    pub id: i64,
    pub session_id: i64,
    pub kind: String,
    pub state: AuthIntentState,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
    pub run_after: DateTime<Utc>,
    pub lease_until: Option<DateTime<Utc>>,
    pub worker_id: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct AuthSessionIntentRow {
    id: i64,
    session_id: i64,
    kind: String,
    state: String,
    payload: serde_json::Value,
    attempts: i32,
    max_attempts: i32,
    run_after: DateTime<Utc>,
    lease_until: Option<DateTime<Utc>>,
    worker_id: Option<String>,
    last_error_code: Option<String>,
    last_error_message: Option<String>,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<AuthSessionIntentRow> for AuthSessionIntent {
    type Error = sqlx::Error;

    fn try_from(row: AuthSessionIntentRow) -> Result<Self, Self::Error> {
        let state = AuthIntentState::parse(&row.state)
            .ok_or_else(|| sqlx::Error::Protocol("unknown auth intent state".to_string()))?;
        Ok(Self {
            id: row.id,
            session_id: row.session_id,
            kind: row.kind,
            state,
            payload: row.payload,
            attempts: row.attempts,
            max_attempts: row.max_attempts,
            run_after: row.run_after,
            lease_until: row.lease_until,
            worker_id: row.worker_id,
            last_error_code: row.last_error_code,
            last_error_message: row.last_error_message,
            created_at: row.created_at,
            completed_at: row.completed_at,
            updated_at: row.updated_at,
        })
    }
}

fn into_intent(
    row: Option<AuthSessionIntentRow>,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    row.map(AuthSessionIntent::try_from).transpose()
}

/// 在业务事务内写入 durable cache intent/outbox。
pub async fn enqueue_intent_tx(
    conn: &mut PgConnection,
    session_id: i64,
    payload: AuthIntentPayload,
    max_attempts: i32,
) -> Result<AuthSessionIntent, sqlx::Error> {
    let row: AuthSessionIntentRow = sqlx::query_as(
        "INSERT INTO auth_session_intents (session_id, kind, payload, max_attempts)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(session_id)
    .bind(payload.kind())
    .bind(payload.to_json())
    .bind(max_attempts)
    .fetch_one(conn)
    .await?;
    row.try_into()
}

/// 领取任意到期或租约过期的 intent；并发实例通过 `SKIP LOCKED` 隔离。
pub async fn claim_intent(
    pool: &PgPool,
    worker_id: &str,
    lease_seconds: i64,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    claim_intent_inner(pool, worker_id, lease_seconds, None).await
}

/// 将领取限定到单一 session，供分片 worker 与互不抢占的合同测试复用。
pub async fn claim_intent_for_session(
    pool: &PgPool,
    worker_id: &str,
    lease_seconds: i64,
    session_id: i64,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    claim_intent_inner(pool, worker_id, lease_seconds, Some(session_id)).await
}

async fn claim_intent_inner(
    pool: &PgPool,
    worker_id: &str,
    lease_seconds: i64,
    session_id: Option<i64>,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, AuthSessionIntentRow>(
        "WITH exhausted AS (
             UPDATE auth_session_intents
             SET state = 'failed', worker_id = NULL, lease_until = NULL,
                 last_error_code = 'auth_intent_attempts_exhausted',
                 last_error_message = 'auth intent exhausted all attempts',
                 completed_at = now(), updated_at = now()
             WHERE state = 'running' AND lease_until <= now()
               AND attempts >= max_attempts
               AND ($3::BIGINT IS NULL OR session_id = $3)
             RETURNING id
         ), exhausted_queued AS (
             UPDATE auth_session_intents
             SET state = 'failed', worker_id = NULL, lease_until = NULL,
                 last_error_code = 'auth_intent_attempts_exhausted',
                 last_error_message = 'auth intent exhausted all attempts',
                 completed_at = now(), updated_at = now()
             WHERE state = 'queued' AND run_after <= now()
               AND attempts >= max_attempts
               AND ($3::BIGINT IS NULL OR session_id = $3)
             RETURNING id
         ), candidate AS (
             SELECT id
             FROM auth_session_intents
             WHERE attempts < max_attempts
               AND ($3::BIGINT IS NULL OR session_id = $3)
               AND (
                    (state = 'queued' AND run_after <= now())
                    OR (state = 'running' AND lease_until <= now())
               )
             ORDER BY run_after, id
             FOR UPDATE SKIP LOCKED
             LIMIT 1
         )
         UPDATE auth_session_intents AS intent
         SET state = 'running', worker_id = $1,
             lease_until = now() + make_interval(secs => $2),
             attempts = intent.attempts + 1, updated_at = now()
         FROM candidate
         WHERE intent.id = candidate.id
         RETURNING intent.*",
    )
    .bind(worker_id)
    .bind(lease_seconds.max(1) as f64)
    .bind(session_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    into_intent(row)
}

/// 仅当前持有者可续租 intent，过期 lease 不能被复活。
pub async fn renew_intent_lease(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query(
        "UPDATE auth_session_intents
         SET lease_until = now() + make_interval(secs => $3), updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()",
    )
    .bind(id)
    .bind(worker_id)
    .bind(lease_seconds.max(1) as f64)
    .execute(pool)
    .await
    .map(|result| result.rows_affected() == 1)
}

/// 完成 intent；Redis 操作成功后保留历史结果行。
pub async fn complete_intent(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionIntentRow>(
        "UPDATE auth_session_intents
         SET state = 'succeeded', worker_id = NULL, lease_until = NULL,
             last_error_code = NULL, last_error_message = NULL,
             completed_at = now(), updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()
         RETURNING *",
    )
    .bind(id)
    .bind(worker_id)
    .fetch_optional(pool)
    .await?;
    into_intent(row)
}

/// 记录暂时错误并始终重排同一 intent；仅当前有效 lease 持有者可更新。
pub async fn reschedule_intent(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    error_code: &str,
    error_message: &str,
    retry_after_seconds: i64,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionIntentRow>(
        "UPDATE auth_session_intents
         SET state = 'queued',
             run_after = now() + make_interval(secs => $5),
             worker_id = NULL, lease_until = NULL,
             last_error_code = $3, last_error_message = $4,
             completed_at = NULL, updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()
         RETURNING *",
    )
    .bind(id)
    .bind(worker_id)
    .bind(error_code)
    .bind(error_message)
    .bind(retry_after_seconds.clamp(0, AUTH_INTENT_MAX_RETRY_DELAY_SECONDS) as f64)
    .fetch_optional(pool)
    .await?;
    into_intent(row)
}

/// 将 kind/payload 等永久数据错误置为 failed；暂时 Redis 错误不得调用本入口。
pub async fn fail_intent_permanently(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    error_code: &str,
    error_message: &str,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionIntentRow>(
        "UPDATE auth_session_intents
         SET state = 'failed', worker_id = NULL, lease_until = NULL,
             last_error_code = $3, last_error_message = $4,
             completed_at = now(), updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()
         RETURNING *",
    )
    .bind(id)
    .bind(worker_id)
    .bind(error_code)
    .bind(error_message)
    .fetch_optional(pool)
    .await?;
    into_intent(row)
}

/// 手动重试复用同一 intent id；保留单调 attempts，并显式增加一次可执行预算。
pub async fn retry_intent_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<AuthSessionIntent>, sqlx::Error> {
    let row = sqlx::query_as::<_, AuthSessionIntentRow>(
        "UPDATE auth_session_intents
         SET state = 'queued', attempts = attempts + 1,
             max_attempts = GREATEST(max_attempts, attempts + 2), run_after = now(),
             worker_id = NULL, lease_until = NULL,
             last_error_code = NULL, last_error_message = NULL,
             completed_at = NULL, updated_at = now()
         WHERE id = $1 AND state = 'failed'
         RETURNING *",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    into_intent(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_hash_is_strongly_validated_and_debug_redacted() {
        let hash = RefreshTokenHash::parse("a".repeat(64)).unwrap();
        assert_eq!(format!("{hash:?}"), "RefreshTokenHash([REDACTED])");
        assert!(RefreshTokenHash::parse("raw-token".to_string()).is_err());
    }
}
