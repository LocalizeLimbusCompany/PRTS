//! 到期文件/文件夹软删除的 durable 清除 worker。

use std::time::Duration;

use chrono::Utc;
use futures_util::future::BoxFuture;

use prts_db::audit::{
    AuditActor, AuditActorKind, AuditEvent, FileHistoryAuditOperation, FileHistoryAuditTarget,
};

use super::{JobErrorCode, JobExecutionError, JobHandler, JobResult};

const PAGE_SIZE: i64 = 25;
const IDLE_RESCAN: Duration = Duration::from_secs(60 * 60);

/// 全局保留期扫描 handler。单个 operation 在独立事务内完成 purge + audit。
pub struct PurgeDeletedFilesHandler {
    db: prts_db::Db,
}

impl PurgeDeletedFilesHandler {
    pub fn new(db: prts_db::Db) -> Self {
        Self { db }
    }
}

impl JobHandler for PurgeDeletedFilesHandler {
    fn kind(&self) -> &'static str {
        "file_retention_cleanup"
    }

    fn execute<'a>(
        &'a self,
        job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
        Box::pin(async move {
            let mut after = None;
            let mut purged_any = false;
            loop {
                let operations =
                    prts_db::file_history::list_due_deletions(&self.db, after, PAGE_SIZE)
                        .await
                        .map_err(database_error)?;
                if operations.is_empty() {
                    break;
                }
                for operation in &operations {
                    after = Some((operation.purge_after, operation.change_set_id));
                    let mut tx = self.db.begin().await.map_err(database_error)?;
                    // 仓储事务先置空 task file/entry live refs 并重算 task_stats，之后才删业务树。
                    let purged = prts_db::file_history::purge_due_operation_tx(
                        &mut tx,
                        operation,
                        Utc::now(),
                    )
                    .await
                    .map_err(database_error)?;
                    if let Some(purged) = purged {
                        let (target, target_id) = match purged.target {
                            prts_core::file_history::FileHistoryTarget::File(id) => {
                                (FileHistoryAuditTarget::File, id)
                            }
                            prts_core::file_history::FileHistoryTarget::Folder(id) => {
                                (FileHistoryAuditTarget::Folder, id)
                            }
                        };
                        prts_db::audit::append_event_tx(
                            &mut tx,
                            AuditActor {
                                id: None,
                                kind: AuditActorKind::System,
                                ip: None,
                            },
                            AuditEvent::FileHistoryChanged {
                                project_id: purged.project_id,
                                target,
                                target_id,
                                operation: FileHistoryAuditOperation::Purge,
                                change_set_id: Some(purged.deletion_change_set_id),
                                source_change_set_id: None,
                                path: &purged.path,
                                affected_folders: purged.folder_count,
                                affected_files: purged.file_count,
                                affected_entries: purged.entry_count,
                                purge_after: None,
                            },
                        )
                        .await
                        .map_err(database_error)?;
                        purged_any = true;
                    }
                    tx.commit().await.map_err(database_error)?;
                }
                if operations.len() < PAGE_SIZE as usize {
                    break;
                }
            }

            let next_run = if purged_any {
                Utc::now()
            } else {
                Utc::now()
                    + chrono::Duration::from_std(IDLE_RESCAN)
                        .expect("one-hour cleanup interval is representable")
            };
            let mut tx = self.db.begin().await.map_err(database_error)?;
            prts_db::jobs::schedule_next_file_retention_cleanup_tx(&mut tx, job.id, next_run)
                .await
                .map_err(database_error)?;
            tx.commit().await.map_err(database_error)?;
            Ok(JobResult::Completed)
        })
    }
}

fn database_error(error: sqlx::Error) -> JobExecutionError {
    tracing::error!(error_class = %database_error_class(&error), "file retention cleanup failed");
    JobExecutionError {
        code: JobErrorCode::FilePurgeFailed,
        message: "file retention cleanup database operation failed".to_string(),
        retryable: true,
        details: None,
    }
}

fn database_error_class(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "pool_unavailable",
        sqlx::Error::Io(_) | sqlx::Error::Tls(_) => "transport",
        sqlx::Error::Database(_) => "database",
        _ => "internal",
    }
}
