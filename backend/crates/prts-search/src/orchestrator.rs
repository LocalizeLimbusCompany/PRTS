//! 混合检索编排：typed filter → 并行召回 → RRF → 一致 fetch。
use crate::rrf::rrf_fuse;
use crate::SearchHit;
use prts_core::search_query::CanonicalSearchCondition;
use prts_db::models::Entry;
use sqlx::PgPool;

pub struct OrchestratorInput<'a> {
    pub project_id: i64,
    pub query: Option<&'a str>,
    pub src_lang: &'a str,
    pub tgt_lang: &'a str,
    pub file_ids: &'a [i64],
    pub restrict_to_file_ids: bool,
    pub states: &'a [String],
    pub questioned: Option<bool>,
    pub conditions: &'a [CanonicalSearchCondition],
    pub case_sensitive: bool,
    pub include_hidden: bool,
    pub per_path: i64,
    pub top_k: i64,
    /// filter-only（score=0）搜索直接在 SQL 做 id keyset；query 搜索由 score/id cursor 过滤。
    pub filter_after_entry_id: Option<i64>,
    /// 向量召回（已排序 id）；None = vector=false 或安全降级。
    pub vector_ids: Option<Vec<i64>>,
}

/// 返回按最终顺序排列的 (Entry, relevance_score)。
pub async fn run(
    pool: &PgPool,
    input: OrchestratorInput<'_>,
) -> Result<Vec<(Entry, f64)>, sqlx::Error> {
    let filter = prts_db::search::SearchExecutionFilter {
        file_ids: input.file_ids,
        restrict_to_file_ids: input.restrict_to_file_ids,
        states: input.states,
        questioned: input.questioned,
        conditions: input.conditions,
        case_sensitive: input.case_sensitive,
        query: input.query,
        include_hidden: input.include_hidden,
    };
    let hits: Vec<SearchHit> = if let Some(query) = input.query {
        let (fts, trgm) = tokio::join!(
            prts_db::search::fts_search(
                pool,
                input.project_id,
                query,
                input.src_lang,
                input.tgt_lang,
                &filter,
                input.per_path
            ),
            prts_db::search::trgm_search(pool, input.project_id, query, &filter, input.per_path),
        );
        let mut paths = vec![fts?, trgm?];
        if let Some(vector_ids) = input.vector_ids {
            paths.push(vector_ids);
        }
        rrf_fuse(&paths)
            .into_iter()
            .take(input.top_k as usize)
            .map(|(id, score)| SearchHit { id, score })
            .collect()
    } else {
        prts_db::search::filter_only_search(
            pool,
            input.project_id,
            &filter,
            input.filter_after_entry_id,
            input.top_k,
        )
        .await?
        .into_iter()
        .map(|id| SearchHit { id, score: 0.0 })
        .collect()
    };

    let ids: Vec<i64> = hits.iter().map(|h| h.id).collect();
    let rows = prts_db::search::fetch_by_ids(pool, input.project_id, &ids, &filter).await?;
    let mut by_id: std::collections::HashMap<i64, Entry> =
        rows.into_iter().map(|e| (e.id, e)).collect();
    let mut out: Vec<(Entry, f64)> = hits
        .iter()
        .filter_map(|h| by_id.remove(&h.id).map(|e| (e, h.score)))
        .collect();
    out.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.id.cmp(&right.0.id))
    });
    Ok(out)
}
