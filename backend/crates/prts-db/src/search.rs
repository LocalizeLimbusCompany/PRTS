//! 结构化搜索资源解析、三路召回与一致 fetch。
//!
//! scope 已由 API 绑定 URL project 后解析成 active file IDs；conditions/states 与规范
//! effective-visible 谓词在 recall/fetch 每一阶段复用，避免先召回后越权取行。
use pgvector::Vector;
use prts_core::search_query::{CanonicalSearchCondition, SearchField, SearchOperator};
use sqlx::{PgPool, Postgres, QueryBuilder};

use crate::models::Entry;

/// 已验证的 path scope 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathScopeResolution {
    Missing,
    Ambiguous,
    Files(Vec<i64>),
}

/// 精确 active file id；cross-project、deleted file 或 deleted ancestor 均 fail closed。
pub async fn resolve_active_file_id(
    pool: &PgPool,
    project_id: i64,
    file_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT file.id FROM files AS file
         WHERE file.project_id = $1 AND file.id = $2 AND file.deleted_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM folders AS ancestor
               WHERE ancestor.project_id = file.project_id
                 AND ancestor.deleted_at IS NOT NULL
                 AND (file.path = ancestor.path
                      OR file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\\')
           )",
    )
    .bind(project_id)
    .bind(file_id)
    .fetch_optional(pool)
    .await
}

/// canonical path 按 segment boundary 精确解析 active file 或 active folder subtree。
pub async fn resolve_path_scope(
    pool: &PgPool,
    project_id: i64,
    path: &str,
) -> Result<PathScopeResolution, sqlx::Error> {
    let file_id: Option<i64> = sqlx::query_scalar(
        "SELECT file.id FROM files AS file
         WHERE file.project_id = $1 AND file.path = $2 AND file.deleted_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM folders AS ancestor
               WHERE ancestor.project_id = file.project_id
                 AND ancestor.deleted_at IS NOT NULL
                 AND (file.path = ancestor.path
                      OR file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\\')
           )",
    )
    .bind(project_id)
    .bind(path)
    .fetch_optional(pool)
    .await?;
    let folder_id: Option<i64> = sqlx::query_scalar(
        "SELECT folder.id FROM folders AS folder
         WHERE folder.project_id = $1 AND folder.path = $2 AND folder.deleted_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM folders AS ancestor
               WHERE ancestor.project_id = folder.project_id
                 AND ancestor.deleted_at IS NOT NULL
                 AND (folder.path = ancestor.path
                      OR folder.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\\')
           )",
    )
    .bind(project_id)
    .bind(path)
    .fetch_optional(pool)
    .await?;
    match (file_id, folder_id) {
        (Some(_), Some(_)) => Ok(PathScopeResolution::Ambiguous),
        (Some(file_id), None) => Ok(PathScopeResolution::Files(vec![file_id])),
        (None, Some(_)) => {
            let file_ids = sqlx::query_scalar(
                "SELECT file.id FROM files AS file
                 WHERE file.project_id = $1 AND file.deleted_at IS NULL
                   AND file.path LIKE prts_escape_like_pattern($2) || '/%' ESCAPE '\\'
                   AND NOT EXISTS (
                       SELECT 1 FROM folders AS ancestor
                       WHERE ancestor.project_id = file.project_id
                         AND ancestor.deleted_at IS NOT NULL
                         AND (file.path = ancestor.path
                              OR file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\\')
                   )
                 ORDER BY file.id",
            )
            .bind(project_id)
            .bind(path)
            .fetch_all(pool)
            .await?;
            Ok(PathScopeResolution::Files(file_ids))
        }
        (None, None) => Ok(PathScopeResolution::Missing),
    }
}

/// task 必须属于 URL project；只返回当前 active、无 deleted ancestor 的 live files。
pub async fn resolve_active_task_file_ids(
    pool: &PgPool,
    project_id: i64,
    task_id: i64,
) -> Result<Option<Vec<i64>>, sqlx::Error> {
    let task_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM tasks WHERE id = $1 AND project_id = $2)")
            .bind(task_id)
            .bind(project_id)
            .fetch_one(pool)
            .await?;
    if !task_exists {
        return Ok(None);
    }
    sqlx::query_scalar(
        "SELECT file.id FROM task_files AS task_file
         JOIN files AS file ON file.id = task_file.live_file_id
         WHERE task_file.task_id = $1 AND file.project_id = $2 AND file.deleted_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM folders AS ancestor
               WHERE ancestor.project_id = file.project_id
                 AND ancestor.deleted_at IS NOT NULL
                 AND (file.path = ancestor.path
                      OR file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%' ESCAPE '\\')
           )
         ORDER BY file.id",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map(Some)
}

/// recall/fetch 公用过滤。
pub struct SearchExecutionFilter<'a> {
    pub file_ids: &'a [i64],
    pub restrict_to_file_ids: bool,
    pub states: &'a [String],
    pub conditions: &'a [CanonicalSearchCondition],
    pub include_hidden: bool,
}

