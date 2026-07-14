//! 文件夹/文件树端点。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use prts_core::file_history::{
    self, FileHistoryMutation, FileHistoryOperation, FileHistoryPlanError, FileHistoryTarget,
    DEFAULT_RETENTION_DAYS,
};
use prts_core::permission::nodes;
use prts_db::audit::{
    AuditActor, AuditActorKind, AuditEvent, FileHistoryAuditOperation, FileHistoryAuditTarget,
};

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

/// 文件夹对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct FolderDto {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub created_at: String,
}

/// 文件对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileDto {
    pub id: i64,
    pub folder_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub entry_count: i32,
    pub state_counts: std::collections::HashMap<String, i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// 项目文件树。
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectTree {
    pub folders: Vec<FolderDto>,
    pub files: Vec<FileDto>,
}

/// 新建文件夹请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateFolderRequest {
    /// 父文件夹；`null` 表示项目根。
    pub parent_id: Option<i64>,
    /// 单个安全路径段。
    pub name: String,
}

/// 文件移动/重命名请求；提交完整期望结构。
#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveFileRequest {
    /// 目标文件夹；`null` 表示项目根。
    pub folder_id: Option<i64>,
    /// 目标名称。
    pub name: String,
}

/// 文件夹移动/重命名请求；提交完整期望结构。
#[derive(Debug, Deserialize, ToSchema)]
pub struct MoveFolderRequest {
    /// 目标父文件夹；`null` 表示项目根。
    pub parent_id: Option<i64>,
    /// 目标名称。
    pub name: String,
}

/// 恢复请求必须显式绑定原删除 operation。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RestoreRequest {
    /// 原删除 change-set id；只清除此 operation 持有的删除字段。
    pub deletion_change_set_id: Uuid,
}

/// 文件历史 mutation 的稳定结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileOperationDto {
    /// 本次新建的 change-set id。
    pub change_set_id: Uuid,
    /// 删除 operation 的到期清除时间；其它操作为 `null`。
    pub purge_after: Option<String>,
}

/// 获取项目文件树。
#[utoipa::path(get, path = "/projects/{id}/tree", tag = "file",
    responses((status = 200, body = ProjectTree), (status = 404)))]
pub async fn get_tree(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
) -> Result<Json<ProjectTree>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;

    let folders = prts_db::files::list_folders(&state.db, id)
        .await
        .map_err(db_err)?;
    let files = prts_db::files::list_files(&state.db, id)
        .await
        .map_err(db_err)?;
    let file_stats = prts_db::stats::files(&state.db, id)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|stats| (stats.file_id, stats))
        .collect::<std::collections::HashMap<_, _>>();

    Ok(Json(ProjectTree {
        folders: folders
            .into_iter()
            .map(|f| FolderDto {
                id: f.id,
                parent_id: f.parent_id,
                name: f.name,
                path: f.path,
                created_at: f.created_at.to_rfc3339(),
            })
            .collect(),
        files: files
            .into_iter()
            .map(|f| {
                let stats = file_stats.get(&f.id);
                let mut state_counts = std::collections::HashMap::new();
                if let Some(stats) = stats {
                    state_counts.insert("untranslated".to_string(), stats.untranslated_count);
                    state_counts.insert("translated".to_string(), stats.translated_count);
                    state_counts.insert("questioned".to_string(), stats.questioned_count);
                    state_counts.insert("checked".to_string(), stats.checked_count);
                    state_counts.insert("reviewed".to_string(), stats.reviewed_count);
                }
                FileDto {
                    id: f.id,
                    folder_id: f.folder_id,
                    name: f.name,
                    path: f.path,
                    entry_count: stats.map_or(0, |value| value.visible_total as i32),
                    state_counts,
                    created_at: f.created_at.to_rfc3339(),
                    updated_at: f.updated_at.to_rfc3339(),
                }
            })
            .collect(),
    }))
}

/// 新建文件夹。需项目「管理」权限。
#[utoipa::path(post, path = "/projects/{id}/folders", tag = "file",
    description = "项目管理 capability 可在 active parent 下创建安全单段文件夹；服务端锁定项目与 parent、检查规范路径冲突，并将业务写与 allowlisted audit 原子提交。",
    request_body = CreateFolderRequest,
    responses(
        (status = 201, body = FolderDto), (status = 400), (status = 403), (status = 409),
        (status = 503, description = "审计服务不可用，文件夹未创建", body = ErrorResponse)
    ))]
