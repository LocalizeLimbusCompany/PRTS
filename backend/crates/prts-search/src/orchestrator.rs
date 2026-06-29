//! 混合检索编排：并行多路 → RRF → 取行 → 按分排序/截窗。
use crate::rrf::rrf_fuse;
use crate::{SearchHit, SortBy};
use prts_db::models::Entry;
use sqlx::PgPool;

pub struct OrchestratorInput<'a> {
    pub project_id: i64,
    pub q: &'a str,
    pub src_lang: &'a str,
    pub tgt_lang: &'a str,
    pub file_ids: &'a [i64],
    pub states: &'a [String],
    pub include_hidden: bool,
    pub per_path: i64,
    pub top_k: i64,
    pub sort: SortBy,
    /// 向量召回（已排序 id）；None = 向量路关闭/降级（后续任务注入）。
    pub vector_ids: Option<Vec<i64>>,
}

/// 返回按最终顺序排列的 (Entry, relevance_score)。
pub async fn run(
    pool: &PgPool,
    input: OrchestratorInput<'_>,
) -> Result<Vec<(Entry, f64)>, sqlx::Error> {
    let (fts, trgm) = tokio::join!(
        prts_db::search::fts_search(
            pool,
            input.project_id,
            input.q,
            input.src_lang,
            input.tgt_lang,
            input.file_ids,
            input.states,
            input.include_hidden,
            input.per_path
        ),
        prts_db::search::trgm_search(
            pool,
            input.project_id,
            input.q,
            input.file_ids,
            input.states,
            input.include_hidden,
            input.per_path
        ),
    );
    let mut paths = vec![fts?, trgm?];
    if let Some(v) = input.vector_ids {
        paths.push(v);
    }

    let fused = rrf_fuse(&paths);
    let hits: Vec<SearchHit> = fused
        .into_iter()
        .take(input.top_k as usize)
        .map(|(id, score)| SearchHit { id, score })
        .collect();

    let ids: Vec<i64> = hits.iter().map(|h| h.id).collect();
    let rows = prts_db::search::fetch_by_ids(pool, input.project_id, &ids).await?;
    let mut by_id: std::collections::HashMap<i64, Entry> =
        rows.into_iter().map(|e| (e.id, e)).collect();
    let mut out: Vec<(Entry, f64)> = hits
        .iter()
        .filter_map(|h| by_id.remove(&h.id).map(|e| (e, h.score)))
        .collect();

    match input.sort {
        SortBy::Relevance => {}
        SortBy::Key => out.sort_by_key(|e| e.0.key.clone()),
        SortBy::UpdatedAt => out.sort_by_key(|e| std::cmp::Reverse(e.0.updated_at)),
    }
    Ok(out)
}
