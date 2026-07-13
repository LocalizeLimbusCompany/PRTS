//! 原始文件上传批次、逻辑文件与 byte-zero attempt 状态机。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgConnection, PgPool};

use crate::audit::{self, AuditActor, AuditActorKind, AuditEvent};
use crate::models::{Job, UploadBatch, UploadBatchFile, UploadFileAttempt};

#[derive(Debug, Clone)]
pub struct UploadDeclaration {
    pub path: String,
    pub declared_bytes: i64,
    pub temp_key: String,
}

#[derive(Debug, Clone)]
pub struct DeclaredUploadFile {
    pub file: UploadBatchFile,
    pub attempt: UploadFileAttempt,
}

#[derive(Debug, Clone)]
pub struct UploadBatchSnapshot {
    pub batch: UploadBatch,
    pub files: Vec<UploadBatchFile>,
    pub attempts: Vec<UploadFileAttempt>,
    /// upload_process 失败时的 allowlisted ordinal/line/column 元数据。
    pub processing_error_details: HashMap<i64, serde_json::Value>,
}

/// 已锁定 upload_process job 对应的单文件事务上下文。
#[derive(Debug, Clone)]
pub struct UploadProcessingContext {
    pub project_id: i64,
    pub actor_id: i64,
    pub batch_id: i64,
    pub batch_file_id: i64,
    pub attempt_id: i64,
    pub path: String,
    pub temp_key: String,
    pub source_langs: Vec<String>,
    pub language_repair_state: String,
}

#[derive(Debug, FromRow)]
struct UploadProcessingRow {
    project_id_snapshot: i64,
    actor_id: Option<i64>,
    batch_state: String,
    batch_file_id: i64,
    attempt_id: i64,
    path: String,
    temp_key: String,
    source_langs: Vec<String>,
    language_repair_state: String,
}

/// 在已锁定、重新授权的项目事务内声明整个批次及首轮 attempts。
pub async fn create_batch_tx(
    conn: &mut PgConnection,
    project_id: i64,
    actor_id: i64,
    declarations: &[UploadDeclaration],
    expires_at: DateTime<Utc>,
) -> Result<UploadBatchSnapshot, sqlx::Error> {
    let total_bytes = declarations
        .iter()
        .try_fold(0_i64, |total, file| total.checked_add(file.declared_bytes))
        .ok_or_else(|| sqlx::Error::Protocol("declared upload bytes overflow".to_string()))?;
    let batch: UploadBatch = sqlx::query_as(
        "INSERT INTO upload_batches (
             project_id, project_id_snapshot, actor_id, state,
             declared_file_count, declared_total_bytes, expires_at
         ) VALUES ($1, $1, $2, 'uploading', $3, $4, $5)
         RETURNING *",
    )
    .bind(project_id)
    .bind(actor_id)
    .bind(declarations.len() as i32)
    .bind(total_bytes)
    .bind(expires_at)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO jobs (
             kind, project_id, stage, payload, max_attempts, run_after
         ) VALUES (
             'upload_cleanup', $1, 'expiry', jsonb_build_object('batch_id', $2), 5, $3
         )",
    )
    .bind(project_id)
    .bind(batch.id)
    .bind(expires_at)
    .execute(&mut *conn)
    .await?;

    let mut files = Vec::with_capacity(declarations.len());
    let mut attempts = Vec::with_capacity(declarations.len());
    for (index, declaration) in declarations.iter().enumerate() {
        let file: UploadBatchFile = sqlx::query_as(
            "INSERT INTO upload_batch_files (
                 batch_id, ordinal, path, declared_bytes, state
             ) VALUES ($1, $2, $3, $4, 'uploading')
             RETURNING *",
        )
        .bind(batch.id)
        .bind(index as i32)
        .bind(&declaration.path)
        .bind(declaration.declared_bytes)
        .fetch_one(&mut *conn)
        .await?;
        let attempt: UploadFileAttempt = sqlx::query_as(
            "INSERT INTO upload_file_attempts (
                 batch_file_id, attempt_number, state, temp_key, cleanup_after
             ) VALUES ($1, 1, 'uploading', $2, $3)
             RETURNING *",
        )
        .bind(file.id)
        .bind(&declaration.temp_key)
        .bind(expires_at)
        .fetch_one(&mut *conn)
        .await?;
        sqlx::query("UPDATE upload_batch_files SET current_attempt_id = $2 WHERE id = $1")
            .bind(file.id)
            .bind(attempt.id)
            .execute(&mut *conn)
            .await?;
        files.push(UploadBatchFile {
            current_attempt_id: Some(attempt.id),
            ..file
        });
        attempts.push(attempt);
    }
    Ok(UploadBatchSnapshot {
        batch,
        files,
        attempts,
        processing_error_details: HashMap::new(),
    })
}

