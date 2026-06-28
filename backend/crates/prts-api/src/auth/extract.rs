//! 当前用户提取器：从 `Authorization: Bearer <token>` 解析身份。
//!
//! token 以 `prts_` 前缀者按 **API Key** 处理（库内查哈希），否则按 **JWT** 处理。
//! 非 `active` 状态的账号一律拒绝。

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use prts_common::Error;
use prts_core::PlatformRole;

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

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or(Error::Unauthorized)?;

        let user = if token.starts_with(prts_auth::token::API_KEY_PREFIX) {
            let hash = prts_auth::token::sha256_hex(token);
            let user = prts_db::api_keys::find_user_by_key_hash(&state.db, &hash)
                .await
                .map_err(|e| Error::internal(format!("db error: {e}")))?
                .ok_or(Error::Unauthorized)?;
            // 记录最近使用（忽略失败）。
            let _ = prts_db::api_keys::touch_last_used(&state.db, &hash).await;
            user
        } else {
            let claims = prts_auth::jwt::decode(token, state.jwt_secret())
                .map_err(|_| Error::Unauthorized)?;
            if !claims.is_valid_at(chrono::Utc::now().timestamp()) {
                return Err(Error::Unauthorized.into());
            }
            prts_db::users::find_by_id(&state.db, claims.sub)
                .await
                .map_err(|e| Error::internal(format!("db error: {e}")))?
                .ok_or(Error::Unauthorized)?
        };

        if user.status != "active" {
            return Err(Error::Forbidden.into());
        }

        Ok(CurrentUser {
            id: user.id,
            platform_role: user.platform_role.as_deref().and_then(PlatformRole::parse),
        })
    }
}
