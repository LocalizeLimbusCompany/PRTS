//! 认证端点：注册 / 登录 / 刷新 / 登出 / ZOOT OAuth。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;

use crate::auth::session::{self, IssuedTokens};
use crate::dto::UserDto;
use crate::error::ApiError;
use crate::state::AppState;
use crate::{appsettings, db_err};

/// 令牌响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// 固定 `"Bearer"`。
    pub token_type: String,
    /// access token 有效秒数。
    pub expires_in: i64,
    pub user: UserDto,
}

impl TokenResponse {
    fn build(tokens: IssuedTokens, user: &prts_db::models::User) -> Self {
        Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: tokens.access_expires_in,
            user: user.into(),
        }
    }
}

/// 注册请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterReq {
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub password: String,
}

/// 账号密码注册。
#[utoipa::path(
    post, path = "/auth/register", tag = "auth",
    request_body = RegisterReq,
    responses(
        (status = 200, description = "注册成功并登录", body = TokenResponse),
        (status = 400, description = "参数错误 / 仅 OAuth 模式"),
        (status = 403, description = "注册已关闭"),
        (status = 409, description = "用户名或邮箱已存在"),
    )
)]
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> Result<Json<TokenResponse>, ApiError> {
    if appsettings::get_bool(&state, appsettings::AUTH_OAUTH_ONLY, false).await {
        return Err(Error::bad_request("仅 OAuth 登录模式，已禁用账号密码注册").into());
    }
    if !appsettings::get_bool(&state, appsettings::AUTH_REGISTRATION_OPEN, true).await {
        return Err(Error::Forbidden.into());
    }

    let username = req.username.trim();
    if username.len() < 3 || username.len() > 32 {
        return Err(Error::bad_request("用户名长度需为 3–32 字符").into());
    }
    if req.password.len() < 8 {
        return Err(Error::bad_request("密码至少 8 位").into());
    }
    if prts_db::users::username_exists(&state.db, username)
        .await
        .map_err(db_err)?
    {
        return Err(Error::Conflict.into());
    }
    let email = req
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty());
    if let Some(email) = email {
        if prts_db::users::find_by_email(&state.db, email)
            .await
            .map_err(db_err)?
            .is_some()
        {
            return Err(Error::Conflict.into());
        }
    }

    let hash = prts_auth::password::hash_password(&req.password)
        .map_err(|e| Error::internal(format!("hash error: {e}")))?;
    // P1：邮箱投递尚未接入，统一创建为 active（require_email_verification 的实际拦截待 SMTP 接入）。
    let user = prts_db::users::create_password_user(&state.db, username, email, &hash, "active")
        .await
        .map_err(db_err)?;
    let user = maybe_bootstrap_super_admin(&state, user).await?;

    let tokens = session::issue(&state, user.id).await?;
    Ok(Json(TokenResponse::build(tokens, &user)))
}

/// 登录请求（`username` 可为用户名或邮箱）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

/// 账号密码登录。
#[utoipa::path(
    post, path = "/auth/login", tag = "auth",
    request_body = LoginReq,
    responses(
        (status = 200, description = "登录成功", body = TokenResponse),
        (status = 401, description = "凭证错误"),
        (status = 403, description = "账号被禁用"),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> Result<Json<TokenResponse>, ApiError> {
    let user = match prts_db::users::find_by_username(&state.db, &req.username)
        .await
        .map_err(db_err)?
    {
        Some(u) => u,
        None => prts_db::users::find_by_email(&state.db, &req.username)
            .await
            .map_err(db_err)?
            .ok_or(Error::Unauthorized)?,
    };

    let hash = user.password_hash.as_deref().ok_or(Error::Unauthorized)?;
    if !prts_auth::password::verify_password(&req.password, hash) {
        return Err(Error::Unauthorized.into());
    }
    if user.status != "active" {
        return Err(Error::Forbidden.into());
    }

    let tokens = session::issue(&state, user.id).await?;
    Ok(Json(TokenResponse::build(tokens, &user)))
}

/// 刷新令牌请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RefreshReq {
    pub refresh_token: String,
}

