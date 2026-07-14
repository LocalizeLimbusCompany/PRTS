//! Owner language resolution 与 metadata-only 平台诊断端点。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_core::permission::nodes;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::{project as paccess, CurrentUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct LanguageIssueDto {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub issue_kind: String,
    pub raw_tag: Option<String>,
    pub canonical_tag: Option<String>,
    pub metadata: serde_json::Value,
    pub current_values: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectLanguageResolutionDto {
    pub project_id: i64,
    pub source_langs: Vec<String>,
    pub primary_source_lang: Option<String>,
    pub target_lang: String,
    pub state: String,
    pub issues: Vec<LanguageIssueDto>,
}

#[utoipa::path(get, path = "/projects/{id}/language-resolution", tag = "project",
    description = "仅项目唯一 owner 可读取需要人工处理的语言问题、raw/canonical tag 和冲突候选值；普通成员与平台管理员不能借此读取私有正文。",
    responses((status = 200, body = ProjectLanguageResolutionDto), (status = 403), (status = 404)))]
pub async fn get_project_language_resolution(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<ProjectLanguageResolutionDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_view()?;
    if user.id != access.project.owner_id {
        return Err(Error::Forbidden.into());
    }
    let issues = prts_db::language_resolution::list_project_issues(&state.db, id)
        .await
        .map_err(db_err)?;
    Ok(Json(ProjectLanguageResolutionDto {
        project_id: id,
        source_langs: access.project.source_langs,
        primary_source_lang: access.project.primary_source_lang,
        target_lang: access.project.target_lang,
        state: access.project.language_repair_state,
        issues: futures_util::future::try_join_all(issues.into_iter().map(|issue| {
            let db = state.db.clone();
            async move {
                let current_values = match issue.entry_id {
                    Some(entry_id) => {
                        let original: Option<serde_json::Value> = sqlx::query_scalar(
                            "SELECT original FROM entries WHERE id = $1 AND project_id = $2",
                        )
                        .bind(entry_id)
                        .bind(id)
                        .fetch_optional(&db)
                        .await
                        .map_err(db_err)?;
                        original
                            .and_then(|value| value.as_object().cloned())
                            .map(|object| {
                                object
                                    .into_values()
                                    .filter_map(|value| value.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default()
                    }
                    None => Vec::new(),
                };
                Ok::<_, ApiError>(LanguageIssueDto {
                    id: issue.id,
                    entity_type: issue.entity_type,
                    entity_id: issue.entity_id_snapshot,
                    issue_kind: issue.issue_kind,
                    raw_tag: issue.raw_tag,
                    canonical_tag: issue.canonical_tag,
                    metadata: issue.metadata,
                    current_values,
                })
            }
        }))
        .await?,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueResolutionReq {
    pub issue_id: i64,
    #[serde(default)]
    pub canonical_tag: Option<String>,
    #[serde(default)]
    pub selected_value: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveProjectLanguagesReq {
    pub source_langs: Vec<String>,
    pub primary_source_lang: String,
    pub target_lang: String,
    pub issues: Vec<IssueResolutionReq>,
}

#[utoipa::path(post, path = "/projects/{id}/language-resolution/resolve", tag = "project",
    description = "仅项目唯一 owner 可一次性提交全部 issue 选择和最终 source/primary/target；所有语言先经共享 BCP-47 canonicalizer，成功后原子排 repair/reconcile job 并写脱敏审计。",
    request_body = ResolveProjectLanguagesReq,
    responses(
        (status = 202), (status = 400), (status = 403), (status = 404),
        (status = 503, body = ErrorResponse)
    ))]
pub async fn resolve_project_languages(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(request): Json<ResolveProjectLanguagesReq>,
) -> Result<StatusCode, ApiError> {
    let (source_langs, primary_source_lang, target_lang) =
        prts_core::language::canonicalize_project_languages(
            &request.source_langs,
            Some(&request.primary_source_lang),
            &request.target_lang,
        )
        .map_err(language_error)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    if locked_access.project.owner_id != user.id {
        return Err(Error::Forbidden.into());
    }
    if locked_access.project.language_repair_state != "needs_language_resolution" {
        return Err(Error::Conflict.into());
    }
    let open_issues = prts_db::language_resolution::lock_project_issues_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    if open_issues.is_empty() {
        return Err(Error::Conflict.into());
    }
    let requested_ids: std::collections::HashSet<_> =
        request.issues.iter().map(|issue| issue.issue_id).collect();
    if requested_ids.len() != request.issues.len()
        || requested_ids.len() != open_issues.len()
        || open_issues
            .iter()
            .any(|issue| !requested_ids.contains(&issue.id))
    {
        return Err(Error::bad_request("必须处理项目的全部未解决语言问题").into());
    }
    for selection in &request.issues {
        let issue = open_issues
            .iter()
            .find(|issue| issue.id == selection.issue_id)
            .ok_or_else(|| Error::bad_request("语言问题不存在"))?;
        let Some(entry_id) = issue.entry_id else {
            if selection.canonical_tag.is_some() || selection.selected_value.is_some() {
                return Err(Error::bad_request("项目级语言问题只使用最终语言设置解决").into());
            }
            continue;
        };
        let canonical_tag = selection
            .canonical_tag
            .as_deref()
            .ok_or_else(|| Error::bad_request("词条语言问题必须指定规范语言"))
            .and_then(|tag| {
                prts_core::canonicalize_language_tag(tag)
                    .map_err(|_| Error::bad_request("词条规范语言无效"))
            })?;
        if !source_langs.contains(&canonical_tag) {
            return Err(Error::bad_request("词条规范语言必须属于项目源语言").into());
        }
        let selected_value = selection
            .selected_value
            .as_deref()
            .ok_or_else(|| Error::bad_request("词条语言问题必须选择现有原文"))?;
        let original = prts_db::language_resolution::lock_entry_original_tx(&mut tx, id, entry_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
        let repaired = repair_entry_original(
            &original,
            &canonical_tag,
            selected_value,
            &primary_source_lang,
        )?;
        let changed =
            prts_db::language_resolution::update_entry_original_tx(&mut tx, id, entry_id, repaired)
                .await
                .map_err(db_err)?;
        if changed != 1 {
            return Err(Error::Conflict.into());
        }
    }
    let issue_ids: Vec<_> = open_issues.iter().map(|issue| issue.id).collect();
    prts_db::language_resolution::complete_owner_resolution_tx(
        &mut tx,
        id,
        &issue_ids,
        &source_langs,
        &primary_source_lang,
        &target_lang,
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
        AuditEvent::ProjectLanguageResolutionCompleted {
            project_id: id,
            issue_count: issue_ids.len(),
            source_language_count: source_langs.len(),
            primary_source_language: &primary_source_lang,
            target_language: &target_lang,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
    Ok(StatusCode::ACCEPTED)
}

fn language_error(error: prts_core::LanguageTagError) -> ApiError {
    match error {
        prts_core::LanguageTagError::Invalid => Error::InvalidLanguageTag.into(),
        prts_core::LanguageTagError::Duplicate => Error::DuplicateLanguageTag.into(),
        prts_core::LanguageTagError::EmptySourceLanguages
        | prts_core::LanguageTagError::PrimaryNotInSourceLanguages => {
            Error::bad_request(error.code()).into()
        }
    }
}

/// 将 owner 明确的 tag mapping 与候选值合并为无歧义 canonical 对象。
fn repair_entry_original(
    original: &serde_json::Value,
    mapped_tag: &str,
    selected_value: &str,
    primary_source_language: &str,
) -> Result<serde_json::Value, ApiError> {
    let original = original
        .as_object()
        .ok_or_else(|| Error::bad_request("词条原文不是语言对象"))?;
    let candidates: Vec<&str> = original
        .values()
        .filter_map(serde_json::Value::as_str)
        .collect();
    if !candidates.contains(&selected_value) {
        return Err(Error::bad_request("选择值必须来自当前冲突候选").into());
    }
    let mut repaired = serde_json::Map::new();
    for (raw_tag, candidate) in original {
        let tag = prts_core::canonicalize_language_tag(raw_tag)
            .unwrap_or_else(|_| mapped_tag.to_string());
        match repaired.get(&tag) {
            Some(existing) if existing != candidate => {
                repaired.insert(tag, serde_json::Value::String(selected_value.to_string()));
            }
            None => {
                repaired.insert(tag, candidate.clone());
            }
            _ => {}
        }
    }
    if !repaired.contains_key(primary_source_language) {
        if mapped_tag != primary_source_language {
            return Err(Error::bad_request("缺少主源原文时必须映射到主源语言").into());
        }
        repaired.insert(
            primary_source_language.to_string(),
            serde_json::Value::String(selected_value.to_string()),
        );
    }
    Ok(serde_json::Value::Object(repaired))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AdminResolutionQuery {
    pub after_project_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminResolutionSummaryDto {
    pub project_id: i64,
    pub project_slug: String,
    pub issue_count: i64,
    pub repair_state: String,
    pub repair_job_id: Option<i64>,
}

#[utoipa::path(get, path = "/admin/language-resolutions", tag = "admin",
    description = "平台项目管理 capability 只能读取待解决项目、issue 数量、repair 状态和 job ID 的 metadata-only 键集列表，不返回私有源文或冲突候选值。",
    params(AdminResolutionQuery), responses((status = 200, body = [AdminResolutionSummaryDto]), (status = 403)))]
pub async fn list_admin_language_resolutions(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<AdminResolutionQuery>,
) -> Result<Json<Vec<AdminResolutionSummaryDto>>, ApiError> {
    user.require_platform(nodes::PLATFORM_PROJECT_MANAGE_ALL)?;
    let rows = prts_db::language_resolution::list_admin_summaries(
        &state.db,
        query.after_project_id,
        query.limit.unwrap_or(50).clamp(1, 100),
    )
    .await
    .map_err(db_err)?;
    Ok(Json(
        rows.into_iter()
            .map(|row| AdminResolutionSummaryDto {
                project_id: row.project_id,
                project_slug: row.project_slug,
                issue_count: row.issue_count,
                repair_state: row.repair_state,
                repair_job_id: row.repair_job_id,
            })
            .collect(),
    ))
}

#[utoipa::path(post, path = "/admin/language-resolutions/{project_id}/retry", tag = "admin",
    description = "平台项目管理 capability 可重试已有失败的 durable language repair job，但不能替 owner 选择 canonical mapping、主源语言或冲突正文。",
    responses((status = 202), (status = 403), (status = 404), (status = 503, body = ErrorResponse)))]
pub async fn retry_admin_language_repair(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    user.require_platform(nodes::PLATFORM_PROJECT_MANAGE_ALL)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let current_admin = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;
    let current_platform_role = current_admin
        .platform_role
        .as_deref()
        .and_then(prts_core::PlatformRole::parse)
        .filter(|role| role.has(nodes::PLATFORM_PROJECT_MANAGE_ALL));
    if current_platform_role.is_none() {
        return Err(Error::Forbidden.into());
    }
    if project.language_repair_state != "repairing" {
        return Err(Error::Conflict.into());
    }
    let repair_job_id = project.language_repair_job_id.ok_or(Error::Conflict)?;
    let retried = prts_db::language_resolution::retry_failed_project_repair_tx(
        &mut tx,
        project_id,
        repair_job_id,
    )
    .await
    .map_err(db_err)?
    .ok_or(Error::Conflict)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ProjectLanguageRepairRetried {
            project_id,
            job_id: retried.id,
            previous_state: &project.language_repair_state,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
    Ok(StatusCode::ACCEPTED)
}
