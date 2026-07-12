//! 持久化任务仓储：类型化创建/完成、可见性键集读取、租约领取与同一行重试。
//!
//! 所有 SQL 均参数化。待删除项目集合由调用方显式传入，因此 foundation 不硬引用
//! `0014` 才会创建的项目删除列。

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgConnection, PgPool};

use crate::models::Job;

/// 项目永久清除在项目行删除后仍需使用的不可变 snapshot。
#[derive(Debug, Clone, Serialize)]
pub struct ProjectPurgeSnapshot {
    pub project_id: i64,
    pub slug: String,
    pub media_keys: Vec<String>,
    pub temp_keys: Vec<String>,
    pub deadline: DateTime<Utc>,
}

/// kind 与 payload 被同一 enum 绑定；调用方无法把任意 JSON 或错误 payload 配给 kind。
#[derive(Debug, Clone)]
pub enum JobKind {
    ProjectPurge(ProjectPurgeSnapshot),
    PrimarySourceLexicalReindex,
    PrimarySourceEmbeddingBackfill,
    LanguageRepair,
    UploadProcess,
    UploadCleanup,
    FilePurge,
    FileRetentionCleanup,
}

impl JobKind {
    /// 解析不需要 payload 的 allowlisted kind。purge 必须显式提供 snapshot，故不从字符串构造。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "primary_source_lexical_reindex" => Some(Self::PrimarySourceLexicalReindex),
            "primary_source_embedding_backfill" => Some(Self::PrimarySourceEmbeddingBackfill),
            "language_repair" => Some(Self::LanguageRepair),
            "upload_process" => Some(Self::UploadProcess),
            "upload_cleanup" => Some(Self::UploadCleanup),
            "file_purge" => Some(Self::FilePurge),
            "file_retention_cleanup" => Some(Self::FileRetentionCleanup),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ProjectPurge(_) => "project_purge",
            Self::PrimarySourceLexicalReindex => "primary_source_lexical_reindex",
            Self::PrimarySourceEmbeddingBackfill => "primary_source_embedding_backfill",
            Self::LanguageRepair => "language_repair",
            Self::UploadProcess => "upload_process",
            Self::UploadCleanup => "upload_cleanup",
            Self::FilePurge => "file_purge",
            Self::FileRetentionCleanup => "file_retention_cleanup",
        }
    }

    fn into_parts(self) -> (&'static str, serde_json::Value) {
        match self {
            Self::ProjectPurge(snapshot) => (
                "project_purge",
                serde_json::to_value(snapshot).expect("allowlisted purge snapshot must serialize"),
            ),
            other => (other.as_str(), serde_json::json!({})),
        }
    }
}

/// allowlisted worker 结果。foundation 不提前定义未来业务 handler 的结果字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobResult {
    Completed,
    EmbeddingSkipped,
}

impl JobResult {
    fn into_json(self) -> serde_json::Value {
        match self {
            Self::Completed => serde_json::json!({}),
            Self::EmbeddingSkipped => serde_json::json!({
                "outcome": "skipped",
                "reason": "embedding_provider_unconfigured",
            }),
        }
    }
}

/// 新建任务参数；payload 只能由 `kind` variant 产生。
#[derive(Debug, Clone)]
pub struct NewJob {
    pub kind: JobKind,
    pub project_id: Option<i64>,
    pub stage: String,
    pub progress_total: Option<i64>,
    pub max_attempts: i32,
    pub run_after: DateTime<Utc>,
}

