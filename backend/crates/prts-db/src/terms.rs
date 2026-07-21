//! source-aware 项目术语仓储与主源切换集合 executor。

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

use crate::models::{Term, TermVersion, TermWithPos};

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
    Deleted,
}

/// 按 id DESC 键集列出术语。
pub async fn list(
    pool: &PgPool,
    project_id: i64,
    primary_source_lang: &str,
    scope: TermListScope,
    query: Option<&str>,
    after: Option<i64>,
    limit: i64,
) -> Result<Vec<TermWithPos>, sqlx::Error> {
    let scope = match scope {
        TermListScope::Current => "current",
        TermListScope::Archived => "archived",
        TermListScope::Mixed => "mixed",
        TermListScope::Deleted => "deleted",
    };
    sqlx::query_as::<_, TermWithPos>(&format!(
        "{TERM_WITH_POS_SELECT}
         WHERE term.project_id = $1
           AND CASE WHEN $4::TEXT = 'deleted'
                    THEN term.deleted_at IS NOT NULL
                    ELSE term.deleted_at IS NULL
               END
           AND ($3::BIGINT IS NULL OR term.id < $3)
           AND ($5::TEXT IS NULL OR term.source_text ILIKE '%' || $5 || '%'
                OR term.translation ILIKE '%' || $5 || '%'
                OR term.notes ILIKE '%' || $5 || '%')
           AND CASE $4::TEXT
                 WHEN 'current' THEN term.archived_at IS NULL AND term.source_lang = $2
                 WHEN 'archived' THEN term.archived_at IS NOT NULL
                 WHEN 'deleted' THEN TRUE
                 ELSE TRUE
               END
         ORDER BY term.id DESC LIMIT $6"
    ))
    .bind(project_id)
    .bind(primary_source_lang)
    .bind(after)
    .bind(scope)
    .bind(query)
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
        "{TERM_WITH_POS_SELECT}
         WHERE term.project_id = $1 AND term.id = $2 AND term.deleted_at IS NULL"
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
    match_mode: &str,
    archived_at: Option<DateTime<Utc>>,
    actor_id: i64,
) -> Result<Option<Term>, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "INSERT INTO terms (
             project_id, source_lang, source_text, translation, notes, pos_id,
             match_mode, archived_at, created_by, updated_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
         ON CONFLICT ON CONSTRAINT terms_identity_unique DO UPDATE
         SET translation = EXCLUDED.translation,
             notes = EXCLUDED.notes,
             match_mode = EXCLUDED.match_mode,
             archived_at = EXCLUDED.archived_at,
             deleted_at = NULL,
             deleted_by = NULL,
             updated_by = EXCLUDED.updated_by,
             version = terms.version + 1
         WHERE terms.deleted_at IS NOT NULL
         RETURNING *",
    )
    .bind(project_id)
    .bind(source_lang)
    .bind(source_text)
    .bind(translation)
    .bind(notes)
    .bind(pos_id)
    .bind(match_mode)
    .bind(archived_at)
    .bind(actor_id)
    .fetch_optional(conn)
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
    match_mode: &str,
    archived_at: Option<DateTime<Utc>>,
    actor_id: i64,
) -> Result<Option<Term>, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "UPDATE terms
         SET source_lang = $3, source_text = $4, translation = $5, notes = $6,
             pos_id = $7, match_mode = $8, archived_at = $9, updated_by = $10,
             version = version + 1
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING *",
    )
    .bind(project_id)
    .bind(term_id)
    .bind(source_lang)
    .bind(source_text)
    .bind(translation)
    .bind(notes)
    .bind(pos_id)
    .bind(match_mode)
    .bind(archived_at)
    .bind(actor_id)
    .fetch_optional(conn)
    .await
}

/// 软删除 URL project 绑定的术语并递增版本。
pub async fn delete_tx(
    conn: &mut PgConnection,
    project_id: i64,
    term_id: i64,
    actor_id: i64,
) -> Result<Option<Term>, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "UPDATE terms SET deleted_at = now(), deleted_by = $3,
             updated_by = $3, version = version + 1
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL RETURNING *",
    )
    .bind(project_id)
    .bind(term_id)
    .bind(actor_id)
    .fetch_optional(conn)
    .await
}

