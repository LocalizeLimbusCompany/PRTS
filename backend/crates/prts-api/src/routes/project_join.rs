//! Public-project self-service joining and manager application decisions.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use prts_common::Error;
use prts_core::permission::{
    authorize_membership_mutation, nodes, MembershipDecision, MembershipMutation,
};
use prts_core::ProjectRole;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};
use prts_realtime::UserEvent;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth::{project as paccess, CurrentUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

const JOIN_WINDOW_SECONDS: u64 = 15 * 60;
const JOIN_USER_FAILURE_LIMIT: i64 = 5;
const JOIN_IP_FAILURE_LIMIT: i64 = 20;

#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectJoinInfoDto {
    pub join_policy: String,
    pub join_default_role: String,
    pub quiz_question: Option<String>,
    pub is_member: bool,
    pub pending_application_id: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectJoinSettingsDto {
    pub join_policy: String,
    pub join_default_role: String,
    pub history_visibility: String,
    pub password_configured: bool,
    pub quiz_question: Option<String>,
    pub quiz_answer_configured: bool,
    pub active: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProjectJoinSettingsReq {
    pub join_policy: String,
    pub join_default_role: String,
    pub history_visibility: String,
    /// Omit to retain the existing Argon2 hash. Plaintext is never returned or audited.
    pub password: Option<String>,
    pub quiz_question: Option<String>,
    /// Omit to retain the existing strict, case-sensitive answer hash.
    pub quiz_answer: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct JoinProjectReq {
    pub password: Option<String>,
    pub answer: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JoinProjectResultDto {
    pub status: String,
    pub role: Option<String>,
    pub application_id: Option<i64>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct JoinApplicationListQuery {
    pub after: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JoinApplicationDto {
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JoinApplicationPageDto {
    pub items: Vec<JoinApplicationDto>,
    pub next_after: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DecideJoinApplicationReq {
    pub approved: bool,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct MemberCandidateQuery {
    pub q: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MemberCandidateDto {
    pub user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
}

#[utoipa::path(get, path = "/projects/{id}/join", tag = "project",
    description = "Return only non-sensitive self-service join metadata for a public project. Private projects return 404 and never expose their join mechanism.",
    responses((status = 200, body = ProjectJoinInfoDto), (status = 404, body = ErrorResponse)))]
pub async fn get_join_info(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<ProjectJoinInfoDto>, ApiError> {
    let project = prts_db::projects::find_by_id(&state.db, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if project.visibility != "public" || project.deletion_scheduled_at.is_some() {
        return Err(Error::NotFound.into());
    }
    let is_member = prts_db::memberships::find_role(&state.db, id, user.id)
        .await
        .map_err(db_err)?
        .is_some();
    let pending = prts_db::project_join::pending_for_user(&state.db, id, user.id)
        .await
        .map_err(db_err)?;
    Ok(Json(ProjectJoinInfoDto {
        quiz_question: (project.join_policy == "quiz")
            .then_some(project.join_quiz_question)
            .flatten(),
        join_policy: project.join_policy,
        join_default_role: project.join_default_role,
        is_member,
        pending_application_id: pending.map(|application| application.id),
    }))
}

#[utoipa::path(get, path = "/projects/{id}/join-settings", tag = "project",
    description = "Read or atomically update project join/history policy. Secret hashes and plaintext credentials are never returned or audited; private projects retain settings but self-service remains inactive.",
    responses((status = 200, body = ProjectJoinSettingsDto), (status = 400, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse)))]
pub async fn get_join_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<ProjectJoinSettingsDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    Ok(Json(join_settings_dto(&access.project)))
}

#[utoipa::path(put, path = "/projects/{id}/join-settings", tag = "project",
    request_body = UpdateProjectJoinSettingsReq,
    description = "Atomically update project join/history policy. Password and strict quiz answer are Argon2-hashed before storage and never included in audit payloads.",
    responses((status = 200, body = ProjectJoinSettingsDto), (status = 400, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)))]
pub async fn put_join_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(request): Json<UpdateProjectJoinSettingsReq>,
) -> Result<Json<ProjectJoinSettingsDto>, ApiError> {
    validate_join_policy(&request.join_policy)?;
    validate_default_role(&request.join_default_role)?;
    validate_history_visibility(&request.history_visibility)?;
    let password_hash = hash_optional_secret(request.password.as_deref(), 8, 256)?;
    let quiz_answer_hash = hash_optional_secret(request.quiz_answer.as_deref(), 1, 256)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let before = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, before.clone())
        .await?
        .require_node(nodes::PROJECT_MANAGE)?;
    let quiz_question = request
        .quiz_question
        .as_deref()
        .map(str::trim)
        .filter(|question| !question.is_empty())
        .map(str::to_string)
        .or_else(|| before.join_quiz_question.clone());
    if quiz_question
        .as_ref()
        .is_some_and(|question| question.chars().count() > 500)
    {
        return Err(Error::validation("PROJECT_JOIN_QUIZ_INVALID").into());
    }
    if request.join_policy == "password"
        && password_hash.is_none()
        && before.join_password_hash.is_none()
    {
        return Err(Error::validation("PROJECT_JOIN_PASSWORD_REQUIRED").into());
    }
    if request.join_policy == "quiz"
        && (quiz_question.is_none()
            || (quiz_answer_hash.is_none() && before.join_quiz_answer_hash.is_none()))
    {
        return Err(Error::validation("PROJECT_JOIN_QUIZ_INVALID").into());
    }
    let mut changed_fields = Vec::with_capacity(6);
    if before.join_policy != request.join_policy {
        changed_fields.push("join_policy");
    }
    if before.join_default_role != request.join_default_role {
        changed_fields.push("join_default_role");
    }
    if before.history_visibility != request.history_visibility {
        changed_fields.push("history_visibility");
    }
    if password_hash.is_some() {
        changed_fields.push("password");
    }
    if quiz_question != before.join_quiz_question {
        changed_fields.push("quiz_question");
    }
    if quiz_answer_hash.is_some() {
        changed_fields.push("quiz_answer");
    }
    let updated = prts_db::projects::update_join_settings_tx(
        &mut tx,
        id,
        &request.join_policy,
        &request.join_default_role,
        &request.history_visibility,
        password_hash.as_deref(),
        password_hash.is_none(),
        quiz_question.as_deref(),
        quiz_answer_hash.as_deref(),
        quiz_answer_hash.is_none(),
    )
    .await
    .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::ProjectJoinSettingsUpdated {
            project_id: id,
            changed_fields: &changed_fields,
            join_policy: &updated.join_policy,
            history_visibility: &updated.history_visibility,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(join_settings_dto(&updated)))
}

#[utoipa::path(post, path = "/projects/{id}/join", tag = "project",
    request_body = JoinProjectReq,
    description = "Join a public project through its current free/password/quiz/application policy. Credential failures share one response and use fixed per-user/project and per-IP/project limits.",
    responses((status = 200, body = JoinProjectResultDto), (status = 202, body = JoinProjectResultDto),
        (status = 400, body = ErrorResponse), (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse), (status = 429, body = ErrorResponse)))]
pub async fn join_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<JoinProjectReq>,
) -> Result<(StatusCode, Json<JoinProjectResultDto>), ApiError> {
    let client_ip = client_ip(&headers, peer);
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if project.visibility != "public" || project.deletion_scheduled_at.is_some() {
        return Err(Error::NotFound.into());
    }
    if prts_db::memberships::find_role_tx(&mut tx, id, user.id)
        .await
        .map_err(db_err)?
        .is_some()
    {
        return Err(Error::validation("PROJECT_ALREADY_MEMBER").into());
    }
    match project.join_policy.as_str() {
        "admin_only" => Err(Error::Forbidden.into()),
        "application" => {
            let message = request.message.unwrap_or_default().trim().to_string();
            if message.chars().count() > 500 {
                return Err(Error::validation("PROJECT_JOIN_MESSAGE_INVALID").into());
            }
            let application =
                prts_db::project_join::create_application_tx(&mut tx, id, user.id, &message)
                    .await
                    .map_err(db_err)?
                    .ok_or_else(|| Error::validation("PROJECT_JOIN_APPLICATION_PENDING"))?;
            let mut notifications = Vec::new();
            for manager_id in prts_db::project_join::manager_ids_tx(&mut tx, id)
                .await
                .map_err(db_err)?
            {
                let notification = prts_db::notifications::create_tx(
                    &mut tx,
                    manager_id,
                    "project_join_application",
                    &serde_json::json!({
                        "project_id": id,
                        "application_id": application.id,
                        "applicant_id": user.id,
                    }),
                )
                .await
                .map_err(db_err)?;
                notifications.push((manager_id, notification));
            }
            prts_db::audit::append_event_tx(
                &mut tx,
                audit_actor(&user),
                AuditEvent::ProjectJoinApplicationSubmitted {
                    project_id: id,
                    application_id: application.id,
                    applicant_id: user.id,
                },
            )
            .await
            .map_err(|_| Error::AuditUnavailable)?;
            tx.commit().await.map_err(db_err)?;
            publish_notifications(&state, notifications).await;
            Ok((
                StatusCode::ACCEPTED,
                Json(JoinProjectResultDto {
                    status: "pending".to_string(),
                    role: None,
                    application_id: Some(application.id),
                }),
            ))
        }
        policy @ ("free" | "password" | "quiz") => {
            if matches!(policy, "password" | "quiz") {
                ensure_not_rate_limited(&state, id, user.id, &client_ip).await?;
                let (provided, expected) = if policy == "password" {
                    (
                        request.password.as_deref(),
                        project.join_password_hash.as_deref(),
                    )
                } else {
                    (
                        request.answer.as_deref(),
                        project.join_quiz_answer_hash.as_deref(),
                    )
                };
                if !matches!((provided, expected), (Some(value), Some(hash)) if prts_auth::password::verify_password(value, hash))
                {
                    record_join_failure(&state, id, user.id, &client_ip).await?;
                    return Err(Error::validation("PROJECT_JOIN_CREDENTIAL_INVALID").into());
                }
            }
            prts_db::memberships::upsert_tx(&mut tx, id, user.id, &project.join_default_role)
                .await
                .map_err(db_err)?;
            prts_db::audit::append_event_tx(
                &mut tx,
                audit_actor(&user),
                AuditEvent::ProjectSelfJoined {
                    project_id: id,
                    user_id: user.id,
                    policy,
                    role: &project.join_default_role,
                },
            )
            .await
            .map_err(|_| Error::AuditUnavailable)?;
            tx.commit().await.map_err(db_err)?;
            Ok((
                StatusCode::OK,
                Json(JoinProjectResultDto {
                    status: "joined".to_string(),
                    role: Some(project.join_default_role),
                    application_id: None,
                }),
            ))
        }
        _ => Err(Error::validation("PROJECT_JOIN_POLICY_INVALID").into()),
    }
}

