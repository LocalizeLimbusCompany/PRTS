//! 24 小时到期后的 DB-first 项目清除与幂等外部清理。

use std::sync::Arc;

use futures_util::future::BoxFuture;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};

use super::{JobErrorCode, JobExecutionError, JobHandler, JobResult};

pub struct PurgeProjectHandler {
    db: prts_db::Db,
    media: Arc<dyn crate::media::MediaStore>,
    temp_root: std::path::PathBuf,
}

impl PurgeProjectHandler {
    pub fn new(
        db: prts_db::Db,
        media: Arc<dyn crate::media::MediaStore>,
        temp_root: std::path::PathBuf,
    ) -> Self {
        Self {
            db,
            media,
            temp_root,
        }
    }

    async fn purge_database(
        &self,
        job: &prts_db::models::Job,
        snapshot: &prts_db::jobs::ProjectPurgeSnapshot,
    ) -> Result<(), sqlx::Error> {
        let worker_id = job.worker_id.as_deref().ok_or(sqlx::Error::RowNotFound)?;
        let mut tx = self.db.begin().await?;
        let Some(project) =
            prts_db::projects::find_by_id_for_update_tx(&mut tx, snapshot.project_id).await?
        else {
            tx.rollback().await?;
            return Ok(());
        };
        prts_db::jobs::find_by_id_for_update_tx(&mut tx, job.id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        if project.deletion_job_id != Some(job.id)
            || project
                .deletion_scheduled_at
                .is_none_or(|deadline| deadline > chrono::Utc::now())
        {
            return Err(sqlx::Error::Protocol(
                "purge project binding or deadline invalid".into(),
            ));
        }
        prts_db::audit::append_event_tx(
            &mut tx,
            AuditActor {
                id: None,
                kind: AuditActorKind::System,
                ip: None,
            },
            AuditEvent::ProjectPurged {
                project_id: snapshot.project_id,
                slug: &snapshot.slug,
                deletion_job_id: job.id,
            },
        )
        .await?;
        prts_db::jobs::detach_project_jobs_tx(&mut tx, snapshot.project_id, job.id).await?;
        prts_db::projects::detach_live_refs_tx(&mut tx, snapshot.project_id).await?;
        prts_db::projects::delete_entry_versions_tx(&mut tx, snapshot.project_id).await?;
        let anchor =
            prts_db::projects::delete_entries_files_folders_tx(&mut tx, snapshot.project_id)
                .await?;
        prts_db::projects::delete_file_history_tx(&mut tx, snapshot.project_id, anchor).await?;
        prts_db::projects::delete_tasks_tx(&mut tx, snapshot.project_id).await?;
        prts_db::projects::delete_terms_tx(&mut tx, snapshot.project_id).await?;
        prts_db::projects::delete_project_metadata_tx(&mut tx, snapshot.project_id).await?;
        prts_db::projects::delete_project_row_tx(&mut tx, snapshot.project_id).await?;
        prts_db::jobs::mark_external_cleanup_pending_tx(&mut tx, job.id, worker_id).await?;
        tx.commit().await
    }

    async fn cleanup_external(
        &self,
        snapshot: &prts_db::jobs::ProjectPurgeSnapshot,
    ) -> std::io::Result<()> {
        for key in &snapshot.media_keys {
            self.media.delete(key).await?;
        }
        for key in &snapshot.temp_keys {
            let path = std::path::Path::new(key);
            if path.is_absolute()
                || path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid temp key",
                ));
            }
            match tokio::fs::remove_file(self.temp_root.join(path)).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

impl JobHandler for PurgeProjectHandler {
    fn kind(&self) -> &'static str {
        "project_purge"
    }

    fn execute<'a>(
        &'a self,
        job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
        Box::pin(async move {
            let snapshot: prts_db::jobs::ProjectPurgeSnapshot =
                serde_json::from_value(job.payload.clone()).map_err(|_| JobExecutionError {
                    code: JobErrorCode::InvalidPayload,
                    message: "invalid purge snapshot".into(),
                    retryable: false,
                    details: None,
                })?;
            if job.stage != "external_cleanup_pending" {
                self.purge_database(job, &snapshot)
                    .await
                    .map_err(database_error)?;
            }
            self.cleanup_external(&snapshot)
                .await
                .map_err(|_| JobExecutionError {
                    code: JobErrorCode::ProjectPurgeFailed,
                    message: "external project cleanup failed".into(),
                    retryable: true,
                    details: None,
                })?;
            Ok(JobResult::Completed)
        })
    }
}

fn database_error(_: sqlx::Error) -> JobExecutionError {
    JobExecutionError {
        code: JobErrorCode::DatabaseUnavailable,
        message: "project purge database operation failed".into(),
        retryable: true,
        details: None,
    }
}
