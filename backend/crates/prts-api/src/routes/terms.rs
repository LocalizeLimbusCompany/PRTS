//! source-aware 项目术语 CRUD、键集列表与当前主源匹配。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use prts_core::permission::nodes;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;
const MAX_MATCH_LIMIT: i64 = 50;

/// 术语列表范围。
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TermScope {
    Current,
    Archived,
    Mixed,
}

impl From<TermScope> for prts_db::terms::TermListScope {
    fn from(value: TermScope) -> Self {
        match value {
            TermScope::Current => Self::Current,
            TermScope::Archived => Self::Archived,
            TermScope::Mixed => Self::Mixed,
        }
    }
}

/// 术语列表键集与范围参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct TermListQuery {
    pub scope: Option<TermScope>,
    pub after: Option<i64>,
    pub limit: Option<i64>,
}

/// 当前主源术语匹配请求；正文不放入 URL 或审计。
#[derive(Debug, Deserialize, ToSchema)]
pub struct TermMatchRequest {
    pub source_text: String,
    pub limit: Option<i64>,
}

/// 创建或完整更新一个 source-aware term。
#[derive(Debug, Deserialize, ToSchema)]
pub struct TermWriteRequest {
    pub source_lang: String,
    pub source_text: String,
    pub translation: String,
    #[serde(default)]
    pub notes: String,
    pub pos_id: Option<i64>,
    #[serde(default)]
    pub archived: bool,
}

/// 术语及双语 POS 名称。
#[derive(Debug, Serialize, ToSchema)]
pub struct TermDto {
    pub id: i64,
    pub project_id: i64,
    pub source_lang: String,
    pub source_text: String,
    pub translation: String,
    pub notes: String,
    pub pos_id: Option<i64>,
    pub pos_name_zh_cn: Option<String>,
    pub pos_name_en: Option<String>,
    pub archived: bool,
    pub archived_at: Option<String>,
    pub created_by: Option<i64>,
    pub updated_by: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// 术语键集页。
#[derive(Debug, Serialize, ToSchema)]
pub struct TermPageDto {
    pub items: Vec<TermDto>,
    pub next_after: Option<i64>,
}

/// 列出可见项目的 current/archived/mixed 术语，按 id DESC 键集分页。
#[utoipa::path(get, path = "/projects/{id}/terms", tag = "term",
    params(("id" = i64, Path), TermListQuery),
    description = "列出项目术语。current 仅包含当前主源的 active terms；archived 仅包含归档项；mixed 返回两者。使用 id 游标，不使用 OFFSET。",
    responses((status = 200, body = TermPageDto), (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse)))]
pub async fn list_terms(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(query): Query<TermListQuery>,
) -> Result<Json<TermPageDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    access.require_language_ready()?;
    let primary = access
        .project
        .primary_source_lang
        .as_deref()
        .ok_or(prts_common::Error::ProjectLanguageResolutionRequired)?;
    let limit = validate_limit(query.limit, MAX_LIMIT)?;
    let rows = prts_db::terms::list(
        &state.db,
        id,
        primary,
        query.scope.unwrap_or(TermScope::Current).into(),
        query.after,
        limit,
    )
    .await
    .map_err(db_err)?;
    let next_after = (rows.len() as i64 == limit)
        .then(|| rows.last().map(|term| term.id))
        .flatten();
    Ok(Json(TermPageDto {
        items: rows.into_iter().map(term_dto).collect(),
        next_after,
    }))
}

/// 创建术语；owner/manager/reviewer 可写，非主源 active 请求稳定拒绝。
#[utoipa::path(post, path = "/projects/{id}/terms", tag = "term",
    request_body = TermWriteRequest,
    description = "创建 source-aware 项目术语。source_lang 使用共享 BCP-47 canonicalizer；任意合法语言可归档保存，但 active term 必须使用事务内当前主源。",
    responses((status = 201, body = TermDto), (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
        (status = 503, body = ErrorResponse)))]
