//! 项目任务列表、详情与完整期望文件集合 mutation。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
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
const MAX_TITLE_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 100_000;
const MAX_FILES_PER_TASK: usize = 500;

/// 任务列表键集参数。
#[derive(Debug, Deserialize, IntoParams)]
pub struct TaskListQuery {
    /// 上一页最后一个 task id。
    pub after: Option<i64>,
    /// 每页 1..=100，默认 50。
    pub limit: Option<i64>,
}

/// 创建任务时提交标题、Markdown 原文与完整期望文件 ID 集合。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: String,
    pub file_ids: Vec<i64>,
}

/// 更新任务时同样提交完整期望文件集合；不能提交 baseline entry IDs。
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTaskRequest {
    pub title: String,
    pub description: String,
    pub file_ids: Vec<i64>,
}

/// 任务列表项，不携带 Markdown 正文。
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskListItemDto {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub created_by: Option<i64>,
    pub denominator: i64,
    pub completed: i64,
    pub completion_ratio: f64,
    pub no_work_required: bool,
    pub file_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 任务列表键集响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskListPageDto {
    pub items: Vec<TaskListItemDto>,
    pub next_after: Option<i64>,
}

/// 一个任务文件；永久清除后只保留 snapshot id。
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskFileDto {
    pub id: i64,
    pub file_id_snapshot: i64,
    pub live_file_id: Option<i64>,
    pub name: Option<String>,
    pub path: Option<String>,
    pub created_at: String,
}

/// 任务详情，description 是待前端共享 MarkdownView 净化的原文。
#[derive(Debug, Serialize, ToSchema)]
pub struct TaskDetailDto {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub description: String,
    pub created_by: Option<i64>,
    pub denominator: i64,
    pub completed: i64,
    pub completion_ratio: f64,
    pub no_work_required: bool,
    pub files: Vec<TaskFileDto>,
    pub created_at: String,
    pub updated_at: String,
}

/// 列出 caller 可见项目的任务，按 task id DESC 键集分页。
#[utoipa::path(get, path = "/projects/{id}/tasks", tag = "task",
    description = "按 task ID DESC 键集列出调用者可见项目的任务和物化进度；公开项目允许只读，正常路径不实时 COUNT entries，文件/词条 ID 均保持 BIGINT/i64。",
    params(("id" = i64, Path), TaskListQuery),
    responses(
        (status = 200, body = TaskListPageDto),
        (status = 400, body = ErrorResponse),
        (status = 404, body = ErrorResponse)
    ))]
pub async fn list_tasks(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(query): Query<TaskListQuery>,
) -> Result<Json<TaskListPageDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    let limit = validate_limit(query.limit)?;
    let records = prts_db::tasks::list(&state.db, id, query.after, limit)
        .await
        .map_err(db_err)?;
    let next_after = (records.len() as i64 == limit)
        .then(|| records.last().map(|record| record.id))
        .flatten();
    Ok(Json(TaskListPageDto {
        items: records.into_iter().map(task_list_dto).collect(),
        next_after,
    }))
}

/// owner/manager 创建任务并在同一事务建立新增文件的 baseline。
#[utoipa::path(post, path = "/projects/{id}/tasks", tag = "task",
    description = "项目任务管理 capability 创建任务，并对新增 active files 当时 effective-visible 且 untranslated 的 entry IDs 建立 immutable baseline；业务、统计与审计原子提交。",
    request_body = CreateTaskRequest,
    responses(
        (status = 201, body = TaskDetailDto),
        (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    ))]
