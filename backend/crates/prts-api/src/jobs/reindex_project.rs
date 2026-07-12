//! Primary-source lexical reindex and embedding-stage handlers。

use std::sync::Arc;

use futures_util::future::BoxFuture;
use prts_search::qwen::QwenProvider;
use tokio::sync::RwLock;

use super::{JobErrorCode, JobExecutionError, JobHandler, JobResult};

/// 按 canonical 主源 exact key 分批重建词法派生列。
pub struct ReindexProjectHandler {
    db: prts_db::Db,
}

impl ReindexProjectHandler {
    pub fn new(db: prts_db::Db) -> Self {
        Self { db }
    }
}

impl JobHandler for ReindexProjectHandler {
    fn kind(&self) -> &'static str {
        "primary_source_lexical_reindex"
    }

    fn execute<'a>(
        &'a self,
        job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
        Box::pin(async move {
            let project_id = job.project_id.ok_or_else(invalid_payload)?;
            let mut cursor = job
                .payload
                .get("cursor")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let mut processed = job.progress_current;
            loop {
                let mut tx = self.db.begin().await.map_err(|_| database_error())?;
                let ids: Vec<i64> = sqlx::query_scalar(
                    "SELECT id FROM entries WHERE project_id = $1 AND id > $2
                     ORDER BY id LIMIT 500",
                )
                .bind(project_id)
                .bind(cursor)
                .fetch_all(&mut *tx)
                .await
                .map_err(|_| database_error())?;
                if ids.is_empty() {
                    tx.rollback().await.map_err(|_| database_error())?;
                    break;
                }
                sqlx::query(
                    "UPDATE entries SET original = original
                     WHERE project_id = $1 AND id = ANY($2::BIGINT[])",
                )
                .bind(project_id)
                .bind(&ids)
                .execute(&mut *tx)
                .await
                .map_err(|_| database_error())?;
                cursor = *ids.last().ok_or_else(invalid_payload)?;
                processed += ids.len() as i64;
                sqlx::query(
                    "UPDATE jobs
                     SET stage = 'lexical', progress_current = $2,
                         payload = jsonb_set(payload, '{cursor}', to_jsonb($3::BIGINT), TRUE),
                         updated_at = now()
                     WHERE id = $1",
                )
                .bind(job.id)
                .bind(processed)
                .bind(cursor)
                .execute(&mut *tx)
                .await
                .map_err(|_| database_error())?;
                tx.commit().await.map_err(|_| database_error())?;
            }

            let mut tx = self.db.begin().await.map_err(|_| database_error())?;
            let embedding_job_id: i64 = match sqlx::query_scalar(
                "SELECT id FROM jobs WHERE project_id = $1
                   AND kind = 'primary_source_embedding_backfill'
                   AND state IN ('queued', 'running', 'paused')
                 ORDER BY id DESC LIMIT 1 FOR UPDATE",
            )
            .bind(project_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| database_error())?
            {
                Some(existing_id) => existing_id,
                None => sqlx::query_scalar(
                    "INSERT INTO jobs (
                         kind, project_id, stage, payload, progress_total, max_attempts
                     ) VALUES (
                         'primary_source_embedding_backfill', $1, 'embedding', '{}', $2, 5
                     )
                     RETURNING id",
                )
                .bind(project_id)
                .bind(job.progress_total)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| database_error())?,
            };
            sqlx::query(
                "UPDATE projects SET lexical_state = 'ready', embedding_state = 'pending',
                     embedding_job_id = $2 WHERE id = $1",
            )
            .bind(project_id)
            .bind(embedding_job_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| database_error())?;
            tx.commit().await.map_err(|_| database_error())?;
            Ok(JobResult::Completed)
        })
    }
}

/// 现有 embedding sweep 负责实际向量化；本 handler 标记 release 可领取该阶段。
pub struct EmbeddingBackfillHandler {
    db: prts_db::Db,
    provider: Arc<Option<QwenProvider>>,
    search_config: Arc<RwLock<prts_db::search_settings::SearchConfig>>,
}

impl EmbeddingBackfillHandler {
    pub fn new(
        db: prts_db::Db,
        provider: Arc<Option<QwenProvider>>,
        search_config: Arc<RwLock<prts_db::search_settings::SearchConfig>>,
    ) -> Self {
        Self {
            db,
            provider,
            search_config,
        }
    }
}