pub async fn create_folder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(request): Json<CreateFolderRequest>,
) -> Result<(StatusCode, Json<FolderDto>), ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, project)
        .await?
        .require_node(nodes::PROJECT_MANAGE)?;
    let parent = match request.parent_id {
        Some(parent_id) => Some(
            prts_db::file_history::lock_folder_tx(&mut tx, id, parent_id)
                .await
                .map_err(db_err)?
                .filter(|folder| folder.is_active())
                .ok_or(prts_common::Error::NotFound)?,
        ),
        None => None,
    };
    let path = file_history::child_path(
        parent.as_ref().map(|folder| folder.path.as_str()),
        &request.name,
    )
    .map_err(map_plan_error)?;
    let active_paths = prts_db::file_history::active_paths_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    if active_paths.folders.contains(&path) {
        return Err(prts_common::Error::Conflict.into());
    }
    let folder = prts_db::file_history::create_folder_tx(
        &mut tx,
        id,
        parent.as_ref().map(|folder| folder.id),
        &request.name,
        &path,
    )
    .await
    .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::FileHistoryChanged {
            project_id: id,
            target: FileHistoryAuditTarget::Folder,
            target_id: folder.id,
            operation: FileHistoryAuditOperation::Create,
            change_set_id: None,
            source_change_set_id: None,
            path: &path,
            affected_folders: 1,
            affected_files: 0,
            affected_entries: 0,
            purge_after: None,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok((
        StatusCode::CREATED,
        Json(FolderDto {
            id: folder.id,
            parent_id: folder.parent_id,
            name: folder.name,
            path: folder.path,
            created_at: folder.created_at.to_rfc3339(),
        }),
    ))
}

/// 移动或重命名文件。需项目「管理」权限。
#[utoipa::path(patch, path = "/projects/{id}/files/{file_id}", tag = "file",
    description = "项目管理 capability 可移动或重命名 active file；目标 folder 必须绑定同一 URL 项目，typed plan 检查规范路径冲突并生成可审计 change set。",
    request_body = MoveFileRequest,
    responses(
        (status = 200, body = FileOperationDto), (status = 400), (status = 403),
        (status = 404), (status = 409),
        (status = 503, description = "审计服务不可用，文件未移动", body = ErrorResponse)
    ))]
pub async fn move_file(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, file_id)): Path<(i64, i64)>,
    Json(request): Json<MoveFileRequest>,
) -> Result<Json<FileOperationDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_manage(&mut tx, &user, id).await?;
    let file = prts_db::file_history::lock_file_tx(&mut tx, id, file_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let destination = match request.folder_id {
        Some(folder_id) => Some(
            prts_db::file_history::lock_folder_tx(&mut tx, id, folder_id)
                .await
                .map_err(db_err)?
                .ok_or(prts_common::Error::NotFound)?,
        ),
        None => None,
    };
    let active_paths = prts_db::file_history::active_paths_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let change_set_id = Uuid::new_v4();
    let plan = file_history::plan_file_move(
        change_set_id,
        file,
        destination.as_ref(),
        &request.name,
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

/// 移动或重命名文件夹及其完整后代路径。需项目「管理」权限。
#[utoipa::path(patch, path = "/projects/{id}/folders/{folder_id}", tag = "file",
    description = "项目管理 capability 可移动或重命名 active folder subtree；typed plan 拒绝环和路径冲突，并原子更新所有后代路径、change set、统计与审计。",
    request_body = MoveFolderRequest,
    responses(
        (status = 200, body = FileOperationDto), (status = 400), (status = 403),
        (status = 404), (status = 409),
        (status = 503, description = "审计服务不可用，文件夹未移动", body = ErrorResponse)
    ))]
pub async fn move_folder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, folder_id)): Path<(i64, i64)>,
    Json(request): Json<MoveFolderRequest>,
) -> Result<Json<FileOperationDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_manage(&mut tx, &user, id).await?;
    let root = prts_db::file_history::lock_folder_tx(&mut tx, id, folder_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let (folders, files) = prts_db::file_history::lock_folder_subtree_tx(&mut tx, id, root.id)
        .await
        .map_err(db_err)?;
    let destination = match request.parent_id {
        Some(parent_id) => Some(
            prts_db::file_history::lock_folder_tx(&mut tx, id, parent_id)
                .await
                .map_err(db_err)?
                .ok_or(prts_common::Error::NotFound)?,
        ),
        None => None,
    };
    let active_paths = prts_db::file_history::active_paths_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let descendants = folders
        .into_iter()
        .filter(|folder| folder.id != root.id)
        .collect();
    let change_set_id = Uuid::new_v4();
    let plan = file_history::plan_folder_move(
        change_set_id,
        root,
        descendants,
        files,
        destination.as_ref(),
        &request.name,
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

/// 软删除文件，不改 entry tombstone。需项目「管理」权限。
#[utoipa::path(delete, path = "/projects/{id}/files/{file_id}", tag = "file",
    description = "项目管理 capability 软删除同一项目的 active file，创建 30 天 restoration change set 并扣除物化 exposure；独立 entry tombstone 保持不变。",
    responses(
        (status = 204),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，文件未删除", body = ErrorResponse)
    ))]