#[utoipa::path(delete, path = "/projects/{id}/join", tag = "project",
    description = "Withdraw a pending application, or leave the project when the caller is a non-owner member. Rejoining later restores project CP from the contribution ledger.",
    responses((status = 204), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse)))]
pub async fn withdraw_or_leave_project(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let role = prts_db::memberships::find_role_tx(&mut tx, id, user.id)
        .await
        .map_err(db_err)?;
    if let Some(role) = role {
        if project.owner_id == user.id || role == "owner" {
            return Err(Error::Forbidden.into());
        }
        prts_db::memberships::remove_tx(&mut tx, id, user.id)
            .await
            .map_err(db_err)?;
        prts_db::audit::append_event_tx(
            &mut tx,
            audit_actor(&user),
            AuditEvent::ProjectMemberLeft {
                project_id: id,
                user_id: user.id,
                previous_role: &role,
            },
        )
        .await
        .map_err(|_| Error::AuditUnavailable)?;
    } else {
        let application = prts_db::project_join::withdraw_tx(&mut tx, id, user.id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
        prts_db::audit::append_event_tx(
            &mut tx,
            audit_actor(&user),
            AuditEvent::ProjectJoinApplicationWithdrawn {
                project_id: id,
                application_id: application.id,
                applicant_id: user.id,
            },
        )
        .await
        .map_err(|_| Error::AuditUnavailable)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/projects/{id}/join-applications", tag = "project",
    params(JoinApplicationListQuery),
    description = "List pending applications with descending-id keyset pagination. Only project managers can read it.",
    responses((status = 200, body = JoinApplicationPageDto), (status = 403, body = ErrorResponse)))]
pub async fn list_join_applications(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<JoinApplicationListQuery>,
) -> Result<Json<JoinApplicationPageDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let rows = prts_db::project_join::list_pending(&state.db, id, query.after, limit)
        .await
        .map_err(db_err)?;
    let next_after = (rows.len() as i64 == limit)
        .then(|| rows.last().map(|application| application.id))
        .flatten();
    Ok(Json(JoinApplicationPageDto {
        items: rows
            .into_iter()
            .map(|application| JoinApplicationDto {
                id: application.id,
                user_id: application.user_id,
                username: application.username,
                avatar_url: application.avatar_url,
                message: application.message,
                created_at: application.created_at.to_rfc3339(),
            })
            .collect(),
        next_after,
    }))
}

