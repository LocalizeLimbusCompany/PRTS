//! 当前用户提取器：从 `Authorization: Bearer <token>` 解析身份。
//!
//! token 以 `prts_` 前缀者按 **API Key** 处理（库内查哈希），否则按 **JWT** 处理。
//! 非 `active` 状态的账号一律拒绝。

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use prts_common::Error;
use prts_core::PlatformRole;
use prts_db::audit::{
    AuditActor, AuditActorKind, AuditEvent, AuthFailureMethod, AuthFailureReason,
};

use crate::error::ApiError;
use crate::state::AppState;

/// 已认证的当前用户（精简身份，权限判定用）。
pub struct CurrentUser {
    pub id: i64,
    pub platform_role: Option<PlatformRole>,
}

impl CurrentUser {
    /// 是否拥有某平台权限节点。
    pub fn has_platform(&self, node: &str) -> bool {
        self.platform_role.map(|r| r.has(node)).unwrap_or(false)
    }

    /// 要求某平台权限节点，否则 403。
    pub fn require_platform(&self, node: &str) -> Result<(), ApiError> {
        if self.has_platform(node) {
            Ok(())
        } else {
            Err(Error::Forbidden.into())
        }
    }
}

/// 已提供凭证的失败判定必须先完成脱敏审计，再返回原 401/403。
async fn reject_presented_credential<T>(
    state: &AppState,
    user_id: i64,
    method: AuthFailureMethod,
    reason: AuthFailureReason,
    rejection: Error,
) -> Result<T, ApiError> {
    crate::auth::session::record_auth_failure(state, user_id, method, reason).await?;
    Err(rejection.into())
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let locale = parts
            .extensions
            .get::<prts_common::i18n::Locale>()
            .copied()
            .unwrap_or_default();
        let result: Result<Self, ApiError> = async {
            let Some(authorization) = parts.headers.get(axum::http::header::AUTHORIZATION) else {
                // 真正未提供凭证不是一次失败认证；MaybeUser 可安全降级为游客。
                return Err(Error::Unauthorized.into());
            };
            let token = match authorization
                .to_str()
                .ok()
                .and_then(|header| header.strip_prefix("Bearer "))
                .map(str::trim)
                .filter(|token| !token.is_empty())
            {
                Some(token) => token,
                None => {
                    return reject_presented_credential(
                        state,
                        0,
                        AuthFailureMethod::Authorization,
                        AuthFailureReason::InvalidCredential,
                        Error::Unauthorized,
                    )
                    .await;
                }
            };

            let user = if token.starts_with(prts_auth::token::API_KEY_PREFIX) {
                let hash = prts_auth::token::sha256_hex(token);
                let user = prts_db::api_keys::find_user_by_key_hash(&state.db, &hash)
                    .await
                    .map_err(|e| Error::internal(format!("db error: {e}")))?;
                let Some(user) = user else {
                    return reject_presented_credential(
                        state,
                        0,
                        AuthFailureMethod::ApiKey,
                        AuthFailureReason::InvalidCredential,
                        Error::Unauthorized,
                    )
                    .await;
                };
                if user.status != "active" {
                    return reject_presented_credential(
                        state,
                        user.id,
                        AuthFailureMethod::ApiKey,
                        AuthFailureReason::AccountInactive,
                        Error::Forbidden,
                    )
                    .await;
                }
                let mut tx = state
                    .db
                    .begin()
                    .await
                    .map_err(|_| Error::AuditUnavailable)?;
                let key = prts_db::api_keys::touch_last_used_tx(&mut tx, &hash)
                    .await
                    .map_err(|e| Error::internal(format!("db error: {e}")))?;
                let Some(key) = key else {
                    tx.rollback()
                        .await
                        .map_err(|e| Error::internal(format!("db error: {e}")))?;
                    return reject_presented_credential(
                        state,
                        user.id,
                        AuthFailureMethod::ApiKey,
                        AuthFailureReason::InvalidCredential,
                        Error::Unauthorized,
                    )
                    .await;
                };
                prts_db::audit::append_event_tx(
                    &mut tx,
                    AuditActor {
                        id: Some(user.id),
                        kind: AuditActorKind::ApiKey,
                        ip: None,
                    },
                    AuditEvent::ApiKeyUsed {
                        key_id: key.id,
                        prefix: &key.prefix,
                    },
                )
                .await
                .map_err(|_| Error::AuditUnavailable)?;
                tx.commit().await.map_err(|_| Error::AuditUnavailable)?;
                user
            } else {
                let claims = match prts_auth::jwt::decode(token, state.jwt_secret()) {
                    Ok(claims) => claims,
                    Err(_) => {
                        return reject_presented_credential(
                            state,
                            0,
                            AuthFailureMethod::Jwt,
                            AuthFailureReason::InvalidCredential,
                            Error::Unauthorized,
                        )
                        .await;
                    }
                };
                if claims.typ != "access" {
                    return reject_presented_credential(
                        state,
                        claims.sub,
                        AuthFailureMethod::Jwt,
                        AuthFailureReason::InvalidTokenType,
                        Error::Unauthorized,
                    )
                    .await;
                }
                if !claims.is_valid_at(chrono::Utc::now().timestamp()) {
                    return reject_presented_credential(
                        state,
                        claims.sub,
                        AuthFailureMethod::Jwt,
                        AuthFailureReason::TokenExpired,
                        Error::Unauthorized,
                    )
                    .await;
                }
                let Some(session_handle) = claims.sid.as_deref() else {
                    return reject_presented_credential(
                        state,
                        claims.sub,
                        AuthFailureMethod::Jwt,
                        AuthFailureReason::MissingSession,
                        Error::Unauthorized,
                    )
                    .await;
                };
                let session = prts_db::auth_sessions::find_active_unexpired_by_handle(
                    &state.db,
                    session_handle,
                )
                .await
                .map_err(|e| Error::internal(format!("db error: {e}")))?;
                let Some(session) = session else {
                    return reject_presented_credential(
                        state,
                        claims.sub,
                        AuthFailureMethod::Jwt,
                        AuthFailureReason::SessionInactive,
                        Error::Unauthorized,
                    )
                    .await;
                };
                if session.user_id != claims.sub {
                    return reject_presented_credential(
                        state,
                        claims.sub,
                        AuthFailureMethod::Jwt,
                        AuthFailureReason::UserMismatch,
                        Error::Unauthorized,
                    )
                    .await;
                }
                let user = prts_db::users::find_by_id(&state.db, claims.sub)
                    .await
                    .map_err(|e| Error::internal(format!("db error: {e}")))?;
                let Some(user) = user else {
                    return reject_presented_credential(
                        state,
                        claims.sub,
                        AuthFailureMethod::Jwt,
                        AuthFailureReason::UserNotFound,
                        Error::Unauthorized,
                    )
                    .await;
                };
                if user.status != "active" {
                    return reject_presented_credential(
                        state,
                        user.id,
                        AuthFailureMethod::Jwt,
                        AuthFailureReason::AccountInactive,
                        Error::Forbidden,
                    )
                    .await;
                }
                user
            };

            Ok(CurrentUser {
                id: user.id,
                platform_role: user.platform_role.as_deref().and_then(PlatformRole::parse),
            })
        }
        .await;
        result.map_err(|error| error.with_locale(locale))
    }
}

/// 可选当前用户：无凭证或凭证无效时为 `None`（用于公开项目的游客只读）。
pub struct MaybeUser(pub Option<CurrentUser>);

impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match CurrentUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(MaybeUser(Some(user))),
            // 公开读取保持既有语义：缺少/无效凭证以及非 active 账号按游客处理。
            Err(error) if matches!(error.code(), "unauthorized" | "forbidden") => {
                Ok(MaybeUser(None))
            }
            // DB 与审计错误绝不能被吞掉；尤其有效 API key 的 touch + audit 必须 fail closed。
            Err(error) => Err(error),
        }
    }
}
