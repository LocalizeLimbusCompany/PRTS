//! 搜索/向量化运行时配置（存 settings 表，管理后台可改）。serde-only（不耦合 utoipa）。
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub embedding_enabled: bool,
    pub embedding_model: String,
    pub embedding_base_url: String,
    pub embedding_batch: i32,
    pub tm_enabled: bool,
    pub tm_min_similarity: f64,
    pub tm_top_n: i32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            embedding_enabled: false,
            embedding_model: "text-embedding-v4".into(),
            embedding_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            embedding_batch: 10,
            tm_enabled: true,
            tm_min_similarity: 0.30,
            tm_top_n: 3,
        }
    }
}

const KEY: &str = "search.config";

/// 规范化：clamp 危险字段到安全区间（纯函数，便于单测）。
pub fn normalize(mut cfg: SearchConfig) -> SearchConfig {
    cfg.embedding_batch = cfg.embedding_batch.clamp(1, 10);
    cfg.tm_top_n = cfg.tm_top_n.clamp(1, 3);
    cfg.tm_min_similarity = cfg.tm_min_similarity.clamp(0.0, 1.0);
    cfg
}

/// 读取（缺失返回默认）。
pub async fn get(pool: &PgPool) -> Result<SearchConfig, sqlx::Error> {
    match crate::settings::get(pool, KEY).await? {
        Some(v) => Ok(serde_json::from_value(v).unwrap_or_default()),
        None => Ok(SearchConfig::default()),
    }
}

/// 写入（规范化后）。
pub async fn set(pool: &PgPool, cfg: SearchConfig, by: Option<i64>) -> Result<(), sqlx::Error> {
    let cfg = normalize(cfg);
    let v = serde_json::to_value(&cfg).expect("SearchConfig serializes");
    crate::settings::set(pool, KEY, &v, by).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_clamps_dangerous_fields() {
        let n = normalize(SearchConfig {
            embedding_batch: 99,
            tm_top_n: 9,
            tm_min_similarity: 2.0,
            ..Default::default()
        });
        assert_eq!(n.embedding_batch, 10);
        assert_eq!(n.tm_top_n, 3);
        assert!((n.tm_min_similarity - 1.0).abs() < 1e-9);
    }
}
