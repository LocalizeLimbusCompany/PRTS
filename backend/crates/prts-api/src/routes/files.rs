//! 文件夹/文件树端点。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use prts_core::permission::nodes;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

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

/// 删除文件（含其词条）。需项目「管理」权限。
#[utoipa::path(delete, path = "/projects/{id}/files/{file_id}", tag = "file",
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
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, project)
        .await?
        .require_node(nodes::PROJECT_MANAGE)?;
    let file = prts_db::files::find_file_for_update_tx(&mut tx, id, file_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    if !prts_db::files::delete_file_tx(&mut tx, id, file_id)
        .await
        .map_err(db_err)?
    {
        return Err(prts_common::Error::NotFound.into());
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::FileDeleted {
            project_id: id,
            file_id,
            path: &file.path,
            entry_count: file.entry_count,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 删除文件夹（含其子项与词条）。需项目「管理」权限。
#[utoipa::path(delete, path = "/projects/{id}/folders/{folder_id}", tag = "file",
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
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, project)
        .await?
        .require_node(nodes::PROJECT_MANAGE)?;
    let folder = prts_db::files::find_folder_for_update_tx(&mut tx, id, folder_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let (file_count, entry_count) =
        prts_db::files::folder_tree_counts_tx(&mut tx, id, &folder.path)
            .await
            .map_err(db_err)?;
    if !prts_db::files::delete_folder_tx(&mut tx, id, folder_id)
        .await
        .map_err(db_err)?
    {
        return Err(prts_common::Error::NotFound.into());
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::FolderDeleted {
            project_id: id,
            folder_id,
            path: &folder.path,
            file_count,
            entry_count,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
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
    fn file_and_folder_delete_lock_project_before_child_snapshots() {
        let source = include_str!("files.rs");
        let file_delete = function_body(
            source,
            "pub async fn delete_file(",
            "pub async fn delete_folder(",
        );
        let folder_delete = function_body(source, "pub async fn delete_folder(", "#[cfg(test)]");

        for (body, child_snapshot) in [
            (file_delete, "files::find_file_for_update_tx"),
            (folder_delete, "files::find_folder_for_update_tx"),
        ] {
            let project_lock = body
                .find("projects::find_by_id_for_update_tx")
                .expect("delete must lock its project row");
            let child_lock = body.find(child_snapshot).expect("child snapshot exists");
            assert!(project_lock < child_lock);
        }
    }
}
