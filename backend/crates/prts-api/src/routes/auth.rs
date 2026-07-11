//! 认证端点：注册 / 登录 / 刷新 / 登出 / ZOOT OAuth。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};
use sqlx::PgConnection;

use crate::auth::session::{self, IssueKind, IssuedTokens};
use crate::dto::UserDto;
use crate::error::{ApiError, ErrorResponse};
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
    fn build(tokens: IssuedTokens, user: UserDto) -> Self {
        Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: tokens.access_expires_in,
            user,
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
        (status = 503, description = "审计服务不可用，注册与令牌签发均未提交", body = ErrorResponse),
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
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let user = prts_db::users::create_password_user_tx(&mut tx, username, email, &hash, "active")
        .await
        .map_err(db_err)?;
    let user = maybe_bootstrap_super_admin_tx(&mut tx, &state, user).await?;

    let tokens = session::issue_tx(&mut tx, &state, user.id, IssueKind::Register).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(TokenResponse::build(tokens, (&user).into())))
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
        (status = 503, description = "审计服务不可用，未返回认证结论或令牌", body = ErrorResponse),
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
        None => match prts_db::users::find_by_email(&state.db, &req.username)
            .await
            .map_err(db_err)?
        {
            Some(user) => user,
            None => {
                session::record_failed_authentication(&state, 0, "password", "invalid_credentials")
                    .await?;
                return Err(Error::Unauthorized.into());
            }
        },
    };

    let password_matches = user
        .password_hash
        .as_deref()
        .is_some_and(|hash| prts_auth::password::verify_password(&req.password, hash));
    if !password_matches {
        session::record_failed_authentication(&state, user.id, "password", "invalid_credentials")
            .await?;
        return Err(Error::Unauthorized.into());
    }
    if user.status != "active" {
        session::record_failed_authentication(&state, user.id, "password", "account_inactive")
            .await?;
        return Err(Error::Forbidden.into());
    }

    let mut tx = state.db.begin().await.map_err(db_err)?;
    let tokens = session::issue_tx(&mut tx, &state, user.id, IssueKind::Login).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(TokenResponse::build(tokens, (&user).into())))
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
        (status = 503, description = "审计服务不可用，令牌轮换未提交", body = ErrorResponse),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshReq>,
) -> Result<Json<TokenResponse>, ApiError> {
    let refreshed = session::refresh(&state, &req.refresh_token).await?;
    Ok(Json(TokenResponse::build(refreshed.tokens, refreshed.user)))
}

/// 登出（吊销 refresh token）。
#[utoipa::path(
    post, path = "/auth/logout", tag = "auth",
    request_body = RefreshReq,
    responses(
        (status = 204, description = "已登出"),
        (status = 503, description = "审计服务不可用，有效会话未被吊销", body = ErrorResponse)
    )
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
    responses(
        (status = 303, description = "跳转回前端"),
        (status = 503, description = "审计服务不可用，未返回 OAuth 认证结论或令牌", body = ErrorResponse)
    )
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
        audit_oauth_failure(&state, &provider, "provider_error").await?;
        return Ok(Redirect::to(&format!("{base}/#/login?error={err}")));
    }

    let Some(code) = q.code else {
        audit_oauth_failure(&state, &provider, "missing_code").await?;
        return Err(Error::bad_request("缺少 code").into());
    };
    let Some(st) = q.state else {
        audit_oauth_failure(&state, &provider, "missing_state").await?;
        return Err(Error::bad_request("缺少 state").into());
    };
    let Some(verifier) = session::take_oauth_state(&state, &st).await? else {
        audit_oauth_failure(&state, &provider, "invalid_state").await?;
        return Err(Error::bad_request("state 无效或已过期").into());
    };

    let Some(zoot) = state.zoot_provider() else {
        audit_oauth_failure(&state, &provider, "provider_unavailable").await?;
        return Err(Error::bad_request("ZOOT 登录未配置").into());
    };
    let identity = match zoot.complete(&code, &verifier).await {
        Ok(identity) => identity,
        Err(error) => {
            audit_oauth_failure(&state, &provider, "exchange_failed").await?;
            return Err(Error::internal(format!("oauth complete error: {error}")).into());
        }
    };

    let existing =
        prts_db::users::find_by_external(&state.db, &identity.provider, &identity.external_id)
            .await
            .map_err(db_err)?;
    let username = if existing.is_none() {
        Some(unique_username(&state, &identity.username, &identity.external_id).await?)
    } else {
        None
    };
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let (user, new_user) = match existing {
        Some(user) => (user, false),
        None => {
            let user = prts_db::users::create_oauth_user_tx(
                &mut tx,
                username.as_deref().expect("new OAuth user has username"),
                identity.avatar_url.as_deref(),
                &identity.provider,
                &identity.external_id,
                &identity.extra,
            )
            .await
            .map_err(db_err)?;
            (user, true)
        }
    };

    if user.status != "active" {
        tx.rollback().await.map_err(db_err)?;
        audit_oauth_failure(&state, &provider, "account_inactive").await?;
        return Err(Error::Forbidden.into());
    }

    let tokens = session::issue_tx(
        &mut tx,
        &state,
        user.id,
        IssueKind::OAuth {
            provider: &identity.provider,
            new_user,
        },
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    let redirect = format!(
        "{base}/#/oauth?access_token={}&refresh_token={}&expires_in={}",
        tokens.access_token, tokens.refresh_token, tokens.access_expires_in
    );
    Ok(Redirect::to(&redirect))
}

/// 若用户名与配置的 bootstrap_admin 一致，则授予 super_admin 并返回更新后的用户。
async fn maybe_bootstrap_super_admin_tx(
    conn: &mut PgConnection,
    state: &AppState,
    mut user: prts_db::models::User,
) -> Result<prts_db::models::User, ApiError> {
    let target = state.settings.auth.bootstrap_admin.trim();
    if !target.is_empty() && user.username == target && user.platform_role.is_none() {
        prts_db::users::set_platform_role_tx(&mut *conn, user.id, Some("super_admin"))
            .await
            .map_err(db_err)?;
        prts_db::audit::append_event_tx(
            conn,
            AuditActor {
                id: None,
                kind: AuditActorKind::System,
                ip: None,
            },
            AuditEvent::AuthBootstrapRoleGranted {
                user_id: user.id,
                role: "super_admin",
            },
        )
        .await
        .map_err(|_| Error::AuditUnavailable)?;
        user.platform_role = Some("super_admin".to_string());
    }
    Ok(user)
}

async fn audit_oauth_failure(
    state: &AppState,
    provider: &str,
    reason_code: &str,
) -> Result<(), ApiError> {
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
        AuditEvent::AuthOAuthFailed {
            target_id: provider,
            provider,
            reason_code,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(|_| Error::AuditUnavailable)?;
    Ok(())
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