/// 公共过滤：scope / state / conditions / effective-visible。push 到已开头的 WHERE。
fn push_filters(qb: &mut QueryBuilder<'_, Postgres>, filter: &SearchExecutionFilter<'_>) {
    if filter.restrict_to_file_ids && filter.file_ids.is_empty() {
        qb.push(" AND FALSE");
    } else if !filter.file_ids.is_empty() {
        qb.push(" AND entry.file_id = ANY(")
            .push_bind(filter.file_ids.to_vec())
            .push(")");
    }
    if !filter.states.is_empty() {
        qb.push(" AND entry.state = ANY(")
            .push_bind(filter.states.to_vec())
            .push(")");
    }
    qb.push(" AND prts_entry_effective_visible(entry.id, ")
        .push_bind(filter.include_hidden)
        .push(")");
    for condition in filter.conditions {
        push_condition(qb, condition);
    }
}

fn push_condition(qb: &mut QueryBuilder<'_, Postgres>, condition: &CanonicalSearchCondition) {
    qb.push(" AND ");
    match &condition.field {
        SearchField::Source(language) => {
            qb.push("(");
            push_operator(
                qb,
                |qb| {
                    qb.push("COALESCE(entry.original ->> ")
                        .push_bind(language.clone())
                        .push(", '')");
                },
                condition.operator,
                &condition.value,
            );
            qb.push(")");
        }
        SearchField::SourceAny => {
            qb.push("EXISTS (SELECT 1 FROM jsonb_each_text(entry.original) AS source_any(language, value) WHERE ");
            push_operator(
                qb,
                |qb| {
                    qb.push("source_any.value");
                },
                condition.operator,
                &condition.value,
            );
            qb.push(")");
        }
        SearchField::Translation => push_operator(
            qb,
            |qb| {
                qb.push("entry.translation");
            },
            condition.operator,
            &condition.value,
        ),
        SearchField::Key => push_operator(
            qb,
            |qb| {
                qb.push("entry.key");
            },
            condition.operator,
            &condition.value,
        ),
    }
}