#[utoipa::path(post, path = "/projects/{id}/join-applications/{application_id}/decision", tag = "project",
    request_body = DecideJoinApplicationReq,
    description = "Approve or reject one pending application under the actor's current assignable-role boundary. Decision, membership, notification and audit commit atomically.",
    responses((status = 204), (status = 400, body = ErrorResponse),
        (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse)))]
pub async fn decide_join_application(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, application_id)): Path<(i64, i64)>,
    Json(request): Json<DecideJoinApplicationReq>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let access = paccess::load_locked_tx(&mut tx, &user, project.clone()).await?;
    access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;
    let application =
        prts_db::project_join::find_pending_for_update_tx(&mut tx, id, application_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
    if prts_db::memberships::find_role_tx(&mut tx, id, application.user_id)
        .await
        .map_err(db_err)?
        .is_some()
    {
        return Err(Error::Conflict.into());
    }
    let role = if request.approved {
        let role = request
            .role
            .as_deref()
            .unwrap_or(&project.join_default_role);
        let requested_role = ProjectRole::parse(role)
            .ok_or_else(|| Error::validation("PROJECT_JOIN_ROLE_INVALID"))?;
        let actor_role = prts_db::memberships::find_role_tx(&mut tx, id, user.id)
            .await
            .map_err(db_err)?
            .as_deref()
            .and_then(ProjectRole::parse);
        let decision = authorize_membership_mutation(
            MembershipMutation::Upsert,
            project.owner_id,
            user.id,
            actor_role,
            user.has_platform(nodes::PLATFORM_PROJECT_MANAGE_ALL),
            application.user_id,
            None,
            Some(requested_role),
        );
        if decision != MembershipDecision::Allow {
            return Err(Error::Forbidden.into());
        }
        prts_db::memberships::upsert_tx(&mut tx, id, application.user_id, role)
            .await
            .map_err(db_err)?;
        Some(role.to_string())
    } else {
        None
    };
    prts_db::project_join::decide_tx(
        &mut tx,
        application.id,
        user.id,
        request.approved,
        role.as_deref(),
    )
    .await
    .map_err(db_err)?;
    let notification = prts_db::notifications::create_tx(
        &mut tx,
        application.user_id,
        "project_join_decision",
        &serde_json::json!({
            "project_id": id,
            "application_id": application.id,
            "approved": request.approved,
            "role": role,
        }),
    )
    .await
    .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        audit_actor(&user),
        AuditEvent::ProjectJoinApplicationDecided {
            project_id: id,
            application_id: application.id,
            applicant_id: application.user_id,
            approved: request.approved,
            role: role.as_deref(),
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    publish_notifications(&state, vec![(application.user_id, notification)]).await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/projects/{id}/member-candidates", tag = "project",
    params(MemberCandidateQuery),
    description = "Autocomplete active non-members: numeric input is an exact UID lookup; username input requires at least two characters and returns at most 20 rows without COUNT.",
    responses((status = 200, body = [MemberCandidateDto]), (status = 400, body = ErrorResponse),
        (status = 403, body = ErrorResponse)))]
