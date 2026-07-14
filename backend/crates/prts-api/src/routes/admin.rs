//! 平台管理端点：设置与角色任免（受平台权限节点保护）。

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::{IntoParams, ToSchema};

use prts_common::Error;
use prts_core::permission::{
    assignable_platform_roles, can_manage_platform_user, nodes, PlatformRole,
};
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};
use prts_db::models::User;

use crate::auth::CurrentUser;
use crate::db_err;
use crate::error::{ApiError, ErrorResponse, RequestLocale};
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

const ADMIN_USER_CURSOR_VERSION: u8 = 1;
const DEFAULT_USER_LIMIT: i64 = 50;
const MAX_USER_LIMIT: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserSort {
    UsernameAsc,
    UsernameDesc,
    CreatedAtAsc,
    CreatedAtDesc,
}

impl UserSort {
    fn parse(value: Option<&str>) -> Result<Self, Error> {
        match value.unwrap_or("created_at_desc") {
            "username_asc" => Ok(Self::UsernameAsc),
            "username_desc" => Ok(Self::UsernameDesc),
            "created_at_asc" => Ok(Self::CreatedAtAsc),
            "created_at_desc" => Ok(Self::CreatedAtDesc),
            _ => Err(Error::bad_request("sort 无效")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::UsernameAsc => "username_asc",
            Self::UsernameDesc => "username_desc",
            Self::CreatedAtAsc => "created_at_asc",
            Self::CreatedAtDesc => "created_at_desc",
        }
    }

    fn db_sort(self) -> prts_db::users::AdminUserSort {
        match self {
            Self::UsernameAsc => prts_db::users::AdminUserSort::UsernameAsc,
            Self::UsernameDesc => prts_db::users::AdminUserSort::UsernameDesc,
            Self::CreatedAtAsc => prts_db::users::AdminUserSort::CreatedAtAsc,
            Self::CreatedAtDesc => prts_db::users::AdminUserSort::CreatedAtDesc,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AdminUserCursorValue {
    Username { value: String },
    CreatedAt { value: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct AdminUserCursorV1 {
    version: u8,
    sort: String,
    filter: String,
    last_value: AdminUserCursorValue,
    last_user_id: i64,
}

/// 管理员用户列表查询。`after` 是签名、版本化且绑定当前筛选/排序的键集游标。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListUsersQuery {
    pub q: Option<String>,
    pub role: Option<String>,
    pub sort: Option<String>,
    pub after: Option<String>,
    pub limit: Option<i64>,
}

/// 单个管理员用户列表项的操作能力。
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserCapabilitiesDto {
    pub can_change_role: bool,
}

/// 管理员用户列表项；刻意不包含密码、邮箱或全为零的 CP 列。
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserDto {
    pub id: i64,
    pub username: String,
    pub platform_role: Option<String>,
    pub password_change_required: bool,
    pub created_at: String,
    pub capabilities: AdminUserCapabilitiesDto,
}

/// 当前 actor 对管理员用户工作流的显式能力。
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserListCapabilitiesDto {
    pub create_user: bool,
    pub assignable_roles: Vec<String>,
}

/// 签名键集分页响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserListResponse {
    pub items: Vec<AdminUserDto>,
    pub next_after: Option<String>,
    pub capabilities: AdminUserListCapabilitiesDto,
}

/// 管理员建号请求。初始密码只用于 Argon2id 哈希，不进入响应、审计或任务。
#[derive(Deserialize, ToSchema)]
pub struct CreateUserReq {
    pub username: String,
    pub initial_password: String,
    /// `super_admin|admin|maintainer|user`；严格秩决定 actor 可选择的子集。
    pub role: String,
}

fn parse_platform_role(value: &str, allow_user: bool) -> Result<Option<PlatformRole>, Error> {
    let normalized = value.trim().to_ascii_lowercase();
    if allow_user && normalized == "user" {
        return Ok(None);
    }
    PlatformRole::parse(&normalized)
        .map(Some)
        .ok_or_else(|| Error::bad_request("role 必须是 super_admin|admin|maintainer|user"))
}

fn normalized_user_filters(
    query: &ListUsersQuery,
) -> Result<(Option<String>, Option<String>, UserSort, i64), Error> {
    let q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    if q.as_ref().is_some_and(|value| value.chars().count() > 64) {
        return Err(Error::bad_request("q 最多 64 字符"));
    }
    let role = query
        .role
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    if let Some(role) = role.as_deref() {
        parse_platform_role(role, true)?;
    }
    let sort = UserSort::parse(query.sort.as_deref())?;
    let limit = query.limit.unwrap_or(DEFAULT_USER_LIMIT);
    if !(1..=MAX_USER_LIMIT).contains(&limit) {
        return Err(Error::bad_request("limit 必须在 1–100 之间"));
    }
    Ok((q, role, sort, limit))
}

fn user_filter_fingerprint(q: Option<&str>, role: Option<&str>) -> String {
    let mut digest = Sha256::new();
    digest.update(b"prts-admin-user-filter-v1\0");
    digest.update(q.unwrap_or("").as_bytes());
    digest.update(b"\0");
    digest.update(role.unwrap_or("").as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn encode_user_cursor(cursor: &AdminUserCursorV1, secret: &[u8]) -> String {
    let payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(cursor).expect("admin user cursor payload must serialize"));
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC accepts arbitrary admin cursor secret length");
    mac.update(b"prts-admin-user-cursor-v1\0");
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{payload}.{signature}")
}

fn invalid_user_cursor(locale: RequestLocale) -> ApiError {
    ApiError::from(Error::validation("ADMIN_USER_CURSOR_INVALID")).with_locale(locale.0)
}

fn decode_user_cursor(
    value: &str,
    secret: &[u8],
    sort: UserSort,
    filter: &str,
    locale: RequestLocale,
) -> Result<prts_db::users::AdminUserAfter, ApiError> {
    let (payload, signature) = value
        .split_once('.')
        .ok_or_else(|| invalid_user_cursor(locale))?;
    if signature.contains('.') {
        return Err(invalid_user_cursor(locale));
    }
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| invalid_user_cursor(locale))?;
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC accepts arbitrary admin cursor secret length");
    mac.update(b"prts-admin-user-cursor-v1\0");
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| invalid_user_cursor(locale))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| invalid_user_cursor(locale))?;
    let cursor: AdminUserCursorV1 =
        serde_json::from_slice(&payload).map_err(|_| invalid_user_cursor(locale))?;
    if cursor.version != ADMIN_USER_CURSOR_VERSION
        || cursor.sort != sort.as_str()
        || cursor.filter != filter
        || cursor.last_user_id <= 0
    {
        return Err(invalid_user_cursor(locale));
    }
    match (sort, cursor.last_value) {
        (
            UserSort::UsernameAsc | UserSort::UsernameDesc,
            AdminUserCursorValue::Username { value },
        ) if !value.is_empty() => Ok(prts_db::users::AdminUserAfter::Username {
            value,
            user_id: cursor.last_user_id,
        }),
        (
            UserSort::CreatedAtAsc | UserSort::CreatedAtDesc,
            AdminUserCursorValue::CreatedAt { value },
        ) => DateTime::parse_from_rfc3339(&value)
            .map(|value| prts_db::users::AdminUserAfter::CreatedAt {
                value: value.with_timezone(&Utc),
                user_id: cursor.last_user_id,
            })
            .map_err(|_| invalid_user_cursor(locale)),
        _ => Err(invalid_user_cursor(locale)),
    }
}

fn admin_user_dto(actor: &CurrentUser, user: &User) -> AdminUserDto {
    let target_role = user.platform_role.as_deref().and_then(PlatformRole::parse);
    AdminUserDto {
        id: user.id,
        username: user.username.clone(),
        platform_role: user.platform_role.clone(),
        password_change_required: user.password_change_required,
        created_at: user.created_at.to_rfc3339(),
        capabilities: AdminUserCapabilitiesDto {
            can_change_role: can_manage_platform_user(
                actor.id,
                actor.platform_role,
                Some(user.id),
                target_role,
                target_role,
            ),
        },
    }
}

/// 列出平台用户，支持 literal q、角色过滤、四种稳定排序与签名键集 cursor。
#[utoipa::path(
    get,
    path = "/admin/users",
    tag = "admin",
    params(ListUsersQuery),
    description = "按用户名关键字和平台角色筛选用户，并使用绑定筛选、排序、最后排序值与用户 id 的签名键集游标分页。响应不包含密码、邮箱或全为零的 CP 列。",
    responses(
        (status = 200, description = "用户键集分页", body = AdminUserListResponse),
        (status = 400, description = "筛选、排序、limit 或 cursor 无效", body = ErrorResponse),
        (status = 403, description = "当前用户没有平台用户管理能力", body = ErrorResponse)
    )
)]
pub async fn list_users(
    State(state): State<AppState>,
    user: CurrentUser,
    locale: RequestLocale,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<AdminUserListResponse>, ApiError> {
    user.require_platform(nodes::PLATFORM_USER_MANAGE)
        .map_err(|error| error.with_locale(locale.0))?;
    let (q, role, sort, limit) = normalized_user_filters(&query)
        .map_err(|error| ApiError::from(error).with_locale(locale.0))?;
    let filter = user_filter_fingerprint(q.as_deref(), role.as_deref());
    let after = query
        .after
        .as_deref()
        .map(|cursor| decode_user_cursor(cursor, state.jwt_secret(), sort, &filter, locale))
        .transpose()?;
    let mut rows = prts_db::users::list_admin_users(
        &state.db,
        q.as_deref(),
        role.as_deref(),
        sort.db_sort(),
        after.as_ref(),
        limit + 1,
    )
    .await
    .map_err(db_err)?;
    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    let next_after = if has_more {
        rows.last().map(|last| {
            let last_value = match sort {
                UserSort::UsernameAsc | UserSort::UsernameDesc => AdminUserCursorValue::Username {
                    value: last.username.clone(),
                },
                UserSort::CreatedAtAsc | UserSort::CreatedAtDesc => {
                    AdminUserCursorValue::CreatedAt {
                        value: last.created_at.to_rfc3339(),
                    }
                }
            };
            encode_user_cursor(
                &AdminUserCursorV1 {
                    version: ADMIN_USER_CURSOR_VERSION,
                    sort: sort.as_str().to_string(),
                    filter: filter.clone(),
                    last_value,
                    last_user_id: last.id,
                },
                state.jwt_secret(),
            )
        })
    } else {
        None
    };
    let assignable_roles = assignable_platform_roles(user.platform_role)
        .iter()
        .map(|role| (*role).to_string())
        .collect::<Vec<_>>();
    Ok(Json(AdminUserListResponse {
        items: rows.iter().map(|row| admin_user_dto(&user, row)).collect(),
        next_after,
        capabilities: AdminUserListCapabilitiesDto {
            create_user: !assignable_roles.is_empty(),
            assignable_roles,
        },
    }))
}

/// 创建需首次登录后改密的平台账号；严格平台秩与角色修改使用同一 typed rule。
#[utoipa::path(
    post,
    path = "/admin/users",
    tag = "admin",
    request_body = CreateUserReq,
    description = "创建密码账号并持久设置非阻断的首次改密提醒。初始密码只用于 Argon2id 哈希，绝不进入响应、审计、任务或错误详情。actor 权限与严格平台秩在写事务内重新读取。",
    responses(
        (status = 201, description = "用户已创建", body = AdminUserDto),
        (status = 400, description = "用户名、密码或角色无效", body = ErrorResponse),
        (status = 403, description = "严格平台秩拒绝", body = ErrorResponse),
        (status = 409, description = "用户名已存在", body = ErrorResponse),
        (status = 503, description = "审计不可用，用户创建已回滚", body = ErrorResponse)
    )
)]
pub async fn create_user(
    State(state): State<AppState>,
    user: CurrentUser,
    locale: RequestLocale,
    Json(req): Json<CreateUserReq>,
) -> Result<(StatusCode, Json<AdminUserDto>), ApiError> {
    user.require_platform(nodes::PLATFORM_USER_MANAGE)
        .map_err(|error| error.with_locale(locale.0))?;
    let username = req.username.trim();
    if !(3..=32).contains(&username.chars().count()) {
        return Err(
            ApiError::from(Error::bad_request("用户名长度需为 3–32 字符")).with_locale(locale.0),
        );
    }
    if !prts_auth::password::validate_new_password(&req.initial_password) {
        return Err(
            ApiError::from(Error::bad_request("密码长度需为 8–256 字符")).with_locale(locale.0),
        );
    }
    let requested_role = parse_platform_role(&req.role, true)
        .map_err(|error| ApiError::from(error).with_locale(locale.0))?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let actor = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| ApiError::from(Error::Unauthorized).with_locale(locale.0))?;
    let actor_role = actor.platform_role.as_deref().and_then(PlatformRole::parse);
    if actor.status != "active"
        || !can_manage_platform_user(user.id, actor_role, None, None, requested_role)
    {
        return Err(ApiError::from(Error::Forbidden).with_locale(locale.0));
    }
    let password_hash = prts_auth::password::hash_password(&req.initial_password)
        .map_err(|_| Error::internal("password hash failed"))?;
    let created = prts_db::users::create_admin_password_user_tx(
        &mut tx,
        username,
        &password_hash,
        requested_role.map(PlatformRole::as_str),
    )
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            ApiError::from(Error::Conflict).with_locale(locale.0)
        } else {
            db_err(error)
        }
    })?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UserCreated {
            user_id: created.id,
            username: &created.username,
            role: created.platform_role.as_deref(),
            password_change_required: created.password_change_required,
        },
    )
    .await
    .map_err(|_| ApiError::from(Error::AuditUnavailable).with_locale(locale.0))?;
    tx.commit().await.map_err(db_err)?;
    let actor_identity = CurrentUser {
        id: user.id,
        platform_role: actor_role,
    };
    Ok((
        StatusCode::CREATED,
        Json(admin_user_dto(&actor_identity, &created)),
    ))
}

