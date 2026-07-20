//! 项目与成员端点。

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_core::permission::{
    authorize_membership_mutation, nodes, MembershipDecision, MembershipMutation,
};
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
    pub comment_policy: String,
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
    pub deletion_scheduled_at: Option<String>,
    pub deletion_job_id: Option<i64>,
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
            comment_policy: p.comment_policy.clone(),
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
            deletion_scheduled_at: p.deletion_scheduled_at.map(|value| value.to_rfc3339()),
            deletion_job_id: p.deletion_job_id,
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
    if access.project.deletion_scheduled_at.is_none() {
        access.require_view()?;
    }

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
    /// private|internal|public；控制词条评论的读取与写入范围。
    pub comment_policy: Option<String>,
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
    description = "仅 projects.owner_id 指向的唯一拥有者可调用。请求先使用共享 BCP-47 canonicalizer，再校验 foundation readiness、冷却期和现有 lexical/embedding job，成功时原子切换主源并排队词法重建。",
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
    let term_plan = prts_core::terms::plan_primary_source_terms(&primary_source_lang)
        .map_err(|error| prts_common::Error::bad_request(error.code()))?;
    prts_db::terms::apply_primary_source_plan_tx(&mut tx, id, &term_plan, user.id)
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
    description = "Atomically update project metadata under project.manage. comment_policy accepts private, internal, or public; language tags are canonicalized, the primary source must remain unchanged here, and target language becomes immutable after the first entry.",
    responses(
        (status = 200, body = ProjectDto),
        (status = 400, description = "Invalid comment policy/language update or locked target language", body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 409, description = "Project language resolution is required", body = ErrorResponse),
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
    let comment_policy = req
        .comment_policy
        .unwrap_or_else(|| before.comment_policy.clone());
    if !matches!(comment_policy.as_str(), "private" | "internal" | "public") {
        return Err(Error::bad_request("invalid_comment_policy").into());
    }
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
    let mut changed_fields = Vec::with_capacity(6);
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
    if comment_policy != before.comment_policy {
        changed_fields.push("comment_policy");
    }
    let updated = prts_db::projects::update_tx(
        &mut tx,
        id,
        &name,
        &description,
        &visibility,
        &source_langs,
        &target_lang,
        &comment_policy,
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

const DELETE_CHALLENGE_TTL_SECONDS: usize = 300;

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteChallengeDto {
    pub challenge_id: String,
    pub prompt: String,
    pub expires_in: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeletionStatusDto {
    pub project_id: i64,
    pub slug: String,
    pub deletion_scheduled_at: String,
    pub deletion_job_id: i64,
}

#[derive(Deserialize, ToSchema)]
pub struct ScheduleDeletionReq {
    pub challenge_id: String,
    pub answer: i64,
}

#[derive(Serialize, Deserialize)]
struct StoredDeleteChallenge {
    user_id: i64,
    project_id: i64,
    expected_answer: i64,
}

fn delete_challenge_key(challenge_id: &str) -> String {
    format!("prts:project-delete-challenge:{challenge_id}")
}

async fn consume_delete_challenge(
    cache: &prts_db::Cache,
    challenge_id: &str,
) -> Result<Option<StoredDeleteChallenge>, ApiError> {
    let mut cache = cache.clone();
    let raw: Option<String> = redis::cmd("GETDEL")
        .arg(delete_challenge_key(challenge_id))
        .query_async(&mut cache)
        .await
        .map_err(|_| Error::internal("delete challenge cache unavailable"))?;
    raw.map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|_| Error::validation("PROJECT_DELETE_CHALLENGE_INVALID").into())
}

#[utoipa::path(
    post,
    path = "/projects/{id}/delete-challenge",
    tag = "project",
    description = "仅唯一 owner 可领取短 TTL、绑定 user/project 的一次性整数数学 challenge。答案不返回、不审计、不写 job。",
    responses(
        (status = 200, body = DeleteChallengeDto),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse)
    )
)]
pub async fn create_delete_challenge(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<DeleteChallengeDto>, ApiError> {
    let project = prts_db::projects::find_by_id(&state.db, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if project.owner_id != user.id || project.deletion_scheduled_at.is_some() {
        return Err(Error::Forbidden.into());
    }
    let mode = match prts_db::settings::get(&state.db, "project_delete_challenge_mode")
        .await
        .map_err(db_err)?
        .and_then(|value| value.as_str().map(str::to_owned))
        .as_deref()
    {
        Some("simple") => prts_core::delete_challenge::ChallengeMode::Simple,
        _ => prts_core::delete_challenge::ChallengeMode::Advanced,
    };
    let challenge_id = prts_auth::token::random_token(40);
    let seed = challenge_id.bytes().fold(0_u64, |value, byte| {
        value.wrapping_mul(131).wrapping_add(u64::from(byte))
    });
    let plan = prts_core::delete_challenge::generate_challenge(mode, seed);
    let stored = StoredDeleteChallenge {
        user_id: user.id,
        project_id: id,
        expected_answer: prts_core::delete_challenge::answer(&plan),
    };
    let mut cache = state.cache.clone();
    let _: () = redis::cmd("SET")
        .arg(delete_challenge_key(&challenge_id))
        .arg(serde_json::to_string(&stored).map_err(|_| Error::internal("challenge encode"))?)
        .arg("EX")
        .arg(DELETE_CHALLENGE_TTL_SECONDS)
        .arg("NX")
        .query_async(&mut cache)
        .await
        .map_err(|_| Error::internal("delete challenge cache unavailable"))?;
    Ok(Json(DeleteChallengeDto {
        challenge_id,
        prompt: plan.prompt(),
        expires_in: DELETE_CHALLENGE_TTL_SECONDS,
    }))
}

/// 删除项目。需项目「删除」权限。
#[utoipa::path(delete, path = "/projects/{id}", tag = "project",
    description = "消费绑定当前 owner/project 的一次性 challenge，并在同一事务创建 project_purge job、设置 24 小时待删除状态和 allowlisted audit。",
    request_body = ScheduleDeletionReq,
    responses(
        (status = 202, body = DeletionStatusDto),
        (status = 400, body = ErrorResponse),
        (status = 403),
        (status = 404),
        (status = 503, description = "审计服务不可用，项目未删除", body = ErrorResponse)
    ))]
