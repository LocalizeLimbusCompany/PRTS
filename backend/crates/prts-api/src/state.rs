//! 应用共享状态。

use std::sync::Arc;

use prts_common::config::Settings;
use prts_db::{Cache, Db};

/// 注入到各处理器的共享状态。所有字段均廉价可 clone（连接池 / 连接管理器内部为 Arc）。
#[derive(Clone)]
pub struct AppState {
    /// PostgreSQL 连接池。
    pub db: Db,
    /// Redis 连接管理器。
    pub cache: Cache,
    /// 运行时配置（只读）。P0 暂未读取，供 P1+（JWT/OAuth/SMTP 等）使用。
    #[allow(dead_code)]
    pub settings: Arc<Settings>,
}
