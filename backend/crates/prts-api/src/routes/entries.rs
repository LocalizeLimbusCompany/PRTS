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
use prts_core::contribution::{calculate_contribution, ContributionKind};
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
    /// 独立于工作流的有疑问标签。
    pub questioned: bool,
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
            questioned: e.questioned,
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
    /// 独立的有疑问标签过滤；缺省时不过滤。
    pub questioned: Option<bool>,
    pub q: Option<String>,
    pub after: Option<i64>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub include_hidden: bool,
}

/// 编辑器浏览模式的物化总数。
#[derive(Debug, Serialize, ToSchema)]
pub struct EntryCountDto {
    pub total_items: i64,
}

/// 从 project_stats/file_stats/task file_stats 读取精确总数，不扫描 entries 热表。
#[utoipa::path(
    get,
    path = "/projects/{id}/entries/count",
    tag = "entry",
    params(("id" = i64, Path), EntryListQuery),
    description = "返回编辑器普通浏览模式的精确词条总数。项目、文件、任务、状态与授权后的 include_hidden 都只读取物化统计；不执行热路径 COUNT(entries)。",
    responses((status = 200, body = EntryCountDto), (status = 400, body = ErrorResponse), (status = 404, body = ErrorResponse))
)]
pub async fn count_entries(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(query): Query<EntryListQuery>,
) -> Result<Json<EntryCountDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    if query.file_id.is_some() && query.task_id.is_some() {
        return Err(Error::bad_request("entry_scope_conflict").into());
    }
    if query.include_hidden && !access.has_node(nodes::PROJECT_ENTRY_HIDE) {
        return Err(Error::Forbidden.into());
    }
    if let Some(file_id) = query.file_id {
        prts_db::search::resolve_active_file_id(&state.db, id, file_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
    }
    if let Some(task_id) = query.task_id {
        prts_db::tasks::find(&state.db, id, task_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
    }
    let states = super::parse_states(query.state.as_deref());
    if query.questioned.is_some() && !states.is_empty() {
        return Err(Error::bad_request("entry_state_and_questioned_count_conflict").into());
    }
    let total_items = prts_db::stats::editor_entry_total(
        &state.db,
        id,
        query.file_id,
        query.task_id,
        &states,
        query.questioned,
        query.include_hidden,
    )
    .await
    .map_err(db_err)?;
    Ok(Json(EntryCountDto { total_items }))
}

/// 列出词条（键集分页 + 过滤）。
#[utoipa::path(get, path = "/projects/{id}/entries", tag = "entry",
    params(("id" = i64, Path), EntryListQuery),
    description = "List editor entries in ascending id order using keyset pagination. file_id and task_id are mutually exclusive; effective visibility is enforced and include_hidden is honored only for authorized collaborators. limit is bounded to 1..=500.",
    responses((status = 200, body = [EntryDto]), (status = 400, body = ErrorResponse), (status = 404, body = ErrorResponse)))]
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
    if let Some(file_id) = q.file_id {
        prts_db::search::resolve_active_file_id(&state.db, id, file_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
    }
    if let Some(task_id) = q.task_id {
        prts_db::tasks::find(&state.db, id, task_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
    }

    let states = super::parse_states(q.state.as_deref());

    // include_hidden 是 owner/manager 的明确管理视图，不随普通编辑权限下发。
    let include_hidden = q.include_hidden && access.has_node(nodes::PROJECT_ENTRY_HIDE);

    let filter = prts_db::entries::EntryFilter {
        file_id: q.file_id,
        task_id: q.task_id,
        states,
        questioned: q.questioned,
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
    description = "Read one entry bound to the URL project. Public projects are readable by guests; private-project existence is not disclosed to unauthorized callers.",
    responses((status = 200, body = EntryDto), (status = 404, body = ErrorResponse)))]
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
    /// 同次保存设置或取消有疑问标签；缺省保留当前值。
    pub questioned: Option<bool>,
    /// 设置有疑问标签时可同时发布为评论；空白内容等同未提供。
    pub question_reason: Option<String>,
}