/// 返回当前 primary 的 active term 候选；应用层按 typed matcher 过滤非 exact 模式。
pub async fn match_candidates(
    pool: &PgPool,
    project_id: i64,
    primary_source_lang: &str,
    source_text: &str,
    limit: i64,
) -> Result<Vec<TermWithPos>, sqlx::Error> {
    sqlx::query_as::<_, TermWithPos>(&format!(
        "{TERM_WITH_POS_SELECT}
         WHERE term.project_id = $1 AND term.source_lang = $2
           AND term.archived_at IS NULL
           AND (term.match_mode <> 'exact' OR position(term.source_text IN $3) > 0)
           AND term.deleted_at IS NULL
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
        "{TERM_WITH_POS_SELECT}
         WHERE term.project_id = $1 AND term.deleted_at IS NULL ORDER BY term.id ASC"
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
        "{TERM_WITH_POS_SELECT}
         WHERE term.project_id = $1 AND term.deleted_at IS NULL ORDER BY term.id ASC"
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
    match_modes: &[String],
    pos_ids: &[Option<i64>],
) -> Result<Vec<Option<i64>>, sqlx::Error> {
    existing_import_ids_executor(
        pool,
        project_id,
        source_langs,
        source_texts,
        match_modes,
        pos_ids,
    )
    .await
}

/// 在 confirm 事务内重新解析 canonical NULL-safe identity。
pub async fn existing_import_ids_tx(
    conn: &mut PgConnection,
    project_id: i64,
    source_langs: &[String],
    source_texts: &[String],
    match_modes: &[String],
    pos_ids: &[Option<i64>],
) -> Result<Vec<Option<i64>>, sqlx::Error> {
    existing_import_ids_executor(
        conn,
        project_id,
        source_langs,
        source_texts,
        match_modes,
        pos_ids,
    )
    .await
}

async fn existing_import_ids_executor<'e, E>(
    executor: E,
    project_id: i64,
    source_langs: &[String],
    source_texts: &[String],
    match_modes: &[String],
    pos_ids: &[Option<i64>],
) -> Result<Vec<Option<i64>>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "WITH input AS (
             SELECT source_lang, source_text, match_mode, pos_id, ordinality
             FROM unnest($2::TEXT[], $3::TEXT[], $4::TEXT[], $5::BIGINT[])
                  WITH ORDINALITY AS value(source_lang, source_text, match_mode, pos_id, ordinality)
         )
         SELECT input.ordinality::BIGINT, term.id
         FROM input
         LEFT JOIN terms AS term
           ON term.project_id = $1
          AND term.source_lang = input.source_lang
          AND term.source_text = input.source_text
          AND term.match_mode = input.match_mode
          AND term.pos_id IS NOT DISTINCT FROM input.pos_id
         ORDER BY input.ordinality",
    )
    .bind(project_id)
    .bind(source_langs)
    .bind(source_texts)
    .bind(match_modes)
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
    match_mode: &str,
    archived_at: Option<DateTime<Utc>>,
    actor_id: i64,
) -> Result<Term, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "INSERT INTO terms (
             project_id, source_lang, source_text, translation, notes, pos_id,
             match_mode, archived_at, created_by, updated_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
         ON CONFLICT ON CONSTRAINT terms_identity_unique DO UPDATE
         SET translation = EXCLUDED.translation,
             notes = EXCLUDED.notes,
             match_mode = EXCLUDED.match_mode,
             archived_at = EXCLUDED.archived_at,
             deleted_at = NULL,
             deleted_by = NULL,
             updated_by = EXCLUDED.updated_by,
             version = terms.version + 1
         RETURNING *",
    )
    .bind(project_id)
    .bind(source_lang)
    .bind(source_text)
    .bind(translation)
    .bind(notes)
    .bind(pos_id)
    .bind(match_mode)
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
    actor_id: i64,
) -> Result<(u64, u64), sqlx::Error> {
    let archived_ids: Vec<i64> = sqlx::query_scalar(
        "UPDATE terms SET archived_at = now(), updated_by = $3, version = version + 1
         WHERE project_id = $1
           AND source_lang <> $2 AND archived_at IS NULL AND deleted_at IS NULL
         RETURNING id",
    )
    .bind(project_id)
    .bind(&plan.primary_source_lang)
    .bind(actor_id)
    .fetch_all(&mut *conn)
    .await?;
    for term_id in &archived_ids {
        append_version_tx(conn, *term_id, "primary_source_archive", actor_id).await?;
    }
    let activated_ids: Vec<i64> = sqlx::query_scalar(
        "UPDATE terms SET archived_at = NULL, updated_by = $3, version = version + 1
         WHERE project_id = $1
           AND source_lang = $2 AND archived_at IS NOT NULL AND deleted_at IS NULL
         RETURNING id",
    )
    .bind(project_id)
    .bind(&plan.primary_source_lang)
    .bind(actor_id)
    .fetch_all(&mut *conn)
    .await?;
    for term_id in &activated_ids {
        append_version_tx(conn, *term_id, "primary_source_activate", actor_id).await?;
    }
    Ok((archived_ids.len() as u64, activated_ids.len() as u64))
}

