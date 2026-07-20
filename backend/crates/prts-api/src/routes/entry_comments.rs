//! 词条评论 API：项目级策略控制读取/写入，正文保存 Markdown 源文并使用键集分页。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_core::permission::nodes;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

const MAX_COMMENT_LENGTH: usize = 4000;
const MAX_PAGE: i64 = 100;

/// 单条词条评论；删除项只返回占位，不返回旧正文。
#[derive(Debug, Serialize, ToSchema)]
pub struct EntryCommentDto {
    pub id: i64,
    pub entry_id: i64,
    pub author_id: Option<i64>,
    pub author_name: String,
    pub author_avatar_url: Option<String>,
    pub content: String,
    pub deleted: bool,
    pub can_edit: bool,
    pub can_delete: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 评论页，同时下发当前主体是否可发表新评论。
#[derive(Debug, Serialize, ToSchema)]
pub struct EntryCommentPageDto {
    pub items: Vec<EntryCommentDto>,
    pub next_after: Option<i64>,
    pub can_comment: bool,
    pub policy: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct EntryCommentListQuery {
    pub after: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EntryCommentWriteRequest {
    /// Markdown 源文，去除首尾空白后须为 1..=4000 字符。
    pub content: String,
}

/// 列出词条评论。internal 策略只允许项目成员读取；其它策略跟随项目可见性。
#[utoipa::path(
    get,
    path = "/projects/{id}/entries/{entry_id}/comments",
    tag = "entry-comment",
    params(("id" = i64, Path, description = "项目 id"), ("entry_id" = i64, Path, description = "词条 id"), EntryCommentListQuery),
    description = "按 id 倒序键集分页读取词条评论。private/public 策略允许所有项目可见者读取；internal 仅允许项目成员或具有全局项目管理能力的主体读取。删除项保留时间线占位但不返回正文。",
    responses(
        (status = 200, body = EntryCommentPageDto),
        (status = 403, description = "internal 评论对当前主体不可见", body = ErrorResponse),
        (status = 404, description = "项目或词条不存在", body = ErrorResponse)
    )
)]
pub async fn list_comments(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((project_id, entry_id)): Path<(i64, i64)>,
    Query(query): Query<EntryCommentListQuery>,
) -> Result<Json<EntryCommentPageDto>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), project_id).await?;
    access.require_view()?;
    require_comment_read(&access)?;
    ensure_entry(&state, project_id, entry_id).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, MAX_PAGE);
    let rows = prts_db::comments::list(&state.db, project_id, entry_id, query.after, limit)
        .await
        .map_err(db_err)?;
    let next_after = (rows.len() as i64 == limit)
        .then(|| rows.last().map(|row| row.id))
        .flatten();
    let can_comment = can_comment(&access);
    let moderator = access.has_node(nodes::PROJECT_MANAGE);
    let user_id = access.user_id;
    Ok(Json(EntryCommentPageDto {
        items: rows
            .into_iter()
            .map(|row| comment_dto(row, user_id, moderator))
            .collect(),
        next_after,
        can_comment,
        policy: access.project.comment_policy,
    }))
}

