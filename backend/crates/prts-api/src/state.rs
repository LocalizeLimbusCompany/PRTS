//! 应用共享状态。

use std::sync::Arc;

use prts_auth::OAuth2Provider;
use prts_common::config::Settings;
use prts_db::{Cache, Db};

/// 注入到各处理器的共享状态。所有字段均廉价可 clone（连接池 / 连接管理器内部为 Arc）。
#[derive(Clone)]
pub struct AppState {
    /// PostgreSQL 连接池。
    pub db: Db,
    /// Redis 连接管理器。
    pub cache: Cache,
    /// 运行时配置（只读）。
    pub settings: Arc<Settings>,
    /// ZOOT OAuth provider（未配置则为 None）。
    pub zoot: Arc<Option<OAuth2Provider>>,
    /// 实时协作 hub（WebSocket + Redis pub/sub）。
    pub realtime: prts_realtime::Hub,
    /// Qwen 向量化 provider（Some 当且仅当 env 配了 api_key）。
    pub embedder: Arc<Option<prts_search::qwen::QwenProvider>>,
    /// 搜索运行时配置（管理后台可热改；worker / 查询每次读快照）。
    pub search_rt: Arc<tokio::sync::RwLock<prts_db::search_settings::SearchConfig>>,
}

impl AppState {
    /// JWT 签名密钥字节。
    pub fn jwt_secret(&self) -> &[u8] {
        self.settings.auth.jwt_secret.as_bytes()
    }

    /// ZOOT provider（未配置则 None）。
    pub fn zoot_provider(&self) -> Option<&OAuth2Provider> {
        (*self.zoot).as_ref()
    }
}