pub async fn find_batch(
    pool: &PgPool,
    project_id: i64,
    batch_id: i64,
) -> Result<Option<UploadBatchSnapshot>, sqlx::Error> {
    let Some(batch) = sqlx::query_as::<_, UploadBatch>(
        "SELECT * FROM upload_batches
         WHERE id = $1 AND project_id_snapshot = $2",
    )
    .bind(batch_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let files = sqlx::query_as::<_, UploadBatchFile>(
        "SELECT * FROM upload_batch_files WHERE batch_id = $1 ORDER BY ordinal",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;
    let attempts = sqlx::query_as::<_, UploadFileAttempt>(
        "SELECT attempt.* FROM upload_file_attempts AS attempt
         JOIN upload_batch_files AS file ON file.id = attempt.batch_file_id
         WHERE file.batch_id = $1 ORDER BY file.ordinal, attempt.attempt_number",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;
    let processing_error_details = sqlx::query_as::<_, (i64, serde_json::Value)>(
        "SELECT file.id, job.result
         FROM upload_batch_files AS file
         JOIN jobs AS job ON job.id = file.processing_job_id
         WHERE file.batch_id = $1
           AND file.last_error_code IS NOT NULL
           AND job.result IS NOT NULL",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();
    Ok(Some(UploadBatchSnapshot {
        batch,
        files,
        attempts,
        processing_error_details,
    }))
}

pub async fn claim_attempt_for_receive_tx(
    conn: &mut PgConnection,
    project_id: i64,
    batch_id: i64,
    batch_file_id: i64,
    attempt_id: i64,
) -> Result<Option<(UploadBatch, UploadBatchFile, UploadFileAttempt)>, sqlx::Error> {
    let Some(batch) = sqlx::query_as::<_, UploadBatch>(
        "SELECT * FROM upload_batches
         WHERE id = $1 AND project_id_snapshot = $2 FOR UPDATE",
    )
    .bind(batch_id)
    .bind(project_id)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    let Some(file) = sqlx::query_as::<_, UploadBatchFile>(
        "SELECT * FROM upload_batch_files
         WHERE id = $1 AND batch_id = $2 FOR UPDATE",
    )
    .bind(batch_file_id)
    .bind(batch_id)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    let attempt = sqlx::query_as::<_, UploadFileAttempt>(
        "UPDATE upload_file_attempts
         SET state = 'receiving'
         WHERE id = $1 AND batch_file_id = $2 AND state = 'uploading'
         RETURNING *",
    )
    .bind(attempt_id)
    .bind(batch_file_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(attempt.map(|attempt| (batch, file, attempt)))
}

/// 完整临时文件发布后，原子记录接收字节数并进入待提交状态。
pub async fn mark_attempt_received_tx(
    conn: &mut PgConnection,
    batch_file_id: i64,
    attempt_id: i64,
    bytes_received: i64,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query(
        "WITH attempt AS (
             UPDATE upload_file_attempts
             SET state = 'queued', bytes_received = $3, finished_at = now()
             WHERE id = $2 AND batch_file_id = $1 AND state = 'receiving'
             RETURNING id
         )
         UPDATE upload_batch_files AS file
         SET state = 'queued', last_error_code = NULL
         FROM attempt
         WHERE file.id = $1 AND file.current_attempt_id = attempt.id",
    )
    .bind(batch_file_id)
    .bind(attempt_id)
    .bind(bytes_received)
    .execute(conn)
    .await?;
    Ok(updated.rows_affected() == 1)
}

pub async fn fail_attempt_tx(
    conn: &mut PgConnection,
    batch_file_id: i64,
    attempt_id: i64,
    error_code: &str,
    bytes_received: i64,
) -> Result<bool, sqlx::Error> {
    let file_id: Option<i64> = sqlx::query_scalar(
        "UPDATE upload_file_attempts
         SET state = 'failed', bytes_received = $3, error_code = $2,
             finished_at = now(), cleanup_after = now()
         WHERE id = $1 AND batch_file_id = $4 AND state IN ('uploading', 'receiving')
         RETURNING batch_file_id",
    )
    .bind(attempt_id)
    .bind(error_code)
    .bind(bytes_received)
    .bind(batch_file_id)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(file_id) = file_id {
        sqlx::query(
            "UPDATE upload_batch_files SET state = 'failed', last_error_code = $2
             WHERE id = $1 AND current_attempt_id = $3",
        )
        .bind(file_id)
        .bind(error_code)
        .bind(attempt_id)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "INSERT INTO jobs (kind, project_id, stage, payload, max_attempts, run_after)
             SELECT 'upload_cleanup', batch.project_id, 'attempt_cleanup',
                    jsonb_build_object('batch_id', batch.id, 'attempt_id', $2), 5, now()
             FROM upload_batch_files AS file
             JOIN upload_batches AS batch ON batch.id = file.batch_id
             WHERE file.id = $1 AND batch.project_id IS NOT NULL",
        )
        .bind(file_id)
        .bind(attempt_id)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "WITH target AS (
                 SELECT batch_id FROM upload_batch_files WHERE id = $1
             ), counts AS (
                 SELECT file.batch_id,
                        count(*) FILTER (WHERE file.state = 'succeeded')::BIGINT AS succeeded,
                        count(*) FILTER (
                            WHERE file.state IN ('uploading', 'queued', 'processing')
                        )::BIGINT AS active
                 FROM upload_batch_files AS file
                 JOIN target ON target.batch_id = file.batch_id
                 GROUP BY file.batch_id
             )
             UPDATE upload_batches AS batch
             SET state = CASE WHEN counts.succeeded > 0
                              THEN 'partially_succeeded' ELSE 'failed' END,
                 completed_at = now()
             FROM counts
             WHERE batch.id = counts.batch_id
               AND batch.state IN ('draft', 'uploading')
               AND counts.active = 0",
        )
        .bind(file_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(file_id.is_some())
}

/// 校验当前已接收 attempts 完整后，为这些逻辑文件创建或复用处理 job。
///
/// 首次提交可跳过传输失败文件；后续 byte-zero retry 也只重新排队对应文件，已经
/// succeeded 的同批文件保持不变。
pub async fn queue_batch_tx(
    conn: &mut PgConnection,
    project_id: i64,
    batch_id: i64,
    actor_id: i64,
) -> Result<Option<Vec<Job>>, sqlx::Error> {
    let Some(batch) = sqlx::query_as::<_, UploadBatch>(
        "SELECT * FROM upload_batches
         WHERE id = $1 AND project_id_snapshot = $2 AND actor_id = $3 FOR UPDATE",
    )
    .bind(batch_id)
    .bind(project_id)
    .bind(actor_id)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    if !matches!(batch.state.as_str(), "uploading" | "draft") {
        return Err(sqlx::Error::Protocol(
            "upload batch cannot be completed".to_string(),
        ));
    }
    let files = sqlx::query_as::<_, UploadBatchFile>(
        "SELECT * FROM upload_batch_files WHERE batch_id = $1 ORDER BY ordinal FOR UPDATE",
    )
    .bind(batch_id)
    .fetch_all(&mut *conn)
    .await?;
    if files
        .iter()
        .any(|file| matches!(file.state.as_str(), "uploading" | "processing"))
    {
        return Err(sqlx::Error::Protocol(
            "upload batch is incomplete".to_string(),
        ));
    }
    let queueable = files
        .into_iter()
        .filter(|file| file.state == "queued")
        .collect::<Vec<_>>();
    if queueable.is_empty() {
        return Err(sqlx::Error::Protocol(
            "upload batch has no received files".to_string(),
        ));
    }
    let ready: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM upload_batch_files AS file
         JOIN upload_file_attempts AS attempt ON attempt.id = file.current_attempt_id
         WHERE file.batch_id = $1 AND file.state = 'queued' AND attempt.state = 'queued'
           AND attempt.bytes_received = file.declared_bytes",
    )
    .bind(batch_id)
    .fetch_one(&mut *conn)
    .await?;
    if ready != queueable.len() as i64 {
        return Err(sqlx::Error::Protocol(
            "upload batch is incomplete".to_string(),
        ));
    }
    let mut jobs = Vec::with_capacity(queueable.len());
    for file in queueable {
        let attempt_id = file.current_attempt_id.ok_or_else(|| {
            sqlx::Error::Protocol("upload file has no current attempt".to_string())
        })?;
        let job: Job = match file.processing_job_id {
            Some(job_id) => {
                sqlx::query_as(
                    "UPDATE jobs SET project_id = $6, state = 'queued', stage = 'queued',
                     run_after = now(), result = NULL, target_file_id = NULL,
                     worker_id = NULL, lease_until = NULL, finished_at = NULL,
                     started_at = NULL,
                     last_error_code = NULL, last_error_message = NULL,
                     attempts = 0, progress_current = 0, progress_total = $5,
                     payload = jsonb_build_object(
                         'batch_id', $2, 'batch_file_id', $3, 'attempt_id', $4
                     )
                 WHERE id = $1 AND kind = 'upload_process'
                   AND upload_batch_file_id = $3
                 RETURNING *",
                )
                .bind(job_id)
                .bind(batch_id)
                .bind(file.id)
                .bind(attempt_id)
                .bind(file.declared_bytes)
                .bind(project_id)
                .fetch_one(&mut *conn)
                .await?
            }
            None => {
                sqlx::query_as(
                    "INSERT INTO jobs (
                     kind, project_id, stage, payload, progress_total,
                     max_attempts, upload_batch_file_id
                 ) VALUES (
                     'upload_process', $1, 'queued',
                     jsonb_build_object('batch_id', $2, 'batch_file_id', $3, 'attempt_id', $4),
                     $5, 1, $3
                 ) RETURNING *",
                )
                .bind(project_id)
                .bind(batch_id)
                .bind(file.id)
                .bind(attempt_id)
                .bind(file.declared_bytes)
                .fetch_one(&mut *conn)
                .await?
            }
        };
        sqlx::query(
            "UPDATE upload_batch_files SET state = 'queued', processing_job_id = $2 WHERE id = $1",
        )
        .bind(file.id)
        .bind(job.id)
        .execute(&mut *conn)
        .await?;
        jobs.push(job);
    }
    sqlx::query("UPDATE upload_batches SET state = 'queued', completed_at = now() WHERE id = $1")
        .bind(batch_id)
        .execute(conn)
        .await?;
    Ok(Some(jobs))
}

/// 在同一 logical file 下创建下一次 byte-zero attempt，保留旧历史与 job id。
pub async fn retry_file_tx(
    conn: &mut PgConnection,
    project_id: i64,
    batch_id: i64,
    batch_file_id: i64,
    actor_id: i64,
    temp_key: &str,
    cleanup_after: DateTime<Utc>,
) -> Result<Option<UploadFileAttempt>, sqlx::Error> {
    let batch_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM upload_batches
             WHERE id = $1 AND project_id_snapshot = $2 AND actor_id = $3
               AND state NOT IN ('cancelling', 'cancelled', 'expired', 'succeeded')
               AND NOT EXISTS (
                   SELECT 1 FROM upload_batch_files
                   WHERE batch_id = $1 AND id <> $4
                     AND state IN ('uploading', 'queued', 'processing')
               )
         )",
    )
    .bind(batch_id)
    .bind(project_id)
    .bind(actor_id)
    .bind(batch_file_id)
    .fetch_one(&mut *conn)
    .await?;
    if !batch_exists {
        return Ok(None);
    }
    let Some(file) = sqlx::query_as::<_, UploadBatchFile>(
        "SELECT * FROM upload_batch_files
         WHERE id = $1 AND batch_id = $2 FOR UPDATE",
    )
    .bind(batch_file_id)
    .bind(batch_id)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    if !matches!(file.state.as_str(), "failed" | "cancelled" | "expired") {
        return Err(sqlx::Error::Protocol(
            "upload file cannot be retried".to_string(),
        ));
    }
    let next_number: i32 = sqlx::query_scalar(
        "SELECT COALESCE(max(attempt_number), 0) + 1
         FROM upload_file_attempts WHERE batch_file_id = $1",
    )
    .bind(batch_file_id)
    .fetch_one(&mut *conn)
    .await?;
    let attempt: UploadFileAttempt = sqlx::query_as(
        "INSERT INTO upload_file_attempts (
             batch_file_id, attempt_number, state, temp_key, cleanup_after
         ) VALUES ($1, $2, 'uploading', $3, $4)
         RETURNING *",
    )
    .bind(batch_file_id)
    .bind(next_number)
    .bind(temp_key)
    .bind(cleanup_after)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE upload_batch_files
         SET state = 'uploading', current_attempt_id = $2, last_error_code = NULL
         WHERE id = $1",
    )
    .bind(batch_file_id)
    .bind(attempt.id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE upload_batches SET state = 'uploading', completed_at = NULL WHERE id = $1")
        .bind(batch_id)
        .execute(conn)
        .await?;
    Ok(Some(attempt))
}

