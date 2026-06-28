//! 用户自助端点：个人资料、关联账号、API Key。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;

use crate::auth::CurrentUser;
use crate::db_err;
use crate::dto::UserDto;
use crate::error::ApiError;
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
    responses((status = 200, body = UserDto), (status = 401)))]
pub async fn update_me(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<UpdateMeReq>,
) -> Result<Json<UserDto>, ApiError> {
    let current = prts_db::users::find_by_id(&state.db, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;

    let description = req.description.unwrap_or(current.description);
    let avatar_url = req.avatar_url.or(current.avatar_url);
    let langs = req.translation_langs.unwrap_or(current.translation_langs);

    let updated = prts_db::users::update_profile(
        &state.db,
        user.id,
        &description,
        avatar_url.as_deref(),
        &langs,
    )
    .await
    .map_err(db_err)?;
    Ok(Json((&updated).into()))
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
    responses((status = 200, body = CreatedApiKey), (status = 400)))]
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
    let rec = prts_db::api_keys::create(&state.db, user.id, name, &key.hash, &key.display_prefix)
        .await
        .map_err(db_err)?;
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
    responses((status = 204), (status = 404)))]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if prts_db::api_keys::revoke(&state.db, user.id, id)
        .await
        .map_err(db_err)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::NotFound.into())
    }
}