pub async fn create_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(request): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<TaskDetailDto>), ApiError> {
    validate_task_input(&request.title, &request.description, &request.file_ids)?;
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_TASK_MANAGE)?;
    let plan = prts_core::tasks::plan_file_set(&[], &request.file_ids).map_err(plan_error)?;

    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_task_manage(&mut tx, &user, id).await?;
    let task = prts_db::tasks::create_tx(
        &mut tx,
        id,
        user.id,
        request.title.trim(),
        &request.description,
    )
    .await
    .map_err(db_err)?;
    let applied = prts_db::tasks::apply_file_set_plan_tx(&mut tx, id, task.id, &plan)
        .await
        .map_err(map_task_db_error)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::TaskCreated {
            project_id: id,
            task_id: task.id,
            file_count: applied.added_files,
            baseline_entry_count: applied.baseline_entries_added,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    let detail = load_detail(&state, id, task.id).await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

/// 读取 caller 可见项目中与 URL project 绑定的任务详情。
#[utoipa::path(get, path = "/projects/{id}/tasks/{task_id}", tag = "task",
    description = "读取与 URL project 严格绑定的可见任务、净化前 Markdown 原文、active file snapshot 和物化进度；跨项目或不可见 task 统一按不存在处理。",
    params(("id" = i64, Path), ("task_id" = i64, Path)),
    responses(
        (status = 200, body = TaskDetailDto),
        (status = 404, body = ErrorResponse)
    ))]
pub async fn get_task(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((id, task_id)): Path<(i64, i64)>,
) -> Result<Json<TaskDetailDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    load_detail(&state, id, task_id).await.map(Json)
}

/// owner/manager 更新元数据与完整期望文件集合；保留文件不会重建 baseline。
#[utoipa::path(put, path = "/projects/{id}/tasks/{task_id}", tag = "task",
    description = "项目任务管理 capability 更新标题、Markdown 原文和完整期望 file ID 集合；保留文件不重建 baseline，移除再加入才创建新 snapshot。",
    request_body = UpdateTaskRequest,
    responses(
        (status = 200, body = TaskDetailDto),
        (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    ))]
