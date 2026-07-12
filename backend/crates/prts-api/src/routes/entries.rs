//! 上传、词条 CRUD、历史、项目导出。

use std::collections::HashMap;
use std::io::{Cursor, Write};

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zip::write::SimpleFileOptions;

use prts_common::Error;
use prts_core::permission::{node_for_state, nodes};
use prts_core::EntryState;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

// ============================= 上传 =============================

/// 上传词条项（线上线格式）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UploadEntryDto {
    pub key: String,
    /// `{bcp47: 源文本}`。
    #[serde(default)]
    pub original: serde_json::Value,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub translation: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

impl From<UploadEntryDto> for prts_db::entries::UploadEntry {
    fn from(d: UploadEntryDto) -> Self {
        Self {
            key: d.key,
            original: d.original,
            context: d.context,
            translation: d.translation,
            state: d.state,
        }
    }
}

/// 上传请求：`path` 为文件全路径（如 `a/b/c.json`）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UploadReq {
    pub path: String,
    pub entries: Vec<UploadEntryDto>,
}

/// 上传结果统计。
#[derive(Debug, Serialize, ToSchema)]
pub struct UploadResult {
    pub file_id: i64,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// 上传词条到项目（按路径自动建文件夹/文件）。需项目「上传」权限。
#[utoipa::path(post, path = "/projects/{id}/upload", tag = "entry",
    summary = "旧版内联 JSON 上传（已弃用）",
    description = "兼容旧客户端的已弃用端点；新客户端应使用流式 upload-batches API。",
    request_body = UploadReq,
    responses(
        (status = 200, body = UploadResult),
        (status = 400),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，上传未提交", body = ErrorResponse)
    ))]
pub async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<UploadReq>,
) -> Result<Json<UploadResult>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    access.require_language_ready()?;

    let path = req.path.trim().trim_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Err(Error::bad_request("path 不能为空，且需含文件名").into());
    }
    if req.entries.is_empty() {
        return Err(Error::bad_request("entries 不能为空").into());
    }

    let upload_entries: Vec<prts_db::entries::UploadEntry> =
        req.entries.into_iter().map(Into::into).collect();
    for e in &upload_entries {
        if e.key.trim().is_empty() {
            return Err(Error::bad_request("每个词条都需非空 key").into());
        }
    }

    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked_access.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked_access.require_language_ready()?;
    let file = prts_db::files::ensure_file_at_path_tx(&mut tx, id, path)
        .await
        .map_err(db_err)?;
    let stats =
        prts_db::entries::bulk_upsert_tx(&mut tx, file.id, id, &upload_entries, Some(user.id))
            .await
            .map_err(db_err)?;
    prts_db::files::refresh_entry_count_tx(&mut tx, file.id)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::EntriesUploaded {
            project_id: id,
            file_id: file.id,
            path: &file.path,
            created: stats.created,
            updated: stats.updated,
            unchanged: stats.unchanged,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;

    Ok(Json(UploadResult {
        file_id: file.id,
        created: stats.created,
        updated: stats.updated,
        unchanged: stats.unchanged,
    }))
}

#[cfg(test)]
mod mutation_lock_tests {
    #[test]
    fn upload_locks_project_before_touching_file_tree() {
        let source = include_str!("entries.rs");
        let upload = source
            .split("// ============================= 词条")
            .next()
            .expect("upload section exists");
        let project_lock = upload
            .find("projects::find_by_id_for_update_tx")
            .expect("upload must lock its project row");
        let file_tree_write = upload
            .find("files::ensure_file_at_path_tx")
            .expect("upload file-tree write exists");
        assert!(project_lock < file_tree_write);
    }
}

// ============================= 词条 =============================

/// 词条对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct EntryDto {
    pub id: i64,
    pub file_id: i64,
    pub key: String,
    pub original: serde_json::Value,
    pub context: String,
    pub translation: String,
    pub state: String,
    pub locked: bool,
    pub hidden: bool,
    pub version: i64,
    pub updated_at: String,
}

impl From<&prts_db::models::Entry> for EntryDto {
    fn from(e: &prts_db::models::Entry) -> Self {
        Self {
            id: e.id,
            file_id: e.file_id,
            key: e.key.clone(),
            original: e.original.clone(),
            context: e.context.clone(),
            translation: e.translation.clone(),
            state: e.state.clone(),
            locked: e.locked,
            hidden: e.hidden,
            version: e.version,
            updated_at: e.updated_at.to_rfc3339(),
        }
    }
}