fn push_operator<F>(
    qb: &mut QueryBuilder<'_, Postgres>,
    mut push_expression: F,
    operator: SearchOperator,
    value: &str,
) where
    F: FnMut(&mut QueryBuilder<'_, Postgres>),
{
    match operator {
        SearchOperator::Contains | SearchOperator::NotContains => {
            qb.push("strpos(");
            push_expression(qb);
            qb.push(", ").push_bind(value.to_string()).push(
                if matches!(operator, SearchOperator::Contains) {
                    ") > 0"
                } else {
                    ") = 0"
                },
            );
        }
        SearchOperator::StartsWith => {
            qb.push("starts_with(");
            push_expression(qb);
            qb.push(", ").push_bind(value.to_string()).push(")");
        }
        SearchOperator::EndsWith => {
            qb.push("right(");
            push_expression(qb);
            qb.push(", char_length(")
                .push_bind(value.to_string())
                .push(")) = ")
                .push_bind(value.to_string());
        }
        SearchOperator::Equals => {
            push_expression(qb);
            qb.push(" = ").push_bind(value.to_string());
        }
    }
}

/// FTS：源/译两列各按其语言 config 匹配；ts_rank 求和排序。
#[allow(clippy::too_many_arguments)]
pub async fn fts_search(
    pool: &PgPool,
    project_id: i64,
    q: &str,
    src_lang: &str,
    tgt_lang: &str,
    filter: &SearchExecutionFilter<'_>,
    per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let per_path = per_path.max(1);
    let mut qb =
        QueryBuilder::new("SELECT entry.id FROM entries AS entry, plainto_tsquery(prts_ts_config(");
    qb.push_bind(src_lang.to_string())
        .push("), ")
        .push_bind(q.to_string())
        .push(") AS sq(query), ");
    qb.push("plainto_tsquery(prts_ts_config(")
        .push_bind(tgt_lang.to_string())
        .push("), ")
        .push_bind(q.to_string())
        .push(") AS tq(query) WHERE entry.project_id = ")
        .push_bind(project_id);
    qb.push(" AND (entry.source_tsv @@ sq.query OR entry.translation_tsv @@ tq.query)");
    push_filters(&mut qb, filter);
    qb.push(" ORDER BY (ts_rank(entry.source_tsv, sq.query) + ts_rank(entry.translation_tsv, tq.query)) DESC, entry.id ASC LIMIT ")
        .push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}

/// trgm：源/译/键三列相似度取最大值排序。pg_trgm `%` 用默认阈值。
pub async fn trgm_search(
    pool: &PgPool,
    project_id: i64,
    q: &str,
    filter: &SearchExecutionFilter<'_>,
    per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let per_path = per_path.max(1);
    let mut qb =
        QueryBuilder::new("SELECT entry.id FROM entries AS entry WHERE entry.project_id = ");
    qb.push_bind(project_id);
    qb.push(" AND (entry.source_text % ")
        .push_bind(q.to_string())
        .push(" OR entry.translation % ")
        .push_bind(q.to_string())
        .push(" OR entry.key % ")
        .push_bind(q.to_string())
        .push(")");
    push_filters(&mut qb, filter);
    qb.push(" ORDER BY GREATEST(similarity(entry.source_text, ")
        .push_bind(q.to_string())
        .push("), similarity(entry.translation, ")
        .push_bind(q.to_string())
        .push("), similarity(entry.key, ")
        .push_bind(q.to_string())
        .push(")) DESC, entry.id ASC LIMIT ")
        .push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}

/// 向量召回：cosine 距离最近的 per_path 条（仅 embedding 非空）。
/// 失败或 embedding 列全空时返回空列表，由调用方降级为 FTS+trgm。
#[allow(clippy::too_many_arguments)]
pub async fn vector_search(
    pool: &PgPool,
    project_id: i64,
    qvec: &[f32],
    filter: &SearchExecutionFilter<'_>,
    per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let per_path = per_path.max(1);
    let v = Vector::from(qvec.to_vec());
    let mut qb =
        QueryBuilder::new("SELECT entry.id FROM entries AS entry WHERE entry.project_id = ");
    qb.push_bind(project_id)
        .push(" AND entry.embedding IS NOT NULL");
    push_filters(&mut qb, filter);
    qb.push(" ORDER BY entry.embedding <=> ")
        .push_bind(v)
        .push(", entry.id ASC LIMIT ")
        .push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}

/// 按 id 取整行，调用方负责按融合顺序重排。project_id 做纵深防御，避免跨项目泄露。
pub async fn fetch_by_ids(
    pool: &PgPool,
    project_id: i64,
    ids: &[i64],
    filter: &SearchExecutionFilter<'_>,
) -> Result<Vec<Entry>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut qb =
        QueryBuilder::new("SELECT entry.* FROM entries AS entry WHERE entry.project_id = ");
    qb.push_bind(project_id)
        .push(" AND entry.id = ANY(")
        .push_bind(ids.to_vec())
        .push(")");
    push_filters(&mut qb, filter);
    qb.build_query_as().fetch_all(pool).await
}

/// 无 query 时按相同 filter 做确定性基础召回；score 由 orchestrator 置零。
pub async fn filter_only_search(
    pool: &PgPool,
    project_id: i64,
    filter: &SearchExecutionFilter<'_>,
    after_entry_id: Option<i64>,
    limit: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let mut qb =
        QueryBuilder::new("SELECT entry.id FROM entries AS entry WHERE entry.project_id = ");
    qb.push_bind(project_id);
    push_filters(&mut qb, filter);
    if let Some(after_entry_id) = after_entry_id {
        qb.push(" AND entry.id > ").push_bind(after_entry_id);
    }
    qb.push(" ORDER BY entry.id ASC LIMIT ")
        .push_bind(limit.max(1));
    qb.build_query_scalar().fetch_all(pool).await
}

