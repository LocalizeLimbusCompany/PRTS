//! 项目头像上传、删除与按项目可见性读取。

use axum::body::{to_bytes, Body};
use axum::extract::{Path, Request, State};
use axum::http::{header, Response, StatusCode};
use prts_common::Error;
use prts_core::permission::nodes;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use crate::auth::{project as paccess, CurrentUser, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::media::{
    project_avatar_key, validate_avatar_content_type, validate_project_avatar,
    AvatarValidationError, AVATAR_CONTENT_TYPE, AVATAR_MAX_BYTES,
};
use crate::state::AppState;

fn invalid_avatar(error: AvatarValidationError) -> ApiError {
    Error::bad_request(error.code()).into()
}

fn media_error(error: std::io::Error) -> ApiError {
    tracing::error!(%error, "project media storage operation failed");
    Error::internal("project media storage unavailable").into()
}

async fn restore_after_failed_commit(state: &AppState, key: &str, previous: Option<&[u8]>) {
    let result = match previous {
        Some(bytes) => state.media.write_atomic(key, bytes).await,
        None => state.media.delete(key).await,
    };
    if let Err(error) = result {
        tracing::error!(%error, %key, "failed to restore project avatar after database rollback");
    }
}

/// 上传或替换项目头像。调用方必须具备项目管理能力。
#[utoipa::path(
    post,
    path = "/projects/{id}/avatar",
    tag = "project",
    request_body(content = Vec<u8>, content_type = "image/webp"),
    params(("id" = i64, Path, description = "项目 ID")),
    responses(
        (status = 204),
        (status = 400, description = "必须是 512KB 内、正方形且尺寸不超过 1024 的有效 WebP", body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, description = "审计服务不可用，头像未发布", body = ErrorResponse)
    )
)]
pub async fn upload_project_avatar(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    user: CurrentUser,
    request: Request,
) -> Result<StatusCode, ApiError> {
    let headers = request.headers();
    let declared_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if declared_length.is_some_and(|length| length > AVATAR_MAX_BYTES) {
        return Err(invalid_avatar(AvatarValidationError::TooLarge));
    }
    validate_avatar_content_type(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
    )
    .map_err(invalid_avatar)?;
    let body = to_bytes(request.into_body(), AVATAR_MAX_BYTES)
        .await
        .map_err(|_| invalid_avatar(AvatarValidationError::TooLarge))?;
    validate_project_avatar(&body).map_err(invalid_avatar)?;

    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;

    let key = project_avatar_key(id);
    let previous = if access.project.avatar_key.is_some() {
        Some(state.media.read(&key).await.map_err(media_error)?)
    } else {
        None
    };
    prts_db::projects::set_avatar_tx(&mut tx, id, &key, AVATAR_CONTENT_TYPE)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ProjectAvatarUpdated {
            project_id: id,
            content_type: AVATAR_CONTENT_TYPE,
            encoded_bytes: body.len(),
            replaced: previous.is_some(),
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    state
        .media
        .write_atomic(&key, &body)
        .await
        .map_err(media_error)?;
    if let Err(error) = tx.commit().await {
        restore_after_failed_commit(&state, &key, previous.as_deref()).await;
        return Err(db_err(error));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 删除项目头像。没有头像时保持幂等成功。
#[utoipa::path(
    delete,
    path = "/projects/{id}/avatar",
    tag = "project",
    params(("id" = i64, Path, description = "项目 ID")),
    responses(
        (status = 204),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, description = "审计服务不可用，头像未删除", body = ErrorResponse)
    )
)]
pub async fn delete_project_avatar(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    user: CurrentUser,
) -> Result<StatusCode, ApiError> {
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let access = paccess::load_locked_tx(&mut tx, &user, project).await?;
    access.require_node(nodes::PROJECT_MANAGE)?;
    if access.project.avatar_key.is_none() {
        tx.commit().await.map_err(db_err)?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let key = project_avatar_key(id);
    let previous = state.media.read(&key).await.map_err(media_error)?;
    prts_db::projects::clear_avatar_tx(&mut tx, id)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::ProjectAvatarDeleted { project_id: id },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    state.media.delete(&key).await.map_err(media_error)?;
    if let Err(error) = tx.commit().await {
        restore_after_failed_commit(&state, &key, Some(&previous)).await;
        return Err(db_err(error));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 按项目可见性读取头像；公开项目允许游客访问，私有项目仅成员与平台管理可见。
#[utoipa::path(
    get,
    path = "/projects/{id}/avatar",
    tag = "project",
    params(("id" = i64, Path, description = "项目 ID")),
    responses(
        (status = 200, content_type = "image/webp", body = Vec<u8>),
        (status = 404, body = ErrorResponse)
    )
)]
pub async fn get_project_avatar(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    MaybeUser(user): MaybeUser,
) -> Result<Response<Body>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;
    if access.project.avatar_key.is_none() {
        return Err(Error::NotFound.into());
    }
    let bytes = state
        .media
        .read(&project_avatar_key(id))
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::NotFound.into()
            } else {
                media_error(error)
            }
        })?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, AVATAR_CONTENT_TYPE)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .map_err(|_| Error::internal("failed to build avatar response").into())
}