pub async fn member_candidates(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<MemberCandidateQuery>,
) -> Result<Json<Vec<MemberCandidateDto>>, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MEMBER_MANAGE)?;
    let query = query.q.trim();
    if query.chars().count() > 64 {
        return Err(Error::validation("PROJECT_MEMBER_QUERY_INVALID").into());
    }
    if query.parse::<i64>().is_err() && query.chars().count() < 2 {
        return Err(Error::validation("PROJECT_MEMBER_QUERY_TOO_SHORT").into());
    }
    let rows = prts_db::project_join::member_candidates(&state.db, id, query)
        .await
        .map_err(db_err)?;
    Ok(Json(
        rows.into_iter()
            .map(|candidate| MemberCandidateDto {
                user_id: candidate.user_id,
                username: candidate.username,
                avatar_url: candidate.avatar_url,
            })
            .collect(),
    ))
}

fn join_settings_dto(project: &prts_db::models::Project) -> ProjectJoinSettingsDto {
    ProjectJoinSettingsDto {
        join_policy: project.join_policy.clone(),
        join_default_role: project.join_default_role.clone(),
        history_visibility: project.history_visibility.clone(),
        password_configured: project.join_password_hash.is_some(),
        quiz_question: project.join_quiz_question.clone(),
        quiz_answer_configured: project.join_quiz_answer_hash.is_some(),
        active: project.visibility == "public",
    }
}