/// 为当前术语状态写入一条不可改写完整快照。
pub async fn append_version_tx(
    conn: &mut PgConnection,
    term_id: i64,
    kind: &str,
    actor_id: i64,
) -> Result<TermVersion, sqlx::Error> {
    sqlx::query_as::<_, TermVersion>(
        "INSERT INTO term_versions (
             project_id, term_id, version, kind, source_lang, source_text,
             translation, notes, pos_id, match_mode, archived_at, deleted_at,
             editor_id, editor_name, editor_avatar_url
         )
         SELECT term.project_id, term.id, term.version, $2, term.source_lang,
                term.source_text, term.translation, term.notes, term.pos_id,
                term.match_mode, term.archived_at, term.deleted_at, actor.id, actor.username,
                actor.avatar_url
         FROM terms AS term JOIN users AS actor ON actor.id = $3
         WHERE term.id = $1
         RETURNING *",
    )
    .bind(term_id)
    .bind(kind)
    .bind(actor_id)
    .fetch_one(conn)
    .await
}

/// 倒序列出术语版本。
pub async fn list_versions(
    pool: &PgPool,
    project_id: i64,
    term_id: i64,
    after: Option<i64>,
    limit: i64,
) -> Result<Vec<TermVersion>, sqlx::Error> {
    sqlx::query_as::<_, TermVersion>(
        "SELECT * FROM term_versions
         WHERE project_id = $1 AND term_id = $2
           AND ($3::BIGINT IS NULL OR version < $3)
         ORDER BY version DESC, id DESC LIMIT $4",
    )
    .bind(project_id)
    .bind(term_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 锁定指定术语版本。
pub async fn find_version_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    term_id: i64,
    version: i64,
) -> Result<Option<TermVersion>, sqlx::Error> {
    sqlx::query_as::<_, TermVersion>(
        "SELECT * FROM term_versions
         WHERE project_id = $1 AND term_id = $2 AND version = $3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(term_id)
    .bind(version)
    .fetch_optional(conn)
    .await
}

/// 把历史快照恢复为新的当前版本；恢复永远重新激活被删除术语。
pub async fn restore_version_tx(
    conn: &mut PgConnection,
    project_id: i64,
    term_id: i64,
    snapshot: &TermVersion,
    actor_id: i64,
) -> Result<Term, sqlx::Error> {
    sqlx::query_as::<_, Term>(
        "UPDATE terms SET source_lang = $3, source_text = $4, translation = $5,
             notes = $6, pos_id = $7, match_mode = $8, archived_at = $9,
             deleted_at = NULL, deleted_by = NULL, updated_by = $10,
             version = version + 1
         WHERE project_id = $1 AND id = $2 RETURNING *",
    )
    .bind(project_id)
    .bind(term_id)
    .bind(&snapshot.source_lang)
    .bind(&snapshot.source_text)
    .bind(&snapshot.translation)
    .bind(&snapshot.notes)
    .bind(snapshot.pos_id)
    .bind(&snapshot.match_mode)
    .bind(snapshot.archived_at)
    .bind(actor_id)
    .fetch_one(conn)
    .await
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
