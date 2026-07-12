//! 项目与成员端点。

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_core::permission::nodes;
use prts_core::ProjectRole;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::dto::capabilities::ProjectCapabilitiesDto;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

/// 项目对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectDto {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub visibility: String,
    pub source_langs: Vec<String>,
    pub primary_source_lang: Option<String>,
    pub target_lang: String,
    pub language_repair_state: String,
    pub primary_source_changed_at: Option<String>,
    pub primary_source_cooldown_until: Option<String>,
    pub lexical_state: String,
    pub lexical_job_id: Option<i64>,
    pub embedding_state: String,
    pub embedding_job_id: Option<i64>,
    pub embedding_degraded_reason: Option<String>,
    pub avatar_url: Option<String>,
    pub avatar_updated_at: Option<String>,
    pub owner_id: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&prts_db::models::Project> for ProjectDto {
    fn from(p: &prts_db::models::Project) -> Self {
        Self {
            id: p.id,
            slug: p.slug.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            visibility: p.visibility.clone(),
            source_langs: p.source_langs.clone(),
            primary_source_lang: p.primary_source_lang.clone(),
            target_lang: p.target_lang.clone(),
            language_repair_state: p.language_repair_state.clone(),
            primary_source_changed_at: p.primary_source_changed_at.map(|value| value.to_rfc3339()),
            primary_source_cooldown_until: p
                .primary_source_changed_at
                .map(|value| (value + chrono::Duration::days(7)).to_rfc3339()),
            lexical_state: p.lexical_state.clone(),
            lexical_job_id: p.lexical_job_id,
            embedding_state: p.embedding_state.clone(),
            embedding_job_id: p.embedding_job_id,
            embedding_degraded_reason: (p.embedding_state == "degraded")
                .then(|| "embedding_provider_unconfigured".to_string()),
            avatar_url: p
                .avatar_key
                .as_ref()
                .map(|_| format!("/api/projects/{}/avatar", p.id)),
            avatar_updated_at: p.avatar_updated_at.map(|value| value.to_rfc3339()),
            owner_id: p.owner_id,
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

/// 项目详情（含各状态词条统计）。
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectDetailDto {
    pub project: ProjectDto,
    pub state_counts: HashMap<String, i64>,
    pub entry_count: i64,
    pub capabilities: ProjectCapabilitiesDto,
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

async fn unique_slug(state: &AppState, base: &str) -> Result<String, ApiError> {
    let base = if base.is_empty() { "project" } else { base };
    if !prts_db::projects::slug_exists(&state.db, base)
        .await
        .map_err(db_err)?
    {
        return Ok(base.to_string());
    }
    for i in 2..10_000 {
        let candidate = format!("{base}-{i}");
        if !prts_db::projects::slug_exists(&state.db, &candidate)
            .await
            .map_err(db_err)?
        {
            return Ok(candidate);
        }
    }
    Ok(format!(
        "{base}-{}",
        prts_auth::token::random_token(6).to_lowercase()
    ))
}

/// 创建项目请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateProjectReq {
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// public | private（缺省 public）。
    #[serde(default)]
    pub visibility: Option<String>,
    pub source_langs: Vec<String>,
    /// 多源项目必须显式提交；单源项目可省略。
    pub primary_source_lang: Option<String>,
    pub target_lang: String,
}

/// 创建项目（创建者成为拥有者）。需平台「创建项目」权限。
#[utoipa::path(post, path = "/projects", tag = "project", request_body = CreateProjectReq,
    responses(
        (status = 200, body = ProjectDto),
        (status = 400),
        (status = 403),
        (status = 503, description = "审计服务不可用，项目未创建", body = ErrorResponse)
    ))]
pub async fn create_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateProjectReq>,
) -> Result<Json<ProjectDto>, ApiError> {
    user.require_platform(nodes::PLATFORM_PROJECT_CREATE)?;

    let name = req.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(Error::bad_request("项目名需为 1–128 字符").into());
    }
    let (source_langs, primary_source_lang, target_lang) =
        prts_core::language::canonicalize_project_languages(
            &req.source_langs,
            req.primary_source_lang.as_deref(),
            &req.target_lang,
        )
        .map_err(language_error)?;
    let release_ready = prts_db::foundation::primary_source_release_ready(&state.db)
        .await
        .map_err(db_err)?;
    if source_langs.len() > 1 && primary_source_lang != source_langs[0] && !release_ready {
        return Err(Error::bad_request("primary_source_release_not_ready").into());
    }
    let base = req
        .slug
        .as_deref()
        .map(slugify)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify(name));
    let slug = unique_slug(&state, &base).await?;
    let visibility = match req.visibility.as_deref() {
        Some("private") => "private",
        _ => "public",
    };

    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::create_with_primary_tx(
        &mut tx,
        &slug,
        name,
        req.description.as_deref().unwrap_or(""),
        visibility,
        &source_langs,
        &primary_source_lang,
        &target_lang,
        user.id,
    )
    .await
    .map_err(db_err)?;
    prts_db::memberships::upsert_tx(&mut tx, project.id, user.id, "owner")
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ProjectCreated {
            project_id: project.id,
            slug: &project.slug,
            visibility: &project.visibility,
            source_langs: &project.source_langs,
            target_lang: &project.target_lang,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;

    Ok(Json((&project).into()))
}