/// 取消未开始的 attempts/jobs；running 单文件事务可自行完成后再收敛 batch。
pub async fn cancel_batch_tx(
    conn: &mut PgConnection,
    project_id: i64,
    batch_id: i64,
    actor_id: i64,
) -> Result<Option<Vec<String>>, sqlx::Error> {
    let batch = sqlx::query_as::<_, UploadBatch>(
        "UPDATE upload_batches SET state = 'cancelling'
         WHERE id = $1 AND project_id_snapshot = $2 AND actor_id = $3
           AND state NOT IN ('cancelled', 'expired', 'succeeded')
         RETURNING *",
    )
    .bind(batch_id)
    .bind(project_id)
    .bind(actor_id)
    .fetch_optional(&mut *conn)
    .await?;
    if batch.is_none() {
        return Ok(None);
    }
    sqlx::query(
        "UPDATE jobs SET state = 'cancelled', finished_at = now()
         WHERE upload_batch_file_id IN (
             SELECT id FROM upload_batch_files WHERE batch_id = $1
         ) AND state IN ('queued', 'paused')",
    )
    .bind(batch_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO jobs (kind, project_id, stage, payload, max_attempts, run_after)
         VALUES (
             'upload_cleanup', $1, 'cancel_cleanup',
             jsonb_build_object('batch_id', $2), 5, now()
         )",
    )
    .bind(project_id)
    .bind(batch_id)
    .execute(&mut *conn)
    .await?;
    let temp_keys: Vec<String> = sqlx::query_scalar(
        "UPDATE upload_file_attempts AS attempt
         SET state = 'cancelled', finished_at = now(), cleanup_after = now()
         FROM upload_batch_files AS file
         WHERE attempt.batch_file_id = file.id AND file.batch_id = $1
           AND attempt.state IN ('uploading', 'receiving', 'queued')
         RETURNING attempt.temp_key",
    )
    .bind(batch_id)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE upload_batch_files SET state = 'cancelled'
         WHERE batch_id = $1 AND state IN ('uploading', 'queued', 'failed', 'expired')",
    )
    .bind(batch_id)
    .execute(&mut *conn)
    .await?;
    let running: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs
         WHERE upload_batch_file_id IN (
             SELECT id FROM upload_batch_files WHERE batch_id = $1
         ) AND state = 'running'",
    )
    .bind(batch_id)
    .fetch_one(&mut *conn)
    .await?;
    if running == 0 {
        sqlx::query(
            "UPDATE upload_batches
             SET state = 'cancelled', cancelled_at = now() WHERE id = $1",
        )
        .bind(batch_id)
        .execute(conn)
        .await?;
    }
    Ok(Some(temp_keys))
}

