//! 用户自助端点：个人资料、关联账号、API Key。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::CurrentUser;
use crate::db_err;
use crate::dto::UserDto;
use crate::error::{ApiError, ErrorResponse, RequestLocale};
use crate::state::AppState;

/// 当前用户资料。
#[utoipa::path(get, path = "/me", tag = "user",
    responses((status = 200, body = UserDto), (status = 401)))]
pub async fn me(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<UserDto>, ApiError> {
    let u = prts_db::users::find_by_id(&state.db, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;
    Ok(Json((&u).into()))
}

/// 更新资料请求（字段缺省表示不变）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMeReq {
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub translation_langs: Option<Vec<String>>,
}

/// 更新当前用户资料。
#[utoipa::path(put, path = "/me", tag = "user", request_body = UpdateMeReq,
    responses(
        (status = 200, body = UserDto),
        (status = 401),
        (status = 503, description = "审计服务不可用，资料更新未提交", body = ErrorResponse)
    ))]
pub async fn update_me(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<UpdateMeReq>,
) -> Result<Json<UserDto>, ApiError> {
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let current = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;

    let description = req
        .description
        .unwrap_or_else(|| current.description.clone());
    let avatar_url = req.avatar_url.or_else(|| current.avatar_url.clone());
    let requested_langs = req
        .translation_langs
        .unwrap_or_else(|| current.translation_langs.clone());
    let langs =
        prts_core::canonicalize_language_tags(&requested_langs).map_err(|error| match error {
            prts_core::LanguageTagError::Invalid => Error::InvalidLanguageTag,
            prts_core::LanguageTagError::Duplicate => Error::DuplicateLanguageTag,
            _ => Error::bad_request(error.code()),
        })?;
    let mut changed_fields = Vec::with_capacity(3);
    if description != current.description {
        changed_fields.push("description");
    }
    if avatar_url != current.avatar_url {
        changed_fields.push("avatar_url");
    }
    if langs != current.translation_langs {
        changed_fields.push("translation_langs");
    }

    let updated = prts_db::users::update_profile_tx(
        &mut tx,
        user.id,
        &description,
        avatar_url.as_deref(),
        &langs,
    )
    .await
    .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UserProfileUpdated {
            user_id: user.id,
            changed_fields: &changed_fields,
            translation_lang_count: langs.len(),
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json((&updated).into()))
}

/// 修改密码请求。明文只参与验证和 Argon2id 哈希，不进入任何通用载荷。
#[derive(Deserialize, ToSchema)]
pub struct ChangePasswordReq {
    pub current_password: String,
    pub new_password: String,
}

/// 修改当前密码并持久清除首次改密提醒。
#[utoipa::path(
    put,
    path = "/me/password",
    tag = "user",
    request_body = ChangePasswordReq,
    description = "在同一事务内锁定并重新验证当前 active 用户及现有密码，替换 Argon2id 哈希、清除非阻断改密提醒并写脱敏 typed audit。密码、哈希和数据库错误均不进入响应或审计。",
    responses(
        (status = 204, description = "密码已修改且提醒已清除"),
        (status = 400, description = "新密码不符合长度要求", body = ErrorResponse),
        (status = 401, description = "当前密码不正确或账号不支持密码", body = ErrorResponse),
        (status = 403, description = "账号不再 active", body = ErrorResponse),
        (status = 503, description = "审计不可用，密码修改已回滚", body = ErrorResponse)
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    locale: RequestLocale,
    Json(req): Json<ChangePasswordReq>,
) -> Result<StatusCode, ApiError> {
    if !prts_auth::password::validate_new_password(&req.new_password) {
        return Err(
            ApiError::from(Error::bad_request("密码长度需为 8–256 字符")).with_locale(locale.0),
        );
    }
    let new_hash = prts_auth::password::hash_password(&req.new_password)
        .map_err(|_| Error::internal("password hash failed"))?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let current = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| ApiError::from(Error::Unauthorized).with_locale(locale.0))?;
    if current.status != "active" {
        return Err(ApiError::from(Error::Forbidden).with_locale(locale.0));
    }
    let current_matches = current
        .password_hash
        .as_deref()
        .is_some_and(|hash| prts_auth::password::verify_password(&req.current_password, hash));
    if !current_matches {
        return Err(ApiError::from(Error::Unauthorized).with_locale(locale.0));
    }
    let reminder_cleared = current.password_change_required;
    prts_db::users::update_password_tx(&mut tx, user.id, &new_hash)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UserPasswordChanged {
            user_id: user.id,
            password_change_required_cleared: reminder_cleared,
        },
    )
    .await
    .map_err(|_| ApiError::from(Error::AuditUnavailable).with_locale(locale.0))?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 公开用户资料（**不含 email**）。