/// 新增评论。public 允许任意已登录项目可见者；private/internal 仅允许项目成员。
#[utoipa::path(
    post,
    path = "/projects/{id}/entries/{entry_id}/comments",
    tag = "entry-comment",
    request_body = EntryCommentWriteRequest,
    description = "新增 Markdown 评论。正文与脱敏审计在同一事务提交；审计只记录 comment/entry/project id，不记录正文。",
    responses(
        (status = 201, body = EntryCommentDto),
        (status = 400, description = "正文为空或超过 4000 字符", body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, description = "审计不可用，评论未创建", body = ErrorResponse)
    )
)]
pub async fn create_comment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((project_id, entry_id)): Path<(i64, i64)>,
    Json(request): Json<EntryCommentWriteRequest>,
) -> Result<(StatusCode, Json<EntryCommentDto>), ApiError> {
    let content = validate_content(&request.content)?;
    let access = paccess::load(&state, Some(&user), project_id).await?;
    access.require_view()?;
    require_comment_write(&access)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    require_comment_write(&locked_access)?;
    prts_db::entries::get_for_update_tx(&mut tx, project_id, entry_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let actor = prts_db::users::find_by_id_for_update_tx(&mut tx, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;
    let comment = prts_db::comments::create_tx(
        &mut tx,
        project_id,
        entry_id,
        user.id,
        &actor.username,
        actor.avatar_url.as_deref(),
        content,
    )
    .await
    .map_err(db_err)?;
    append_audit(
        &mut tx,
        &user,
        AuditEvent::EntryCommentCreated {
            project_id,
            entry_id,
            comment_id: comment.id,
        },
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    publish_changed(&state, project_id, entry_id, user.id).await;
    Ok((
        StatusCode::CREATED,
        Json(comment_dto(
            comment,
            Some(user.id),
            locked_access.has_node(nodes::PROJECT_MANAGE),
        )),
    ))
}

/// 作者编辑自己的评论；管理/拥有者不替他人改写正文。
#[utoipa::path(
    put,
    path = "/projects/{id}/entries/{entry_id}/comments/{comment_id}",
    tag = "entry-comment",
    request_body = EntryCommentWriteRequest,
    description = "只有评论作者可编辑未删除正文。编辑与脱敏审计同事务提交。",
    responses((status = 200, body = EntryCommentDto), (status = 400, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 503, body = ErrorResponse))
)]
pub async fn update_comment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((project_id, entry_id, comment_id)): Path<(i64, i64, i64)>,
    Json(request): Json<EntryCommentWriteRequest>,
) -> Result<Json<EntryCommentDto>, ApiError> {
    let content = validate_content(&request.content)?;
    let access = paccess::load(&state, Some(&user), project_id).await?;
    access.require_view()?;
    require_comment_read(&access)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    require_comment_read(&locked_access)?;
    let current = prts_db::comments::find_for_update_tx(&mut tx, project_id, entry_id, comment_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if current.deleted_at.is_some() {
        return Err(Error::NotFound.into());
    }
    if current.author_id != Some(user.id) {
        return Err(Error::Forbidden.into());
    }
    let updated = prts_db::comments::update_tx(&mut tx, comment_id, content)
        .await
        .map_err(db_err)?;
    append_audit(
        &mut tx,
        &user,
        AuditEvent::EntryCommentUpdated {
            project_id,
            entry_id,
            comment_id,
        },
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    publish_changed(&state, project_id, entry_id, user.id).await;
    Ok(Json(comment_dto(
        updated,
        Some(user.id),
        locked_access.has_node(nodes::PROJECT_MANAGE),
    )))
}

/// 作者可删除自己的评论；项目管理/拥有者可执行内容治理删除。
#[utoipa::path(
    delete,
    path = "/projects/{id}/entries/{entry_id}/comments/{comment_id}",
    tag = "entry-comment",
    description = "软删除评论并清空正文。作者可删除自己的评论；具有 project.manage 的主体可治理删除任意评论。",
    responses((status = 204), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 503, body = ErrorResponse))
)]
pub async fn delete_comment(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((project_id, entry_id, comment_id)): Path<(i64, i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), project_id).await?;
    access.require_view()?;
    require_comment_read(&access)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked_access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    require_comment_read(&locked_access)?;
    let current = prts_db::comments::find_for_update_tx(&mut tx, project_id, entry_id, comment_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if current.deleted_at.is_some() {
        return Err(Error::NotFound.into());
    }
    let moderated = current.author_id != Some(user.id);
    if moderated && !locked_access.has_node(nodes::PROJECT_MANAGE) {
        return Err(Error::Forbidden.into());
    }
    prts_db::comments::delete_tx(&mut tx, comment_id, user.id)
        .await
        .map_err(db_err)?;
    append_audit(
        &mut tx,
        &user,
        AuditEvent::EntryCommentDeleted {
            project_id,
            entry_id,
            comment_id,
            moderated,
        },
    )
    .await?;
    tx.commit().await.map_err(db_err)?;
    publish_changed(&state, project_id, entry_id, user.id).await;
    Ok(StatusCode::NO_CONTENT)
}

fn validate_content(content: &str) -> Result<&str, ApiError> {
    let content = content.trim();
    if content.is_empty() || content.chars().count() > MAX_COMMENT_LENGTH {
        Err(Error::bad_request("invalid_comment_content").into())
    } else {
        Ok(content)
    }
}

fn require_comment_read(access: &paccess::ProjectAccess) -> Result<(), ApiError> {
    if access.project.comment_policy != "internal" || access.effective_role().is_some() {
        Ok(())
    } else {
        Err(Error::Forbidden.into())
    }
}

fn can_comment(access: &paccess::ProjectAccess) -> bool {
    access.user_id.is_some()
        && (access.project.comment_policy == "public" || access.effective_role().is_some())
}

fn require_comment_write(access: &paccess::ProjectAccess) -> Result<(), ApiError> {
    if can_comment(access) {
        Ok(())
    } else if access.user_id.is_none() {
        Err(Error::Unauthorized.into())
    } else {
        Err(Error::Forbidden.into())
    }
}

async fn ensure_entry(state: &AppState, project_id: i64, entry_id: i64) -> Result<(), ApiError> {
    prts_db::entries::get(&state.db, project_id, entry_id)
        .await
        .map_err(db_err)?
        .map(|_| ())
        .ok_or_else(|| Error::NotFound.into())
}

fn comment_dto(
    row: prts_db::models::EntryComment,
    user_id: Option<i64>,
    moderator: bool,
) -> EntryCommentDto {
    let deleted = row.deleted_at.is_some();
    EntryCommentDto {
        id: row.id,
        entry_id: row.entry_id,
        author_id: row.author_id,
        author_name: row.author_name,
        author_avatar_url: row.author_avatar_url,
        content: if deleted { String::new() } else { row.content },
        deleted,
        can_edit: !deleted && row.author_id == user_id,
        can_delete: !deleted && (row.author_id == user_id || moderator),
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }
}

async fn append_audit(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    event: AuditEvent<'_>,
) -> Result<(), ApiError> {
    prts_db::audit::append_event_tx(
        conn,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        event,
    )
    .await
    .map(|_| ())
    .map_err(|_| Error::AuditUnavailable.into())
}

async fn publish_changed(state: &AppState, project_id: i64, entry_id: i64, by: i64) {
    state
        .realtime
        .publish(
            project_id,
            &prts_realtime::RoomEvent::EntryCommentChanged { entry_id, by },
        )
        .await;
}
