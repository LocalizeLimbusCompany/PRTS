//! 三路召回的参数化查询。每路返回按相关度降序的 entry id（≤ per_path）。
use pgvector::Vector;
use sqlx::{PgPool, Postgres, QueryBuilder};

use crate::models::Entry;

/// 公共过滤：file / state / hidden 可见性。push 到已开头的 WHERE。
fn push_filters(
    qb: &mut QueryBuilder<'_, Postgres>,
    file_ids: &[i64],
    states: &[String],
    include_hidden: bool,
) {
    if !file_ids.is_empty() {
        qb.push(" AND file_id = ANY(")
            .push_bind(file_ids.to_vec())
            .push(")");
    }
    if !states.is_empty() {
        qb.push(" AND state = ANY(")
            .push_bind(states.to_vec())
            .push(")");
    }
    if !include_hidden {
        qb.push(" AND hidden = FALSE");
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
    file_ids: &[i64],
    states: &[String],
    include_hidden: bool,
    per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let per_path = per_path.max(1);
    let mut qb = QueryBuilder::new("SELECT id FROM entries, plainto_tsquery(prts_ts_config(");
    qb.push_bind(src_lang.to_string())
        .push("), ")
        .push_bind(q.to_string())
        .push(") AS sq(query), ");
    qb.push("plainto_tsquery(prts_ts_config(")
        .push_bind(tgt_lang.to_string())
        .push("), ")
        .push_bind(q.to_string())
        .push(") AS tq(query) WHERE project_id = ")
        .push_bind(project_id);
    qb.push(" AND (source_tsv @@ sq.query OR translation_tsv @@ tq.query)");
    push_filters(&mut qb, file_ids, states, include_hidden);
    qb.push(" ORDER BY (ts_rank(source_tsv, sq.query) + ts_rank(translation_tsv, tq.query)) DESC LIMIT ")
        .push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}

/// trgm：源/译/键三列相似度取最大值排序。pg_trgm `%` 用默认阈值。
pub async fn trgm_search(
    pool: &PgPool,
    project_id: i64,
    q: &str,
    file_ids: &[i64],
    states: &[String],
    include_hidden: bool,
    per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let per_path = per_path.max(1);
    let mut qb = QueryBuilder::new("SELECT id FROM entries WHERE project_id = ");
    qb.push_bind(project_id);
    qb.push(" AND (source_text % ")
        .push_bind(q.to_string())
        .push(" OR translation % ")
        .push_bind(q.to_string())
        .push(" OR key % ")
        .push_bind(q.to_string())
        .push(")");
    push_filters(&mut qb, file_ids, states, include_hidden);
    qb.push(" ORDER BY GREATEST(similarity(source_text, ")
        .push_bind(q.to_string())
        .push("), similarity(translation, ")
        .push_bind(q.to_string())
        .push("), similarity(key, ")
        .push_bind(q.to_string())
        .push(")) DESC LIMIT ")
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
    file_ids: &[i64],
    states: &[String],
    include_hidden: bool,
    per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let per_path = per_path.max(1);
    let v = Vector::from(qvec.to_vec());
    let mut qb = QueryBuilder::new("SELECT id FROM entries WHERE project_id = ");
    qb.push_bind(project_id).push(" AND embedding IS NOT NULL");
    push_filters(&mut qb, file_ids, states, include_hidden);
    qb.push(" ORDER BY embedding <=> ")
        .push_bind(v)
        .push(" LIMIT ")
        .push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}

/// 按 id 取整行，调用方负责按融合顺序重排。project_id 做纵深防御，避免跨项目泄露。
pub async fn fetch_by_ids(
    pool: &PgPool,
    project_id: i64,
    ids: &[i64],
) -> Result<Vec<Entry>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_as::<_, Entry>("SELECT * FROM entries WHERE project_id = $1 AND id = ANY($2)")
        .bind(project_id)
        .bind(ids.to_vec())
        .fetch_all(pool)
        .await
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
    let row: Option<(String, Option<Vector>)> =
        sqlx::query_as("SELECT source_text, embedding FROM entries WHERE id = $1")
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
         JOIN memberships m ON m.project_id = p.id AND m.user_id = $2
         WHERE p.target_lang = $3
           AND e.state IN ('translated','questioned','checked','reviewed')
           AND e.translation <> '' AND e.source_text <> ''
           AND e.id <> $4 AND e.embedding IS NOT NULL
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
         JOIN memberships m ON m.project_id = p.id AND m.user_id = $2
         WHERE p.target_lang = $3
           AND e.state IN ('translated','questioned','checked','reviewed')
           AND e.translation <> '' AND e.source_text <> ''
           AND e.id <> $4
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