pub async fn delete_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(req): Json<ScheduleDeletionReq>,
) -> Result<(StatusCode, Json<DeletionStatusDto>), ApiError> {
    let challenge = consume_delete_challenge(&state.cache, &req.challenge_id)
        .await?
        .ok_or_else(|| Error::validation("PROJECT_DELETE_CHALLENGE_INVALID"))?;
    if challenge.user_id != user.id
        || challenge.project_id != id
        || challenge.expected_answer != req.answer
    {
        return Err(Error::validation("PROJECT_DELETE_CHALLENGE_INVALID").into());
    }
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let before = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if before.owner_id != user.id {
        return Err(Error::Forbidden.into());
    }
    if before.deletion_scheduled_at.is_some() {
        return Err(Error::ProjectPendingDeletion.into());
    }
    let scheduled_at = chrono::Utc::now() + chrono::Duration::hours(24);
    let media_keys = before.avatar_key.iter().cloned().collect();
    let temp_keys = prts_db::projects::purge_temp_keys_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    let purge_job = prts_db::jobs::create_tx(
        &mut tx,
        prts_db::jobs::NewJob {
            kind: prts_db::jobs::JobKind::ProjectPurge(prts_db::jobs::ProjectPurgeSnapshot {
                project_id: id,
                slug: before.slug.clone(),
                media_keys,
                temp_keys,
                deadline: scheduled_at,
            }),
            project_id: Some(id),
            stage: "awaiting_deadline".to_string(),
            progress_total: None,
            max_attempts: 10,
            run_after: scheduled_at,
        },
    )
    .await
    .map_err(db_err)?;
    let pending =
        prts_db::projects::schedule_deletion_tx(&mut tx, id, user.id, purge_job.id, scheduled_at)
            .await
            .map_err(db_err)?;
    prts_db::jobs::pause_for_pending_projects_tx(&mut tx, &[id])
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ProjectDeletionScheduled {
            project_id: id,
            slug: &before.slug,
            deletion_job_id: purge_job.id,
            scheduled_at,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
    Ok((
        StatusCode::ACCEPTED,
        Json(DeletionStatusDto {
            project_id: id,
            slug: pending.slug,
            deletion_scheduled_at: scheduled_at.to_rfc3339(),
            deletion_job_id: purge_job.id,
        }),
    ))
}

#[utoipa::path(get, path = "/projects/{id}/deletion", tag = "project",
    description = "仅唯一 owner 可读取待删除倒计时；其它主体按项目不可见处理。",
    responses((status = 200, body = DeletionStatusDto), (status = 404, body = ErrorResponse)))]