impl JobHandler for EmbeddingBackfillHandler {
    fn kind(&self) -> &'static str {
        "primary_source_embedding_backfill"
    }

    fn execute<'a>(
        &'a self,
        job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
        Box::pin(async move {
            let project_id = job.project_id.ok_or_else(invalid_payload)?;
            let config = self.search_config.read().await.clone();
            let Some(provider) = (config.embedding_enabled)
                .then(|| self.provider.as_ref().as_ref())
                .flatten()
            else {
                let mut tx = self.db.begin().await.map_err(|_| database_error())?;
                sqlx::query("UPDATE projects SET embedding_state = 'degraded' WHERE id = $1")
                    .bind(project_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| database_error())?;
                sqlx::query("UPDATE jobs SET stage = 'embedding_skipped' WHERE id = $1")
                    .bind(job.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| database_error())?;
                tx.commit().await.map_err(|_| database_error())?;
                return Ok(JobResult::EmbeddingSkipped);
            };

            let mut tx = self.db.begin().await.map_err(|_| database_error())?;
            sqlx::query("UPDATE projects SET embedding_state = 'running' WHERE id = $1")
                .bind(project_id)
                .execute(&mut *tx)
                .await
                .map_err(|_| database_error())?;
            sqlx::query("UPDATE jobs SET stage = 'embedding' WHERE id = $1")
                .bind(job.id)
                .execute(&mut *tx)
                .await
                .map_err(|_| database_error())?;
            tx.commit().await.map_err(|_| database_error())?;

            let mut cursor = job
                .payload
                .get("cursor")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let mut processed = job.progress_current;
            loop {
                let rows: Vec<(i64, String)> = sqlx::query_as(
                    "SELECT id, source_text FROM entries
                     WHERE project_id = $1 AND id > $2 AND source_text <> ''
                     ORDER BY id LIMIT 50",
                )
                .bind(project_id)
                .bind(cursor)
                .fetch_all(&self.db)
                .await
                .map_err(|_| database_error())?;
                if rows.is_empty() {
                    break;
                }
                for chunk in rows.chunks(config.embedding_batch.clamp(1, 10) as usize) {
                    let texts: Vec<String> = chunk
                        .iter()
                        .map(|(_, source_text)| source_text.clone())
                        .collect();
                    let vectors = provider
                        .embed_batch(&config.embedding_base_url, &config.embedding_model, &texts)
                        .await
                        .map_err(|_| JobExecutionError {
                            code: JobErrorCode::DatabaseUnavailable,
                            message: "embedding provider request failed".to_string(),
                            retryable: true,
                        })?;
                    if vectors.len() != chunk.len() {
                        return Err(JobExecutionError {
                            code: JobErrorCode::InvalidPayload,
                            message: "embedding provider returned mismatched batch".to_string(),
                            retryable: true,
                        });
                    }
                    let mut tx = self.db.begin().await.map_err(|_| database_error())?;
                    for ((entry_id, captured_source), vector) in chunk.iter().zip(vectors) {
                        sqlx::query(
                            "UPDATE entries SET embedding = $1, embed_attempts = 0
                             WHERE id = $2 AND project_id = $3 AND source_text = $4",
                        )
                        .bind(pgvector::Vector::from(vector))
                        .bind(entry_id)
                        .bind(project_id)
                        .bind(captured_source)
                        .execute(&mut *tx)
                        .await
                        .map_err(|_| database_error())?;
                    }
                    cursor = chunk
                        .last()
                        .map(|(entry_id, _)| *entry_id)
                        .unwrap_or(cursor);
                    processed += chunk.len() as i64;
                    sqlx::query(
                        "UPDATE jobs
                         SET progress_current = $2, stage = 'embedding',
                             payload = jsonb_set(payload, '{cursor}', to_jsonb($3::BIGINT), TRUE),
                             updated_at = now()
                         WHERE id = $1",
                    )
                    .bind(job.id)
                    .bind(processed)
                    .bind(cursor)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| database_error())?;
                    tx.commit().await.map_err(|_| database_error())?;
                }
            }
            sqlx::query("UPDATE projects SET embedding_state = 'ready' WHERE id = $1")
                .bind(project_id)
                .execute(&self.db)
                .await
                .map_err(|_| database_error())?;
            Ok(JobResult::Completed)
        })
    }
}

fn invalid_payload() -> JobExecutionError {
    JobExecutionError {
        code: JobErrorCode::InvalidPayload,
        message: "project-scoped job lacks project id".to_string(),
        retryable: false,
    }
}

fn database_error() -> JobExecutionError {
    JobExecutionError {
        code: JobErrorCode::DatabaseUnavailable,
        message: "project reindex database operation failed".to_string(),
        retryable: true,
    }
}