/// 在现有事务连接内创建任务，供业务写与审计原子提交。
pub async fn create_tx(conn: &mut PgConnection, input: NewJob) -> Result<Job, sqlx::Error> {
    if input.project_id.is_none() && !matches!(&input.kind, JobKind::LanguageRepair) {
        return Err(sqlx::Error::Protocol(
            "new project-scoped job requires project_id".to_string(),
        ));
    }
    let (kind, payload) = input.kind.into_parts();
    sqlx::query_as(
        "INSERT INTO jobs (
             kind, project_id, stage, payload, progress_total, max_attempts, run_after
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(kind)
    .bind(input.project_id)
    .bind(input.stage)
    .bind(payload)
    .bind(input.progress_total)
    .bind(input.max_attempts)
    .bind(input.run_after)
    .fetch_one(conn)
    .await
}

/// 按稳定 id 读取任务。route 仍必须在返回前执行 kind policy 与项目可见性检查。
pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 在事务内锁定任务，供 mutation 编排。
pub async fn find_by_id_for_update_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM jobs WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// 按项目、允许 kind 集合和筛选条件键集列出任务；权限过滤在 LIMIT 之前下推。
pub async fn list_by_project(
    pool: &PgPool,
    project_id: i64,
    after_id: Option<i64>,
    allowed_kinds: &[String],
    kind: Option<&str>,
    state: Option<&str>,
    limit: i64,
) -> Result<Vec<Job>, sqlx::Error> {
    if allowed_kinds.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as(
        "SELECT * FROM jobs
         WHERE project_id = $1
           AND ($2::BIGINT IS NULL OR id < $2)
           AND kind = ANY($3::TEXT[])
           AND ($4::TEXT IS NULL OR kind = $4)
           AND ($5::TEXT IS NULL OR state = $5)
         ORDER BY id DESC
         LIMIT $6",
    )
    .bind(project_id)
    .bind(after_id)
    .bind(allowed_kinds)
    .bind(kind)
    .bind(state)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 只领取当前进程已注册 handler 的任务，避免未知 kind 被错误消费。
///
/// 同一 SQL 先把已耗尽的到期行稳定终结，再从 `attempts < max_attempts` 中领取候选；
/// 最后一次尝试崩溃后不会再次执行外部副作用。
pub async fn claim_next_for_kinds(
    pool: &PgPool,
    worker_id: &str,
    lease_seconds: i64,
    pending_project_ids: &[i64],
    kinds: &[String],
) -> Result<Option<Job>, sqlx::Error> {
    claim_next_inner(
        pool,
        worker_id,
        lease_seconds,
        pending_project_ids,
        kinds,
        false,
        &[],
    )
    .await
}

/// 在精确 job id 范围内领取，供分片/定向恢复 worker 与隔离的合同测试使用。
pub async fn claim_next_for_ids_and_kinds(
    pool: &PgPool,
    worker_id: &str,
    lease_seconds: i64,
    pending_project_ids: &[i64],
    kinds: &[String],
    job_ids: &[i64],
) -> Result<Option<Job>, sqlx::Error> {
    if job_ids.is_empty() {
        return Ok(None);
    }
    claim_next_inner(
        pool,
        worker_id,
        lease_seconds,
        pending_project_ids,
        kinds,
        true,
        job_ids,
    )
    .await
}

async fn claim_next_inner(
    pool: &PgPool,
    worker_id: &str,
    lease_seconds: i64,
    pending_project_ids: &[i64],
    kinds: &[String],
    restrict_to_ids: bool,
    job_ids: &[i64],
) -> Result<Option<Job>, sqlx::Error> {
    if kinds.is_empty() {
        return Ok(None);
    }
    let mut tx = pool.begin().await?;
    pause_for_pending_projects_tx(&mut tx, pending_project_ids).await?;
    let job = sqlx::query_as(
        "WITH exhausted AS (
             UPDATE jobs
             SET state = 'failed', worker_id = NULL, lease_until = NULL,
                 last_error_code = 'job_attempts_exhausted',
                 last_error_message = 'job exhausted all attempts',
                 finished_at = now(), updated_at = now()
             WHERE attempts >= max_attempts
               AND kind = ANY($4::TEXT[])
               AND (NOT $5 OR id = ANY($6::BIGINT[]))
               AND (
                    (state = 'queued' AND run_after <= now())
                    OR (state = 'running' AND lease_until <= now())
               )
             RETURNING id
         ), candidate AS (
             SELECT id
             FROM jobs
             WHERE attempts < max_attempts
               AND kind = ANY($4::TEXT[])
               AND (NOT $5 OR id = ANY($6::BIGINT[]))
               AND (
                    (state = 'queued' AND run_after <= now())
                    OR (state = 'running' AND lease_until <= now())
               )
               AND (
                    kind IN ('project_purge', 'language_repair')
                    OR (
                        project_id IS NOT NULL
                        AND NOT (project_id = ANY($3::BIGINT[]))
                    )
               )
             ORDER BY run_after, id
             FOR UPDATE SKIP LOCKED
             LIMIT 1
         )
         UPDATE jobs AS job
         SET state = 'running', pause_reason = NULL, worker_id = $1,
             lease_until = now() + make_interval(secs => $2),
             attempts = job.attempts + 1,
             started_at = COALESCE(job.started_at, now()),
             finished_at = NULL, updated_at = now()
         FROM candidate
         WHERE job.id = candidate.id
         RETURNING job.*",
    )
    .bind(worker_id)
    .bind(lease_seconds.max(1) as f64)
    .bind(pending_project_ids)
    .bind(kinds)
    .bind(restrict_to_ids)
    .bind(job_ids)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(job)
}

/// 将待删除项目的非 purge 活跃任务暂停；调用方传空集合时是无操作。
pub async fn pause_for_pending_projects_tx(
    conn: &mut PgConnection,
    project_ids: &[i64],
) -> Result<u64, sqlx::Error> {
    if project_ids.is_empty() {
        return Ok(0);
    }
    sqlx::query(
        "UPDATE jobs
         SET state = 'paused', pause_reason = 'project_pending_deletion',
             worker_id = NULL, lease_until = NULL, updated_at = now()
         WHERE project_id = ANY($1::BIGINT[])
           AND kind <> 'project_purge'
           AND state IN ('queued', 'running')",
    )
    .bind(project_ids)
    .execute(conn)
    .await
    .map(|result| result.rows_affected())
}

/// 取消删除后只恢复由 pending-deletion gate 暂停的任务。
pub async fn resume_project_jobs(pool: &PgPool, project_id: i64) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "UPDATE jobs
         SET state = 'queued', pause_reason = NULL, run_after = now(), updated_at = now()
         WHERE project_id = $1 AND state = 'paused'
           AND pause_reason = 'project_pending_deletion'",
    )
    .bind(project_id)
    .execute(pool)
    .await
    .map(|result| result.rows_affected())
}