/// 列表查询参数。
#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    #[serde(default)]
    pub mine: bool,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub per_page: Option<i64>,
}

/// 列出项目：`mine=true` 列出我参与的（需登录），否则列出公开项目。
#[utoipa::path(get, path = "/projects", tag = "project",
    responses((status = 200, body = [ProjectDto])))]
pub async fn list_projects(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Query(q): Query<ListProjectsQuery>,
) -> Result<Json<Vec<ProjectDto>>, ApiError> {
    let projects = if q.mine {
        let u = user.as_ref().ok_or(Error::Unauthorized)?;
        prts_db::projects::list_for_user(&state.db, u.id)
            .await
            .map_err(db_err)?
    } else {
        let per = q.per_page.unwrap_or(50).clamp(1, 200);
        let page = q.page.unwrap_or(1).max(1);
        prts_db::projects::list_public(&state.db, per, (page - 1) * per)
            .await
            .map_err(db_err)?
    };
    Ok(Json(projects.iter().map(ProjectDto::from).collect()))
}

/// 项目详情。
#[utoipa::path(get, path = "/projects/{id}", tag = "project",
    responses((status = 200, body = ProjectDetailDto), (status = 404)))]
pub async fn get_project(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
) -> Result<Json<ProjectDetailDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;

    let stats = prts_db::stats::project(&state.db, id)
        .await
        .map_err(db_err)?;
    let mut state_counts = HashMap::new();
    state_counts.insert("untranslated".to_string(), stats.untranslated_count);
    state_counts.insert("translated".to_string(), stats.translated_count);
    state_counts.insert("questioned".to_string(), stats.questioned_count);
    state_counts.insert("checked".to_string(), stats.checked_count);
    state_counts.insert("reviewed".to_string(), stats.reviewed_count);
    let release_ready = prts_db::foundation::primary_source_release_ready(&state.db)
        .await
        .map_err(db_err)?;
    let capabilities = access.capabilities(release_ready).into();

    Ok(Json(ProjectDetailDto {
        project: (&access.project).into(),
        state_counts,
        entry_count: stats.visible_total,
        capabilities,
    }))
}

/// 更新项目请求（字段缺省表示不变）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProjectReq {
    pub name: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<String>,
    pub source_langs: Option<Vec<String>>,
    pub target_lang: Option<String>,
}

/// 主源语言切换请求。移除当前主源时必须在同一请求中提交替代后的完整源语言集合。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePrimarySourceReq {
    pub source_langs: Vec<String>,
    pub primary_source_lang: String,
}

