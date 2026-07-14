//! 上传、词条 CRUD、历史、项目导出。

use std::collections::{BTreeMap, HashMap, HashSet};
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
    pub translation: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
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
#[deprecated(note = "use the streaming upload-batches API")]
pub async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<UploadReq>,
) -> Result<Json<UploadResult>, ApiError> {
    tracing::info!(
        compatibility_endpoint = "legacy_upload",
        project_id = id,
        "deprecated compatibility endpoint used"
    );
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

    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked_access.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked_access.require_language_ready()?;
    let staged_entries =
        legacy_replacement_entries(req.entries, &locked_access.project.source_langs)?;
    let file = prts_db::files::ensure_file_at_path_tx(&mut tx, id, path)
        .await
        .map_err(db_err)?;
    prts_db::entries::create_replacement_temp_tables_tx(&mut tx)
        .await
        .map_err(db_err)?;
    for chunk in staged_entries.chunks(250) {
        prts_db::entries::stage_replacement_entries_tx(&mut tx, chunk)
            .await
            .map_err(db_err)?;
    }
    if prts_db::entries::finalize_replacement_staging_tx(&mut tx)
        .await
        .map_err(db_err)?
        .is_some()
    {
        return Err(Error::bad_request("upload_duplicate_key").into());
    }
    prts_db::entries::lock_replacement_entries_tx(&mut tx, file.id)
        .await
        .map_err(db_err)?;
    prts_db::entries::declare_replacement_input_cursor_tx(&mut tx, file.id)
        .await
        .map_err(db_err)?;
    let effective_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT transaction_timestamp()")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let mut missing_ordinal = staged_entries.len() as i64;
    let mut summary = prts_core::upload_replacement::ReplacementSummary::default();
    let mut stats_delta = prts_core::upload_replacement::EntryStatsDelta::default();
    loop {
        let page = prts_db::entries::plan_and_stage_replacement_page_tx(
            &mut tx,
            &mut missing_ordinal,
            effective_at,
            500,
        )
        .await
        .map_err(db_err)?;
        if !page.has_rows {
            break;
        }
        summary.inserted += page.plan.summary.inserted;
        summary.restored += page.plan.summary.restored;
        summary.source_changed += page.plan.summary.source_changed;
        summary.tombstoned += page.plan.summary.tombstoned;
        summary.unchanged += page.plan.summary.unchanged;
        stats_delta += page.plan.stats_delta;
    }
    let applied = prts_db::entries::apply_staged_replacement_tx(
        &mut tx,
        id,
        file.id,
        &file.path,
        user.id,
        summary,
        stats_delta,
        effective_at,
    )
    .await
    .map_err(db_err)?;
    let changed_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM prts_upload_replacement_plan")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
    let updated = usize::try_from(changed_count)
        .unwrap_or(usize::MAX)
        .saturating_sub(applied.summary.inserted);
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
            created: applied.summary.inserted,
            updated,
            unchanged: applied.summary.unchanged,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;

    Ok(Json(UploadResult {
        file_id: file.id,
        created: applied.summary.inserted,
        updated,
        unchanged: applied.summary.unchanged,
    }))
}

/// 旧内联端点也必须进入同一 typed replacement 真值；这里只做协议层 canonicalization。
fn legacy_replacement_entries(
    entries: Vec<UploadEntryDto>,
    project_source_langs: &[String],
) -> Result<Vec<prts_db::entries::ReplacementStagedEntry>, ApiError> {
    let source_langs = project_source_langs
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut seen_keys = HashMap::new();
    let mut staged = Vec::with_capacity(entries.len());
    for (ordinal, entry) in entries.into_iter().enumerate() {
        let UploadEntryDto {
            key,
            original: raw_original,
            translation,
            state: raw_state,
        } = entry;
        if key.trim().is_empty() {
            return Err(Error::bad_request("每个词条都需非空 key").into());
        }
        if seen_keys.insert(key.clone(), ordinal).is_some() {
            return Err(Error::bad_request("upload_duplicate_key").into());
        }
        let object = raw_original
            .as_object()
            .ok_or_else(|| Error::bad_request("upload_invalid_entry"))?;
        let mut original = BTreeMap::new();
        for (raw_tag, value) in object {
            let canonical = prts_core::canonicalize_language_tag(raw_tag)
                .map_err(|_| Error::bad_request("upload_invalid_language"))?;
            if !source_langs.contains(canonical.as_str()) {
                return Err(Error::bad_request("upload_source_language_mismatch").into());
            }
            let text = value
                .as_str()
                .ok_or_else(|| Error::bad_request("upload_invalid_entry"))?;
            if original.insert(canonical, text.to_string()).is_some() {
                return Err(Error::bad_request("upload_invalid_language").into());
            }
        }
        let state = match raw_state {
            Some(state) => Some(
                EntryState::parse(&state)
                    .ok_or_else(|| Error::bad_request("upload_invalid_entry"))?,
            ),
            None => None,
        };
        staged.push(prts_db::entries::ReplacementStagedEntry {
            ordinal: ordinal as i64,
            key,
            original,
            translation,
            state,
        });
    }
    Ok(staged)
}

