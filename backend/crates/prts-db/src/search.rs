//! 三路召回的参数化查询。每路返回按相关度降序的 entry id（≤ per_path）。
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
    let mut qb = QueryBuilder::new(
        "SELECT id FROM entries, plainto_tsquery(prts_ts_config(",
    );
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

/// 按 id 取整行，调用方负责按融合顺序重排。project_id 做纵深防御，避免跨项目泄露。
pub async fn fetch_by_ids(pool: &PgPool, project_id: i64, ids: &[i64]) -> Result<Vec<Entry>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_as::<_, Entry>("SELECT * FROM entries WHERE project_id = $1 AND id = ANY($2)")
        .bind(project_id)
        .bind(ids.to_vec())
        .fetch_all(pool)
        .await
}