/// 切换项目主源，并原子排队词法重建任务。只有 `projects.owner_id` 指向的拥有者可调用。
#[utoipa::path(
    put,
    path = "/projects/{id}/primary-source",
    tag = "project",
    request_body = ChangePrimarySourceReq,
    responses(
        (status = 200, description = "主源未变化或已排队重建", body = ProjectDto),
        (status = 400, description = "语言、冷却或重建状态不允许变化", body = ErrorResponse),
        (status = 403, description = "仅项目唯一拥有者可更改", body = ErrorResponse),
        (status = 404, description = "项目不存在", body = ErrorResponse),
        (status = 503, description = "foundation 或审计不可用，项目未变化", body = ErrorResponse)
    )
)]
pub async fn change_primary_source(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<ChangePrimarySourceReq>,
) -> Result<Json<ProjectDto>, ApiError> {
    let release_ready = prts_db::foundation::primary_source_release_ready(&state.db)
        .await
        .map_err(db_err)?;
    if !release_ready {
        return Err(Error::bad_request("primary_source_release_not_ready").into());
    }
    let source_langs =
        prts_core::canonicalize_language_tags(&req.source_langs).map_err(language_error)?;
    if source_langs.is_empty() {
        return Err(language_error(
            prts_core::LanguageTagError::EmptySourceLanguages,
        ));
    }
    let primary_source_lang =
        prts_core::canonicalize_language_tag(&req.primary_source_lang).map_err(language_error)?;
    if !source_langs.contains(&primary_source_lang) {
        return Err(language_error(
            prts_core::LanguageTagError::PrimaryNotInSourceLanguages,
        ));
    }

    let mut tx = state.db.begin().await.map_err(db_err)?;
    let before = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, before.clone()).await?;
    if user.id != before.owner_id {
        return Err(Error::Forbidden.into());
    }
    if before.language_repair_state != "ready" {
        return Err(Error::ProjectLanguageResolutionRequired.into());
    }
    let previous_primary = before
        .primary_source_lang
        .as_deref()
        .ok_or(Error::ProjectLanguageResolutionRequired)?;
    if previous_primary == primary_source_lang {
        tx.rollback().await.map_err(db_err)?;
        return Ok(Json((&before).into()));
    }
    locked_access.require_node(nodes::PROJECT_MANAGE)?;

    let cooldown_active = before
        .primary_source_changed_at
        .is_some_and(|changed_at| changed_at + chrono::Duration::days(7) > chrono::Utc::now());
    let lexical_job_state = match before.lexical_job_id {
        Some(job_id) => prts_db::jobs::find_by_id_for_update_tx(&mut tx, job_id)
            .await
            .map_err(db_err)?
            .map(|job| job.state),
        None => None,
    };
    let embedding_job_state = match before.embedding_job_id {
        Some(job_id) => prts_db::jobs::find_by_id_for_update_tx(&mut tx, job_id)
            .await
            .map_err(db_err)?
            .map(|job| job.state),
        None => None,
    };
    prts_core::project_language::validate_primary_source_change(
        release_ready,
        true,
        true,
        cooldown_active,
        lexical_job_state.as_deref(),
        embedding_job_state.as_deref(),
    )
    .map_err(primary_source_change_error)?;

    let entry_count = prts_db::entries::count_project_entries_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let lexical_job = prts_db::jobs::create_tx(
        &mut tx,
        prts_db::jobs::NewJob {
            kind: prts_db::jobs::JobKind::PrimarySourceLexicalReindex,
            project_id: Some(id),
            stage: "lexical".to_string(),
            progress_total: Some(entry_count),
            max_attempts: 5,
            run_after: chrono::Utc::now(),
        },
    )
    .await
    .map_err(db_err)?;
    let updated = prts_db::projects::change_primary_source_tx(
        &mut tx,
        id,
        &source_langs,
        &primary_source_lang,
        lexical_job.id,
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
        AuditEvent::ProjectPrimarySourceChanged {
            project_id: id,
            previous_primary_source: previous_primary,
            new_primary_source: &primary_source_lang,
            source_language_count: source_langs.len(),
            lexical_job_id: lexical_job.id,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
    Ok(Json((&updated).into()))
}

fn primary_source_change_error(
    error: prts_core::project_language::PrimarySourceChangeError,
) -> ApiError {
    match error {
        prts_core::project_language::PrimarySourceChangeError::NotOwner => Error::Forbidden.into(),
        prts_core::project_language::PrimarySourceChangeError::LanguageResolutionRequired => {
            Error::ProjectLanguageResolutionRequired.into()
        }
        _ => Error::bad_request(error.code()).into(),
    }
}

/// 更新项目元信息。需项目「管理」权限。
#[utoipa::path(put, path = "/projects/{id}", tag = "project", request_body = UpdateProjectReq,
    responses(
        (status = 200, body = ProjectDto),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，项目未更新", body = ErrorResponse)
    ))]
pub async fn update_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProjectReq>,
) -> Result<Json<ProjectDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let before = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, before.clone())
        .await?
        .require_node(nodes::PROJECT_MANAGE)?;

    let visibility = match req.visibility.as_deref() {
        Some("private") => "private".to_string(),
        Some("public") => "public".to_string(),
        _ => before.visibility.clone(),
    };
    let name = req.name.unwrap_or_else(|| before.name.clone());
    let description = req
        .description
        .unwrap_or_else(|| before.description.clone());
    let requested_source_langs = req
        .source_langs
        .unwrap_or_else(|| before.source_langs.clone());
    let requested_target_lang = req
        .target_lang
        .unwrap_or_else(|| before.target_lang.clone());
    let primary_source_lang = before.primary_source_lang.as_deref();
    let (source_langs, canonical_primary, target_lang) =
        prts_core::language::canonicalize_project_languages(
            &requested_source_langs,
            primary_source_lang,
            &requested_target_lang,
        )
        .map_err(language_error)?;
    if target_lang != before.target_lang
        && prts_db::entries::count_project_entries_tx(&mut tx, id)
            .await
            .map_err(db_err)?
            > 0
    {
        return Err(Error::bad_request("target_language_locked_after_first_entry").into());
    }
    if before.language_repair_state != "ready" {
        return Err(Error::ProjectLanguageResolutionRequired.into());
    }
    if Some(canonical_primary.as_str()) != primary_source_lang {
        return Err(Error::bad_request("主源语言只能通过专用重建流程更改").into());
    }
    let mut changed_fields = Vec::with_capacity(5);
    if name != before.name {
        changed_fields.push("name");
    }
    if description != before.description {
        changed_fields.push("description");
    }
    if visibility != before.visibility {
        changed_fields.push("visibility");
    }
    if source_langs != before.source_langs {
        changed_fields.push("source_langs");
    }
    if target_lang != before.target_lang {
        changed_fields.push("target_lang");
    }
    let updated = prts_db::projects::update_tx(
        &mut tx,
        id,
        &name,
        &description,
        &visibility,
        &source_langs,
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
        AuditEvent::ProjectUpdated {
            project_id: id,
            changed_fields: &changed_fields,
            visibility: &updated.visibility,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json((&updated).into()))
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

/// 删除项目。需项目「删除」权限。
#[utoipa::path(delete, path = "/projects/{id}", tag = "project",
    responses(
        (status = 204),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，项目未删除", body = ErrorResponse)
    ))]