/// 词条列表查询。
#[derive(Debug, Deserialize)]
pub struct EntryListQuery {
    pub file_id: Option<i64>,
    /// 逗号分隔的状态过滤。
    pub state: Option<String>,
    pub q: Option<String>,
    pub after: Option<i64>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub include_hidden: bool,
}

/// 列出词条（键集分页 + 过滤）。
#[utoipa::path(get, path = "/projects/{id}/entries", tag = "entry",
    responses((status = 200, body = [EntryDto]), (status = 404)))]
pub async fn list_entries(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(q): Query<EntryListQuery>,
) -> Result<Json<Vec<EntryDto>>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;

    let states = super::parse_states(q.state.as_deref());

    // 仅当成员可编辑时才允许查看隐藏词条
    let include_hidden = q.include_hidden && access.has_node(nodes::PROJECT_ENTRY_EDIT);

    let filter = prts_db::entries::EntryFilter {
        file_id: q.file_id,
        states,
        query: q.q,
        include_hidden,
    };
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let entries = prts_db::entries::list(&state.db, id, &filter, q.after, limit)
        .await
        .map_err(db_err)?;
    Ok(Json(entries.iter().map(EntryDto::from).collect()))
}

/// 获取单条词条。
#[utoipa::path(get, path = "/projects/{id}/entries/{entry_id}", tag = "entry",
    responses((status = 200, body = EntryDto), (status = 404)))]
pub async fn get_entry(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((id, entry_id)): Path<(i64, i64)>,
) -> Result<Json<EntryDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    let entry = prts_db::entries::get(&state.db, id, entry_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    Ok(Json((&entry).into()))
}

/// 更新词条请求（乐观锁：`version` 须与当前一致）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateEntryReq {
    pub translation: String,
    pub state: String,
    pub version: i64,
}

/// 更新词条译文与状态。按目标状态校验权限；锁定词条仅管理/拥有者可改；版本冲突返回 409。
#[utoipa::path(put, path = "/projects/{id}/entries/{entry_id}", tag = "entry", request_body = UpdateEntryReq,
    responses(
        (status = 200, body = EntryDto),
        (status = 403),
        (status = 404),
        (status = 409),
        (status = 503, description = "审计服务不可用，词条更新未提交", body = ErrorResponse)
    ))]
pub async fn update_entry(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, entry_id)): Path<(i64, i64)>,
    Json(req): Json<UpdateEntryReq>,
) -> Result<Json<EntryDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    let target =
        EntryState::parse(&req.state).ok_or_else(|| Error::bad_request("非法的目标状态"))?;
    access.require_node(node_for_state(target))?;

    let kind = if matches!(target, EntryState::Checked | EntryState::Reviewed) {
        "review"
    } else {
        "edit"
    };
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    access.require_node(node_for_state(target))?;
    let entry = prts_db::entries::get_for_update_tx(&mut tx, id, entry_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if entry.locked
        && !access
            .effective_role()
            .is_some_and(|role| role.can_edit_locked())
    {
        return Err(Error::Forbidden.into());
    }
    let updated = prts_db::entries::update_translation_tx(
        &mut tx,
        entry_id,
        req.version,
        &req.translation,
        target.as_str(),
        kind,
        Some(user.id),
    )
    .await
    .map_err(db_err)?;

    match updated {
        Some(e) => {
            prts_db::audit::append_event_tx(
                &mut tx,
                AuditActor {
                    id: Some(user.id),
                    kind: AuditActorKind::User,
                    ip: None,
                },
                AuditEvent::EntryUpdated {
                    project_id: id,
                    entry_id: e.id,
                    previous_version: entry.version,
                    new_version: e.version,
                    previous_state: &entry.state,
                    new_state: &e.state,
                },
            )
            .await
            .map_err(|_| Error::AuditUnavailable)?;
            tx.commit().await.map_err(db_err)?;
            state
                .realtime
                .publish(
                    id,
                    &prts_realtime::RoomEvent::EntryUpdated {
                        entry_id: e.id,
                        version: e.version,
                        by: user.id,
                    },
                )
                .await;
            Ok(Json((&e).into()))
        }
        None => Err(Error::Conflict.into()), // 版本冲突
    }
}

/// 设置标志请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetFlagsReq {
    pub locked: Option<bool>,
    pub hidden: Option<bool>,
}

/// 设置词条锁定/隐藏。锁定需 `entry.lock`，隐藏需 `entry.hide`。
#[utoipa::path(patch, path = "/projects/{id}/entries/{entry_id}/flags", tag = "entry", request_body = SetFlagsReq,
    responses(
        (status = 200, body = EntryDto),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，词条标志未更新", body = ErrorResponse)
    ))]