// ────────────────────────────────────────────────────────────────
// TM 建议（Translation Memory Suggestions）
// ────────────────────────────────────────────────────────────────

/// TM 建议候选行（含来源项目名）。
#[derive(Debug, sqlx::FromRow)]
pub struct SuggestionRow {
    pub entry_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub source_text: String,
    pub translation: String,
    pub state: String,
    pub similarity: f64,
}

/// 当前词条的 source_text 与 embedding（用于建议召回；None = 词条不存在）。
pub async fn current_search_fields(
    pool: &PgPool,
    entry_id: i64,
) -> Result<Option<(String, Option<Vec<f32>>)>, sqlx::Error> {
    let row: Option<(String, Option<Vector>)> = sqlx::query_as(
        "SELECT source_text, embedding FROM entries
             WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(entry_id)
    .fetch_optional(pool)
    .await?;
    // pgvector::Vector 转 Vec<f32>：用 as_slice().to_vec()。
    Ok(row.map(|(st, emb)| (st, emb.map(|v| v.as_slice().to_vec()))))
}

/// 向量版 TM 建议：当前词条已有 embedding。仅用户已加入项目 + 同 target_lang。
#[allow(clippy::too_many_arguments)]
pub async fn suggestions_vector(
    pool: &PgPool,
    user_id: i64,
    target_lang: &str,
    cur_embedding: &[f32],
    cur_entry_id: i64,
    min_sim: f64,
    top_n: i64,
) -> Result<Vec<SuggestionRow>, sqlx::Error> {
    let v = Vector::from(cur_embedding.to_vec());
    sqlx::query_as::<_, SuggestionRow>(
        "SELECT e.id AS entry_id, p.id AS project_id, p.name AS project_name,
                e.source_text, e.translation, e.state,
                (1 - (e.embedding <=> $1))::float8 AS similarity
         FROM entries e
         JOIN projects p ON p.id = e.project_id
         JOIN files f ON f.id = e.file_id
         JOIN memberships m ON m.project_id = p.id AND m.user_id = $2
         WHERE p.target_lang = $3
           AND e.state IN ('translated','questioned','checked','reviewed')
           AND e.translation <> '' AND e.source_text <> ''
           AND e.id <> $4 AND e.embedding IS NOT NULL
           AND e.deleted_at IS NULL AND f.deleted_at IS NULL AND NOT e.hidden
           AND p.language_repair_state = 'ready' AND p.lexical_state = 'ready'
           AND (1 - (e.embedding <=> $1)) >= $5
         ORDER BY e.embedding <=> $1
         LIMIT $6",
    )
    .bind(v)
    .bind(user_id)
    .bind(target_lang)
    .bind(cur_entry_id)
    .bind(min_sim)
    .bind(top_n)
    .fetch_all(pool)
    .await
}

/// trgm 版 TM 建议：向量关或当前词条无 embedding 时，用源文相似度。
#[allow(clippy::too_many_arguments)]
pub async fn suggestions_trgm(
    pool: &PgPool,
    user_id: i64,
    target_lang: &str,
    cur_source: &str,
    cur_entry_id: i64,
    min_sim: f64,
    top_n: i64,
) -> Result<Vec<SuggestionRow>, sqlx::Error> {
    sqlx::query_as::<_, SuggestionRow>(
        "SELECT e.id AS entry_id, p.id AS project_id, p.name AS project_name,
                e.source_text, e.translation, e.state,
                similarity(e.source_text, $1)::float8 AS similarity
         FROM entries e
         JOIN projects p ON p.id = e.project_id
         JOIN files f ON f.id = e.file_id
         JOIN memberships m ON m.project_id = p.id AND m.user_id = $2
         WHERE p.target_lang = $3
           AND e.state IN ('translated','questioned','checked','reviewed')
           AND e.translation <> '' AND e.source_text <> ''
           AND e.id <> $4
           AND e.deleted_at IS NULL AND f.deleted_at IS NULL AND NOT e.hidden
           AND p.language_repair_state = 'ready' AND p.lexical_state = 'ready'
           AND similarity(e.source_text, $1) >= $5
         ORDER BY similarity(e.source_text, $1) DESC
         LIMIT $6",
    )
    .bind(cur_source)
    .bind(user_id)
    .bind(target_lang)
    .bind(cur_entry_id)
    .bind(min_sim)
    .bind(top_n)
    .fetch_all(pool)
    .await
}
