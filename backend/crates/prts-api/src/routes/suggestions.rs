//! GET /projects/{id}/entries/{entry_id}/suggestions — 跨项目 TM 翻译建议。
//!
//! 仅从「当前用户已加入的项目」中、按源文相似度召回既有译文（≤ `tm_top_n` 条）。
//! 向量开启且当前词条已嵌入时用语义相似度，否则降级为 pg_trgm 源文相似度。

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;

use prts_db::search::SuggestionRow;

use crate::auth::{project as paccess, MaybeUser};
use crate::db_err;
use crate::error::ApiError;
use crate::state::AppState;

/// TM 建议项：来源词条 + 相似度。点击后将 `translation` 填入译文框。
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SuggestionDto {
    /// 来源词条 ID。
    pub entry_id: i64,
    /// 来源项目 ID。
    pub project_id: i64,
    /// 来源项目名（用于展示建议出处）。
    pub project_name: String,
    /// 来源词条的源文本。
    pub source_text: String,
    /// 来源词条的译文（建议填入的内容）。
    pub translation: String,
    /// 来源词条状态。
    pub state: String,
    /// 相似度，值域 (0, 1]，越大越相似。
    pub similarity: f64,
}

impl From<SuggestionRow> for SuggestionDto {
    fn from(r: SuggestionRow) -> Self {
        Self {
            entry_id: r.entry_id,
            project_id: r.project_id,
            project_name: r.project_name,
            source_text: r.source_text,
            translation: r.translation,
            state: r.state,
            similarity: r.similarity,
        }
    }
}

/// 获取某词条的 TM 翻译建议（跨项目，仅限当前用户已加入的项目）。
///
/// - 需要登录；游客或 TM 关闭（`tm_enabled = false`）时返回空列表。
/// - 召回范围：当前用户为成员、且 `target_lang` 与本项目一致的项目里，
///   状态 ≥ 已翻译、译文非空、与当前词条源文相似的词条（排除当前词条自身）。
/// - 向量开启且当前词条已嵌入 → 语义（cosine）相似度；否则 pg_trgm 源文相似度（优雅降级）。
#[utoipa::path(
    get,
    path = "/projects/{id}/entries/{entry_id}/suggestions",
    tag = "search",
    params(
        ("id" = i64, Path, description = "项目 ID"),
        ("entry_id" = i64, Path, description = "词条 ID"),
    ),
    responses(
        (status = 200, body = [SuggestionDto], description = "建议列表，按相似度降序，最多 tm_top_n 条"),
        (status = 404, description = "项目不存在或无访问权限"),
    )
)]
pub async fn entry_suggestions(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((id, entry_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<SuggestionDto>>, ApiError> {
    // 校验对当前项目的访问权限（私有项目对游客返回 404）。
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;

    let cfg = state.search_rt.read().await.clone();

    // 建议依赖「用户已加入的项目」，故需登录；游客或 TM 关闭时无建议。
    let user_id = match (&user, cfg.tm_enabled) {
        (Some(u), true) => u.id,
        _ => return Ok(Json(vec![])),
    };

    // 当前词条的源文本与 embedding；词条不存在则返回空列表。
    let (cur_source, cur_emb) = match prts_db::search::current_search_fields(&state.db, entry_id)
        .await
        .map_err(db_err)?
    {
        Some(fields) => fields,
        None => return Ok(Json(vec![])),
    };

    let tgt = access.project.target_lang.as_str();
    let min_sim = cfg.tm_min_similarity;
    let top_n = cfg.tm_top_n as i64;

    // 向量开启且当前词条已嵌入 → 语义相似度；否则 trgm 源文相似度。
    let rows = if cfg.embedding_enabled {
        match cur_emb {
            Some(emb) => {
                prts_db::search::suggestions_vector(
                    &state.db, user_id, tgt, &emb, entry_id, min_sim, top_n,
                )
                .await
            }
            None => {
                prts_db::search::suggestions_trgm(
                    &state.db,
                    user_id,
                    tgt,
                    &cur_source,
                    entry_id,
                    min_sim,
                    top_n,
                )
                .await
            }
        }
    } else {
        prts_db::search::suggestions_trgm(
            &state.db,
            user_id,
            tgt,
            &cur_source,
            entry_id,
            min_sim,
            top_n,
        )
        .await
    }
    .map_err(db_err)?;

    Ok(Json(rows.into_iter().map(SuggestionDto::from).collect()))
}
