//! 平台运行时设置的便捷读取（带默认值）。底层是 `settings` 表（key → JSONB）。

use crate::state::AppState;

/// 仅 OAuth 登录模式（禁用账号密码）。
pub const AUTH_OAUTH_ONLY: &str = "auth.oauth_only";
/// 是否开放注册。
pub const AUTH_REGISTRATION_OPEN: &str = "auth.registration_open";
// 注：`auth.require_email_verification` 设置键在邮件投递接入（后续）时再引用，暂不定义常量。

/// 读取布尔设置，缺失或类型不符时回退默认值。
pub async fn get_bool(state: &AppState, key: &str, default: bool) -> bool {
    match prts_db::settings::get(&state.db, key).await {
        Ok(Some(v)) => v.as_bool().unwrap_or(default),
        _ => default,
    }
}
