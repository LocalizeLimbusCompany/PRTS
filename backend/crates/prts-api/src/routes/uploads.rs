//! 原始 JSON 文件批次声明、流式传输、提交、重试、状态与取消。

use std::path::{Component, Path as FilePath, PathBuf};

use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::Json;
use futures_util::StreamExt;
use prts_common::Error;
use prts_core::permission::nodes;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use utoipa::ToSchema;

use crate::auth::{project as paccess, CurrentUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UploadFileDeclarationReq {
    pub path: String,
    pub size: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUploadBatchReq {
    pub files: Vec<UploadFileDeclarationReq>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadAttemptDto {
    pub id: i64,
    pub attempt_number: i32,
    pub state: String,
    pub bytes_received: i64,
    pub error_code: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadBatchFileDto {
    pub id: i64,
    pub ordinal: i32,
    pub path: String,
    pub declared_bytes: i64,
    pub state: String,
    pub processing_job_id: Option<i64>,
    pub target_file_id: Option<i64>,
    pub last_error_code: Option<String>,
    pub attempts: Vec<UploadAttemptDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadBatchDto {
    pub id: i64,
    pub project_id: i64,
    pub state: String,
    pub declared_file_count: i32,
    pub declared_total_bytes: i64,
    pub expires_at: String,
    pub files: Vec<UploadBatchFileDto>,
}

fn snapshot_dto(snapshot: prts_db::uploads::UploadBatchSnapshot) -> UploadBatchDto {
    UploadBatchDto {
        id: snapshot.batch.id,
        project_id: snapshot.batch.project_id_snapshot,
        state: snapshot.batch.state,
        declared_file_count: snapshot.batch.declared_file_count,
        declared_total_bytes: snapshot.batch.declared_total_bytes,
        expires_at: snapshot.batch.expires_at.to_rfc3339(),
        files: snapshot
            .files
            .into_iter()
            .map(|file| UploadBatchFileDto {
                id: file.id,
                ordinal: file.ordinal,
                path: file.path,
                declared_bytes: file.declared_bytes,
                state: file.state,
                processing_job_id: file.processing_job_id,
                target_file_id: file.target_file_id,
                last_error_code: file.last_error_code,
                attempts: snapshot
                    .attempts
                    .iter()
                    .filter(|attempt| attempt.batch_file_id == file.id)
                    .map(|attempt| UploadAttemptDto {
                        id: attempt.id,
                        attempt_number: attempt.attempt_number,
                        state: attempt.state.clone(),
                        bytes_received: attempt.bytes_received,
                        error_code: attempt.error_code.clone(),
                        started_at: attempt.started_at.to_rfc3339(),
                        finished_at: attempt.finished_at.map(|value| value.to_rfc3339()),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn normalize_upload_path(raw: &str) -> Result<String, ApiError> {
    let normalized = raw.trim().replace('\\', "/");
    if normalized.is_empty() || normalized.len() > 1024 || normalized.starts_with('/') {
        return Err(Error::bad_request("upload_path_invalid").into());
    }
    let path = FilePath::new(&normalized);
    if path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().is_empty()
        })
        || normalized.split('/').any(is_reserved_path_segment)
        || !normalized.to_ascii_lowercase().ends_with(".json")
    {
        return Err(Error::bad_request("upload_path_invalid").into());
    }
    Ok(normalized)
}

fn is_reserved_path_segment(segment: &str) -> bool {
    if segment.is_empty()
        || matches!(segment, "." | "..")
        || segment.starts_with('.')
        || segment.ends_with('.')
        || segment.ends_with(' ')
        || segment.chars().any(|character| {
            character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
    {
        return true;
    }
    let device_name = segment
        .split_once('.')
        .map_or(segment, |(stem, _extension)| stem)
        .to_ascii_uppercase();
    matches!(device_name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || device_name
            .strip_prefix("COM")
            .or_else(|| device_name.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn temp_key(project_id: i64, batch_hint: &str) -> String {
    format!(
        "projects/{project_id}/uploads/{batch_hint}/{}.json",
        prts_auth::token::random_token(18).to_lowercase()
    )
}

fn validate_declarations(
    project_id: i64,
    files: Vec<UploadFileDeclarationReq>,
    config: &prts_db::upload_settings::UploadConfig,
) -> Result<(Vec<prts_db::uploads::UploadDeclaration>, i64), ApiError> {
    if files.is_empty() || files.len() > config.max_files_per_batch as usize {
        return Err(Error::bad_request("upload_file_count_exceeded").into());
    }
    let mut declarations = Vec::with_capacity(files.len());
    let mut total_bytes = 0_i64;
    let batch_hint = prts_auth::token::random_token(12).to_lowercase();
    for file in files {
        if file.size < 0 || file.size > config.max_bytes_per_file {
            return Err(Error::bad_request("upload_file_size_exceeded").into());
        }
        total_bytes = total_bytes
            .checked_add(file.size)
            .ok_or_else(|| Error::bad_request("upload_batch_size_exceeded"))?;
        declarations.push(prts_db::uploads::UploadDeclaration {
            path: normalize_upload_path(&file.path)?,
            declared_bytes: file.size,
            temp_key: temp_key(project_id, &batch_hint),
        });
    }
    if total_bytes > config.max_bytes_per_batch {
        return Err(Error::bad_request("upload_batch_size_exceeded").into());
    }
    let mut paths = declarations
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::bad_request("upload_path_conflict").into());
    }
    Ok((declarations, total_bytes))
}

fn temp_path(root: &str, key: &str) -> Result<PathBuf, ApiError> {
    let relative = FilePath::new(key);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Error::internal("invalid upload temp key").into());
    }
    Ok(PathBuf::from(root).join(relative))
}

async fn remove_temp(root: &str, key: &str) {
    if let Ok(path) = temp_path(root, key) {
        for candidate in [path.clone(), path.with_extension("part")] {
            if let Err(error) = tokio::fs::remove_file(candidate).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(%error, %key, "failed to remove upload temp file");
                }
            }
        }
    }
}

async fn record_attempt_failure(
    state: &AppState,
    user: &CurrentUser,
    project_id: i64,
    batch_id: i64,
    batch_file_id: i64,
    attempt_id: i64,
    error_code: &'static str,
    bytes_received: i64,
) -> Result<(), ApiError> {
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked = paccess::load_locked_tx(&mut tx, user, project).await?;
    locked.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked.require_language_ready()?;
    if !prts_db::uploads::fail_attempt_tx(
        &mut tx,
        batch_file_id,
        attempt_id,
        error_code,
        bytes_received,
    )
    .await
    .map_err(db_err)?
    {
        return Err(Error::Conflict.into());
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UploadAttemptFailed {
            project_id,
            batch_id,
            batch_file_id,
            attempt_id,
            bytes_received,
            error_code,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
    Ok(())
}

/// 声明上传批次及每个文件的首次 byte-zero attempt。
#[utoipa::path(
    post,
    path = "/projects/{id}/upload-batches",
    tag = "upload",
    request_body = CreateUploadBatchReq,
    responses(
        (status = 200, body = UploadBatchDto),
        (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
pub async fn create_batch(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    user: CurrentUser,
    Json(request): Json<CreateUploadBatchReq>,
) -> Result<Json<UploadBatchDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), project_id).await?;
    access.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    access.require_language_ready()?;
    let config = prts_db::upload_settings::get(&state.db)
        .await
        .map_err(db_err)?;
    let (declarations, total_bytes) = validate_declarations(project_id, request.files, &config)?;

    let expires_at =
        chrono::Utc::now() + chrono::Duration::hours(i64::from(config.upload_batch_expiry_hours));
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked.require_language_ready()?;
    let snapshot =
        prts_db::uploads::create_batch_tx(&mut tx, project_id, user.id, &declarations, expires_at)
            .await
            .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UploadBatchCreated {
            project_id,
            batch_id: snapshot.batch.id,
            file_count: snapshot.files.len(),
            total_bytes,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(snapshot_dto(snapshot)))
}

/// 从 byte zero 流式接收一次 attempt；不接受 Range/offset 续传。
#[utoipa::path(
    put,
    path = "/projects/{id}/upload-batches/{batch_id}/files/{file_id}/attempts/{attempt_id}",
    tag = "upload",
    request_body(content = Vec<u8>, content_type = "application/json"),
    responses(
        (status = 204),
        (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse)
    )
)]
pub async fn receive_attempt(
    State(state): State<AppState>,
    Path((project_id, batch_id, file_id, attempt_id)): Path<(i64, i64, i64, i64)>,
    user: CurrentUser,
    request: Request,
) -> Result<StatusCode, ApiError> {
    if request.headers().contains_key(header::RANGE)
        || request.headers().contains_key(header::CONTENT_RANGE)
    {
        return Err(Error::bad_request("upload_resume_not_supported").into());
    }
    let access = paccess::load(&state, Some(&user), project_id).await?;
    access.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    let config = prts_db::upload_settings::get(&state.db)
        .await
        .map_err(db_err)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked.require_language_ready()?;
    let Some((batch, file, attempt)) = prts_db::uploads::claim_attempt_for_receive_tx(
        &mut tx, project_id, batch_id, file_id, attempt_id,
    )
    .await
    .map_err(db_err)?
    else {
        return Err(Error::NotFound.into());
    };
    if batch.actor_id != Some(user.id)
        || batch.state != "uploading"
        || file.state != "uploading"
        || file.current_attempt_id != Some(attempt_id)
        || attempt.state != "receiving"
    {
        return Err(Error::Conflict.into());
    }
    if file.declared_bytes > config.max_bytes_per_file {
        return Err(Error::bad_request("upload_file_size_exceeded").into());
    }
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UploadAttemptStarted {
            project_id,
            batch_id,
            batch_file_id: file_id,
            attempt_id,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;

    let destination = temp_path(
        &state.settings.media.upload_temp_directory,
        &attempt.temp_key,
    )?;
    let parent = destination
        .parent()
        .ok_or_else(|| Error::internal("upload temp path has no parent"))?;
    if tokio::fs::create_dir_all(parent).await.is_err() {
        record_attempt_failure(
            &state,
            &user,
            project_id,
            batch_id,
            file_id,
            attempt_id,
            "upload_temp_storage_unavailable",
            0,
        )
        .await?;
        return Err(Error::internal("upload temp storage unavailable").into());
    }
    let partial = destination.with_extension("part");
    let mut output = match tokio::fs::File::create(&partial).await {
        Ok(output) => output,
        Err(_) => {
            record_attempt_failure(
                &state,
                &user,
                project_id,
                batch_id,
                file_id,
                attempt_id,
                "upload_temp_storage_unavailable",
                0,
            )
            .await?;
            return Err(Error::internal("upload temp storage unavailable").into());
        }
    };
    let mut received = 0_i64;
    let mut stream = request.into_body().into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => {
                drop(output);
                let _ = tokio::fs::remove_file(&partial).await;
                record_attempt_failure(
                    &state,
                    &user,
                    project_id,
                    batch_id,
                    file_id,
                    attempt_id,
                    "upload_stream_interrupted",
                    received,
                )
                .await?;
                return Err(Error::bad_request("upload_stream_interrupted").into());
            }
        };
        let Some(next_received) = received.checked_add(chunk.len() as i64) else {
            drop(output);
            let _ = tokio::fs::remove_file(&partial).await;
            record_attempt_failure(
                &state,
                &user,
                project_id,
                batch_id,
                file_id,
                attempt_id,
                "upload_file_size_exceeded",
                received,
            )
            .await?;
            return Err(Error::bad_request("upload_file_size_exceeded").into());
        };
        received = next_received;
        if received > file.declared_bytes || received > config.max_bytes_per_file {
            drop(output);
            let _ = tokio::fs::remove_file(&partial).await;
            record_attempt_failure(
                &state,
                &user,
                project_id,
                batch_id,
                file_id,
                attempt_id,
                "upload_file_size_mismatch",
                received,
            )
            .await?;
            return Err(Error::bad_request("upload_file_size_mismatch").into());
        }
        if output.write_all(&chunk).await.is_err() {
            drop(output);
            let _ = tokio::fs::remove_file(&partial).await;
            record_attempt_failure(
                &state,
                &user,
                project_id,
                batch_id,
                file_id,
                attempt_id,
                "upload_temp_storage_unavailable",
                received,
            )
            .await?;
            return Err(Error::internal("upload temp storage unavailable").into());
        }
    }
    if output.flush().await.is_err() {
        drop(output);
        let _ = tokio::fs::remove_file(&partial).await;
        record_attempt_failure(
            &state,
            &user,
            project_id,
            batch_id,
            file_id,
            attempt_id,
            "upload_temp_storage_unavailable",
            received,
        )
        .await?;
        return Err(Error::internal("upload temp storage unavailable").into());
    }
    drop(output);
    if received != file.declared_bytes {
        let _ = tokio::fs::remove_file(&partial).await;
        record_attempt_failure(
            &state,
            &user,
            project_id,
            batch_id,
            file_id,
            attempt_id,
            "upload_file_size_mismatch",
            received,
        )
        .await?;
        return Err(Error::bad_request("upload_file_size_mismatch").into());
    }
    if tokio::fs::rename(&partial, &destination).await.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
        record_attempt_failure(
            &state,
            &user,
            project_id,
            batch_id,
            file_id,
            attempt_id,
            "upload_temp_storage_unavailable",
            received,
        )
        .await?;
        return Err(Error::internal("upload temp storage unavailable").into());
    }
    let finalize = async {
        let mut tx = state.db.begin().await.map_err(db_err)?;
        let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
            .await
            .map_err(db_err)?
            .ok_or(Error::NotFound)?;
        let locked = paccess::load_locked_tx(&mut tx, &user, project).await?;
        locked.require_node(nodes::PROJECT_FILE_UPLOAD)?;
        locked.require_language_ready()?;
        if !prts_db::uploads::mark_attempt_received_tx(&mut tx, file_id, attempt_id, received)
            .await
            .map_err(db_err)?
        {
            return Err(Error::Conflict.into());
        }
        prts_db::audit::append_event_tx(
            &mut tx,
            AuditActor {
                id: Some(user.id),
                kind: AuditActorKind::User,
                ip: None,
            },
            AuditEvent::UploadAttemptReceived {
                project_id,
                batch_id,
                batch_file_id: file_id,
                attempt_id,
                bytes_received: received,
            },
        )
        .await
        .map_err(|_| Error::AuditUnavailable)?;
        tx.commit().await.map_err(db_err)?;
        Ok::<(), ApiError>(())
    }
    .await;
    if let Err(error) = finalize {
        remove_temp(
            &state.settings.media.upload_temp_directory,
            &attempt.temp_key,
        )
        .await;
        return Err(error);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 提交完整批次并为每个逻辑文件排队一个可复用 processing job。
#[utoipa::path(
    post,
    path = "/projects/{id}/upload-batches/{batch_id}/complete",
    tag = "upload",
    responses(
        (status = 200, body = UploadBatchDto),
        (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
pub async fn complete_batch(
    State(state): State<AppState>,
    Path((project_id, batch_id)): Path<(i64, i64)>,
    user: CurrentUser,
) -> Result<Json<UploadBatchDto>, ApiError> {
    let access = paccess::load(&state, Some(&user), project_id).await?;
    access.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked.require_language_ready()?;
    let jobs = prts_db::uploads::queue_batch_tx(&mut tx, project_id, batch_id, user.id)
        .await
        .map_err(|error| match error {
            sqlx::Error::Protocol(_) => Error::bad_request("upload_batch_incomplete").into(),
            other => db_err(other),
        })?
        .ok_or(Error::NotFound)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UploadBatchQueued {
            project_id,
            batch_id,
            file_count: jobs.len(),
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    state.job_worker.wake();
    let snapshot = prts_db::uploads::find_batch(&state.db, project_id, batch_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    Ok(Json(snapshot_dto(snapshot)))
}

/// 读取一个批次的文件、当前状态和全部 attempts 历史。
#[utoipa::path(
    get,
    path = "/projects/{id}/upload-batches/{batch_id}",
    tag = "upload",
    responses(
        (status = 200, body = UploadBatchDto),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse)
    )
)]
pub async fn get_batch(
    State(state): State<AppState>,
    Path((project_id, batch_id)): Path<(i64, i64)>,
    user: CurrentUser,
) -> Result<Json<UploadBatchDto>, ApiError> {
    paccess::load(&state, Some(&user), project_id)
        .await?
        .require_node(nodes::PROJECT_FILE_UPLOAD)?;
    let snapshot = prts_db::uploads::find_batch(&state.db, project_id, batch_id)
        .await
        .map_err(db_err)?
        .filter(|snapshot| snapshot.batch.actor_id == Some(user.id))
        .ok_or(Error::NotFound)?;
    Ok(Json(snapshot_dto(snapshot)))
}

/// 为失败逻辑文件创建新的 byte-zero attempt；processing job id 保持不变。
#[utoipa::path(
    post,
    path = "/projects/{id}/upload-batches/{batch_id}/files/{file_id}/retry",
    tag = "upload",
    responses(
        (status = 200, body = UploadAttemptDto),
        (status = 400, body = ErrorResponse),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse)
    )
)]
pub async fn retry_file(
    State(state): State<AppState>,
    Path((project_id, batch_id, file_id)): Path<(i64, i64, i64)>,
    user: CurrentUser,
) -> Result<Json<UploadAttemptDto>, ApiError> {
    paccess::load(&state, Some(&user), project_id)
        .await?
        .require_node(nodes::PROJECT_FILE_UPLOAD)?;
    let config = prts_db::upload_settings::get(&state.db)
        .await
        .map_err(db_err)?;
    let cleanup_after =
        chrono::Utc::now() + chrono::Duration::hours(i64::from(config.upload_batch_expiry_hours));
    let key = temp_key(project_id, &format!("batch-{batch_id}"));
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked.require_language_ready()?;
    let attempt = prts_db::uploads::retry_file_tx(
        &mut tx,
        project_id,
        batch_id,
        file_id,
        user.id,
        &key,
        cleanup_after,
    )
    .await
    .map_err(|error| match error {
        sqlx::Error::Protocol(_) => Error::bad_request("upload_file_not_retryable").into(),
        other => db_err(other),
    })?
    .ok_or(Error::NotFound)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::UploadFileRetried {
            project_id,
            batch_id,
            batch_file_id: file_id,
            attempt_id: attempt.id,
            attempt_number: attempt.attempt_number,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(UploadAttemptDto {
        id: attempt.id,
        attempt_number: attempt.attempt_number,
        state: attempt.state,
        bytes_received: attempt.bytes_received,
        error_code: attempt.error_code,
        started_at: attempt.started_at.to_rfc3339(),
        finished_at: attempt.finished_at.map(|value| value.to_rfc3339()),
    }))
}

/// 取消批次；已进入单文件事务的 worker 可原子完成，其余文件立即取消。
#[utoipa::path(
    post,
    path = "/projects/{id}/upload-batches/{batch_id}/cancel",
    tag = "upload",
    responses(
        (status = 204),
        (status = 401, body = ErrorResponse),
        (status = 403, body = ErrorResponse),
        (status = 404, body = ErrorResponse),
        (status = 503, body = ErrorResponse)
    )
)]
pub async fn cancel_batch(
    State(state): State<AppState>,
    Path((project_id, batch_id)): Path<(i64, i64)>,
    user: CurrentUser,
) -> Result<StatusCode, ApiError> {
    paccess::load(&state, Some(&user), project_id)
        .await?
        .require_node(nodes::PROJECT_FILE_UPLOAD)?;
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    let locked = paccess::load_locked_tx(&mut tx, &user, project).await?;
    locked.require_node(nodes::PROJECT_FILE_UPLOAD)?;
    locked.require_language_ready()?;
    let temp_keys = prts_db::uploads::cancel_batch_tx(&mut tx, project_id, batch_id, user.id)
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
        AuditEvent::UploadBatchCancelled {
            project_id,
            batch_id,
            file_count: temp_keys.len(),
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    for key in temp_keys {
        remove_temp(&state.settings.media.upload_temp_directory, &key).await;
    }
    state.job_worker.wake();
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_paths_reject_traversal_reserved_segments_and_conflicts() {
        for invalid in [
            "../escape.json",
            "/root.json",
            "folder//file.json",
            "folder/.hidden.json",
            "folder/CON.json",
            "folder/com1.data.json",
            "folder/name.json ",
            "folder/invalid?.json",
            "folder/not-json.txt",
        ] {
            assert!(
                normalize_upload_path(invalid).is_err(),
                "accepted {invalid}"
            );
        }
        assert_eq!(
            normalize_upload_path("folder\\nested\\file.JSON")
                .ok()
                .expect("valid upload path"),
            "folder/nested/file.JSON"
        );
    }

    #[test]
    fn upload_attempt_route_rejects_offset_headers_before_body_processing() {
        let source = include_str!("uploads.rs");
        let route = source
            .split_once("pub async fn receive_attempt(")
            .unwrap()
            .1
            .split_once("pub async fn complete_batch(")
            .unwrap()
            .0;
        assert!(route.contains("header::RANGE"));
        assert!(route.contains("header::CONTENT_RANGE"));
        assert!(route.contains("upload_resume_not_supported"));
        assert!(
            route.find("upload_resume_not_supported").unwrap()
                < route.find("into_data_stream").unwrap()
        );
    }

    #[test]
    fn declarations_enforce_runtime_file_and_batch_limits_without_reading_bodies() {
        let config = prts_db::upload_settings::UploadConfig::default();
        let declarations = |count: usize, size: i64| {
            (0..count)
                .map(|index| UploadFileDeclarationReq {
                    path: format!("folder/{index}.json"),
                    size,
                })
                .collect()
        };

        assert!(validate_declarations(1, declarations(501, 1), &config).is_err());
        assert!(validate_declarations(1, declarations(1, 100 * 1024 * 1024 + 1), &config).is_err());
        let mut over_batch = declarations(20, 100 * 1024 * 1024);
        over_batch.push(UploadFileDeclarationReq {
            path: "folder/remainder.json".to_string(),
            size: 48 * 1024 * 1024 + 1,
        });
        assert!(validate_declarations(1, over_batch, &config).is_err());
        assert!(validate_declarations(
            1,
            vec![
                UploadFileDeclarationReq {
                    path: "duplicate.json".to_string(),
                    size: 1,
                },
                UploadFileDeclarationReq {
                    path: "duplicate.json".to_string(),
                    size: 1,
                },
            ],
            &config,
        )
        .is_err());
    }
}
