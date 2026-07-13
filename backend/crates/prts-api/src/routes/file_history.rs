//! 文件 change-set 历史列表与服务端物化回滚端点。

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use prts_core::permission::nodes;

use crate::auth::{project as paccess, CurrentUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

use super::files::{apply_and_audit_plan, lock_project_history, map_plan_error, FileOperationDto};

/// 文件历史键集查询。
#[derive(Debug, Deserialize, IntoParams)]
pub struct FileHistoryQuery {
    /// 上一页最后一个 change-set UUID。
    pub after: Option<Uuid>,
    /// 可选文件目标过滤。
    pub file_id: Option<i64>,
    /// 可选文件夹目标过滤。
    pub folder_id: Option<i64>,
    /// 每页 1..=100，默认 50。
    pub limit: Option<i64>,
}

/// 一条 allowlisted change item。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileChangeItemDto {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: Option<i64>,
    pub operation: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub ordinal: i32,
    pub created_at: String,
}

/// 一个文件 change set 及其有序 deltas。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileChangeSetDto {
    pub id: Uuid,
    pub file_id: Option<i64>,
    pub folder_id: Option<i64>,
    pub actor_id: Option<i64>,
    pub operation: String,
    pub path_snapshot: String,
    pub metadata: Value,
    pub created_at: String,
    pub items: Vec<FileChangeItemDto>,
}

/// 文件历史键集响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileHistoryPage {
    pub items: Vec<FileChangeSetDto>,
    pub next_after: Option<Uuid>,
}

/// 项目成员读取文件历史；公开游客不可读取正文 delta。
#[utoipa::path(get, path = "/projects/{id}/file-history", tag = "file-history",
    params(("id" = i64, Path), FileHistoryQuery),
    responses(
        (status = 200, body = FileHistoryPage),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse)
    ))]
pub async fn list_history(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<FileHistoryQuery>,
) -> Result<Json<FileHistoryPage>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_HISTORY_VIEW)?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(prts_common::Error::bad_request("limit must be between 1 and 100").into());
    }
    let records = prts_db::file_history::list_history(
        &state.db,
        id,
        query.after,
        query.file_id,
        query.folder_id,
        limit,
    )
    .await
    .map_err(map_history_db_error)?;
    let next_after = (records.len() as i64 == limit)
        .then(|| records.last().map(|record| record.change_set.id))
        .flatten();
    let items = records
        .into_iter()
        .map(|record| FileChangeSetDto {
            id: record.change_set.id,
            file_id: record.change_set.file_id,
            folder_id: record.change_set.folder_id,
            actor_id: record.change_set.actor_id,
            operation: record.change_set.operation,
            path_snapshot: record.change_set.path_snapshot,
            metadata: record.change_set.metadata,
            created_at: record.change_set.created_at.to_rfc3339(),
            items: record
                .items
                .into_iter()
                .map(|item| FileChangeItemDto {
                    id: item.id,
                    entity_type: item.entity_type,
                    entity_id: item.entity_id_snapshot,
                    operation: item.operation,
                    before: item.before_value,
                    after: item.after_value,
                    ordinal: item.ordinal,
                    created_at: item.created_at.to_rfc3339(),
                })
                .collect(),
        })
        .collect();
    Ok(Json(FileHistoryPage { items, next_after }))
}

/// owner/manager 将文件回滚到指定 change set 之后的物化版本。
#[utoipa::path(post,
    path = "/projects/{id}/files/{file_id}/history/{change_set_id}/rollback",
    tag = "file-history",
    responses(
        (status = 200, body = FileOperationDto),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 409, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    ))]
pub async fn rollback_file(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, file_id, target_change_set_id)): Path<(i64, i64, Uuid)>,
) -> Result<Json<FileOperationDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_HISTORY_ROLLBACK)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_history(&mut tx, &user, id).await?;
    let (current, target) = prts_db::file_history::materialize_file_rollback_tx(
        &mut tx,
        id,
        file_id,
        target_change_set_id,
    )
    .await
    .map_err(map_history_db_error)?;
    if let Some(folder_id) = target.file.folder_id {
        let _target_folder = prts_db::file_history::lock_folder_tx(&mut tx, id, folder_id)
            .await
            .map_err(db_err)?
            .filter(|folder| folder.is_active())
            .ok_or(prts_common::Error::Conflict)?;
    }
    let active_paths = prts_db::file_history::active_paths_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let change_set_id = Uuid::new_v4();
    let plan = prts_core::file_history::plan_file_rollback(
        change_set_id,
        target_change_set_id,
        current,
        target,
        &active_paths,
    )
    .map_err(map_plan_error)?;
    apply_and_audit_plan(&mut tx, id, &user, &plan, None).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(FileOperationDto {
        change_set_id,
        purge_after: None,
    }))
}

/// owner/manager 将文件夹结构回滚到指定 change set 之后的物化版本。
#[utoipa::path(post,
    path = "/projects/{id}/folders/{folder_id}/history/{change_set_id}/rollback",
    tag = "file-history",
    responses(
        (status = 200, body = FileOperationDto),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 409, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    ))]
pub async fn rollback_folder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, folder_id, target_change_set_id)): Path<(i64, i64, Uuid)>,
) -> Result<Json<FileOperationDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_HISTORY_ROLLBACK)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_history(&mut tx, &user, id).await?;
    let (current_root, descendants, files, target_root) =
        prts_db::file_history::materialize_folder_rollback_tx(
            &mut tx,
            id,
            folder_id,
            target_change_set_id,
        )
        .await
        .map_err(map_history_db_error)?;
    let destination = match target_root.parent_id {
        Some(parent_id) => Some(
            prts_db::file_history::lock_folder_tx(&mut tx, id, parent_id)
                .await
                .map_err(db_err)?
                .filter(|folder| folder.is_active())
                .ok_or(prts_common::Error::Conflict)?,
        ),
        None => None,
    };
    let active_paths = prts_db::file_history::active_paths_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let change_set_id = Uuid::new_v4();
    let plan = prts_core::file_history::plan_folder_rollback(
        change_set_id,
        target_change_set_id,
        current_root,
        descendants,
        files,
        target_root,
        destination.as_ref(),
        &active_paths,
    )
    .map_err(map_plan_error)?;
    apply_and_audit_plan(&mut tx, id, &user, &plan, None).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(FileOperationDto {
        change_set_id,
        purge_after: None,
    }))
}

fn map_history_db_error(error: sqlx::Error) -> ApiError {
    if matches!(error, sqlx::Error::RowNotFound) {
        prts_common::Error::NotFound.into()
    } else {
        db_err(error)
    }
}