fn validate_join_policy(value: &str) -> Result<(), ApiError> {
    if matches!(
        value,
        "application" | "free" | "admin_only" | "password" | "quiz"
    ) {
        Ok(())
    } else {
        Err(Error::validation("PROJECT_JOIN_POLICY_INVALID").into())
    }
}

fn validate_default_role(value: &str) -> Result<(), ApiError> {
    if matches!(value, "translator" | "reviewer") {
        Ok(())
    } else {
        Err(Error::validation("PROJECT_JOIN_ROLE_INVALID").into())
    }
}

fn validate_history_visibility(value: &str) -> Result<(), ApiError> {
    if matches!(value, "viewers" | "members" | "managers") {
        Ok(())
    } else {
        Err(Error::validation("PROJECT_HISTORY_VISIBILITY_INVALID").into())
    }
}

fn hash_optional_secret(
    value: Option<&str>,
    min_chars: usize,
    max_chars: usize,
) -> Result<Option<String>, ApiError> {
    let Some(value) = value else { return Ok(None) };
    let count = value.chars().count();
    if !(min_chars..=max_chars).contains(&count) {
        return Err(Error::validation("PROJECT_JOIN_SECRET_INVALID").into());
    }
    prts_auth::password::hash_password(value)
        .map(Some)
        .map_err(|_| Error::internal("join secret hash failed").into())
}

fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| value.parse::<std::net::IpAddr>().is_ok())
        .map(str::to_string)
        .unwrap_or_else(|| peer.ip().to_string())
}

fn rate_keys(project_id: i64, user_id: i64, ip: &str) -> (String, String) {
    let ip_hash = prts_auth::token::sha256_hex(ip);
    (
        format!("prts:join-failure:user:{project_id}:{user_id}"),
        format!("prts:join-failure:ip:{project_id}:{ip_hash}"),
    )
}

async fn ensure_not_rate_limited(
    state: &AppState,
    project_id: i64,
    user_id: i64,
    ip: &str,
) -> Result<(), ApiError> {
    let (user_key, ip_key) = rate_keys(project_id, user_id, ip);
    let mut cache = state.cache.clone();
    let (user_failures, ip_failures): (Option<i64>, Option<i64>) = redis::pipe()
        .get(user_key)
        .get(ip_key)
        .query_async(&mut cache)
        .await
        .map_err(|_| Error::internal("join rate limit unavailable"))?;
    if user_failures.unwrap_or(0) >= JOIN_USER_FAILURE_LIMIT
        || ip_failures.unwrap_or(0) >= JOIN_IP_FAILURE_LIMIT
    {
        Err(Error::validation("PROJECT_JOIN_RATE_LIMITED").into())
    } else {
        Ok(())
    }
}

async fn record_join_failure(
    state: &AppState,
    project_id: i64,
    user_id: i64,
    ip: &str,
) -> Result<(), ApiError> {
    let (user_key, ip_key) = rate_keys(project_id, user_id, ip);
    let script = redis::Script::new(
        "local value = redis.call('INCR', KEYS[1]); if value == 1 then redis.call('EXPIRE', KEYS[1], ARGV[1]) end; return value",
    );
    let mut cache = state.cache.clone();
    let _: i64 = script
        .key(user_key)
        .arg(JOIN_WINDOW_SECONDS)
        .invoke_async(&mut cache)
        .await
        .map_err(|_| Error::internal("join rate limit unavailable"))?;
    let _: i64 = script
        .key(ip_key)
        .arg(JOIN_WINDOW_SECONDS)
        .invoke_async(&mut cache)
        .await
        .map_err(|_| Error::internal("join rate limit unavailable"))?;
    Ok(())
}

async fn publish_notifications(
    state: &AppState,
    notifications: Vec<(i64, prts_db::models::Notification)>,
) {
    for (user_id, notification) in notifications {
        state
            .realtime
            .publish_user(
                user_id,
                &UserEvent::Notification {
                    id: notification.id,
                    kind: notification.kind,
                    payload: notification.payload,
                },
            )
            .await;
    }
}

fn audit_actor(user: &CurrentUser) -> AuditActor<'static> {
    AuditActor {
        id: Some(user.id),
        kind: if user.credential_kind == crate::auth::CredentialKind::ApiKey {
            AuditActorKind::ApiKey
        } else {
            AuditActorKind::User
        },
        ip: None,
    }
}