pub async fn create_term(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(request): Json<TermWriteRequest>,
) -> Result<(StatusCode, Json<TermDto>), ApiError> {
    validate_request(&request)?;
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_TERM_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = lock_term_mutation(&mut tx, &user, id).await?;
    let plan = term_plan(&request, &project)?;
    require_pos(&mut tx, request.pos_id).await?;
    let archived_at = plan.archived.then(Utc::now);
    let term = prts_db::terms::create_tx(
        &mut tx,
        id,
        &plan.source_lang,
        request.source_text.trim(),
        &request.translation,
        &request.notes,
        request.pos_id,
        archived_at,
        user.id,
    )
    .await
    .map_err(map_term_db_error)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::TermCreated {
            project_id: id,
            term_id: term.id,
            source_lang: &term.source_lang,
            pos_id: term.pos_id,
            archived: term.archived_at.is_some(),
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    let result = load_term(&state, id, term.id).await?;
    Ok((StatusCode::CREATED, Json(result)))
}

/// 读取 URL project 绑定的单个术语。
#[utoipa::path(get, path = "/projects/{id}/terms/{term_id}", tag = "term",
    params(("id" = i64, Path), ("term_id" = i64, Path)),
    description = "读取项目内术语；term 与 URL project 不匹配时返回 404。",
    responses((status = 200, body = TermDto), (status = 404, body = ErrorResponse),
        (status = 409, body = ErrorResponse)))]
pub async fn get_term(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((id, term_id)): Path<(i64, i64)>,
) -> Result<Json<TermDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    access.require_language_ready()?;
    load_term(&state, id, term_id).await.map(Json)
}

/// 完整更新 URL project 绑定的术语。
#[utoipa::path(put, path = "/projects/{id}/terms/{term_id}", tag = "term",
    request_body = TermWriteRequest,
    description = "更新 source-aware 项目术语。事务内重新校验权限、项目状态、当前主源、POS 引用与 URL project 绑定。",
    responses((status = 200, body = TermDto), (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
        (status = 503, body = ErrorResponse)))]
