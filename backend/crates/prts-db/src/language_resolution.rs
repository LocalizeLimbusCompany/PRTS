//! 项目语言歧义诊断与 resolution 事务仓储。

use chrono::Utc;
use sqlx::{PgConnection, PgPool};

use crate::jobs::{JobKind, NewJob};
use crate::models::{Job, LanguageResolutionIssue, LanguageResolutionSummary};

/// 唯一 owner 可读取的项目 open issues。
pub async fn list_project_issues(
    pool: &PgPool,
    project_id: i64,
) -> Result<Vec<LanguageResolutionIssue>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM language_resolution_issues
         WHERE project_id = $1 AND resolved_at IS NULL ORDER BY id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// 在已锁项目事务中锁定全部 open issues。
pub async fn lock_project_issues_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<Vec<LanguageResolutionIssue>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM language_resolution_issues
         WHERE project_id = $1 AND resolved_at IS NULL ORDER BY id FOR UPDATE",
    )
    .bind(project_id)
    .fetch_all(conn)
    .await
}

/// 锁定并读取冲突 entry 的当前候选；owner 选择必须基于该事务快照。
pub async fn lock_entry_original_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT original FROM entries
         WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(entry_id)
    .bind(project_id)
    .fetch_optional(conn)
    .await
}

/// 在同一 resolution 事务中写回已验证的 canonical entry 原文对象。
pub async fn update_entry_original_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
    original: serde_json::Value,
) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "UPDATE entries SET original = $3
         WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL",
    )
    .bind(entry_id)
    .bind(project_id)
    .bind(original)
    .execute(&mut *conn)
    .await
    .map(|result| result.rows_affected())
}

/// 完成 owner 选择并原子排入下一次 canonical repair。
pub async fn complete_owner_resolution_tx(
    conn: &mut PgConnection,
    project_id: i64,
    issue_ids: &[i64],
    source_languages: &[String],
    primary_source_language: &str,
    target_language: &str,
) -> Result<Job, sqlx::Error> {
    let resolved = sqlx::query(
        "UPDATE language_resolution_issues SET resolved_at = now()
         WHERE project_id = $1 AND id = ANY($2::BIGINT[]) AND resolved_at IS NULL",
    )
    .bind(project_id)
    .bind(issue_ids)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if resolved != issue_ids.len() as u64 {
        return Err(sqlx::Error::Protocol(
            "language resolution issue set changed during transaction".to_string(),
        ));
    }

    let job = crate::jobs::create_tx(
        conn,
        NewJob {
            kind: JobKind::LanguageRepair,
            project_id: Some(project_id),
            stage: "projects".to_string(),
            progress_total: None,
            max_attempts: 5,
            run_after: Utc::now(),
        },
    )
    .await?;
    sqlx::query(
        "UPDATE projects SET source_langs = $2, primary_source_lang = $3, target_lang = $4,
             language_repair_state = 'repairing', language_repair_job_id = $5,
             lexical_state = 'pending', lexical_job_id = NULL,
             embedding_state = 'pending', embedding_job_id = NULL
         WHERE id = $1",
    )
    .bind(project_id)
    .bind(source_languages)
    .bind(primary_source_language)
    .bind(target_language)
    .bind(job.id)
    .execute(&mut *conn)
    .await?;
    Ok(job)
}

/// 平台管理员只能重试属于该项目且已经失败的 repair job。
pub async fn retry_failed_project_repair_tx(
    conn: &mut PgConnection,
    project_id: i64,
    job_id: i64,
) -> Result<Option<Job>, sqlx::Error> {
    let job = sqlx::query_as::<_, Job>(
        "SELECT * FROM jobs
         WHERE id = $1 AND project_id = $2 AND kind = 'language_repair' FOR UPDATE",
    )
    .bind(job_id)
    .bind(project_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(job) = job else {
        return Ok(None);
    };
    if job.state != "failed" {
        return Ok(None);
    }
    let retried = crate::jobs::manual_retry_tx(conn, job_id).await?;
    if retried.is_some() {
        sqlx::query(
            "UPDATE projects SET language_repair_state = 'repairing'
             WHERE id = $1 AND language_repair_job_id = $2",
        )
        .bind(project_id)
        .bind(job_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(retried)
}

/// 平台管理员只读取项目标识、状态与数量，不读取 issue 详情或 entry 正文。
pub async fn list_admin_summaries(
    pool: &PgPool,
    after_project_id: Option<i64>,
    limit: i64,
) -> Result<Vec<LanguageResolutionSummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT project.id AS project_id, project.slug AS project_slug,
                count(issue.id) AS issue_count,
                project.language_repair_state AS repair_state,
                project.language_repair_job_id AS repair_job_id
         FROM projects AS project
         JOIN language_resolution_issues AS issue
           ON issue.project_id = project.id AND issue.resolved_at IS NULL
         WHERE ($1::BIGINT IS NULL OR project.id > $1)
         GROUP BY project.id, project.slug, project.language_repair_state,
                  project.language_repair_job_id
         ORDER BY project.id LIMIT $2",
    )
    .bind(after_project_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}
