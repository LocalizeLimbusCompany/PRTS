//! 服务元信息端点。

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use crate::dto::upload::UploadConfigDto;
use crate::error::ApiError;
use crate::state::AppState;
use crate::{appsettings, db_err};

/// 服务版本信息。
#[derive(Serialize, ToSchema)]
pub struct VersionInfo {
    /// 服务名（crate 名）。
    pub name: String,
    /// 语义化版本（来自 Cargo 包版本）。
    pub version: String,
}

/// 登录页可安全公开使用的认证能力。
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthConfigDto {
    /// 是否允许账号密码登录。
    pub password_login_enabled: bool,
    /// 是否允许账号密码自助注册。
    pub password_registration_enabled: bool,
    /// 当前已配置且可用的 OAuth provider 稳定标识。
    pub oauth_providers: Vec<String>,
}

/// 返回服务名与版本。
#[utoipa::path(
    get,
    path = "/version",
    tag = "meta",
    responses((status = 200, description = "服务版本", body = VersionInfo))
)]
pub async fn version() -> Json<VersionInfo> {
    Json(VersionInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// 返回当前上传客户端限制，不包含清理保留期或内部存储路径。
#[utoipa::path(
    get,
    path = "/meta/upload-config",
    tag = "meta",
    description = "返回新流式上传客户端的文件数、单文件字节数、批次字节数和浏览器并发限制；不返回内部清理周期、临时卷路径或任何密钥。",
    responses((status = 200, description = "上传客户端运行时限制", body = UploadConfigDto))
)]
pub async fn upload_config(
    State(state): State<AppState>,
) -> Result<Json<UploadConfigDto>, ApiError> {
    let config = prts_db::upload_settings::get(&state.db)
        .await
        .map_err(db_err)?;
    Ok(Json(config.into()))
}

/// 返回公开认证能力，供登录与注册页决定可用入口。
#[utoipa::path(
    get,
    path = "/meta/auth-config",
    tag = "meta",
    description = "只返回密码登录、密码注册和已启用 OAuth provider 的公开能力；不返回 client id、secret、端点或其它敏感配置。",
    responses(
        (status = 200, description = "当前公开认证能力", body = AuthConfigDto),
        (status = 500, description = "无法安全读取认证设置")
    )
)]
pub async fn auth_config(State(state): State<AppState>) -> Result<Json<AuthConfigDto>, ApiError> {
    let oauth_only = appsettings::get_bool(&state, appsettings::AUTH_OAUTH_ONLY, false)
        .await
        .map_err(db_err)?;
    let registration_open =
        appsettings::get_bool(&state, appsettings::AUTH_REGISTRATION_OPEN, true)
            .await
            .map_err(db_err)?;
    let oauth_providers = if state.zoot_provider().is_some() {
        vec!["zoot".to_string()]
    } else {
        Vec::new()
    };
    Ok(Json(build_auth_config(
        oauth_only,
        registration_open,
        oauth_providers,
    )))
}

fn build_auth_config(
    oauth_only: bool,
    registration_open: bool,
    oauth_providers: Vec<String>,
) -> AuthConfigDto {
    AuthConfigDto {
        password_login_enabled: !oauth_only,
        password_registration_enabled: !oauth_only && registration_open,
        oauth_providers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_auth_config_matches_password_and_registration_policy() {
        let oauth_only = build_auth_config(true, true, vec!["zoot".to_string()]);
        assert!(!oauth_only.password_login_enabled);
        assert!(!oauth_only.password_registration_enabled);
        assert_eq!(oauth_only.oauth_providers, ["zoot"]);

        let closed_registration = build_auth_config(false, false, Vec::new());
        assert!(closed_registration.password_login_enabled);
        assert!(!closed_registration.password_registration_enabled);

        let password_and_oauth = build_auth_config(false, true, vec!["zoot".to_string()]);
        assert!(password_and_oauth.password_login_enabled);
        assert!(password_and_oauth.password_registration_enabled);
    }
}