/// 读取全部平台设置（key → 值）。
#[utoipa::path(get, path = "/admin/settings", tag = "admin",
    responses((status = 200, description = "设置键值对"), (status = 403)))]
pub async fn get_settings(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<serde_json::Value>, ApiError> {
    user.require_platform(nodes::PLATFORM_SETTINGS)?;
    let all = prts_db::settings::list_all(&state.db)
        .await
        .map_err(db_err)?;
    let map: serde_json::Map<String, serde_json::Value> =
        all.into_iter().map(|s| (s.key, s.value)).collect();
    Ok(Json(serde_json::Value::Object(map)))
}

/// 更新设置请求（部分更新）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSettingsReq {
    pub settings: HashMap<String, serde_json::Value>,
}

/// 批量写入平台设置。
#[utoipa::path(put, path = "/admin/settings", tag = "admin", request_body = UpdateSettingsReq,
    responses(
        (status = 204),
        (status = 403),
        (status = 503, description = "审计服务不可用，平台设置未更新", body = ErrorResponse)
    ))]
pub async fn update_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<UpdateSettingsReq>,
) -> Result<StatusCode, ApiError> {
    user.require_platform(nodes::PLATFORM_SETTINGS)?;
    let mut settings: Vec<_> = req.settings.into_iter().collect();
    settings.sort_by(|left, right| left.0.cmp(&right.0));
    if settings
        .iter()
        .any(|(key, _)| matches!(key.as_str(), "search.config" | "upload.config"))
    {
        return Err(Error::bad_request("保留设置必须通过对应的类型化设置端点修改").into());
    }
    let keys: Vec<String> = settings.iter().map(|(key, _)| key.clone()).collect();
    let mut tx = state.db.begin().await.map_err(db_err)?;
    for (key, value) in &settings {
        prts_db::settings::set_tx(&mut tx, key, value, Some(user.id))
            .await
            .map_err(db_err)?;
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::SettingsUpdated { keys: &keys },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 角色任免请求（`role` 为 null 表示降为普通用户）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct GrantRoleReq {
    pub role: Option<String>,
}

/// 按严格平台秩修改平台角色。
#[utoipa::path(post, path = "/admin/users/{id}/role", tag = "admin", request_body = GrantRoleReq,
    description = "在同一事务内锁定并重新读取 actor 与 target，使用 prts-core 严格平台秩规则校验当前秩和请求后秩，再写角色与 typed allowlisted audit。不能修改自己、同级或更高角色。",
    responses(
        (status = 204, description = "角色已更新"),
        (status = 400, description = "角色值无效", body = ErrorResponse),
        (status = 403, description = "严格平台秩拒绝", body = ErrorResponse),
        (status = 404, description = "目标用户不存在", body = ErrorResponse),
        (status = 503, description = "审计服务不可用，平台角色未更新", body = ErrorResponse)
    ))]
