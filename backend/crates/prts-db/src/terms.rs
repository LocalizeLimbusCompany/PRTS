//! source-aware 项目术语仓储与主源切换集合 executor。

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

use crate::models::{Term, TermWithPos};

const TERM_WITH_POS_SELECT: &str =
    "SELECT term.*, pos.name_zh_cn AS pos_name_zh_cn, pos.name_en AS pos_name_en
     FROM terms AS term
     LEFT JOIN pos_presets AS pos ON pos.id = term.pos_id";

/// 术语列表过滤。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermListScope {
    Current,
    Archived,
    Mixed,
}

/// 按 id DESC 键集列出术语。
pub async fn list(
    pool: &PgPool,
    project_id: i64,
    primary_source_lang: &str,
    scope: TermListScope,
    after: Option<i64>,
    limit: i64,
) -> Result<Vec<TermWithPos>, sqlx::Error> {
    let scope = match scope {
        TermListScope::Current => "current",
        TermListScope::Archived => "archived",
        TermListScope::Mixed => "mixed",
    };
    sqlx::query_as::<_, TermWithPos>(&format!(
        "{TERM_WITH_POS_SELECT}
         WHERE term.project_id = $1
           AND ($3::BIGINT IS NULL OR term.id < $3)
           AND CASE $4::TEXT
                 WHEN 'current' THEN term.archived_at IS NULL AND term.source_lang = $2
                 WHEN 'archived' THEN term.archived_at IS NOT NULL
                 ELSE TRUE
               END
         ORDER BY term.id DESC LIMIT $5"
    ))
    .bind(project_id)
    .bind(primary_source_lang)
    .bind(after)
    .bind(scope)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 读取 URL project 绑定的术语。
pub async fn find(
    pool: &PgPool,
    project_id: i64,
    term_id: i64,
) -> Result<Option<TermWithPos>, sqlx::Error> {
    sqlx::query_as::<_, TermWithPos>(&format!(
        "{TERM_WITH_POS_SELECT} WHERE term.project_id = $1 AND term.id = $2"
    ))
    .bind(project_id)
    .bind(term_id)
    .fetch_optional(pool)
    .await
}

/// 锁定 URL project 绑定的术语。
pub async fn find_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    term_id: i64,
) -> Result<Option<Term>, sqlx::Error> {
    sqlx::query_as::<_, Term>("SELECT * FROM terms WHERE project_id = $1 AND id = $2 FOR UPDATE")
        .bind(project_id)
        .bind(term_id)
        .fetch_optional(conn)
        .await
}

/// 在调用方事务内创建术语。
#[allow(clippy::too_many_arguments)]
pub async fn create_tx(
    conn: &mut PgConnection,
    project_id: i64,
    source_lang: &str,
    source_text: &str,
    translation: &str,
    notes: &str,
    pos_id: Option<i64>,
    archived_at: Option<DateTime<Utc>>,
    actor_id: i64,
) -> Result<Term, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "INSERT INTO terms (
             project_id, source_lang, source_text, translation, notes, pos_id,
             archived_at, created_by, updated_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
         RETURNING *",
    )
    .bind(project_id)
    .bind(source_lang)
    .bind(source_text)
    .bind(translation)
    .bind(notes)
    .bind(pos_id)
    .bind(archived_at)
    .bind(actor_id)
    .fetch_one(conn)
    .await
}

/// 在调用方事务内更新 URL project 绑定的术语。
#[allow(clippy::too_many_arguments)]
pub async fn update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    term_id: i64,
    source_lang: &str,
    source_text: &str,
    translation: &str,
    notes: &str,
    pos_id: Option<i64>,
    archived_at: Option<DateTime<Utc>>,
    actor_id: i64,
) -> Result<Option<Term>, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "UPDATE terms
         SET source_lang = $3, source_text = $4, translation = $5, notes = $6,
             pos_id = $7, archived_at = $8, updated_by = $9
         WHERE project_id = $1 AND id = $2 RETURNING *",
    )
    .bind(project_id)
    .bind(term_id)
    .bind(source_lang)
    .bind(source_text)
    .bind(translation)
    .bind(notes)
    .bind(pos_id)
    .bind(archived_at)
    .bind(actor_id)
    .fetch_optional(conn)
    .await
}

/// 删除 URL project 绑定的术语。
pub async fn delete_tx(
    conn: &mut PgConnection,
    project_id: i64,
    term_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM terms WHERE project_id = $1 AND id = $2")
        .bind(project_id)
        .bind(term_id)
        .execute(conn)
        .await
        .map(|result| result.rows_affected() == 1)
}