/// 更新词条译文与状态。状态变化按目标状态校验 capability；锁定词条与 presence force
/// 分别要求显式 capability；无论是否 force，expected version 冲突都返回 409。
#[utoipa::path(put, path = "/projects/{id}/entries/{entry_id}", tag = "entry", request_body = UpdateEntryReq,
    description = "Atomically save translation, workflow state and the independent questioned tag with optimistic version checking. locked entries and force_presence require their explicit capabilities; force never bypasses a stale version. When questioned=true is submitted, an optional Markdown reason up to 4000 characters can be created as a comment in the same transaction.",
    responses(
        (status = 200, body = EntryDto),
        (status = 400, description = "Invalid state or question reason", body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 409, description = "Optimistic version conflict", body = ErrorResponse),
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
    let question_reason = req
        .question_reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    if question_reason.is_some_and(|reason| reason.chars().count() > 4000) {
        return Err(Error::bad_request("invalid_comment_content").into());
    }
    if question_reason.is_some() && req.questioned != Some(true) {
        return Err(Error::bad_request("question_reason_requires_questioned_tag").into());
    }
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
    let contribution_kind = ContributionKind::for_target_state(target);
    let kind = contribution_kind.as_str();
    let contribution =
        calculate_contribution(&entry.translation, &req.translation, contribution_kind)
            .map_err(|_| Error::internal("contribution score overflow"))?;
    let updated = prts_db::entries::update_translation_tx(
        &mut tx,
        entry_id,
        req.version,
        &req.translation,
        target.as_str(),
        req.questioned,
        kind,
        Some(user.id),
    )
    .await
    .map_err(db_err)?;

    match updated {
        Some(e) => {
            prts_db::contributions::award_tx(&mut tx, user.id, id, e.id, e.version, contribution)
                .await
                .map_err(db_err)?;
            let question_comment = if let Some(reason) = question_reason {
                let actor = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
                    .await
                    .map_err(db_err)?
                    .ok_or(Error::Unauthorized)?;
                let comment = prts_db::comments::create_tx(
                    &mut tx,
                    id,
                    e.id,
                    user.id,
                    &actor.username,
                    actor.avatar_url.as_deref(),
                    reason,
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
                    AuditEvent::EntryCommentCreated {
                        project_id: id,
                        entry_id: e.id,
                        comment_id: comment.id,
                    },
                )
                .await
                .map_err(|_| Error::AuditUnavailable)?;
                true
            } else {
                false
            };
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
                    previous_questioned: entry.questioned,
                    new_questioned: e.questioned,
                    forced_presence: req.force_presence,
                    cp_tenths_awarded: contribution.cp_tenths,
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
            if question_comment {
                state
                    .realtime
                    .publish(
                        id,
                        &prts_realtime::RoomEvent::EntryCommentChanged {
                            entry_id: e.id,
                            by: user.id,
                        },
                    )
                    .await;
            }
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
    description = "Update the orthogonal locked and/or hidden flags without changing workflow state. Each mutation increments the entry version, appends an actor snapshot to history, updates materialized visibility statistics, writes audit, and broadcasts the new version.",
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
    let updated = prts_db::entries::set_flags_tx(
        &mut tx,
        id,
        entry_id,
        req.locked,
        req.hidden,
        Some(user.id),
    )
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
    pub translation: String,
    pub state: String,
    pub questioned: bool,
    pub locked: bool,
    pub hidden: bool,
    pub original: serde_json::Value,
    pub editor_id: Option<i64>,
    pub editor_name: Option<String>,
    pub editor_avatar_url: Option<String>,
    pub created_at: String,
}

/// 词条历史。
#[utoipa::path(get, path = "/projects/{id}/entries/{entry_id}/history", tag = "entry",
    description = "Return up to 200 newest-first complete entry snapshots for editor diff rendering. Legacy partial rows are materialized oldest-to-newest; source-upload changes, translation/state edits, flag changes and rollbacks include editor display snapshots when available.",
    responses((status = 200, body = [EntryVersionDto]), (status = 404, body = ErrorResponse)))]
pub async fn entry_history(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((id, entry_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<EntryVersionDto>>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    // 确认词条属于该项目
    let entry = prts_db::entries::get(&state.db, id, entry_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let versions = prts_db::entries::list_versions_with_editor(&state.db, entry_id, 200)
        .await
        .map_err(db_err)?;
    let current_actor = match entry.updated_by {
        Some(user_id) => prts_db::users::find_by_id(&state.db, user_id)
            .await
            .map_err(db_err)?,
        None => None,
    };
    // Legacy rows may contain partial snapshots. Materialize from oldest to newest so a source
    // snapshot propagates forward, then return the editor-facing newest-first timeline.
    let mut materialized = Vec::with_capacity(versions.len());
    let mut original = entry.original.clone();
    let mut translation = String::new();
    let mut workflow_state = "untranslated".to_string();
    let mut questioned = false;
    for version in versions.into_iter().rev() {
        if let Some(value) = version.original {
            original = value;
        }
        if let Some(value) = version.translation {
            translation = value;
        }
        if let Some(value) = version.state {
            workflow_state = value;
        }
        if let Some(value) = version.questioned {
            questioned = value;
        }
        materialized.push(EntryVersionDto {
            version: version.version,
            kind: version.kind,
            translation: translation.clone(),
            state: workflow_state.clone(),
            questioned,
            locked: version.locked,
            hidden: version.hidden,
            original: original.clone(),
            editor_id: version.editor_id,
            editor_name: version.editor_name,
            editor_avatar_url: version.editor_avatar_url,
            created_at: version.created_at.to_rfc3339(),
        });
    }
    materialized.reverse();
    let mut result = Vec::with_capacity(materialized.len() + 1);
    if materialized
        .first()
        .is_none_or(|version| version.version != entry.version)
    {
        result.push(EntryVersionDto {
            version: entry.version,
            kind: "current".to_string(),
            translation: entry.translation.clone(),
            state: entry.state.clone(),
            questioned: entry.questioned,
            locked: entry.locked,
            hidden: entry.hidden,
            original: entry.original.clone(),
            editor_id: entry.updated_by,
            editor_name: current_actor.as_ref().map(|actor| actor.username.clone()),
            editor_avatar_url: current_actor.and_then(|actor| actor.avatar_url),
            created_at: entry.updated_at.to_rfc3339(),
        });
    }
    result.extend(materialized);
    Ok(Json(result))
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