pub async fn delete_file(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, file_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_manage(&mut tx, &user, id).await?;
    let file = prts_db::file_history::lock_file_tx(&mut tx, id, file_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let change_set_id = Uuid::new_v4();
    let plan = file_history::plan_file_delete(change_set_id, file).map_err(map_plan_error)?;
    let purge_after = Utc::now() + Duration::days(DEFAULT_RETENTION_DAYS);
    apply_and_audit_plan(&mut tx, id, &user, &plan, Some(purge_after)).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 软删除文件夹 active subtree 与 active descendant files，不改 entry tombstone。
#[utoipa::path(delete, path = "/projects/{id}/folders/{folder_id}", tag = "file",
    description = "项目管理 capability 以一个 operation 软删除 active folder subtree 与 active descendant files，维护物化 exposure，但不写入或清除独立 entry tombstone。",
    responses(
        (status = 204),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，文件夹未删除", body = ErrorResponse)
    ))]
pub async fn delete_folder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, folder_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_manage(&mut tx, &user, id).await?;
    let folder = prts_db::file_history::lock_folder_tx(&mut tx, id, folder_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let (folders, files) = prts_db::file_history::lock_folder_subtree_tx(&mut tx, id, folder.id)
        .await
        .map_err(db_err)?;
    let change_set_id = Uuid::new_v4();
    let plan = file_history::plan_folder_delete(change_set_id, folder_id, folders, files)
        .map_err(map_plan_error)?;
    let purge_after = Utc::now() + Duration::days(DEFAULT_RETENTION_DAYS);
    apply_and_audit_plan(&mut tx, id, &user, &plan, Some(purge_after)).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 恢复一个明确删除 operation 持有的文件。
#[utoipa::path(post, path = "/projects/{id}/files/{file_id}/restore", tag = "file",
    description = "项目历史回滚 capability 必须提交原 deletion change-set ID；恢复只清除该 operation 持有的 file 删除字段，拒绝路径冲突且不触碰 entry tombstone。",
    request_body = RestoreRequest,
    responses(
        (status = 200, body = FileOperationDto), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "审计服务不可用，文件未恢复", body = ErrorResponse)
    ))]
pub async fn restore_file(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, file_id)): Path<(i64, i64)>,
    Json(request): Json<RestoreRequest>,
) -> Result<Json<FileOperationDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_HISTORY_ROLLBACK)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_history(&mut tx, &user, id).await?;
    let file = prts_db::file_history::lock_file_tx(&mut tx, id, file_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let folders = prts_db::file_history::lock_all_folders_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let active_paths = prts_db::file_history::active_paths_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let change_set_id = Uuid::new_v4();
    let plan = file_history::plan_file_restore(
        change_set_id,
        request.deletion_change_set_id,
        file,
        &folders,
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

/// 恢复一个明确删除 operation 持有的文件夹树。
#[utoipa::path(post, path = "/projects/{id}/folders/{folder_id}/restore", tag = "file",
    description = "项目历史回滚 capability 必须提交原 deletion change-set ID；恢复只清除该 operation 持有的 folder/file subtree 标记，早先删除后代和 entry tombstone 保持删除。",
    request_body = RestoreRequest,
    responses(
        (status = 200, body = FileOperationDto), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "审计服务不可用，文件夹未恢复", body = ErrorResponse)
    ))]
