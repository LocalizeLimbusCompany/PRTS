//! GET /projects/{id}/search — 混合搜索（FTS + pg_trgm + RRF 融合）。
//!
//! 向量召回路径目前关闭（Phase 3 / Task 16 注入），仅执行 FTS + trgm 双路召回。

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use prts_common::Error;
use prts_core::permission::nodes;
use prts_search::orchestrator::{run, OrchestratorInput};
use prts_search::SortBy;

use crate::auth::{project as paccess, MaybeUser};
use crate::db_err;
use crate::error::ApiError;
use crate::routes::entries::EntryDto;
use crate::state::AppState;

// ============================= 搜索查询参数 =============================

// 定向源/译关键字过滤（source_q/translation_q）作为后续"高级筛选"增强项，暂不在本期实现。

/// 混合搜索查询参数。
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SearchQuery {
    /// 主查询词，同时匹配源文与译文。
    pub q: Option<String>,
    /// 限定单个文件 ID（可选）。
    pub file_id: Option<i64>,
    /// 逗号分隔的词条状态过滤（如 `untranslated,translated`）。
    pub state: Option<String>,
    /// 结果排序：`relevance`（默认）| `key` | `updated_at`。
    pub sort: Option<String>,
    /// 结果偏移量（分页用，不超过 200）。
    pub offset: Option<i64>,
    /// 每页条数（1–100，默认 50）。
    pub limit: Option<i64>,
    /// 是否包含隐藏词条；需要项目「编辑」权限。
    #[serde(default)]
    pub include_hidden: bool,
}

// ============================= 搜索命中 DTO =============================

/// 搜索命中：词条全量信息 + 相关度分（RRF 分数）。
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchHitDto {
    /// 词条信息（展开字段）。
    #[serde(flatten)]
    #[schema(inline)]
    pub entry: EntryDto,
    /// RRF 相关度分，值域 (0, 1]，越大越相关。
    pub relevance: f64,
}

// ============================= 搜索处理器 =============================

/// 混合搜索词条（FTS + pg_trgm + RRF 融合，向量路径当前关闭）。
///
/// - 公开项目任意用户可搜索；私有项目需成员资格。
/// - `q` 不能为空，否则返回 400。
/// - `include_hidden = true` 需要项目「编辑」权限，否则静默忽略。
/// - 结果先经 RRF 融合（最多 200 候选），再按 `offset`/`limit` 窗口截取。
#[utoipa::path(
    get,
    path = "/projects/{id}/search",
    tag = "search",
    params(("id" = i64, Path, description = "项目 ID"), SearchQuery),
    responses(
        (status = 200, body = [SearchHitDto], description = "搜索命中列表，按相关度排序"),
        (status = 400, description = "缺少搜索词或参数非法"),
        (status = 404, description = "项目不存在或无访问权限"),
    )
)]
pub async fn search_entries(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHitDto>>, ApiError> {
    // 1. 解析项目访问权限（私有项目对游客返回 404）
    let access = paccess::load(&state, user.as_ref(), id).await?;
    access.require_view()?;

    // 2. 确定主查询词；q 为空则 400
    let main_q = q
        .q
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");

    if main_q.is_empty() {
        return Err(Error::bad_request("search requires a non-empty `q`").into());
    }

    // 3. 解析状态过滤与 include_hidden 权限门控
    let states = super::parse_states(q.state.as_deref());
    let include_hidden = q.include_hidden && access.has_node(nodes::PROJECT_ENTRY_EDIT);

    // 4. 解析排序方式
    let sort = match q.sort.as_deref() {
        Some("key") => SortBy::Key,
        Some("updated_at") => SortBy::UpdatedAt,
        _ => SortBy::Relevance,
    };

    // 5. 分页参数
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let offset = q.offset.unwrap_or(0).clamp(0, 199);
    let per_path: i64 = 100;
    let top_k: i64 = 200;

    // 6. 从项目结构获取源/目标语言
    let src_lang = access
        .project
        .source_langs
        .first()
        .map(|s| s.as_str())
        .unwrap_or("");
    let tgt_lang = access.project.target_lang.as_str();

    // 7. 文件过滤列表
    let file_ids: Vec<i64> = q.file_id.into_iter().collect();

    // 8. 调用编排器（向量 IDs = None，向量路径当前关闭）
    let results = run(
        &state.db,
        OrchestratorInput {
            project_id: id,
            q: main_q,
            src_lang,
            tgt_lang,
            file_ids: &file_ids,
            states: &states,
            include_hidden,
            per_path,
            top_k,
            sort,
            vector_ids: None,
        },
    )
    .await
    .map_err(db_err)?;

    // 9. 应用 offset/limit 窗口并转换为 DTO
    let window: Vec<SearchHitDto> = results
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|(e, score)| SearchHitDto {
            entry: EntryDto::from(&e),
            relevance: score,
        })
        .collect();

    Ok(Json(window))
}
