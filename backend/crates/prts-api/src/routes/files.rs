//! 文件夹/文件树端点。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use prts_core::permission::nodes;

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::error::ApiError;
use crate::state::AppState;

/// 文件夹对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct FolderDto {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub path: String,
}

/// 文件对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct FileDto {
    pub id: i64,
    pub folder_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub entry_count: i32,
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

    Ok(Json(ProjectTree {
        folders: folders
            .into_iter()
            .map(|f| FolderDto {
                id: f.id,
                parent_id: f.parent_id,
                name: f.name,
                path: f.path,
            })
            .collect(),
        files: files
            .into_iter()
            .map(|f| FileDto {
                id: f.id,
                folder_id: f.folder_id,
                name: f.name,
                path: f.path,
                entry_count: f.entry_count,
            })
            .collect(),
    }))
}

/// 删除文件（含其词条）。需项目「管理」权限。
#[utoipa::path(delete, path = "/projects/{id}/files/{file_id}", tag = "file",
    responses((status = 204), (status = 403), (status = 404)))]
pub async fn delete_file(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, file_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    if prts_db::files::delete_file(&state.db, id, file_id)
        .await
        .map_err(db_err)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(prts_common::Error::NotFound.into())
    }
}

/// 删除文件夹（含其子项与词条）。需项目「管理」权限。
#[utoipa::path(delete, path = "/projects/{id}/folders/{folder_id}", tag = "file",
    responses((status = 204), (status = 403), (status = 404)))]
pub async fn delete_folder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, folder_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    if prts_db::files::delete_folder(&state.db, id, folder_id)
        .await
        .map_err(db_err)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(prts_common::Error::NotFound.into())
    }
}