/// 用 refresh token 轮换出新令牌对。
#[utoipa::path(
    post, path = "/auth/refresh", tag = "auth",
    request_body = RefreshReq,
    responses(
        (status = 200, description = "刷新成功", body = TokenResponse),
        (status = 401, description = "refresh 无效或已过期"),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshReq>,
) -> Result<Json<TokenResponse>, ApiError> {
    let tokens = session::refresh(&state, &req.refresh_token).await?;
    let user = prts_db::users::find_by_id(&state.db, tokens.user_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;
    Ok(Json(TokenResponse::build(tokens, &user)))
}

/// 登出（吊销 refresh token）。
#[utoipa::path(
    post, path = "/auth/logout", tag = "auth",
    request_body = RefreshReq,
    responses((status = 204, description = "已登出"))
)]
pub async fn logout(
    State(state): State<AppState>,
    Json(req): Json<RefreshReq>,
) -> Result<StatusCode, ApiError> {
    session::revoke(&state, &req.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// OAuth 发起响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthStartResponse {
    /// 引导浏览器跳转的授权 URL。
    pub authorize_url: String,
}

/// 发起第三方登录（当前支持 `zoot`）。
#[utoipa::path(
    get, path = "/auth/oauth/{provider}/start", tag = "auth",
    responses(
        (status = 200, description = "返回授权跳转 URL", body = OAuthStartResponse),
        (status = 400, description = "未配置该 provider"),
        (status = 404, description = "未知 provider"),
    )
)]
pub async fn oauth_start(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<OAuthStartResponse>, ApiError> {
    if provider != "zoot" {
        return Err(Error::NotFound.into());
    }
    let zoot = state
        .zoot_provider()
        .ok_or_else(|| Error::bad_request("ZOOT 登录未配置"))?;

    let oauth_state = prts_auth::token::random_token(32);
    let start = zoot
        .begin(oauth_state)
        .map_err(|e| Error::internal(format!("oauth begin error: {e}")))?;
    session::store_oauth_state(&state, &start.state, &start.code_verifier).await?;

    Ok(Json(OAuthStartResponse {
        authorize_url: start.authorize_url,
    }))
}

/// OAuth 回调查询参数。
#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// 第三方登录回调：换取身份、登录或创建用户，并跳转回前端（令牌经 URL fragment 传递）。
#[utoipa::path(
    get, path = "/auth/oauth/{provider}/callback", tag = "auth",
    responses((status = 303, description = "跳转回前端"))
)]
pub async fn oauth_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(q): Query<OAuthCallbackQuery>,
) -> Result<Redirect, ApiError> {
    let base = state.settings.auth.public_base_url.trim_end_matches('/');
    if provider != "zoot" {
        return Err(Error::NotFound.into());
    }
    if let Some(err) = q.error.as_deref() {
        return Ok(Redirect::to(&format!("{base}/#/login?error={err}")));
    }

    let code = q.code.ok_or_else(|| Error::bad_request("缺少 code"))?;
    let st = q.state.ok_or_else(|| Error::bad_request("缺少 state"))?;
    let verifier = session::take_oauth_state(&state, &st)
        .await?
        .ok_or_else(|| Error::bad_request("state 无效或已过期"))?;

    let zoot = state
        .zoot_provider()
        .ok_or_else(|| Error::bad_request("ZOOT 登录未配置"))?;
    let identity = zoot
        .complete(&code, &verifier)
        .await
        .map_err(|e| Error::internal(format!("oauth complete error: {e}")))?;

    let user = match prts_db::users::find_by_external(
        &state.db,
        &identity.provider,
        &identity.external_id,
    )
    .await
    .map_err(db_err)?
    {
        Some(u) => u,
        None => {
            let username =
                unique_username(&state, &identity.username, &identity.external_id).await?;
            prts_db::users::create_oauth_user(
                &state.db,
                &username,
                identity.avatar_url.as_deref(),
                &identity.provider,
                &identity.external_id,
                &identity.extra,
            )
            .await
            .map_err(db_err)?
        }
    };

    if user.status != "active" {
        return Err(Error::Forbidden.into());
    }

    let tokens = session::issue(&state, user.id).await?;
    let redirect = format!(
        "{base}/#/oauth?access_token={}&refresh_token={}&expires_in={}",
        tokens.access_token, tokens.refresh_token, tokens.access_expires_in
    );
    Ok(Redirect::to(&redirect))
}

/// 若用户名与配置的 bootstrap_admin 一致，则授予 super_admin 并返回更新后的用户。
async fn maybe_bootstrap_super_admin(
    state: &AppState,
    user: prts_db::models::User,
) -> Result<prts_db::models::User, ApiError> {
    let target = state.settings.auth.bootstrap_admin.trim();
    if !target.is_empty() && user.username == target && user.platform_role.is_none() {
        prts_db::users::set_platform_role(&state.db, user.id, Some("super_admin"))
            .await
            .map_err(db_err)?;
        if let Some(updated) = prts_db::users::find_by_id(&state.db, user.id)
            .await
            .map_err(db_err)?
        {
            return Ok(updated);
        }
    }
    Ok(user)
}

/// 为 OAuth 新用户找一个未占用的用户名。
async fn unique_username(
    state: &AppState,
    preferred: &str,
    external_id: &str,
) -> Result<String, ApiError> {
    let base = {
        let t = preferred.trim();
        if t.is_empty() {
            format!("zoot_{external_id}")
        } else {
            t.to_string()
        }
    };
    if !prts_db::users::username_exists(&state.db, &base)
        .await
        .map_err(db_err)?
    {
        return Ok(base);
    }
    for i in 1..1000 {
        let candidate = format!("{base}_{i}");
        if !prts_db::users::username_exists(&state.db, &candidate)
            .await
            .map_err(db_err)?
        {
            return Ok(candidate);
        }
    }
    Ok(format!("{base}_{external_id}"))
}
