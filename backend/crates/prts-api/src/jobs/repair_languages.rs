//! Durable legacy BCP-47 repair handler。

use std::collections::BTreeMap;

use futures_util::future::BoxFuture;
use serde_json::{Map, Value};

use super::{JobErrorCode, JobExecutionError, JobHandler, JobResult};

/// 分批规范化 legacy users/projects/entries 的语言数据。
pub struct RepairLanguagesHandler {
    db: prts_db::Db,
}

impl RepairLanguagesHandler {
    pub fn new(db: prts_db::Db) -> Self {
        Self { db }
    }

    fn database_error() -> JobExecutionError {
        JobExecutionError {
            code: JobErrorCode::DatabaseUnavailable,
            message: "language repair database operation failed".to_string(),
            retryable: true,
            details: None,
        }
    }
}

impl JobHandler for RepairLanguagesHandler {
    fn kind(&self) -> &'static str {
        "language_repair"
    }

    fn execute<'a>(
        &'a self,
        job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
        Box::pin(async move {
            match job.project_id {
                Some(project_id) => self.repair_project(job.id, project_id).await?,
                None => self.repair_users(job.id).await?,
            }
            Ok(JobResult::Completed)
        })
    }
}

impl RepairLanguagesHandler {
    async fn repair_users(&self, job_id: i64) -> Result<(), JobExecutionError> {
        let mut cursor = 0i64;
        loop {
            let users: Vec<(i64, Vec<String>)> = sqlx::query_as(
                "SELECT id, translation_langs FROM users WHERE id > $1 ORDER BY id LIMIT 500",
            )
            .bind(cursor)
            .fetch_all(&self.db)
            .await
            .map_err(|_| Self::database_error())?;
            if users.is_empty() {
                break;
            }
            let mut tx = self.db.begin().await.map_err(|_| Self::database_error())?;
            for (user_id, languages) in &users {
                match prts_core::canonicalize_language_tags(languages) {
                    Ok(canonical) => {
                        sqlx::query("UPDATE users SET translation_langs = $2 WHERE id = $1")
                            .bind(user_id)
                            .bind(canonical)
                            .execute(&mut *tx)
                            .await
                            .map_err(|_| Self::database_error())?;
                    }
                    Err(_) => {
                        sqlx::query(
                            "INSERT INTO language_resolution_issues (
                                 user_id, entity_type, entity_id_snapshot, issue_kind, metadata
                             ) VALUES ($1, 'user', $1::TEXT, 'invalid_tag',
                                 jsonb_build_object('language_count', $2))
                             ON CONFLICT DO NOTHING",
                        )
                        .bind(user_id)
                        .bind(languages.len() as i64)
                        .execute(&mut *tx)
                        .await
                        .map_err(|_| Self::database_error())?;
                        sqlx::query("UPDATE users SET translation_langs = '{}' WHERE id = $1")
                            .bind(user_id)
                            .execute(&mut *tx)
                            .await
                            .map_err(|_| Self::database_error())?;
                    }
                }
                cursor = *user_id;
            }
            sqlx::query("UPDATE jobs SET stage = 'users', progress_current = $2 WHERE id = $1")
                .bind(job_id)
                .bind(cursor)
                .execute(&mut *tx)
                .await
                .map_err(|_| Self::database_error())?;
            tx.commit().await.map_err(|_| Self::database_error())?;
        }
        Ok(())
    }

    async fn repair_project(&self, job_id: i64, project_id: i64) -> Result<(), JobExecutionError> {
        let project = prts_db::projects::find_by_id(&self.db, project_id)
            .await
            .map_err(|_| Self::database_error())?
            .ok_or(JobExecutionError {
                code: JobErrorCode::InvalidPayload,
                message: "language repair project disappeared".to_string(),
                retryable: false,
                details: None,
            })?;
        if project.language_repair_state == "ready"
            && project.language_repair_job_id == Some(job_id)
            && project.lexical_job_id.is_some()
        {
            return Ok(());
        }
        let (source_languages, primary_source_language, target_language) =
            match prts_core::language::canonicalize_project_languages(
                &project.source_langs,
                project
                    .primary_source_lang
                    .as_deref()
                    .or_else(|| project.source_langs.first().map(String::as_str)),
                &project.target_lang,
            ) {
                Ok(languages) => languages,
                Err(error) => {
                    self.mark_project_unresolved(project_id, error.code())
                        .await?;
                    return Err(JobExecutionError {
                        code: JobErrorCode::LanguageResolutionRequired,
                        message: "project language configuration requires owner resolution"
                            .to_string(),
                        retryable: false,
                        details: None,
                    });
                }
            };

        let mut cursor = 0i64;
        let mut unresolved = false;
        loop {
            let rows: Vec<(i64, Value)> = sqlx::query_as(
                "SELECT id, original FROM entries
                 WHERE project_id = $1 AND id > $2 ORDER BY id LIMIT 500",
            )
            .bind(project_id)
            .bind(cursor)
            .fetch_all(&self.db)
            .await
            .map_err(|_| Self::database_error())?;
            if rows.is_empty() {
                break;
            }
            let mut tx = self.db.begin().await.map_err(|_| Self::database_error())?;
            for (entry_id, original) in &rows {
                match canonicalize_original(original, &primary_source_language) {
                    Ok(canonical) => {
                        sqlx::query("UPDATE entries SET original = $2 WHERE id = $1")
                            .bind(entry_id)
                            .bind(canonical)
                            .execute(&mut *tx)
                            .await
                            .map_err(|_| Self::database_error())?;
                    }
                    Err(issue_kind) => {
                        unresolved = true;
                        sqlx::query(
                            "INSERT INTO language_resolution_issues (
                                 project_id, entry_id, entity_type, entity_id_snapshot,
                                 issue_kind, metadata
                             ) VALUES ($1, $2, 'entry', $2::TEXT, $3,
                                 jsonb_build_object('key_count', $4))
                             ON CONFLICT DO NOTHING",
                        )
                        .bind(project_id)
                        .bind(entry_id)
                        .bind(issue_kind)
                        .bind(original.as_object().map_or(0, Map::len) as i64)
                        .execute(&mut *tx)
                        .await
                        .map_err(|_| Self::database_error())?;
                    }
                }
                cursor = *entry_id;
            }
            sqlx::query("UPDATE jobs SET stage = 'entries', progress_current = $2 WHERE id = $1")
                .bind(job_id)
                .bind(cursor)
                .execute(&mut *tx)
                .await
                .map_err(|_| Self::database_error())?;
            tx.commit().await.map_err(|_| Self::database_error())?;
        }

        if unresolved {
            self.mark_project_unresolved(project_id, "entry_language_issue")
                .await?;
            return Err(JobExecutionError {
                code: JobErrorCode::LanguageResolutionRequired,
                message: "entry language keys require owner resolution".to_string(),
                retryable: false,
                details: None,
            });
        }

        let mut tx = self.db.begin().await.map_err(|_| Self::database_error())?;
        let lexical_job = prts_db::jobs::create_tx(
            &mut tx,
            prts_db::jobs::NewJob {
                kind: prts_db::jobs::JobKind::PrimarySourceLexicalReindex,
                project_id: Some(project_id),
                stage: "lexical".to_string(),
                progress_total: None,
                max_attempts: 5,
                run_after: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|_| Self::database_error())?;
        sqlx::query(
            "UPDATE projects SET source_langs = $2, primary_source_lang = $3,
                 target_lang = $4, language_repair_state = 'ready',
                 lexical_state = 'rebuilding', lexical_job_id = $5,
                 embedding_state = 'pending', embedding_job_id = NULL
             WHERE id = $1",
        )
        .bind(project_id)
        .bind(source_languages)
        .bind(&primary_source_language)
        .bind(target_language)
        .bind(lexical_job.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| Self::database_error())?;
        sqlx::query(
            "UPDATE workspace_foundation_state SET
                 ready_project_count = (SELECT count(*) FROM projects WHERE language_repair_state = 'ready'),
                 unresolved_project_count = (SELECT count(*) FROM projects WHERE language_repair_state = 'needs_language_resolution'),
                 updated_at = now() WHERE singleton",
        )
        .execute(&mut *tx)
        .await
        .map_err(|_| Self::database_error())?;
        tx.commit().await.map_err(|_| Self::database_error())?;
        Ok(())
    }

    async fn mark_project_unresolved(
        &self,
        project_id: i64,
        reason: &str,
    ) -> Result<(), JobExecutionError> {
        let mut tx = self.db.begin().await.map_err(|_| Self::database_error())?;
        sqlx::query(
            "UPDATE projects SET language_repair_state = 'needs_language_resolution',
                 lexical_state = 'pending' WHERE id = $1",
        )
        .bind(project_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| Self::database_error())?;
        sqlx::query(
            "INSERT INTO language_resolution_issues (
                 project_id, entity_type, entity_id_snapshot, issue_kind, metadata
             ) VALUES ($1, 'project', $1::TEXT, 'invalid_tag', jsonb_build_object('reason', $2))
             ON CONFLICT DO NOTHING",
        )
        .bind(project_id)
        .bind(reason)
        .execute(&mut *tx)
        .await
        .map_err(|_| Self::database_error())?;
        tx.commit().await.map_err(|_| Self::database_error())?;
        Ok(())
    }
}

fn canonicalize_original(
    original: &Value,
    primary_source_language: &str,
) -> Result<Value, &'static str> {
    let object = original.as_object().ok_or("invalid_tag")?;
    let mut canonical = BTreeMap::<String, Value>::new();
    for (raw_tag, value) in object {
        let tag = prts_core::canonicalize_language_tag(raw_tag).map_err(|_| "invalid_tag")?;
        if let Some(existing) = canonical.get(&tag) {
            if existing != value {
                return Err("conflicting_original_keys");
            }
        } else {
            canonical.insert(tag, value.clone());
        }
    }
    if !canonical.contains_key(primary_source_language) {
        return Err("primary_not_in_sources");
    }
    Ok(Value::Object(canonical.into_iter().collect()))
}