pub async fn grant_role(
    State(state): State<AppState>,
    user: CurrentUser,
    locale: RequestLocale,
    Path(id): Path<i64>,
    Json(req): Json<GrantRoleReq>,
) -> Result<StatusCode, ApiError> {
    user.require_platform(nodes::PLATFORM_USER_MANAGE)
        .map_err(|error| error.with_locale(locale.0))?;
    let requested_role = req
        .role
        .as_deref()
        .map(|role| parse_platform_role(role, true))
        .transpose()
        .map_err(|error| ApiError::from(error).with_locale(locale.0))?
        .flatten();
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let actor = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| ApiError::from(Error::Unauthorized).with_locale(locale.0))?;
    let target = if id == actor.id {
        actor.clone()
    } else {
        prts_db::users::find_by_id_for_update_tx(&mut tx, id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| ApiError::from(Error::NotFound).with_locale(locale.0))?
    };
    let actor_role = actor.platform_role.as_deref().and_then(PlatformRole::parse);
    let target_role = target
        .platform_role
        .as_deref()
        .and_then(PlatformRole::parse);
    if actor.status != "active"
        || !can_manage_platform_user(user.id, actor_role, Some(id), target_role, requested_role)
    {
        return Err(ApiError::from(Error::Forbidden).with_locale(locale.0));
    }
    prts_db::users::set_platform_role_tx(&mut tx, id, requested_role.map(PlatformRole::as_str))
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UserPlatformRoleChanged {
            user_id: id,
            previous_role: target.platform_role.as_deref(),
            new_role: requested_role.map(PlatformRole::as_str),
        },
    )
    .await
    .map_err(|_| ApiError::from(Error::AuditUnavailable).with_locale(locale.0))?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}
