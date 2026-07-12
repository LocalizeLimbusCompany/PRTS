//! 平台管理端点：设置与角色任免（受平台权限节点保护）。

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use prts_common::Error;
use prts_core::permission::nodes;
use prts_core::PlatformRole;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::CurrentUser;
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

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

/// 任免平台角色（仅总管理员）。
#[utoipa::path(post, path = "/admin/users/{id}/role", tag = "admin", request_body = GrantRoleReq,
    responses(
        (status = 204),
        (status = 400),
        (status = 403),
        (status = 503, description = "审计服务不可用，平台角色未更新", body = ErrorResponse)
    ))]
pub async fn grant_role(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<GrantRoleReq>,
) -> Result<StatusCode, ApiError> {
    user.require_platform(nodes::PLATFORM_ADMIN_GRANT)?;
    if let Some(role) = req.role.as_deref() {
        if PlatformRole::parse(role).is_none() {
            return Err(Error::bad_request("role 必须是 super_admin|admin|maintainer").into());
        }
    }
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let target = prts_db::users::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    prts_db::users::set_platform_role_tx(&mut tx, id, req.role.as_deref())
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
            new_role: req.role.as_deref(),
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}