pub async fn deletion_status(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<DeletionStatusDto>, ApiError> {
    let project = prts_db::projects::find_by_id(&state.db, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if project.owner_id != user.id {
        return Err(Error::NotFound.into());
    }
    let scheduled_at = project.deletion_scheduled_at.ok_or(Error::NotFound)?;
    let job_id = project.deletion_job_id.ok_or(Error::NotFound)?;
    Ok(Json(DeletionStatusDto {
        project_id: id,
        slug: project.slug,
        deletion_scheduled_at: scheduled_at.to_rfc3339(),
        deletion_job_id: job_id,
    }))
}

#[utoipa::path(post, path = "/projects/{id}/deletion/cancel", tag = "project",
    description = "仅唯一 owner 可在 deadline 前取消；事务内锁定 project/job、重验 owner/state，恢复普通 jobs 并写 allowlisted audit。",
    responses((status = 204), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse)))]
pub async fn cancel_deletion(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if project.owner_id != user.id {
        return Err(Error::Forbidden.into());
    }
    let deadline = project.deletion_scheduled_at.ok_or(Error::NotFound)?;
    if deadline <= chrono::Utc::now() {
        return Err(Error::Conflict.into());
    }
    let job_id = project.deletion_job_id.ok_or(Error::NotFound)?;
    prts_db::jobs::find_by_id_for_update_tx(&mut tx, job_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if !prts_db::jobs::cancel_purge_tx(&mut tx, job_id, id)
        .await
        .map_err(db_err)?
    {
        return Err(Error::Conflict.into());
    }
    prts_db::projects::cancel_deletion_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    prts_db::jobs::resume_project_jobs_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ProjectDeletionCancelled {
            project_id: id,
            slug: &project.slug,
            deletion_job_id: job_id,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
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
    pub capabilities: MemberCapabilitiesDto,
}

/// 当前 actor 针对单个项目成员可执行的操作。
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberCapabilitiesDto {
    pub assignable_roles: Vec<String>,
    pub can_change_role: bool,
    pub can_remove: bool,
}

/// 列出项目成员。
#[utoipa::path(
    get,
    path = "/projects/{id}/members",
    tag = "project",
    description = "列出可见项目的成员，并为当前主体下发逐目标角色修改与移除 capability。前端不得从角色名称推导权限。",
    responses(
        (status = 200, description = "成员列表与逐目标 capability", body = [MemberDto]),
        (status = 404, description = "项目不存在或对当前主体不可见", body = ErrorResponse)
    )
)]
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
    let actor_id = user.as_ref().map(|actor| actor.id);
    let actor_membership = match actor_id {
        Some(actor_id) => prts_db::memberships::find_role(&state.db, id, actor_id)
            .await
            .map_err(db_err)?
            .as_deref()
            .and_then(ProjectRole::parse),
        None => None,
    };
    let actor_can_manage_all_projects = user
        .as_ref()
        .is_some_and(|actor| actor.has_platform(nodes::PLATFORM_PROJECT_MANAGE_ALL));
    let project_owner_id = access.project.owner_id;
    Ok(Json(
        members
            .into_iter()
            .map(|member| {
                let target_membership = ProjectRole::parse(&member.role);
                let assignable_roles = [
                    ProjectRole::Manager,
                    ProjectRole::Reviewer,
                    ProjectRole::Translator,
                ]
                .into_iter()
                .filter(|requested_role| {
                    actor_id.is_some_and(|actor_id| {
                        authorize_membership_mutation(
                            MembershipMutation::Upsert,
                            project_owner_id,
                            actor_id,
                            actor_membership,
                            actor_can_manage_all_projects,
                            member.user_id,
                            target_membership,
                            Some(*requested_role),
                        ) == MembershipDecision::Allow
                    })
                })
                .map(|role| role.as_str().to_string())
                .collect::<Vec<_>>();
                let can_remove = actor_id.is_some_and(|actor_id| {
                    authorize_membership_mutation(
                        MembershipMutation::Remove,
                        project_owner_id,
                        actor_id,
                        actor_membership,
                        actor_can_manage_all_projects,
                        member.user_id,
                        target_membership,
                        None,
                    ) == MembershipDecision::Allow
                });
                MemberDto {
                    user_id: member.user_id,
                    username: member.username,
                    avatar_url: member.avatar_url,
                    role: member.role,
                    created_at: member.created_at.to_rfc3339(),
                    capabilities: MemberCapabilitiesDto {
                        can_change_role: !assignable_roles.is_empty(),
                        assignable_roles,
                        can_remove,
                    },
                }
            })
            .collect(),
    ))
}