pub async fn restore_folder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, folder_id)): Path<(i64, i64)>,
    Json(request): Json<RestoreRequest>,
) -> Result<Json<FileOperationDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_HISTORY_ROLLBACK)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_history(&mut tx, &user, id).await?;
    let folder = prts_db::file_history::lock_folder_tx(&mut tx, id, folder_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let all_folders = prts_db::file_history::lock_all_folders_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let (_, files) = prts_db::file_history::lock_folder_subtree_tx(&mut tx, id, folder.id)
        .await
        .map_err(db_err)?;
    let active_paths = prts_db::file_history::active_paths_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let change_set_id = Uuid::new_v4();
    let plan = file_history::plan_folder_restore(
        change_set_id,
        request.deletion_change_set_id,
        folder_id,
        all_folders,
        files,
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

async fn lock_project_manage(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: &CurrentUser,
    project_id: i64,
) -> Result<(), ApiError> {
    let project = prts_db::projects::find_by_id_for_update_tx(tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    paccess::load_locked_tx(tx, user, project)
        .await?
        .require_node(nodes::PROJECT_MANAGE)
}

pub(super) async fn lock_project_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: &CurrentUser,
    project_id: i64,
) -> Result<(), ApiError> {
    let project = prts_db::projects::find_by_id_for_update_tx(tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    paccess::load_locked_tx(tx, user, project)
        .await?
        .require_node(nodes::PROJECT_HISTORY_ROLLBACK)
}

pub(super) async fn apply_and_audit_plan(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: i64,
    user: &CurrentUser,
    plan: &prts_core::file_history::FileHistoryPlan,
    purge_after: Option<chrono::DateTime<Utc>>,
) -> Result<(), ApiError> {
    let effective_at = Utc::now();
    prts_db::file_history::apply_plan_tx(
        tx,
        project_id,
        user.id,
        effective_at,
        purge_after.unwrap_or(effective_at),
        plan,
    )
    .await
    .map_err(db_err)?;
    let (target, target_id) = match plan.target {
        FileHistoryTarget::File(id) => (FileHistoryAuditTarget::File, id),
        FileHistoryTarget::Folder(id) => (FileHistoryAuditTarget::Folder, id),
    };
    let operation = match plan.operation {
        FileHistoryOperation::Move => FileHistoryAuditOperation::Move,
        FileHistoryOperation::Rename => FileHistoryAuditOperation::Rename,
        FileHistoryOperation::Delete => FileHistoryAuditOperation::Delete,
        FileHistoryOperation::Restore => FileHistoryAuditOperation::Restore,
        FileHistoryOperation::Rollback => FileHistoryAuditOperation::Rollback,
    };
    let (affected_folders, affected_files, mut affected_entries) =
        plan.mutations
            .iter()
            .fold((0, 0, 0), |mut counts, mutation| {
                match mutation {
                    FileHistoryMutation::UpdateFolderStructure { .. }
                    | FileHistoryMutation::DeleteFolder { .. }
                    | FileHistoryMutation::RestoreFolder { .. } => counts.0 += 1,
                    FileHistoryMutation::UpdateFileStructure { .. }
                    | FileHistoryMutation::DeleteFile { .. }
                    | FileHistoryMutation::RestoreFile { .. } => counts.1 += 1,
                    FileHistoryMutation::ReplaceEntry { .. } => counts.2 += 1,
                }
                counts
            });
    if affected_entries == 0
        && matches!(
            plan.operation,
            FileHistoryOperation::Delete | FileHistoryOperation::Restore
        )
    {
        affected_entries = usize::try_from(plan.project_stats_delta.visible_total.unsigned_abs())
            .unwrap_or(usize::MAX);
    }
    prts_db::audit::append_event_tx(
        tx,
        audit_actor(user),
        AuditEvent::FileHistoryChanged {
            project_id,
            target,
            target_id,
            operation,
            change_set_id: Some(plan.change_set_id),
            source_change_set_id: plan.source_change_set_id,
            path: &plan.path_snapshot,
            affected_folders,
            affected_files,
            affected_entries,
            purge_after,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    Ok(())
}

fn audit_actor(user: &CurrentUser) -> AuditActor<'static> {
    AuditActor {
        id: Some(user.id),
        kind: AuditActorKind::User,
        ip: None,
    }
}

pub(super) fn map_plan_error(error: FileHistoryPlanError) -> ApiError {
    use FileHistoryPlanError::*;
    match error {
        InvalidName | MoveIntoDescendant | InvalidTree | DuplicateEntity { .. } => {
            prts_common::Error::bad_request("invalid file tree operation").into()
        }
        TargetDeleted | DestinationDeleted | MissingTarget | MissingRollbackEntry { .. } => {
            prts_common::Error::NotFound.into()
        }
        PathConflict { .. }
        | NoChange
        | AncestorDeleted
        | OperationNotOwned { .. }
        | RollbackTargetMismatch
        | RollbackTargetDeleted => prts_common::Error::Conflict.into(),
    }
}

#[cfg(test)]
mod mutation_lock_tests {
    fn function_body<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        source
            .split_once(start)
            .expect("function start exists")
            .1
            .split_once(end)
            .expect("function end exists")
            .0
    }

    #[test]
    fn file_and_folder_delete_lock_project_before_typed_tree_snapshots() {
        let source = include_str!("files.rs");
        let file_delete = function_body(
            source,
            "pub async fn delete_file(",
            "pub async fn delete_folder(",
        );
        let folder_delete = function_body(source, "pub async fn delete_folder(", "#[cfg(test)]");

        for (body, child_snapshot) in [
            (file_delete, "file_history::lock_file_tx"),
            (folder_delete, "file_history::lock_folder_tx"),
        ] {
            let project_lock = body
                .find("lock_project_manage")
                .expect("delete must lock its project row");
            let child_lock = body.find(child_snapshot).expect("child snapshot exists");
            assert!(project_lock < child_lock);
        }
    }
}