///
/// 供私信会话页展示对话方头名/头像等。资料本身对已认证用户公开；
/// 出于红线 §8「GET /users/{id} 不下发 email」，返回前显式清空 email。
#[utoipa::path(get, path = "/users/{id}", tag = "user",
    params(("id" = i64, Path, description = "用户 id")),
    responses((status = 200, body = UserDto), (status = 404)))]
pub async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<UserDto>, ApiError> {
    let u = prts_db::users::find_by_id(&state.db, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let mut dto: UserDto = (&u).into();
    dto.email = None; // 公开资料不下发 email。
    Ok(Json(dto))
}

/// 关联账号对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct ExternalAccountDto {
    pub provider: String,
    pub external_id: String,
    pub created_at: String,
}

/// 列出当前用户的关联账号。
#[utoipa::path(get, path = "/me/accounts", tag = "user",
    responses((status = 200, body = [ExternalAccountDto])))]
pub async fn my_accounts(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<ExternalAccountDto>>, ApiError> {
    let list = prts_db::users::list_external_accounts(&state.db, user.id)
        .await
        .map_err(db_err)?;
    Ok(Json(
        list.iter()
            .map(|a| ExternalAccountDto {
                provider: a.provider.clone(),
                external_id: a.external_id.clone(),
                created_at: a.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

/// API Key 对外表示（不含明文）。
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeyDto {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

/// 创建 API Key 请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyReq {
    pub name: String,
}

/// 新建 API Key 响应（含一次性明文）。
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedApiKey {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    /// 明文 Key，**仅此一次返回**，请立即保存。
    pub key: String,
}

/// 创建 API Key。
#[utoipa::path(post, path = "/me/api-keys", tag = "user", request_body = CreateApiKeyReq,
    responses(
        (status = 200, body = CreatedApiKey),
        (status = 400),
        (status = 503, description = "审计服务不可用，API Key 未创建", body = ErrorResponse)
    ))]
pub async fn create_api_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateApiKeyReq>,
) -> Result<Json<CreatedApiKey>, ApiError> {
    let name = req.name.trim();
    if name.is_empty() || name.len() > 64 {
        return Err(Error::bad_request("名称需为 1–64 字符").into());
    }
    let key = prts_auth::token::generate_api_key();
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let rec = prts_db::api_keys::create_tx(&mut tx, user.id, name, &key.hash, &key.display_prefix)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ApiKeyCreated {
            key_id: rec.id,
            name: &rec.name,
            prefix: &rec.prefix,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(CreatedApiKey {
        id: rec.id,
        name: rec.name,
        prefix: rec.prefix,
        created_at: rec.created_at.to_rfc3339(),
        key: key.plaintext,
    }))
}

/// 列出当前用户的 API Key。
#[utoipa::path(get, path = "/me/api-keys", tag = "user",
    responses((status = 200, body = [ApiKeyDto])))]
pub async fn list_api_keys(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<ApiKeyDto>>, ApiError> {
    let list = prts_db::api_keys::list_by_user(&state.db, user.id)
        .await
        .map_err(db_err)?;
    Ok(Json(
        list.iter()
            .map(|k| ApiKeyDto {
                id: k.id,
                name: k.name.clone(),
                prefix: k.prefix.clone(),
                created_at: k.created_at.to_rfc3339(),
                last_used_at: k.last_used_at.map(|t| t.to_rfc3339()),
            })
            .collect(),
    ))
}

/// 吊销一条 API Key。
#[utoipa::path(delete, path = "/me/api-keys/{id}", tag = "user",
    responses(
        (status = 204),
        (status = 404),
        (status = 503, description = "审计服务不可用，API Key 未吊销", body = ErrorResponse)
    ))]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let record = prts_db::api_keys::revoke_tx(&mut tx, user.id, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ApiKeyRevoked {
            key_id: record.id,
            prefix: &record.prefix,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}