fn membership_decision_error(decision: MembershipDecision) -> Result<(), ApiError> {
    match decision {
        MembershipDecision::Allow => Ok(()),
        MembershipDecision::OwnerTransferForbidden => {
            Err(Error::validation("PROJECT_OWNER_TRANSFER_FORBIDDEN").into())
        }
        MembershipDecision::Forbidden => Err(Error::Forbidden.into()),
        MembershipDecision::TargetNotFound => Err(Error::NotFound.into()),
    }
}

/// 添加/更新成员请求。
#[derive(Debug, Deserialize, ToSchema)]
pub struct AddMemberReq {
    pub username: String,
    /// manager | reviewer | translator；本轮不支持拥有者转让。
    pub role: String,
}

/// 添加或更新项目成员。需项目「成员管理」权限。
#[utoipa::path(post, path = "/projects/{id}/members", tag = "project",
    description = "新增成员或更新其角色。事务内锁定项目并重新读取 actor、target 与唯一 owner，再应用 prts-core typed 授权矩阵并原子写入 allowlisted audit。",
    request_body = AddMemberReq,
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
    let requested_role = ProjectRole::parse(&req.role)
        .ok_or_else(|| Error::validation("PROJECT_MEMBER_ROLE_INVALID"))?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let actor = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;
    let actor_membership = prts_db::memberships::find_role_tx(&mut tx, id, user.id)
        .await
        .map_err(db_err)?
        .as_deref()
        .and_then(ProjectRole::parse);
    let target = prts_db::users::find_by_username_for_update_tx(&mut tx, req.username.trim())
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let previous_role = prts_db::memberships::find_role_tx(&mut tx, id, target.id)
        .await
        .map_err(db_err)?;
    membership_decision_error(authorize_membership_mutation(
        MembershipMutation::Upsert,
        project.owner_id,
        user.id,
        actor_membership,
        actor
            .platform_role
            .as_deref()
            .and_then(prts_core::PlatformRole::parse)
            .is_some_and(|role| role.has(nodes::PLATFORM_PROJECT_MANAGE_ALL)),
        target.id,
        previous_role.as_deref().and_then(ProjectRole::parse),
        Some(requested_role),
    ))?;
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
    description = "移除非 owner 项目成员。事务内锁定项目并重新读取 actor、target 与角色，应用 prts-core typed 授权矩阵后与 allowlisted audit 原子提交。",
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
    let actor = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;
    let actor_membership = prts_db::memberships::find_role_tx(&mut tx, id, user.id)
        .await
        .map_err(db_err)?
        .as_deref()
        .and_then(ProjectRole::parse);
    prts_db::users::find_by_id_for_update_tx(&mut tx, user_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let previous_role = prts_db::memberships::find_role_tx(&mut tx, id, user_id)
        .await
        .map_err(db_err)?;
    membership_decision_error(authorize_membership_mutation(
        MembershipMutation::Remove,
        project.owner_id,
        user.id,
        actor_membership,
        actor
            .platform_role
            .as_deref()
            .and_then(prts_core::PlatformRole::parse)
            .is_some_and(|role| role.has(nodes::PLATFORM_PROJECT_MANAGE_ALL)),
        user_id,
        previous_role.as_deref().and_then(ProjectRole::parse),
        None,
    ))?;
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