/// 返回当前 primary active terms 中命中给定源文本的候选；不返回其它语言或归档项。
pub async fn match_current(
    pool: &PgPool,
    project_id: i64,
    primary_source_lang: &str,
    source_text: &str,
    limit: i64,
) -> Result<Vec<TermWithPos>, sqlx::Error> {
    sqlx::query_as::<_, TermWithPos>(&format!(
        "{TERM_WITH_POS_SELECT}
         WHERE term.project_id = $1 AND term.source_lang = $2
           AND term.archived_at IS NULL AND position(term.source_text IN $3) > 0
         ORDER BY length(term.source_text) DESC, term.id ASC LIMIT $4"
    ))
    .bind(project_id)
    .bind(primary_source_lang)
    .bind(source_text)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 混合导出始终返回项目全部 current/archived 术语及双语 POS 名称。
pub async fn list_for_export(
    pool: &PgPool,
    project_id: i64,
) -> Result<Vec<TermWithPos>, sqlx::Error> {
    sqlx::query_as::<_, TermWithPos>(&format!(
        "{TERM_WITH_POS_SELECT} WHERE term.project_id = $1 ORDER BY term.id ASC"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// 在导出审计事务内读取 mixed 术语快照。
pub async fn list_for_export_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<Vec<TermWithPos>, sqlx::Error> {
    sqlx::query_as::<_, TermWithPos>(&format!(
        "{TERM_WITH_POS_SELECT} WHERE term.project_id = $1 ORDER BY term.id ASC"
    ))
    .bind(project_id)
    .fetch_all(conn)
    .await
}

/// 批量解析 canonical NULL-safe identity 当前对应的 term id。
pub async fn existing_import_ids(
    pool: &PgPool,
    project_id: i64,
    source_langs: &[String],
    source_texts: &[String],
    pos_ids: &[Option<i64>],
) -> Result<Vec<Option<i64>>, sqlx::Error> {
    existing_import_ids_executor(pool, project_id, source_langs, source_texts, pos_ids).await
}

/// 在 confirm 事务内重新解析 canonical NULL-safe identity。
pub async fn existing_import_ids_tx(
    conn: &mut PgConnection,
    project_id: i64,
    source_langs: &[String],
    source_texts: &[String],
    pos_ids: &[Option<i64>],
) -> Result<Vec<Option<i64>>, sqlx::Error> {
    existing_import_ids_executor(conn, project_id, source_langs, source_texts, pos_ids).await
}

async fn existing_import_ids_executor<'e, E>(
    executor: E,
    project_id: i64,
    source_langs: &[String],
    source_texts: &[String],
    pos_ids: &[Option<i64>],
) -> Result<Vec<Option<i64>>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "WITH input AS (
             SELECT source_lang, source_text, pos_id, ordinality
             FROM unnest($2::TEXT[], $3::TEXT[], $4::BIGINT[])
                  WITH ORDINALITY AS value(source_lang, source_text, pos_id, ordinality)
         )
         SELECT input.ordinality::BIGINT, term.id
         FROM input
         LEFT JOIN terms AS term
           ON term.project_id = $1
          AND term.source_lang = input.source_lang
          AND term.source_text = input.source_text
          AND term.pos_id IS NOT DISTINCT FROM input.pos_id
         ORDER BY input.ordinality",
    )
    .bind(project_id)
    .bind(source_langs)
    .bind(source_texts)
    .bind(pos_ids)
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().map(|(_, id)| id).collect())
}

/// Confirm 事务内按冻结的 NULL-safe 唯一约束 upsert 单行术语。
#[allow(clippy::too_many_arguments)]
pub async fn upsert_import_tx(
    conn: &mut PgConnection,
    project_id: i64,
    source_lang: &str,
    source_text: &str,
    translation: &str,
    notes: &str,
    pos_id: Option<i64>,
    archived_at: Option<DateTime<Utc>>,
    actor_id: i64,
) -> Result<Term, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "INSERT INTO terms (
             project_id, source_lang, source_text, translation, notes, pos_id,
             archived_at, created_by, updated_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
         ON CONFLICT ON CONSTRAINT terms_identity_unique DO UPDATE
         SET translation = EXCLUDED.translation,
             notes = EXCLUDED.notes,
             archived_at = EXCLUDED.archived_at,
             updated_by = EXCLUDED.updated_by
         RETURNING *",
    )
    .bind(project_id)
    .bind(source_lang)
    .bind(source_text)
    .bind(translation)
    .bind(notes)
    .bind(pos_id)
    .bind(archived_at)
    .bind(actor_id)
    .fetch_one(conn)
    .await
}

/// 主源切换时归档全部非新主源 active term，并激活新主源全部 archived term。
pub async fn apply_primary_source_plan_tx(
    conn: &mut PgConnection,
    project_id: i64,
    plan: &prts_core::terms::PrimarySourceTermsPlan,
) -> Result<(u64, u64), sqlx::Error> {
    let archived = sqlx::query(
        "UPDATE terms SET archived_at = now()
         WHERE project_id = $1
           AND source_lang <> $2 AND archived_at IS NULL",
    )
    .bind(project_id)
    .bind(&plan.primary_source_lang)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    let activated = sqlx::query(
        "UPDATE terms SET archived_at = NULL
         WHERE project_id = $1
           AND source_lang = $2 AND archived_at IS NOT NULL",
    )
    .bind(project_id)
    .bind(&plan.primary_source_lang)
    .execute(conn)
    .await?
    .rows_affected();
    Ok((archived, activated))
}

/// 兼容 Task 7 尚未创建/已经创建 deletion_scheduled_at 两种 schema 的只读 gate。
pub async fn project_pending_deletion_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(to_jsonb(project)->>'deletion_scheduled_at' IS NOT NULL, FALSE)
         FROM projects AS project WHERE id = $1",
    )
    .bind(project_id)
    .fetch_one(conn)
    .await
}

/// Preview 使用的只读 pending-deletion gate；兼容未来 deletion schema。
pub async fn project_pending_deletion(pool: &PgPool, project_id: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(to_jsonb(project)->>'deletion_scheduled_at' IS NOT NULL, FALSE)
         FROM projects AS project WHERE id = $1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
}
