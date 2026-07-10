//! 持久化任务进度与受控手动重试端点。
//!
//! handler 只做协议、资源鉴权与事务编排；所有持久化 mutation 位于 `prts-db`。

use axum::extract::{Path, Query, State};
use axum::Json;
use prts_common::Error;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::{project as paccess, CurrentUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse, RequestLocale};
use crate::state::AppState;

/// 对外任务进度。安全起见不暴露 payload/result、worker id 或内部租约。
#[derive(Debug, Serialize, ToSchema)]
pub struct JobDto {
    pub id: i64,
    pub kind: String,
    pub project_id: Option<i64>,
    pub state: String,
    pub stage: String,
    pub progress_current: i64,
    pub progress_total: Option<i64>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry_at: Option<String>,
    pub last_error_code: Option<String>,
    pub manual_retry_allowed: bool,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

impl JobDto {
    fn from_job(job: prts_db::models::Job, manual_retry_allowed: bool) -> Self {
        let next_retry_at =
            (job.state == "queued" && job.attempts > 0).then(|| job.run_after.to_rfc3339());
        Self {
            id: job.id,
            kind: job.kind,
            project_id: job.project_id,
            state: job.state,
            stage: job.stage,
            progress_current: job.progress_current,
            progress_total: job.progress_total,
            attempts: job.attempts,
            max_attempts: job.max_attempts,
            next_retry_at,
            last_error_code: job.last_error_code,
            manual_retry_allowed,
            created_at: job.created_at.to_rfc3339(),
            started_at: job.started_at.map(|value| value.to_rfc3339()),
            finished_at: job.finished_at.map(|value| value.to_rfc3339()),
            updated_at: job.updated_at.to_rfc3339(),
        }
    }
}

/// 项目任务键集查询参数。
#[derive(Debug, Deserialize)]
pub struct ListJobsQuery {
    pub after_id: Option<i64>,
    pub kind: Option<String>,
    pub state: Option<String>,
    pub limit: Option<i64>,
}

/// 项目任务键集列表。
#[derive(Debug, Serialize, ToSchema)]
pub struct JobListDto {
    pub items: Vec<JobDto>,
    pub next_after: Option<i64>,
}

/// 对外分页上限严格为 1..=100；非法值返回 400，不能静默改写用户请求。
fn validated_limit(limit: Option<i64>) -> Result<i64, Error> {
    let limit = limit.unwrap_or(50);
    if (1..=100).contains(&limit) {
        Ok(limit)
    } else {
        Err(Error::bad_request("limit must be between 1 and 100"))
    }
}

/// 按稳定 id 查询任务进度。必须先通过任务所属项目的现有可见性鉴权。
#[utoipa::path(
    get,
    path = "/jobs/{id}",
    tag = "job",
    params(("id" = i64, Path, description = "稳定任务 ID")),
    responses(
        (status = 200, description = "任务进度", body = JobDto),
        (status = 401, description = "必须登录后按所属资源鉴权", body = ErrorResponse),
        (status = 403, description = "缺少该任务 kind 的业务 capability", body = ErrorResponse),
        (status = 404, description = "任务、所属资源或未知 kind 不可见", body = ErrorResponse)
    )
)]
pub async fn get_job(
    State(state): State<AppState>,
    RequestLocale(locale): RequestLocale,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<JobDto>, ApiError> {
    let result: Result<Json<JobDto>, ApiError> = async {
        let job = prts_db::jobs::find_by_id(&state.db, id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
        let retry_allowed = authorize_job_view(&state, &user, &job).await?;
        Ok(Json(JobDto::from_job(job, retry_allowed)))
    }
    .await;
    result.map_err(|error| error.with_locale(locale))
}

/// 按所属项目过滤任务进度，使用 `id DESC` 键集分页。
#[utoipa::path(
    get,
    path = "/projects/{project_id}/jobs",
    tag = "job",
    params(
        ("project_id" = i64, Path, description = "所属项目 ID"),
        ("after_id" = Option<i64>, Query, description = "上一页末尾任务 ID"),
        ("kind" = Option<String>, Query, description = "任务 kind 精确筛选"),
        ("state" = Option<String>, Query, description = "任务状态精确筛选"),
        ("limit" = Option<i64>, Query, description = "1..100，默认 50")
    ),
    responses(
        (status = 200, description = "项目任务进度", body = JobListDto),
        (status = 400, description = "筛选参数无效", body = ErrorResponse),
        (status = 401, description = "必须登录", body = ErrorResponse),
        (status = 403, description = "缺少任务 kind 的业务 capability", body = ErrorResponse),
        (status = 404, description = "项目、kind 或 capability 不可见", body = ErrorResponse)
    )
)]
pub async fn list_project_jobs(
    State(state): State<AppState>,
    RequestLocale(locale): RequestLocale,
    user: CurrentUser,
    Path(project_id): Path<i64>,
    Query(query): Query<ListJobsQuery>,
) -> Result<Json<JobListDto>, ApiError> {
    let result: Result<Json<JobListDto>, ApiError> = async {
        let access = paccess::load(&state, Some(&user), project_id).await?;
        access.require_view()?;
        if let Some(state_filter) = query.state.as_deref() {
            if prts_core::JobState::parse(state_filter).is_none() {
                return Err(Error::bad_request("invalid job state").into());
            }
        }
        let allowed_kinds: Vec<String> = if let Some(kind) = query.kind.as_deref() {
            prts_core::jobs::job_view_policy(kind).ok_or(Error::NotFound)?;
            if !can_view_job(&access, &user, kind) {
                return Err(Error::NotFound.into());
            }
            vec![kind.to_string()]
        } else {
            prts_core::jobs::KNOWN_JOB_KINDS
                .iter()
                .copied()
                .filter(|kind| can_view_job(&access, &user, kind))
                .map(str::to_string)
                .collect()
        };
        let limit = validated_limit(query.limit)?;
        let mut jobs = prts_db::jobs::list_by_project(
            &state.db,
            project_id,
            query.after_id,
            &allowed_kinds,
            query.kind.as_deref(),
            query.state.as_deref(),
            limit + 1,
        )
        .await
        .map_err(db_err)?;
        let has_more = jobs.len() > limit as usize;
        jobs.truncate(limit as usize);
        let next_after = has_more.then(|| jobs.last().expect("non-empty visible page").id);
        let items = jobs
            .into_iter()
            .map(|job| {
                let retry_allowed =
                    prts_core::jobs::manual_retry_policy(&job.kind).is_some_and(|policy| {
                        prts_core::JobState::parse(&job.state)
                            .is_some_and(prts_core::JobState::manual_retry_allowed)
                            && (!policy.owner_only || user.id == access.project.owner_id)
                            && access.has_node(policy.permission_node)
                    });
                JobDto::from_job(job, retry_allowed)
            })
            .collect();
        Ok(Json(JobListDto { items, next_after }))
    }
    .await;
    result.map_err(|error| error.with_locale(locale))
}

