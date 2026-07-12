//! 词条数据访问：批量上传（带 key 覆盖与差异）、键集分页、乐观锁更新、历史。

use std::collections::HashMap;

use serde::Deserialize;
use sqlx::{PgConnection, PgPool, Postgres, QueryBuilder};

use crate::models::{Entry, EntryVersion};

/// 上传词条（来自上传 JSON 的单项）。
#[derive(Debug, Clone, Deserialize)]
pub struct UploadEntry {
    pub key: String,
    /// `{bcp47: 源文本}` 对象。
    #[serde(default)]
    pub original: serde_json::Value,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub translation: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

/// 上传统计。
#[derive(Debug, Default, Clone, Copy)]
pub struct UpsertStats {
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// 词条列表筛选条件。
#[derive(Debug, Default, Clone)]
pub struct EntryFilter {
    pub file_id: Option<i64>,
    pub states: Vec<String>,
    /// 关键字（P2 用 ILIKE；语义/全文检索见 P4）。
    pub query: Option<String>,
    pub include_hidden: bool,
}

/// 在低频项目重建事务内统计全部词条，作为一次性 job 进度基线。
pub async fn count_project_entries_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM entries WHERE project_id = $1")
        .bind(project_id)
        .fetch_one(conn)
        .await
}

fn normalize_state(s: Option<&str>) -> &'static str {
    match s {
        Some("translated") => "translated",
        Some("questioned") => "questioned",
        Some("checked") => "checked",
        Some("reviewed") => "reviewed",
        _ => "untranslated",
    }
}

/// 批量上传词条到指定文件：
/// - key 不存在 → 插入；
/// - key 存在且源文(original)有变化 → 覆盖源文、置未翻译、版本+1、**保留旧译文**，并记一条历史快照；
/// - key 存在且源文未变 → 不动。
///
/// 分批事务处理，便于承载大文件（性能可后续以 UNNEST/COPY 进一步优化）。
pub async fn bulk_upsert(
    pool: &PgPool,
    file_id: i64,
    project_id: i64,
    entries: &[UploadEntry],
    editor_id: Option<i64>,
) -> Result<UpsertStats, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let stats = bulk_upsert_tx(&mut tx, file_id, project_id, entries, editor_id).await?;
    tx.commit().await?;
    Ok(stats)
}

/// 在调用方事务内批量 upsert 词条；所有 chunk 与最终审计共用一次提交。
pub async fn bulk_upsert_tx(
    conn: &mut PgConnection,
    file_id: i64,
    project_id: i64,
    entries: &[UploadEntry],
    editor_id: Option<i64>,
) -> Result<UpsertStats, sqlx::Error> {
    let mut stats = UpsertStats::default();

    for chunk in entries.chunks(500) {
        let keys: Vec<String> = chunk.iter().map(|e| e.key.clone()).collect();
        let existing_rows: Vec<(i64, String, serde_json::Value)> = sqlx::query_as(
            "SELECT id, key, original FROM entries WHERE file_id = $1 AND key = ANY($2)",
        )
        .bind(file_id)
        .bind(&keys)
        .fetch_all(&mut *conn)
        .await?;

        let mut existing: HashMap<String, (i64, serde_json::Value)> = HashMap::new();
        for (id, key, original) in existing_rows {
            existing.insert(key, (id, original));
        }

        let mut to_insert: Vec<&UploadEntry> = Vec::new();
        for e in chunk {
            match existing.get(&e.key) {
                None => to_insert.push(e),
                Some((id, old_original)) => {
                    if old_original != &e.original {
                        // 先记录变更前快照
                        sqlx::query(
                            "INSERT INTO entry_versions (entry_id, version, kind, translation, state, original, editor_id)
                             SELECT id, version, 'source_update', translation, state, original, $2 FROM entries WHERE id = $1",
                        )
                        .bind(id)
                        .bind(editor_id)
                        .execute(&mut *conn)
                        .await?;
                        // 覆盖源文、置未翻译、版本+1（保留 translation）
                        sqlx::query(
                            "UPDATE entries SET original = $2, context = $3, state = 'untranslated',
                                 version = version + 1, updated_by = $4 WHERE id = $1",
                        )
                        .bind(id)
                        .bind(&e.original)
                        .bind(e.context.clone().unwrap_or_default())
                        .bind(editor_id)
                        .execute(&mut *conn)
                        .await?;
                        stats.updated += 1;
                    } else {
                        stats.unchanged += 1;
                    }
                }
            }
        }

        if !to_insert.is_empty() {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO entries (file_id, project_id, key, original, context, translation, state) ",
            );
            qb.push_values(to_insert.iter(), |mut b, e| {
                b.push_bind(file_id)
                    .push_bind(project_id)
                    .push_bind(&e.key)
                    .push_bind(&e.original)
                    .push_bind(e.context.clone().unwrap_or_default())
                    .push_bind(e.translation.clone().unwrap_or_default())
                    .push_bind(normalize_state(e.state.as_deref()));
            });
            qb.build().execute(&mut *conn).await?;
            stats.created += to_insert.len();
        }
    }

    Ok(stats)
}

