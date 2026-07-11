//! 搜索/向量化运行时配置管理端点（仅平台设置管理员可用）。
//!
//! - `GET  /admin/settings/search`：读取当前搜索配置快照，并返回 Qwen API Key 是否已配置（不下发 key 值）。
//! - `PUT  /admin/settings/search`：写入新配置（规范化后持久化），更新内存快照，返回最新状态。

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_core::permission::nodes;
use prts_db::search_settings::SearchConfig;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

// ============================= DTO =============================

/// 搜索/向量化配置数据传输对象（对应 SearchConfig，含全部可配字段）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchConfigDto {
    /// 是否启用向量 embedding（需 Qwen API Key 已配置）。
    pub embedding_enabled: bool,
    /// 向量模型名称，如 `text-embedding-v4`。
    pub embedding_model: String,
    /// Embedding 接口基础 URL（默认 DashScope 兼容模式）。
    pub embedding_base_url: String,
    /// 每批向量化词条数（范围 1–10，超出会被规范化）。
    pub embedding_batch: i32,
    /// 是否启用翻译记忆（TM）召回。
    pub tm_enabled: bool,
    /// TM 最低相似度阈值（0.0–1.0）。
    pub tm_min_similarity: f64,
    /// TM 最多返回结果数（范围 1–3，超出会被规范化）。
    pub tm_top_n: i32,
}

impl From<SearchConfig> for SearchConfigDto {
    fn from(c: SearchConfig) -> Self {
        Self {
            embedding_enabled: c.embedding_enabled,
            embedding_model: c.embedding_model,
            embedding_base_url: c.embedding_base_url,
            embedding_batch: c.embedding_batch,
            tm_enabled: c.tm_enabled,
            tm_min_similarity: c.tm_min_similarity,
            tm_top_n: c.tm_top_n,
        }
    }
}

impl From<SearchConfigDto> for SearchConfig {
    fn from(d: SearchConfigDto) -> Self {
        Self {
            embedding_enabled: d.embedding_enabled,
            embedding_model: d.embedding_model,
            embedding_base_url: d.embedding_base_url,
            embedding_batch: d.embedding_batch,
            tm_enabled: d.tm_enabled,
            tm_min_similarity: d.tm_min_similarity,
            tm_top_n: d.tm_top_n,
        }
    }
}

/// 搜索设置响应：配置字段 + Qwen API Key 是否已配置（绝不下发 key 值）。
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchSettingsDto {
    #[serde(flatten)]
    #[schema(inline)]
    pub config: SearchConfigDto,
    /// 是否已在 env 配置 Qwen API Key（绝不下发 key 值）。
    pub embedding_key_present: bool,
}

// ============================= 处理器 =============================

/// 读取搜索/向量化运行时配置。
///
/// 返回当前内存快照中的搜索配置，以及 Qwen API Key 是否已通过环境变量配置（不返回 key 值本身）。
/// 需要 `platform.settings` 权限节点（管理员及以上）。
#[utoipa::path(
    get,
    path = "/admin/settings/search",
    tag = "admin",
    responses(
        (status = 200, description = "当前搜索配置", body = SearchSettingsDto),
        (status = 401, description = "未认证"),
        (status = 403, description = "权限不足")
    )
)]
pub async fn get_search_settings(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<SearchSettingsDto>, ApiError> {
    user.require_platform(nodes::PLATFORM_SETTINGS)?;

    let config: SearchConfigDto = state.search_rt.read().await.clone().into();
    let embedding_key_present = state.settings.embedding.qwen.is_configured();

    Ok(Json(SearchSettingsDto {
        config,
        embedding_key_present,
    }))
}

/// 更新搜索/向量化运行时配置。
///
/// 写入新配置（规范化处理危险字段后持久化），同步更新内存快照，返回写入后的最新配置。
/// Qwen API Key 始终保持 env-only，不可通过此接口修改，响应中只返回是否已配置的布尔值。
/// 需要 `platform.settings` 权限节点（管理员及以上）。
#[utoipa::path(
    put,
    path = "/admin/settings/search",
    tag = "admin",
    request_body = SearchConfigDto,
    responses(
        (status = 200, description = "更新后的搜索配置", body = SearchSettingsDto),
        (status = 400, description = "请求体格式错误"),
        (status = 401, description = "未认证"),
        (status = 403, description = "权限不足"),
        (status = 503, description = "审计服务不可用，搜索设置未更新", body = ErrorResponse)
    )
)]
pub async fn put_search_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<SearchConfigDto>,
) -> Result<Json<SearchSettingsDto>, ApiError> {
    user.require_platform(nodes::PLATFORM_SETTINGS)?;
    let updated = state
        .search_settings_updater
        .update(user.id, body.into())
        .await?;
    Ok(Json(SearchSettingsDto {
        config: updated.into(),
        embedding_key_present: state.settings.embedding.qwen.is_configured(),
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn put_search_settings_delegates_to_cancel_safe_worker() {
        let source = include_str!("admin_settings.rs");
        let route = source
            .split_once("pub async fn put_search_settings(")
            .expect("PUT handler exists")
            .1
            .split_once("#[cfg(test)]")
            .expect("PUT handler ends before tests")
            .0;
        assert!(route.contains("search_settings_updater"));
        assert!(!route.contains("state.db.begin"));
        assert!(!route.contains("search_rt.write"));
    }
}