#[cfg(test)]
mod mutation_lock_tests {
    use super::UpdateEntryReq;

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

    #[test]
    fn update_defaults_presence_force_off_and_uses_capabilities_not_role_names() {
        let request: UpdateEntryReq = serde_json::from_value(serde_json::json!({
            "translation": "translated",
            "state": "translated",
            "version": 7
        }))
        .unwrap();
        assert!(!request.force_presence);

        let source = include_str!("entries.rs");
        let update = source
            .rsplit(concat!("pub async fn ", "update_entry("))
            .next()
            .and_then(|section| section.split("/// 设置标志请求").next())
            .expect("entry update section exists");
        assert!(update.contains("force_save_presence"));
        assert!(update.contains("edit_locked_entry"));
        assert!(!update.contains("effective_role"));
        assert!(!update.contains("\"owner\""));
        assert!(!update.contains("\"manager\""));
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
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct EntryListQuery {
    pub file_id: Option<i64>,
    /// 当前任务范围；只按 active task files 限定，不按 baseline entries 限制。
    pub task_id: Option<i64>,
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
    params(("id" = i64, Path), EntryListQuery),
    responses((status = 200, body = [EntryDto]), (status = 404)))]
pub async fn list_entries(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(q): Query<EntryListQuery>,
) -> Result<Json<Vec<EntryDto>>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    if q.file_id.is_some() && q.task_id.is_some() {
        return Err(Error::bad_request("entry_scope_conflict").into());
    }
    if let Some(task_id) = q.task_id {
        prts_db::tasks::find(&state.db, id, task_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
    }

    let states = super::parse_states(q.state.as_deref());

    // 仅当成员可编辑时才允许查看隐藏词条
    let include_hidden = q.include_hidden && access.has_node(nodes::PROJECT_ENTRY_EDIT);

    let filter = prts_db::entries::EntryFilter {
        file_id: q.file_id,
        task_id: q.task_id,
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
    /// 只覆盖客户端检测到的 presence 冲突；绝不绕过 expected version。
    #[serde(default)]
    pub force_presence: bool,
}

/// 更新词条译文与状态。状态变化按目标状态校验 capability；锁定词条与 presence force
/// 分别要求显式 capability；无论是否 force，expected version 冲突都返回 409。
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
    access.require_node(nodes::PROJECT_ENTRY_EDIT)?;
    if req.force_presence && !access.capabilities(false).force_save_presence {
        return Err(Error::Forbidden.into());
    }
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    access.require_node(nodes::PROJECT_ENTRY_EDIT)?;
    if req.force_presence && !access.capabilities(false).force_save_presence {
        return Err(Error::Forbidden.into());
    }
    let entry = prts_db::entries::get_for_update_tx(&mut tx, id, entry_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if entry.locked && !access.capabilities(false).edit_locked_entry {
        return Err(Error::Forbidden.into());
    }
    if entry.state != target.as_str() {
        access.require_node(node_for_state(target))?;
    }
    let kind = if entry.state != target.as_str()
        && matches!(target, EntryState::Checked | EntryState::Reviewed)
    {
        "review"
    } else {
        "edit"
    };
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
                    forced_presence: req.force_presence,
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