/// 手动重试失败任务。复用原 job id，并在同一事务追加 allowlisted 审计。
#[utoipa::path(
    post,
    path = "/jobs/{id}/retry",
    tag = "job",
    params(("id" = i64, Path, description = "稳定任务 ID")),
    responses(
        (status = 200, description = "已重新排队", body = JobDto),
        (status = 400, description = "当前状态不可重试", body = ErrorResponse),
        (status = 401, description = "必须登录", body = ErrorResponse),
        (status = 403, description = "缺少任务所属资源的业务权限", body = ErrorResponse),
        (status = 404, description = "任务或所属资源不存在", body = ErrorResponse)
    )
)]
pub async fn retry_job(
    State(state): State<AppState>,
    RequestLocale(locale): RequestLocale,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<JobDto>, ApiError> {
    let result: Result<Json<JobDto>, ApiError> = async {
        let mut tx = state.db.begin().await.map_err(db_err)?;
        let current = prts_db::jobs::find_by_id_for_update_tx(&mut tx, id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
        prts_core::jobs::job_view_policy(&current.kind).ok_or(Error::NotFound)?;
        let project_id = current.project_id.ok_or(Error::NotFound)?;
        let access = paccess::load(&state, Some(&user), project_id).await?;
        access.require_view()?;
        if !can_view_job(&access, &user, &current.kind) {
            return Err(Error::Forbidden.into());
        }
        let retry_policy = prts_core::jobs::manual_retry_policy(&current.kind)
            .ok_or_else(|| Error::bad_request("manual retry is not supported for this job kind"))?;
        if retry_policy.owner_only && access.project.owner_id != user.id {
            return Err(Error::Forbidden.into());
        }
        access.require_node(retry_policy.permission_node)?;
        let state_value = prts_core::JobState::parse(&current.state)
            .ok_or_else(|| Error::internal("unknown job state persisted"))?;
        if !state_value.manual_retry_allowed() {
            return Err(Error::bad_request("job is not manually retryable").into());
        }
        let updated = prts_db::jobs::manual_retry_tx(&mut tx, id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| Error::bad_request("job state changed; retry rejected"))?;
        prts_db::audit::append_job_retried_tx(
            &mut tx,
            prts_db::audit::AuditActor {
                id: Some(user.id),
                kind: prts_db::audit::AuditActorKind::User,
                ip: None,
            },
            updated.id,
            updated.project_id,
            &updated.kind,
            current.attempts,
            updated.attempts,
        )
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        state.job_worker.wake();

        Ok(Json(JobDto::from_job(updated, false)))
    }
    .await;
    result.map_err(|error| error.with_locale(locale))
}

/// 返回当前主体是否还具备手动重试能力；读取本身始终按所属资源鉴权。
async fn authorize_job_view(
    state: &AppState,
    user: &CurrentUser,
    job: &prts_db::models::Job,
) -> Result<bool, ApiError> {
    let project_id = job.project_id.ok_or(Error::NotFound)?;
    let access = paccess::load(state, Some(user), project_id).await?;
    access.require_view()?;
    if !can_view_job(&access, user, &job.kind) {
        return Err(Error::NotFound.into());
    }
    let retry_allowed = prts_core::jobs::manual_retry_policy(&job.kind).is_some_and(|policy| {
        prts_core::JobState::parse(&job.state)
            .is_some_and(prts_core::JobState::manual_retry_allowed)
            && (!policy.owner_only || user.id == access.project.owner_id)
            && access.has_node(policy.permission_node)
    });
    Ok(retry_allowed)
}

/// 按 allowlisted kind 与现有项目 capability 判定可见性。
fn can_view_job(access: &paccess::ProjectAccess, user: &CurrentUser, kind: &str) -> bool {
    match prts_core::jobs::job_view_policy(kind) {
        Some(policy) if policy.owner_only => access.project.owner_id == user.id,
        Some(policy) => access.has_node(policy.permission_node),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_limit_is_strictly_bounded_instead_of_clamped() {
        assert_eq!(validated_limit(None).unwrap(), 50);
        assert_eq!(validated_limit(Some(1)).unwrap(), 1);
        assert_eq!(validated_limit(Some(100)).unwrap(), 100);
        assert!(validated_limit(Some(0)).is_err());
        assert!(validated_limit(Some(101)).is_err());
    }

    #[test]
    fn job_dto_never_serializes_internal_error_message() {
        let now = chrono::Utc::now();
        let job = prts_db::models::Job {
            id: 1,
            kind: "upload_process".to_string(),
            project_id: Some(2),
            state: "failed".to_string(),
            pause_reason: None,
            stage: "processing".to_string(),
            payload: serde_json::json!({}),
            result: None,
            progress_current: 0,
            progress_total: Some(1),
            attempts: 1,
            max_attempts: 3,
            run_after: now,
            lease_until: None,
            worker_id: None,
            last_error_code: Some("provider_unavailable".to_string()),
            last_error_message: Some("stack trace raw-refresh-token".to_string()),
            created_at: now,
            started_at: Some(now),
            finished_at: Some(now),
            updated_at: now,
        };
        let json = serde_json::to_value(JobDto::from_job(job, true)).unwrap();
        assert_eq!(json["last_error_code"], "provider_unavailable");
        assert!(json.get("last_error_message").is_none());
        assert!(!json.to_string().contains("raw-refresh-token"));
    }
}

#[cfg(all(test, feature = "db-tests"))]
mod db_tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

    fn unique(prefix: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{prefix}-{nanos}")
    }

    async fn test_state() -> AppState {
        MIGRATED
            .get_or_init(|| async {
                let migration_url =
                    std::env::var("MIGRATION_DATABASE_URL").expect("MIGRATION_DATABASE_URL 未设置");
                let migration_pool = prts_db::connect_postgres(&migration_url, 1).await.unwrap();
                let mut connection = migration_pool.acquire().await.unwrap();
                prts_db::run_migrations(&mut connection, "prts_runtime")
                    .await
                    .unwrap();
                drop(connection);
                migration_pool.close().await;
            })
            .await;
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL 未设置");
        let redis_url = std::env::var("PRTS__REDIS__URL").expect("PRTS__REDIS__URL 未设置");
        let db = prts_db::connect_postgres(&database_url, 10).await.unwrap();
        prts_db::verify_runtime_role(&db, "prts_runtime")
            .await
            .unwrap();
        let cache = prts_db::connect_redis(&redis_url).await.unwrap();
        let realtime = prts_realtime::Hub::new(&redis_url).await.unwrap();
        let settings = Arc::new(
            prts_common::config::Settings::load_from("__audit_jobs_missing_config__").unwrap(),
        );
        let registry = crate::jobs::JobRegistry::new(Vec::new());
        let job_worker = crate::job_worker::spawn(
            db.clone(),
            registry,
            Arc::new(crate::job_worker::NoPendingDeletions),
        );
        AppState {
            db,
            cache,
            settings,
            zoot: Arc::new(None),
            realtime,
            embedder: Arc::new(None),
            search_rt: Arc::new(tokio::sync::RwLock::new(Default::default())),
            job_worker,
        }
    }

    fn bearer(user_id: i64, state: &AppState) -> String {
        let now = chrono::Utc::now().timestamp();
        let token = prts_auth::jwt::encode(
            &prts_auth::jwt::Claims {
                sub: user_id,
                iat: now,
                exp: now + 600,
                typ: "access".to_string(),
            },
            state.jwt_secret(),
        );
        format!("Bearer {token}")
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn create_failed_job(state: &AppState, project_id: i64, kind: &str) -> i64 {
        let job_id = if let Some(kind) = prts_db::jobs::JobKind::parse(kind) {
            let mut tx = state.db.begin().await.unwrap();
            let job = prts_db::jobs::create_tx(
                &mut tx,
                prts_db::jobs::NewJob {
                    kind,
                    project_id: Some(project_id),
                    stage: "queued".to_string(),
                    progress_total: None,
                    max_attempts: 1,
                    run_after: chrono::Utc::now(),
                },
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
            job.id
        } else {
            sqlx::query_scalar(
                "INSERT INTO jobs (kind, project_id, payload) VALUES ($1, $2, '{}') RETURNING id",
            )
            .bind(kind)
            .bind(project_id)
            .fetch_one(&state.db)
            .await
            .unwrap()
        };
        sqlx::query(
            "UPDATE jobs
             SET state = 'running', attempts = 1, worker_id = 'http-test-worker',
                 lease_until = now() + interval '5 minutes'
             WHERE id = $1",
        )
        .bind(job_id)
        .execute(&state.db)
        .await
        .unwrap();
        prts_db::jobs::fail_attempt(
            &state.db,
            job_id,
            "http-test-worker",
            "test_failure",
            "redacted test failure",
            false,
            0,
        )
        .await
        .unwrap()
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn audit_jobs_http_authorization_and_filtering_contract() {
        let state = test_state().await;
        let owner = prts_db::users::create_password_user(
            &state.db,
            &unique("job-owner"),
            None,
            "test-hash",
            "active",
        )
        .await
        .unwrap();
        let manager = prts_db::users::create_password_user(
            &state.db,
            &unique("job-manager"),
            None,
            "test-hash",
            "active",
        )
        .await
        .unwrap();
        let admin = prts_db::users::create_password_user(
            &state.db,
            &unique("job-admin"),
            None,
            "test-hash",
            "active",
        )
        .await
        .unwrap();
        let outsider = prts_db::users::create_password_user(
            &state.db,
            &unique("job-outsider"),
            None,
            "test-hash",
            "active",
        )
        .await
        .unwrap();
        prts_db::users::set_platform_role(&state.db, admin.id, Some("admin"))
            .await
            .unwrap();
        let slug = unique("job-http-project");
        let project = prts_db::projects::create(
            &state.db,
            &slug,
            &slug,
            "",
            "public",
            &["en".to_string()],
            "zh-Hans",
            owner.id,
        )
        .await
        .unwrap();
        prts_db::memberships::upsert(&state.db, project.id, owner.id, "owner")
            .await
            .unwrap();
        prts_db::memberships::upsert(&state.db, project.id, manager.id, "manager")
            .await
            .unwrap();
        let upload_id = create_failed_job(&state, project.id, "upload_process").await;
        let primary_id =
            create_failed_job(&state, project.id, "primary_source_lexical_reindex").await;
        let unknown_id = create_failed_job(&state, project.id, "unknown_internal_job").await;
        let private_slug = unique("job-private-project");
        let private_project = prts_db::projects::create(
            &state.db,
            &private_slug,
            &private_slug,
            "",
            "private",
            &["en".to_string()],
            "zh-Hans",
            owner.id,
        )
        .await
        .unwrap();
        prts_db::memberships::upsert(&state.db, private_project.id, owner.id, "owner")
            .await
            .unwrap();
        let private_job_id = create_failed_job(&state, private_project.id, "upload_process").await;
        let app = crate::routes::app(state.clone());

        let anonymous = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/jobs/{upload_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let anonymous_en = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/jobs/{upload_id}"))
                    .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous_en.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            body_json(anonymous_en).await,
            serde_json::json!({"code": "unauthorized", "message": "Unauthorized"})
        );

        let missing_zh = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/jobs/9223372036854770000")
                    .header(header::AUTHORIZATION, bearer(owner.id, &state))
                    .header(header::ACCEPT_LANGUAGE, "zh-CN")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_zh.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_json(missing_zh).await,
            serde_json::json!({"code": "not_found", "message": "资源不存在"})
        );

        let unknown = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/jobs/{unknown_id}"))
                    .header(header::AUTHORIZATION, bearer(owner.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

        let unknown_retry = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/jobs/{unknown_id}/retry"))
                    .header(header::AUTHORIZATION, bearer(owner.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let missing_retry = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/jobs/9223372036854770000/retry")
                    .header(header::AUTHORIZATION, bearer(owner.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown_retry.status(), StatusCode::NOT_FOUND);
        assert_eq!(missing_retry.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_json(unknown_retry).await,
            body_json(missing_retry).await
        );

        let listed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/jobs", project.id))
                    .header(header::AUTHORIZATION, bearer(owner.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
        let listed = body_json(listed).await;
        let ids: Vec<i64> = listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_i64().unwrap())
            .collect();
        assert!(ids.contains(&upload_id));
        assert!(ids.contains(&primary_id));
        assert!(!ids.contains(&unknown_id));

        let explicit_forbidden_kind = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/projects/{}/jobs?kind=primary_source_lexical_reindex",
                        project.id
                    ))
                    .header(header::AUTHORIZATION, bearer(manager.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(explicit_forbidden_kind.status(), StatusCode::NOT_FOUND);

        let visible_after_hidden = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/jobs?limit=1", project.id))
                    .header(header::AUTHORIZATION, bearer(manager.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(visible_after_hidden.status(), StatusCode::OK);
        let visible_after_hidden = body_json(visible_after_hidden).await;
        assert_eq!(visible_after_hidden["items"].as_array().unwrap().len(), 1);
        assert_eq!(visible_after_hidden["items"][0]["id"], upload_id);
        assert!(visible_after_hidden["next_after"].is_null());

        let zero_limit_en = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/jobs?limit=0", project.id))
                    .header(header::AUTHORIZATION, bearer(manager.id, &state))
                    .header(header::ACCEPT_LANGUAGE, "en")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(zero_limit_en.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(zero_limit_en).await,
            serde_json::json!({"code": "bad_request", "message": "Bad request"})
        );

        let oversized_limit_zh = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/projects/{}/jobs?limit=101", project.id))
                    .header(header::AUTHORIZATION, bearer(manager.id, &state))
                    .header(header::ACCEPT_LANGUAGE, "zh-CN")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized_limit_zh.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(oversized_limit_zh).await,
            serde_json::json!({"code": "bad_request", "message": "请求参数有误"})
        );

        for uri in [
            format!("/jobs/{private_job_id}"),
            format!("/projects/{}/jobs", private_project.id),
        ] {
            let hidden_private = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header(header::AUTHORIZATION, bearer(outsider.id, &state))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(hidden_private.status(), StatusCode::NOT_FOUND);
        }

        let admin_retry = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/jobs/{primary_id}/retry"))
                    .header(header::AUTHORIZATION, bearer(admin.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(admin_retry.status(), StatusCode::FORBIDDEN);

        let manager_retry = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/jobs/{upload_id}/retry"))
                    .header(header::AUTHORIZATION, bearer(manager.id, &state))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(manager_retry.status(), StatusCode::OK);

        sqlx::query("DELETE FROM jobs WHERE project_id IN ($1, $2)")
            .bind(project.id)
            .bind(private_project.id)
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM projects WHERE id IN ($1, $2)")
            .bind(project.id)
            .bind(private_project.id)
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id IN ($1, $2, $3, $4)")
            .bind(owner.id)
            .bind(manager.id)
            .bind(admin.id)
            .bind(outsider.id)
            .execute(&state.db)
            .await
            .unwrap();
    }
}