/// 键集分页列出词条（按 id 游标）。
pub async fn list(
    pool: &PgPool,
    project_id: i64,
    filter: &EntryFilter,
    after_id: Option<i64>,
    limit: i64,
) -> Result<Vec<Entry>, sqlx::Error> {
    let mut qb = QueryBuilder::<Postgres>::new(
        "SELECT entry.* FROM entries AS entry
         JOIN files AS file ON file.id = entry.file_id
         WHERE entry.project_id = ",
    );
    qb.push_bind(project_id);
    qb.push(" AND entry.deleted_at IS NULL AND file.deleted_at IS NULL");

    if let Some(fid) = filter.file_id {
        qb.push(" AND entry.file_id = ");
        qb.push_bind(fid);
    }
    if !filter.states.is_empty() {
        qb.push(" AND entry.state = ANY(");
        qb.push_bind(filter.states.clone());
        qb.push(")");
    }
    if !filter.include_hidden {
        qb.push(" AND entry.hidden = FALSE");
    }
    if let Some(q) = filter.query.as_deref().filter(|s| !s.is_empty()) {
        let pat = format!("%{q}%");
        qb.push(" AND (entry.key ILIKE ");
        qb.push_bind(pat.clone());
        qb.push(" OR entry.translation ILIKE ");
        qb.push_bind(pat.clone());
        qb.push(" OR entry.original::text ILIKE ");
        qb.push_bind(pat);
        qb.push(")");
    }
    if let Some(after) = after_id {
        qb.push(" AND entry.id > ");
        qb.push_bind(after);
    }
    qb.push(" ORDER BY entry.id LIMIT ");
    qb.push_bind(limit);

    qb.build_query_as::<Entry>().fetch_all(pool).await
}

/// 按 id 查词条（限定项目）。
pub async fn get(
    pool: &PgPool,
    project_id: i64,
    entry_id: i64,
) -> Result<Option<Entry>, sqlx::Error> {
    sqlx::query_as::<_, Entry>(
        "SELECT entry.* FROM entries AS entry
         JOIN files AS file ON file.id = entry.file_id
         WHERE entry.id = $1 AND entry.project_id = $2
           AND entry.deleted_at IS NULL AND file.deleted_at IS NULL",
    )
    .bind(entry_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

/// 在调用方事务内锁定项目所属词条并返回一致快照。
pub async fn get_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
) -> Result<Option<Entry>, sqlx::Error> {
    sqlx::query_as::<_, Entry>("SELECT * FROM entries WHERE id = $1 AND project_id = $2 FOR UPDATE")
        .bind(entry_id)
        .bind(project_id)
        .fetch_optional(conn)
        .await
}

/// 乐观锁更新译文与状态：仅当 `expected_version` 匹配才更新。
/// 返回 `Ok(None)` 表示版本冲突（已被他人修改）。同时记录一条历史。
pub async fn update_translation(
    pool: &PgPool,
    entry_id: i64,
    expected_version: i64,
    translation: &str,
    state: &str,
    kind: &str,
    editor_id: Option<i64>,
) -> Result<Option<Entry>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let updated = update_translation_tx(
        &mut tx,
        entry_id,
        expected_version,
        translation,
        state,
        kind,
        editor_id,
    )
    .await?;
    tx.commit().await?;
    Ok(updated)
}

/// 在调用方事务内乐观锁更新译文/状态并写 entry version。
#[allow(clippy::too_many_arguments)]
pub async fn update_translation_tx(
    conn: &mut PgConnection,
    entry_id: i64,
    expected_version: i64,
    translation: &str,
    state: &str,
    kind: &str,
    editor_id: Option<i64>,
) -> Result<Option<Entry>, sqlx::Error> {
    let updated: Option<Entry> = sqlx::query_as::<_, Entry>(
        "UPDATE entries SET translation = $3, state = $4, version = version + 1, updated_by = $5
         WHERE id = $1 AND version = $2 RETURNING *",
    )
    .bind(entry_id)
    .bind(expected_version)
    .bind(translation)
    .bind(state)
    .bind(editor_id)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(ref e) = updated {
        sqlx::query(
            "INSERT INTO entry_versions (entry_id, version, kind, translation, state, editor_id)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(e.id)
        .bind(e.version)
        .bind(kind)
        .bind(&e.translation)
        .bind(&e.state)
        .bind(editor_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(updated)
}

/// 设置锁定/隐藏标志（None 表示不变）。
pub async fn set_flags(
    pool: &PgPool,
    project_id: i64,
    entry_id: i64,
    locked: Option<bool>,
    hidden: Option<bool>,
) -> Result<Option<Entry>, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    set_flags_tx(&mut connection, project_id, entry_id, locked, hidden).await
}

/// 在调用方事务内设置词条正交 flags。
pub async fn set_flags_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
    locked: Option<bool>,
    hidden: Option<bool>,
) -> Result<Option<Entry>, sqlx::Error> {
    sqlx::query_as::<_, Entry>(
        "UPDATE entries SET locked = COALESCE($3, locked), hidden = COALESCE($4, hidden)
         WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(entry_id)
    .bind(project_id)
    .bind(locked)
    .bind(hidden)
    .fetch_optional(conn)
    .await
}

/// 列出词条历史。
pub async fn list_versions(
    pool: &PgPool,
    entry_id: i64,
    limit: i64,
) -> Result<Vec<EntryVersion>, sqlx::Error> {
    sqlx::query_as::<_, EntryVersion>(
        "SELECT * FROM entry_versions WHERE entry_id = $1 ORDER BY version DESC, id DESC LIMIT $2",
    )
    .bind(entry_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 统计项目内各状态词条数。
pub async fn count_by_state(
    pool: &PgPool,
    project_id: i64,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    sqlx::query_as::<_, (String, i64)>(
        "SELECT state, COUNT(*) FROM entries WHERE project_id = $1 GROUP BY state",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// 导出用：取项目全部词条（按文件、key 排序）。
pub async fn list_for_export(pool: &PgPool, project_id: i64) -> Result<Vec<Entry>, sqlx::Error> {
    sqlx::query_as::<_, Entry>(
        "SELECT entry.* FROM entries AS entry
         JOIN files AS file ON file.id = entry.file_id
         WHERE entry.project_id = $1 AND entry.deleted_at IS NULL
           AND file.deleted_at IS NULL AND NOT entry.hidden
         ORDER BY entry.file_id, entry.key",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}