pub async fn set_entry_flags(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, entry_id)): Path<(i64, i64)>,
    Json(req): Json<SetFlagsReq>,
) -> Result<Json<EntryDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    if req.locked.is_some() {
        access.require_node(nodes::PROJECT_ENTRY_LOCK)?;
    }
    if req.hidden.is_some() {
        access.require_node(nodes::PROJECT_ENTRY_HIDE)?;
    }
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    if req.locked.is_some() {
        access.require_node(nodes::PROJECT_ENTRY_LOCK)?;
    }
    if req.hidden.is_some() {
        access.require_node(nodes::PROJECT_ENTRY_HIDE)?;
    }
    prts_db::entries::get_for_update_tx(&mut tx, id, entry_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let updated = prts_db::entries::set_flags_tx(&mut tx, id, entry_id, req.locked, req.hidden)
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
        AuditEvent::EntryFlagsUpdated {
            project_id: id,
            entry_id,
            locked: updated.locked,
            hidden: updated.hidden,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state
        .realtime
        .publish(
            id,
            &prts_realtime::RoomEvent::EntryUpdated {
                entry_id: updated.id,
                version: updated.version,
                by: user.id,
            },
        )
        .await;
    Ok(Json((&updated).into()))
}

/// 词条历史项。
#[derive(Debug, Serialize, ToSchema)]
pub struct EntryVersionDto {
    pub version: i64,
    pub kind: String,
    pub translation: Option<String>,
    pub state: Option<String>,
    pub original: Option<serde_json::Value>,
    pub editor_id: Option<i64>,
    pub created_at: String,
}

/// 词条历史。
#[utoipa::path(get, path = "/projects/{id}/entries/{entry_id}/history", tag = "entry",
    responses((status = 200, body = [EntryVersionDto]), (status = 404)))]
pub async fn entry_history(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((id, entry_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<EntryVersionDto>>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    // 确认词条属于该项目
    prts_db::entries::get(&state.db, id, entry_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let versions = prts_db::entries::list_versions(&state.db, entry_id, 200)
        .await
        .map_err(db_err)?;
    Ok(Json(
        versions
            .into_iter()
            .map(|v| EntryVersionDto {
                version: v.version,
                kind: v.kind,
                translation: v.translation,
                state: v.state,
                original: v.original,
                editor_id: v.editor_id,
                created_at: v.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

// ============================= 导出 =============================

/// 下载项目导出包（zip，保留目录结构，每文件含 key/original/translation）。
#[utoipa::path(get, path = "/projects/{id}/export", tag = "entry",
    responses(
        (status = 200, description = "application/zip"),
        (status = 404),
        (status = 503, description = "审计服务不可用，敏感导出未生成", body = ErrorResponse)
    ))]
pub async fn export_project(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;

    let files = prts_db::files::list_files(&state.db, id)
        .await
        .map_err(db_err)?;
    let entries = prts_db::entries::list_for_export(&state.db, id)
        .await
        .map_err(db_err)?;

    // 按文件分组（entries 已按 file_id 排序）
    let mut grouped: HashMap<i64, Vec<&prts_db::models::Entry>> = HashMap::new();
    for e in &entries {
        grouped.entry(e.file_id).or_default().push(e);
    }

    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::<u8>::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for file in &files {
        let arr: Vec<serde_json::Value> = grouped
            .get(&file.id)
            .into_iter()
            .flatten()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "original": e.original,
                    "translation": e.translation,
                })
            })
            .collect();
        let json = serde_json::to_vec_pretty(&arr).map_err(|e| Error::internal(e.to_string()))?;
        zw.start_file(file.path.clone(), opts)
            .map_err(|e| Error::internal(e.to_string()))?;
        zw.write_all(&json)
            .map_err(|e| Error::internal(e.to_string()))?;
    }
    let cursor = zw.finish().map_err(|e| Error::internal(e.to_string()))?;
    let bytes = cursor.into_inner();

    let audit_actor = match user.as_ref() {
        Some(user) => AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        None => AuditActor {
            id: None,
            kind: AuditActorKind::Anonymous,
            ip: None,
        },
    };
    let mut tx = state.db.begin().await.map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor,
        AuditEvent::ProjectExported {
            project_id: id,
            file_count: files.len(),
            entry_count: entries.len(),
            include_hidden: false,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;

    let filename = format!("{}.zip", access.project.slug);
    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ),
    ];
    Ok((StatusCode::OK, headers, bytes).into_response())
}
