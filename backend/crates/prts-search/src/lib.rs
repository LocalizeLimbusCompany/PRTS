//! `prts-search` —— 混合搜索（FTS + pg_trgm + pgvector，RRF 融合）。
//!
//! 设计（见 plan §12、docs/architecture.md §3.3）：三路召回并行，RRF 融合排序；
//! 向量化经可插拔 [`EmbeddingProvider`]（默认 Qwen 云 API），不可用时自动降级为 FTS + trgm。
//!
//! P0 仅定义向量化抽象与检索过滤骨架；具体编排与 SQL 见 P4。

pub mod rrf;

use serde::{Deserialize, Serialize};

/// 文本向量化提供方。默认实现调用 Qwen 云 API；可替换为本地模型或其它服务。
///
/// 方法将在 P4 以 `async fn` 落地（批量 embedding）。此处先固定类型契约。
pub trait EmbeddingProvider: Send + Sync {
    /// provider 标识（如 `"qwen"`）。
    fn id(&self) -> &str;
    /// 向量维度（用于建表与 pgvector 索引）。
    fn dimensions(&self) -> usize;
}

/// 检索过滤条件（多条件组合）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    /// 限定文件 id。
    #[serde(default)]
    pub file_ids: Vec<i64>,
    /// 限定目录 id。
    #[serde(default)]
    pub folder_ids: Vec<i64>,
    /// 限定词条状态（线上线标识，如 `"untranslated"`）。
    #[serde(default)]
    pub states: Vec<String>,
}

/// 排序方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    /// 相关度（默认）。
    #[default]
    Relevance,
    /// 按键名。
    Key,
    /// 按更新时间。
    UpdatedAt,
}

/// 融合后的命中：entry id + 相关度分（RRF）。
#[derive(Debug, Clone, Copy)]
pub struct SearchHit {
    pub id: i64,
    pub score: f64,
}