/// 仅当前且尚未过期的 lease 持有者可以续租。
pub async fn renew_lease(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    lease_seconds: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query(
        "UPDATE jobs
         SET lease_until = now() + make_interval(secs => $3), updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()",
    )
    .bind(id)
    .bind(worker_id)
    .bind(lease_seconds.max(1) as f64)
    .execute(pool)
    .await
    .map(|result| result.rows_affected() == 1)
}

/// 更新运行中且 lease 有效的任务进度。
pub async fn update_progress(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    current: i64,
    total: Option<i64>,
    stage: &str,
) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE jobs
         SET progress_current = $3,
             progress_total = COALESCE($4, progress_total),
             stage = $5,
             updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()
         RETURNING *",
    )
    .bind(id)
    .bind(worker_id)
    .bind(current)
    .bind(total)
    .bind(stage)
    .fetch_optional(pool)
    .await
}

/// 标记当前 worker 的任务成功；repository 只接受 allowlisted result 类型。
pub async fn complete(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    result: JobResult,
) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE jobs
         SET state = 'succeeded', result = $3, worker_id = NULL, lease_until = NULL,
             progress_current = COALESCE(progress_total, progress_current),
             finished_at = now(), updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()
         RETURNING *",
    )
    .bind(id)
    .bind(worker_id)
    .bind(result.into_json())
    .fetch_optional(pool)
    .await
}

/// 记录执行错误；可重试且未耗尽时回到 queued，否则进入 failed。
pub async fn fail_attempt(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    error_code: &str,
    error_message: &str,
    retryable: bool,
    retry_after_seconds: i64,
) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE jobs
         SET state = CASE
                 WHEN $5 AND attempts < max_attempts THEN 'queued'
                 ELSE 'failed'
             END,
             run_after = CASE
                 WHEN $5 AND attempts < max_attempts
                     THEN now() + make_interval(secs => $6)
                 ELSE run_after
             END,
             last_error_code = $3, last_error_message = $4,
             worker_id = NULL, lease_until = NULL,
             finished_at = CASE
                 WHEN $5 AND attempts < max_attempts THEN NULL
                 ELSE now()
             END,
             updated_at = now()
         WHERE id = $1 AND state = 'running' AND worker_id = $2
           AND lease_until > now()
         RETURNING *",
    )
    .bind(id)
    .bind(worker_id)
    .bind(error_code)
    .bind(error_message)
    .bind(retryable)
    .bind(retry_after_seconds.max(0) as f64)
    .fetch_optional(pool)
    .await
}

/// 在现有事务中手动重试失败任务；保留单调 attempts 并增加一次执行预算。
pub async fn manual_retry_tx(conn: &mut PgConnection, id: i64) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE jobs
         SET state = 'queued', pause_reason = NULL, attempts = attempts + 1,
             max_attempts = GREATEST(max_attempts, attempts + 2), run_after = now(),
             lease_until = NULL, worker_id = NULL,
             last_error_code = NULL, last_error_message = NULL,
             finished_at = NULL, updated_at = now()
         WHERE id = $1 AND state = 'failed' AND project_id IS NOT NULL
         RETURNING *",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}
