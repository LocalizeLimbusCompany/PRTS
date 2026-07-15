//! 平台运行时设置的便捷读取（带默认值）。底层是 `settings` 表（key → JSONB）。

use crate::state::AppState;

/// 仅 OAuth 登录模式（禁用账号密码）。
pub const AUTH_OAUTH_ONLY: &str = "auth.oauth_only";
/// 是否开放注册。
pub const AUTH_REGISTRATION_OPEN: &str = "auth.registration_open";
// 注：`auth.require_email_verification` 设置键在邮件投递接入（后续）时再引用，暂不定义常量。

/// 读取布尔设置；缺失时回退默认值，已存在但类型错误时拒绝继续。
///
/// 认证开关必须把数据库错误传到 HTTP 边界，避免在 `oauth-only`
/// 模式下因读取失败而意外放行密码认证。
pub async fn get_bool(
    state: &AppState,
    key: &str,
    default: bool,
) -> Result<bool, prts_db::DbError> {
    match prts_db::settings::get(&state.db, key).await? {
        None => Ok(default),
        Some(serde_json::Value::Bool(value)) => Ok(value),
        Some(_) => Err(prts_db::DbError::Decode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("setting {key} must be boolean"),
        )))),
    }
}