/// 领取到期批次并返回需要幂等删除的临时 key。
pub async fn expire_due(pool: &PgPool, limit: i64) -> Result<Vec<String>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let batch_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM upload_batches
         WHERE expires_at <= now()
           AND state IN ('draft', 'uploading', 'queued', 'processing', 'cancelling')
         ORDER BY expires_at, id FOR UPDATE SKIP LOCKED LIMIT $1",
    )
    .bind(limit.clamp(1, 500))
    .fetch_all(&mut *tx)
    .await?;
    if batch_ids.is_empty() {
        tx.rollback().await?;
        return Ok(Vec::new());
    }
    sqlx::query(
        "UPDATE jobs SET state = 'cancelled', finished_at = now()
         WHERE upload_batch_file_id IN (
             SELECT id FROM upload_batch_files WHERE batch_id = ANY($1::BIGINT[])
         ) AND state IN ('queued', 'paused')",
    )
    .bind(&batch_ids)
    .execute(&mut *tx)
    .await?;
    let keys: Vec<String> = sqlx::query_scalar(
        "UPDATE upload_file_attempts AS attempt
         SET state = 'expired', finished_at = now(), cleanup_after = now()
         FROM upload_batch_files AS file
         WHERE attempt.batch_file_id = file.id AND file.batch_id = ANY($1::BIGINT[])
           AND attempt.state NOT IN ('processing', 'succeeded')
         RETURNING attempt.temp_key",
    )
    .bind(&batch_ids)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE upload_batch_files SET state = 'expired'
         WHERE batch_id = ANY($1::BIGINT[]) AND state NOT IN ('processing', 'succeeded')",
    )
    .bind(&batch_ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE upload_batches SET state = 'expired' WHERE id = ANY($1::BIGINT[])")
        .bind(&batch_ids)
        .execute(&mut *tx)
        .await?;
    let expired = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT project_id_snapshot, id, declared_file_count::BIGINT
         FROM upload_batches WHERE id = ANY($1::BIGINT[])",
    )
    .bind(&batch_ids)
    .fetch_all(&mut *tx)
    .await?;
    for (project_id, batch_id, file_count) in expired {
        audit::append_event_tx(
            &mut tx,
            AuditActor {
                id: None,
                kind: AuditActorKind::System,
                ip: None,
            },
            AuditEvent::UploadBatchExpired {
                project_id,
                batch_id,
                file_count,
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(keys)
}

/// 返回到期且尚未确认物理删除的 terminal attempt 临时对象。
pub async fn list_cleanup_candidates(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, temp_key FROM upload_file_attempts
         WHERE cleanup_after <= now() AND cleaned_at IS NULL
           AND state IN ('failed', 'cancelled', 'expired', 'succeeded')
         ORDER BY cleanup_after, id LIMIT $1",
    )
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await
}

/// 物理对象及其 `.part` 兄弟均已不存在后，记录幂等清理完成。
pub async fn mark_attempt_cleaned(pool: &PgPool, attempt_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let cleaned = sqlx::query_as::<_, (i64, i64, i64)>(
        "WITH cleaned AS (
             UPDATE upload_file_attempts SET cleaned_at = now()
             WHERE id = $1 AND cleaned_at IS NULL
               AND state IN ('failed', 'cancelled', 'expired', 'succeeded')
             RETURNING batch_file_id
         )
         SELECT batch.project_id_snapshot, file.batch_id, file.id
         FROM cleaned
         JOIN upload_batch_files AS file ON file.id = cleaned.batch_file_id
         JOIN upload_batches AS batch ON batch.id = file.batch_id",
    )
    .bind(attempt_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((project_id, batch_id, batch_file_id)) = cleaned {
        audit::append_event_tx(
            &mut tx,
            AuditActor {
                id: None,
                kind: AuditActorKind::System,
                ip: None,
            },
            AuditEvent::UploadAttemptCleaned {
                project_id,
                batch_id,
                batch_file_id,
                attempt_id,
            },
        )
        .await?;
    }
    tx.commit().await
}

/// 在文件事务内锁定并重新校验 upload_process 的项目、actor、逻辑文件与 attempt。
///
/// 只有 owner/manager 或具备平台全项目管理能力的主体可继续；撤权、语言 repair gate、
/// stale attempt 或取消竞态均 fail closed。成功后把 attempt/file 标为 processing。
pub async fn begin_processing_tx(
    conn: &mut PgConnection,
    job_id: i64,
    project_id: i64,
    batch_id: i64,
    batch_file_id: i64,
    attempt_id: i64,
) -> Result<Option<UploadProcessingContext>, sqlx::Error> {
    let row: Option<UploadProcessingRow> = sqlx::query_as(
        "SELECT batch.project_id_snapshot,
                batch.actor_id,
                batch.state AS batch_state,
                file.id AS batch_file_id,
                attempt.id AS attempt_id,
                file.path,
                attempt.temp_key,
                project.source_langs,
                project.language_repair_state
         FROM jobs AS job
         JOIN upload_batch_files AS file
           ON file.id = job.upload_batch_file_id
         JOIN upload_batches AS batch ON batch.id = file.batch_id
         JOIN upload_file_attempts AS attempt
           ON attempt.id = file.current_attempt_id
         JOIN projects AS project ON project.id = batch.project_id
         WHERE job.id = $1
           AND job.kind = 'upload_process'
           AND job.state = 'running'
           AND job.project_id = $2
           AND batch.project_id = $2
           AND batch.project_id_snapshot = $2
           AND batch.id = $3
           AND file.id = $4
           AND attempt.id = $5
           AND file.processing_job_id = job.id
           AND file.state = 'queued'
           AND attempt.state = 'queued'
           AND batch.state IN ('queued', 'processing', 'cancelling')
         FOR UPDATE OF project, batch, file, attempt",
    )
    .bind(job_id)
    .bind(project_id)
    .bind(batch_id)
    .bind(batch_file_id)
    .bind(attempt_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let actor_id = row.actor_id.ok_or_else(|| {
        sqlx::Error::Protocol("upload processing actor no longer exists".to_string())
    })?;
    let authorized: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1
             FROM users AS actor
             JOIN projects AS project ON project.id = $1
             LEFT JOIN memberships AS membership
               ON membership.project_id = project.id AND membership.user_id = actor.id
             WHERE actor.id = $2
               AND (
                    project.owner_id = actor.id
                    OR membership.role IN ('owner', 'manager')
                    OR actor.platform_role IN ('super_admin', 'admin')
               )
         )",
    )
    .bind(row.project_id_snapshot)
    .bind(actor_id)
    .fetch_one(&mut *conn)
    .await?;
    if !authorized {
        return Err(sqlx::Error::Protocol(
            "upload processing permission was revoked".to_string(),
        ));
    }
    sqlx::query(
        "UPDATE upload_file_attempts
         SET state = 'processing'
         WHERE id = $1 AND state = 'queued'",
    )
    .bind(row.attempt_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE upload_batch_files
         SET state = 'processing'
         WHERE id = $1 AND state = 'queued'",
    )
    .bind(row.batch_file_id)
    .execute(&mut *conn)
    .await?;
    if row.batch_state != "cancelling" {
        sqlx::query("UPDATE upload_batches SET state = 'processing' WHERE id = $1")
            .bind(batch_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(Some(UploadProcessingContext {
        project_id: row.project_id_snapshot,
        actor_id,
        batch_id,
        batch_file_id: row.batch_file_id,
        attempt_id: row.attempt_id,
        path: row.path,
        temp_key: row.temp_key,
        source_langs: row.source_langs,
        language_repair_state: row.language_repair_state,
    }))
}

/// replacement 业务写、history 与 audit 成功后，在同一事务收敛 attempt/file/batch。
pub async fn mark_processing_succeeded_tx(
    conn: &mut PgConnection,
    job_id: i64,
    context: &UploadProcessingContext,
    target_file_id: i64,
) -> Result<(), sqlx::Error> {
    let attempt_updated = sqlx::query(
        "UPDATE upload_file_attempts
         SET state = 'succeeded', target_file_id = $2,
             error_code = NULL, finished_at = now(), cleanup_after = now()
         WHERE id = $1 AND batch_file_id = $3 AND state = 'processing'",
    )
    .bind(context.attempt_id)
    .bind(target_file_id)
    .bind(context.batch_file_id)
    .execute(&mut *conn)
    .await?;
    if attempt_updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "upload processing attempt lost its state".to_string(),
        ));
    }
    let file_updated = sqlx::query(
        "UPDATE upload_batch_files
         SET state = 'succeeded', target_file_id = $2, last_error_code = NULL
         WHERE id = $1 AND processing_job_id = $3 AND state = 'processing'",
    )
    .bind(context.batch_file_id)
    .bind(target_file_id)
    .bind(job_id)
    .execute(&mut *conn)
    .await?;
    if file_updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "upload logical file lost its processing state".to_string(),
        ));
    }
    let job_updated = sqlx::query("UPDATE jobs SET target_file_id = $2 WHERE id = $1")
        .bind(job_id)
        .bind(target_file_id)
        .execute(&mut *conn)
        .await?;
    if job_updated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "upload processing job disappeared".to_string(),
        ));
    }
    reconcile_batch_state_tx(&mut *conn, context.batch_id).await
}