pub async fn delete_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_DELETE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let before = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, before.clone())
        .await?
        .require_node(nodes::PROJECT_DELETE)?;
    prts_db::projects::delete_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ProjectDeleted {
            project_id: id,
            slug: &before.slug,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 成员对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberDto {
    pub user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: String,
}

/// 列出项目成员。
#[utoipa::path(get, path = "/projects/{id}/members", tag = "project",
    responses((status = 200, body = [MemberDto]), (status = 404)))]
pub async fn list_members(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
) -> Result<Json<Vec<MemberDto>>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    let members = prts_db::memberships::list(&state.db, id)
        .await
        .map_err(db_err)?;
    Ok(Json(
        members
            .into_iter()
            .map(|m| MemberDto {
                user_id: m.user_id,
                username: m.username,
                avatar_url: m.avatar_url,
                role: m.role,
                created_at: m.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

/// 添加/更新成员请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddMemberReq {
    pub username: String,
    /// manager | reviewer | translator；本轮不支持拥有者转让。
    pub role: String,
}

/// 添加或更新项目成员。需项目「成员管理」权限。
#[utoipa::path(post, path = "/projects/{id}/members", tag = "project", request_body = AddMemberReq,
    responses(
        (status = 204),
        (status = 400),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，成员关系未更新", body = ErrorResponse)
    ))]
pub async fn add_member(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<AddMemberReq>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;
    if !matches!(req.role.as_str(), "manager" | "reviewer" | "translator") {
        return Err(Error::bad_request("role 必须是 manager|reviewer|translator").into());
    }
    let target = prts_db::users::find_by_username(&state.db, req.username.trim())
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked_access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;
    if locked_access.effective_role() == Some(ProjectRole::Manager) && req.role == "manager" {
        return Err(Error::Forbidden.into());
    }
    let previous_role = prts_db::memberships::find_role_tx(&mut tx, id, target.id)
        .await
        .map_err(db_err)?;
    prts_db::memberships::upsert_tx(&mut tx, id, target.id, &req.role)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::MembershipUpserted {
            project_id: id,
            member_id: target.id,
            previous_role: previous_role.as_deref(),
            new_role: &req.role,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 移除项目成员。需项目「成员管理」权限。不可移除最后一个拥有者。
#[utoipa::path(delete, path = "/projects/{id}/members/{user_id}", tag = "project",
    responses(
        (status = 204),
        (status = 400),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，成员关系未移除", body = ErrorResponse)
    ))]
pub async fn remove_member(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, user_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;

    // 锁定项目会串行化 add/remove；即使目标 membership 尚不存在，也不会发生 gap race。
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, project)
        .await?
        .require_node(nodes::PROJECT_MEMBER_MANAGE)?;
    let previous_role = prts_db::memberships::find_role_tx(&mut tx, id, user_id)
        .await
        .map_err(db_err)?;
    if let Some(role) = previous_role.as_deref() {
        if role == "owner"
            && prts_db::memberships::count_role_tx(&mut tx, id, "owner")
                .await
                .map_err(db_err)?
                <= 1
        {
            return Err(Error::bad_request("不能移除最后一个拥有者").into());
        }
    }
    let previous_role = previous_role.ok_or(Error::NotFound)?;
    if prts_db::memberships::remove_tx(&mut tx, id, user_id)
        .await
        .map_err(db_err)?
    {
        prts_db::audit::append_event_tx(
            &mut tx,
            AuditActor {
                id: Some(user.id),
                kind: AuditActorKind::User,
                ip: None,
            },
            AuditEvent::MembershipRemoved {
                project_id: id,
                member_id: user_id,
                previous_role: &previous_role,
            },
        )
        .await
        .map_err(|_| Error::AuditUnavailable)?;
        tx.commit().await.map_err(db_err)?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::NotFound.into())
    }
}
