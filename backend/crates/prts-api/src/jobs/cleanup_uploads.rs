//! 上传临时文件的 durable 到期清理处理器。

use std::path::{Component, Path, PathBuf};

use futures_util::future::BoxFuture;

use super::{JobErrorCode, JobExecutionError, JobHandler, JobResult};

pub struct CleanupUploadsHandler {
    db: prts_db::Db,
    temp_root: PathBuf,
}

impl CleanupUploadsHandler {
    pub fn new(db: prts_db::Db, temp_root: impl Into<PathBuf>) -> Self {
        Self {
            db,
            temp_root: temp_root.into(),
        }
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, JobExecutionError> {
        let relative = Path::new(key);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(JobExecutionError {
                code: JobErrorCode::InvalidPayload,
                message: "upload cleanup temp key is invalid".to_string(),
                retryable: false,
            });
        }
        Ok(self.temp_root.join(relative))
    }
}

impl JobHandler for CleanupUploadsHandler {
    fn kind(&self) -> &'static str {
        "upload_cleanup"
    }

    fn execute<'a>(
        &'a self,
        _job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
        Box::pin(async move {
            prts_db::uploads::expire_due(&self.db, 500)
                .await
                .map_err(database_error)?;
            let candidates = prts_db::uploads::list_cleanup_candidates(&self.db, 1000)
                .await
                .map_err(database_error)?;
            for (attempt_id, key) in candidates {
                let path = self.path_for(&key)?;
                for candidate in [path.clone(), path.with_extension("part")] {
                    match tokio::fs::remove_file(candidate).await {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(JobExecutionError {
                                code: JobErrorCode::DatabaseUnavailable,
                                message: format!("upload temp cleanup failed: {error}"),
                                retryable: true,
                            });
                        }
                    }
                }
                prts_db::uploads::mark_attempt_cleaned(&self.db, attempt_id)
                    .await
                    .map_err(database_error)?;
            }
            Ok(JobResult::Completed)
        })
    }
}

fn database_error(error: sqlx::Error) -> JobExecutionError {
    JobExecutionError {
        code: JobErrorCode::DatabaseUnavailable,
        message: format!("upload cleanup database operation failed: {error}"),
        retryable: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_temp_key_traversal() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let handler = CleanupUploadsHandler::new(pool, "uploads");
        assert!(handler.path_for("../secret").is_err());
        assert!(handler.path_for("projects/1/uploads/file.json").is_ok());
    }
}