pub async fn update_term(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, term_id)): Path<(i64, i64)>,
    Json(request): Json<TermWriteRequest>,
) -> Result<Json<TermDto>, ApiError> {
    validate_request(&request)?;
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_TERM_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = lock_term_mutation(&mut tx, &user, id).await?;
    let current = prts_db::terms::find_for_update_tx(&mut tx, id, term_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let plan = term_plan(&request, &project)?;
    require_pos(&mut tx, request.pos_id).await?;
    let archived_at = if plan.archived {
        current.archived_at.or_else(|| Some(Utc::now()))
    } else {
        None
    };
    let mut changed_fields = Vec::new();
    if current.source_lang != plan.source_lang {
        changed_fields.push("source_lang");
    }
    if current.source_text != request.source_text.trim() {
        changed_fields.push("source_text");
    }
    if current.translation != request.translation {
        changed_fields.push("translation");
    }
    if current.notes != request.notes {
        changed_fields.push("notes");
    }
    if current.pos_id != request.pos_id {
        changed_fields.push("pos_id");
    }
    if current.archived_at.is_some() != plan.archived {
        changed_fields.push("archived");
    }
    let updated = prts_db::terms::update_tx(
        &mut tx,
        id,
        term_id,
        &plan.source_lang,
        request.source_text.trim(),
        &request.translation,
        &request.notes,
        request.pos_id,
        archived_at,
        user.id,
    )
    .await
    .map_err(map_term_db_error)?
    .ok_or(prts_common::Error::NotFound)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::TermUpdated {
            project_id: id,
            term_id,
            source_lang: &updated.source_lang,
            pos_id: updated.pos_id,
            archived: updated.archived_at.is_some(),
            changed_field_count: changed_fields.len(),
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    load_term(&state, id, term_id).await.map(Json)
}

/// 删除 URL project 绑定的术语。
#[utoipa::path(delete, path = "/projects/{id}/terms/{term_id}", tag = "term",
    params(("id" = i64, Path), ("term_id" = i64, Path)),
    description = "永久删除一个项目术语；事务内重新校验术语管理权限与项目状态，并写脱敏审计。",
    responses((status = 204), (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
        (status = 409, body = ErrorResponse), (status = 503, body = ErrorResponse)))]
pub async fn delete_term(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, term_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_TERM_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_term_mutation(&mut tx, &user, id).await?;
    let term = prts_db::terms::find_for_update_tx(&mut tx, id, term_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    if !prts_db::terms::delete_tx(&mut tx, id, term_id)
        .await
        .map_err(db_err)?
    {
        return Err(prts_common::Error::NotFound.into());
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::TermDeleted {
            project_id: id,
            term_id,
            source_lang: &term.source_lang,
            pos_id: term.pos_id,
            archived: term.archived_at.is_some(),
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 只匹配当前 primary 的 active terms。
#[utoipa::path(post, path = "/projects/{id}/terms/match", tag = "term",
    params(("id" = i64, Path)), request_body = TermMatchRequest,
    description = "在 JSON 请求体给定的当前主源文本中匹配 active terms；正文不会进入 URL、审计或错误响应，也不会返回归档项或其它 source_lang。",
    responses((status = 200, body = Vec<TermDto>), (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse)))]
pub async fn match_terms(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Json(request): Json<TermMatchRequest>,
) -> Result<Json<Vec<TermDto>>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    access.require_language_ready()?;
    let primary = access
        .project
        .primary_source_lang
        .as_deref()
        .ok_or(prts_common::Error::ProjectLanguageResolutionRequired)?;
    let limit = validate_limit(request.limit, MAX_MATCH_LIMIT)?;
    let rows = prts_db::terms::match_current(&state.db, id, primary, &request.source_text, limit)
        .await
        .map_err(db_err)?;
    Ok(Json(rows.into_iter().map(term_dto).collect()))
}

async fn lock_term_mutation(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    project_id: i64,
) -> Result<prts_db::models::Project, ApiError> {
    let project = prts_db::projects::find_by_id_for_update_tx(conn, project_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let access = paccess::load_locked_tx(conn, user, project.clone()).await?;
    access.require_node(nodes::PROJECT_TERM_MANAGE)?;
    access.require_language_ready()?;
    if prts_db::terms::project_pending_deletion_tx(conn, project_id)
        .await
        .map_err(db_err)?
    {
        return Err(prts_common::Error::ProjectPendingDeletion.into());
    }
    Ok(project)
}

fn term_plan(
    request: &TermWriteRequest,
    project: &prts_db::models::Project,
) -> Result<prts_core::terms::TermWritePlan, ApiError> {
    let primary = project
        .primary_source_lang
        .as_deref()
        .ok_or(prts_common::Error::ProjectLanguageResolutionRequired)?;
    prts_core::terms::plan_term_write(
        &request.source_lang,
        primary,
        request.archived,
        project.language_repair_state == "ready",
        false,
    )
    .map_err(map_term_rule_error)
}

async fn require_pos(conn: &mut sqlx::PgConnection, pos_id: Option<i64>) -> Result<(), ApiError> {
    if prts_db::pos::exists_tx(conn, pos_id)
        .await
        .map_err(db_err)?
    {
        Ok(())
    } else {
        Err(prts_common::Error::NotFound.into())
    }
}

fn validate_request(request: &TermWriteRequest) -> Result<(), ApiError> {
    if request.source_text.trim().is_empty() {
        return Err(prts_common::Error::bad_request("term source text is required").into());
    }
    Ok(())
}

fn validate_limit(limit: Option<i64>, max: i64) -> Result<i64, ApiError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=max).contains(&limit) {
        return Err(prts_common::Error::bad_request("invalid terminology limit").into());
    }
    Ok(limit)
}

fn map_term_rule_error(error: prts_core::terms::TermRuleError) -> ApiError {
    match error {
        prts_core::terms::TermRuleError::InvalidLanguageTag => {
            prts_common::Error::InvalidLanguageTag.into()
        }
        prts_core::terms::TermRuleError::ActiveSourceMismatch => {
            prts_common::Error::TermActiveSourceMismatch.into()
        }
        prts_core::terms::TermRuleError::LanguageResolutionRequired => {
            prts_common::Error::ProjectLanguageResolutionRequired.into()
        }
        prts_core::terms::TermRuleError::ProjectPendingDeletion => {
            prts_common::Error::ProjectPendingDeletion.into()
        }
    }
}

fn map_term_db_error(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .and_then(|database| database.constraint())
        == Some("terms_identity_unique")
    {
        prts_common::Error::DuplicateTerm.into()
    } else {
        db_err(error)
    }
}

fn audit_actor(user: &CurrentUser) -> AuditActor<'_> {
    AuditActor {
        id: Some(user.id),
        kind: AuditActorKind::User,
        ip: None,
    }
}

async fn load_term(state: &AppState, project_id: i64, term_id: i64) -> Result<TermDto, ApiError> {
    prts_db::terms::find(&state.db, project_id, term_id)
        .await
        .map_err(db_err)?
        .map(term_dto)
        .ok_or_else(|| prts_common::Error::NotFound.into())
}

fn term_dto(term: prts_db::models::TermWithPos) -> TermDto {
    TermDto {
        id: term.id,
        project_id: term.project_id,
        source_lang: term.source_lang,
        source_text: term.source_text,
        translation: term.translation,
        notes: term.notes,
        pos_id: term.pos_id,
        pos_name_zh_cn: term.pos_name_zh_cn,
        pos_name_en: term.pos_name_en,
        archived: term.archived_at.is_some(),
        archived_at: term.archived_at.map(|value| value.to_rfc3339()),
        created_by: term.created_by,
        updated_by: term.updated_by,
        created_at: term.created_at.to_rfc3339(),
        updated_at: term.updated_at.to_rfc3339(),
    }
}