pub async fn update_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, task_id)): Path<(i64, i64)>,
    Json(request): Json<UpdateTaskRequest>,
) -> Result<Json<TaskDetailDto>, ApiError> {
    validate_task_input(&request.title, &request.description, &request.file_ids)?;
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_TASK_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_task_manage(&mut tx, &user, id).await?;
    let current_task = prts_db::tasks::find_for_update_tx(&mut tx, id, task_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let current_files = prts_db::tasks::active_file_ids_tx(&mut tx, id, task_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let plan =
        prts_core::tasks::plan_file_set(&current_files, &request.file_ids).map_err(plan_error)?;
    let title = request.title.trim();
    let mut changed_fields = Vec::new();
    if current_task.title != title {
        changed_fields.push("title");
    }
    if current_task.description != request.description {
        changed_fields.push("description");
    }
    if !plan.added_file_ids.is_empty() || !plan.removed_file_ids.is_empty() {
        changed_fields.push("files");
    }
    prts_db::tasks::update_metadata_tx(&mut tx, id, task_id, title, &request.description)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let applied = prts_db::tasks::apply_file_set_plan_tx(&mut tx, id, task_id, &plan)
        .await
        .map_err(map_task_db_error)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::TaskUpdated {
            project_id: id,
            task_id,
            changed_fields: &changed_fields,
            retained_files: applied.retained_files,
            added_files: applied.added_files,
            removed_files: applied.removed_files,
            baseline_entries_added: applied.baseline_entries_added,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    load_detail(&state, id, task_id).await.map(Json)
}

/// owner/manager 显式删除任务及其 task files/baselines。
#[utoipa::path(delete, path = "/projects/{id}/tasks/{task_id}", tag = "task",
    description = "项目任务管理 capability 显式删除与 URL project 绑定的 task、task files 和 baseline snapshots；删除计数与 allowlisted audit 在同一事务 fail-closed 提交。",
    params(("id" = i64, Path), ("task_id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    ))]
pub async fn delete_task(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, task_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_TASK_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    lock_project_task_manage(&mut tx, &user, id).await?;
    prts_db::tasks::find_for_update_tx(&mut tx, id, task_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let (file_count, baseline_entry_count) =
        prts_db::tasks::snapshot_counts_tx(&mut tx, id, task_id)
            .await
            .map_err(db_err)?
            .ok_or(prts_common::Error::NotFound)?;
    if !prts_db::tasks::delete_tx(&mut tx, id, task_id)
        .await
        .map_err(db_err)?
    {
        return Err(prts_common::Error::NotFound.into());
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::TaskDeleted {
            project_id: id,
            task_id,
            file_count,
            baseline_entry_count,
        },
    )
    .await
    .map_err(|_| prts_common::Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn lock_project_task_manage(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    project_id: i64,
) -> Result<(), ApiError> {
    let project = prts_db::projects::find_by_id_for_update_tx(conn, project_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    paccess::load_locked_tx(conn, user, project)
        .await?
        .require_node(nodes::PROJECT_TASK_MANAGE)
}

async fn load_detail(
    state: &AppState,
    project_id: i64,
    task_id: i64,
) -> Result<TaskDetailDto, ApiError> {
    let task = prts_db::tasks::find(&state.db, project_id, task_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let stats = prts_db::tasks::stats(&state.db, project_id, task_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let files = prts_db::tasks::file_details(&state.db, project_id, task_id)
        .await
        .map_err(db_err)?
        .ok_or(prts_common::Error::NotFound)?;
    let progress = prts_core::tasks::TaskProgress {
        denominator: stats.denominator,
        completed: stats.completed,
    };
    Ok(TaskDetailDto {
        id: task.id,
        project_id: task.project_id,
        title: task.title,
        description: task.description,
        created_by: task.created_by,
        denominator: progress.denominator,
        completed: progress.completed,
        completion_ratio: progress.completion_ratio(),
        no_work_required: progress.no_work_required(),
        files: files
            .into_iter()
            .map(|file| TaskFileDto {
                id: file.id,
                file_id_snapshot: file.file_id_snapshot,
                live_file_id: file.live_file_id,
                name: file.name,
                path: file.path,
                created_at: file.created_at.to_rfc3339(),
            })
            .collect(),
        created_at: task.created_at.to_rfc3339(),
        updated_at: task.updated_at.to_rfc3339(),
    })
}

fn task_list_dto(record: prts_db::tasks::TaskListItem) -> TaskListItemDto {
    let progress = prts_core::tasks::TaskProgress {
        denominator: record.denominator,
        completed: record.completed,
    };
    TaskListItemDto {
        id: record.id,
        project_id: record.project_id,
        title: record.title,
        created_by: record.created_by,
        denominator: progress.denominator,
        completed: progress.completed,
        completion_ratio: progress.completion_ratio(),
        no_work_required: progress.no_work_required(),
        file_count: record.file_count,
        created_at: record.created_at.to_rfc3339(),
        updated_at: record.updated_at.to_rfc3339(),
    }
}

fn validate_limit(limit: Option<i64>) -> Result<i64, ApiError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(prts_common::Error::bad_request("task_limit_invalid").into());
    }
    Ok(limit)
}

fn validate_task_input(title: &str, description: &str, file_ids: &[i64]) -> Result<(), ApiError> {
    let title_len = title.trim().chars().count();
    if title_len == 0 || title_len > MAX_TITLE_CHARS {
        return Err(prts_common::Error::bad_request("task_title_invalid").into());
    }
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(prts_common::Error::bad_request("task_description_too_long").into());
    }
    if file_ids.len() > MAX_FILES_PER_TASK {
        return Err(prts_common::Error::bad_request("task_file_limit_exceeded").into());
    }
    Ok(())
}

fn plan_error(error: prts_core::tasks::TaskPlanError) -> ApiError {
    let code = match error {
        prts_core::tasks::TaskPlanError::DuplicateFileId => "task_file_duplicate",
        prts_core::tasks::TaskPlanError::InvalidFileId => "task_file_id_invalid",
    };
    prts_common::Error::bad_request(code).into()
}

fn map_task_db_error(error: sqlx::Error) -> ApiError {
    if matches!(error, sqlx::Error::RowNotFound) {
        prts_common::Error::NotFound.into()
    } else {
        db_err(error)
    }
}