/// job 失败事务内把 queued/processing attempt 与 logical file 标为 failed，并保存
/// allowlisted 位置元数据到 job result；原始上传正文和 parser 文本不会持久化。
pub async fn mark_processing_failed_tx(
    conn: &mut PgConnection,
    job_id: i64,
    error_code: &str,
    error_details: Option<&serde_json::Value>,
) -> Result<(), sqlx::Error> {
    let row: Option<(i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT batch.project_id_snapshot, file.batch_id, file.id,
                attempt.id, attempt.bytes_received
         FROM jobs AS job
         JOIN upload_batch_files AS file ON file.id = job.upload_batch_file_id
         JOIN upload_batches AS batch ON batch.id = file.batch_id
         JOIN upload_file_attempts AS attempt ON attempt.id = file.current_attempt_id
         WHERE job.id = $1 AND job.kind = 'upload_process'
         FOR UPDATE OF file, attempt",
    )
    .bind(job_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((project_id, batch_id, batch_file_id, attempt_id, bytes_received)) = row else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE upload_file_attempts
         SET state = 'failed', error_code = $2, finished_at = now(), cleanup_after = now()
         WHERE id = $1 AND state IN ('queued', 'processing')",
    )
    .bind(attempt_id)
    .bind(error_code)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE upload_batch_files
         SET state = 'failed', last_error_code = $2
         WHERE id = $1 AND state IN ('queued', 'processing')",
    )
    .bind(batch_file_id)
    .bind(error_code)
    .execute(&mut *conn)
    .await?;
    if let Some(details) = error_details {
        sqlx::query("UPDATE jobs SET result = $2 WHERE id = $1")
            .bind(job_id)
            .bind(details)
            .execute(&mut *conn)
            .await?;
    }
    audit::append_event_tx(
        &mut *conn,
        AuditActor {
            id: None,
            kind: AuditActorKind::System,
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
    .await?;
    reconcile_batch_state_tx(&mut *conn, batch_id).await
}

async fn reconcile_batch_state_tx(
    conn: &mut PgConnection,
    batch_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "WITH counts AS (
             SELECT count(*)::BIGINT AS total,
                    count(*) FILTER (WHERE state = 'succeeded')::BIGINT AS succeeded,
                    count(*) FILTER (
                        WHERE state IN ('uploading', 'queued', 'processing')
                    )::BIGINT AS active
             FROM upload_batch_files WHERE batch_id = $1
         )
         UPDATE upload_batches AS batch
         SET state = CASE
                 WHEN batch.state = 'cancelling' AND counts.active = 0 THEN 'cancelled'
                 WHEN counts.active > 0 THEN
                     CASE WHEN batch.state = 'cancelling' THEN 'cancelling' ELSE 'processing' END
                 WHEN counts.succeeded = counts.total THEN 'succeeded'
                 WHEN counts.succeeded > 0 THEN 'partially_succeeded'
                 ELSE 'failed'
             END,
             completed_at = CASE WHEN counts.active = 0 THEN now() ELSE batch.completed_at END,
             cancelled_at = CASE
                 WHEN batch.state = 'cancelling' AND counts.active = 0 THEN now()
                 ELSE batch.cancelled_at
             END
         FROM counts
         WHERE batch.id = $1",
    )
    .bind(batch_id)
    .execute(conn)
    .await?;
    Ok(())
}
