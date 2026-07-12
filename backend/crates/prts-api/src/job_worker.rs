//! 持久化任务 worker：受控领取、租约续期、崩溃接管与结果持久化。

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use tokio::sync::Notify;
use tokio::time::{Instant, MissedTickBehavior};

use crate::jobs::{JobErrorCode, JobExecutionError, JobRegistry};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const LEASE_SECONDS: i64 = 30;
const RENEW_INTERVAL: Duration = Duration::from_secs(10);

/// 待删除项目提供方。`0014` 可替换实现；foundation 默认实现不查询未来列。
pub trait PendingDeletionSource: Send + Sync {
    fn project_ids(&self) -> BoxFuture<'_, Result<Vec<i64>, sqlx::Error>>;
}

/// `0014` 前的 dormant gate：当前 schema 尚无待删除项目。
pub struct NoPendingDeletions;

impl PendingDeletionSource for NoPendingDeletions {
    fn project_ids(&self) -> BoxFuture<'_, Result<Vec<i64>, sqlx::Error>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// worker 唤醒句柄。新建/重试任务提交后可减少轮询等待，不暴露 worker 内部状态。
#[derive(Clone)]
pub struct JobWorkerControl {
    notify: Arc<Notify>,
}

impl JobWorkerControl {
    /// 通知 worker 尽快检查队列。
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    async fn wait(&self) {
        tokio::select! {
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
            _ = self.notify.notified() => {}
        }
    }
}

/// 启动持久化 worker。空 handler 注册表时只休眠，不会误领未知任务。
pub fn spawn(
    db: prts_db::Db,
    registry: JobRegistry,
    pending_deletions: Arc<dyn PendingDeletionSource>,
) -> (JobWorkerControl, tokio::task::JoinHandle<()>) {
    let control = JobWorkerControl {
        notify: Arc::new(Notify::new()),
    };
    let loop_control = control.clone();
    let worker_id = format!(
        "job-worker-{}",
        prts_auth::token::random_token(12).to_lowercase()
    );
    let handle = tokio::spawn(async move {
        loop {
            if registry.kinds().is_empty() {
                loop_control.wait().await;
                continue;
            }
            if let Err(error) =
                run_once(&db, &registry, pending_deletions.as_ref(), &worker_id).await
            {
                tracing::error!(%error, "durable job worker iteration failed");
            }
            loop_control.wait().await;
        }
    });
    (control, handle)
}

async fn run_once(
    db: &prts_db::Db,
    registry: &JobRegistry,
    pending_deletions: &dyn PendingDeletionSource,
    worker_id: &str,
) -> anyhow::Result<()> {
    let kinds = registry.kinds();
    if kinds.is_empty() {
        return Ok(());
    }
    let pending_project_ids = pending_deletions.project_ids().await?;
    let Some(job) = prts_db::jobs::claim_next_for_kinds(
        db,
        worker_id,
        LEASE_SECONDS,
        &pending_project_ids,
        &kinds,
    )
    .await?
    else {
        return Ok(());
    };

    let Some(handler) = registry.get(&job.kind) else {
        // 注册表在 claim 后发生替换时安全释放为可人工处理的失败，而不是静默成功。
        persist_failure(
            db,
            &job,
            worker_id,
            JobExecutionError {
                code: JobErrorCode::HandlerUnavailable,
                message: "registered job handler disappeared".to_string(),
                retryable: false,
            },
        )
        .await?;
        return Ok(());
    };

    let execution = handler.execute(&job);
    tokio::pin!(execution);
    let mut renew = tokio::time::interval_at(Instant::now() + RENEW_INTERVAL, RENEW_INTERVAL);
    renew.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            outcome = &mut execution => {
                match outcome {
                    Ok(result) => {
                        let updated = prts_db::jobs::complete(db, job.id, worker_id, result).await?;
                        if updated.is_none() {
                            tracing::warn!(job_id = job.id, "job completion lost its lease");
                        }
                    }
                    Err(error) => persist_failure(db, &job, worker_id, error).await?,
                }
                return Ok(());
            }
            _ = renew.tick() => {
                if !prts_db::jobs::renew_lease(db, job.id, worker_id, LEASE_SECONDS).await? {
                    tracing::warn!(job_id = job.id, "job worker lost lease during execution");
                    return Ok(());
                }
            }
        }
    }
}

async fn persist_failure(
    db: &prts_db::Db,
    job: &prts_db::models::Job,
    worker_id: &str,
    error: JobExecutionError,
) -> Result<(), sqlx::Error> {
    let retry_after = prts_core::jobs::retry_backoff_seconds(job.attempts);
    let retryable = error.retryable;
    let (error_code, error_message) = normalize_execution_error(error);
    let mut tx = db.begin().await?;
    let failed_job = prts_db::jobs::fail_attempt_tx(
        &mut tx,
        job.id,
        worker_id,
        error_code,
        &error_message,
        retryable,
        retry_after,
    )
    .await?;
    if let Some(failed_job) = failed_job.filter(|updated| updated.state == "failed") {
        match failed_job.kind.as_str() {
            "primary_source_lexical_reindex" => {
                sqlx::query(
                    "UPDATE projects SET lexical_state = 'failed'
                     WHERE id = $1 AND lexical_job_id = $2",
                )
                .bind(failed_job.project_id)
                .bind(failed_job.id)
                .execute(&mut *tx)
                .await?;
            }
            "primary_source_embedding_backfill" => {
                sqlx::query(
                    "UPDATE projects SET embedding_state = 'failed'
                     WHERE id = $1 AND embedding_job_id = $2",
                )
                .bind(failed_job.project_id)
                .bind(failed_job.id)
                .execute(&mut *tx)
                .await?;
            }
            _ => {}
        }
    }
    tx.commit().await?;
    Ok(())
}

/// 把 handler 任意错误收敛为稳定 code 与固定、短小、脱敏的内部消息。
fn normalize_execution_error(error: JobExecutionError) -> (&'static str, String) {
    let JobExecutionError {
        code,
        message,
        retryable: _,
    } = error;
    drop(message);
    (code.as_str(), code.redacted_message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_exposes_control_and_join_handle_for_supervision() {
        type PendingSource = Arc<dyn PendingDeletionSource>;
        type SpawnedWorker = (JobWorkerControl, tokio::task::JoinHandle<()>);
        type SpawnFn = fn(prts_db::Db, JobRegistry, PendingSource) -> SpawnedWorker;
        let _spawn: SpawnFn = spawn;
    }

    #[test]
    fn handler_errors_are_stable_and_redacted_before_persistence() {
        let (code, message) = normalize_execution_error(JobExecutionError {
            code: JobErrorCode::HandlerUnavailable,
            message: "stack trace Authorization: Bearer raw-access-token".to_string(),
            retryable: true,
        });
        assert_eq!(code, "job_handler_unavailable");
        assert!(message.len() <= 128);
        assert!(!message.contains("raw-access-token"));
        assert!(!message.contains("Authorization"));
    }
}
