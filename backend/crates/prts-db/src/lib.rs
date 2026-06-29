//! `prts-db` —— 数据访问基础设施。
//!
//! 提供 PostgreSQL 连接池、Redis 连接管理器、迁移执行与健康探测，
//! 以及账号 / API Key / 设置等数据访问（[`models`] + 各仓储模块）。

pub mod api_keys;
pub mod entries;
pub mod files;
pub mod memberships;
pub mod models;
pub mod projects;
pub mod search;
pub mod settings;
pub mod users;

/// 数据库错误别名，便于上层映射而无需直接依赖 sqlx。
pub use sqlx::Error as DbError;

use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// PostgreSQL 连接池别名。
pub type Db = PgPool;
/// Redis 连接管理器别名（内部多路复用、自动重连、可廉价 clone）。
pub type Cache = ConnectionManager;

/// 创建 PostgreSQL 连接池。
pub async fn connect_postgres(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(url)
        .await
}

/// 创建 Redis 连接管理器。
pub async fn connect_redis(url: &str) -> Result<ConnectionManager, redis::RedisError> {
    let client = redis::Client::open(url)?;
    ConnectionManager::new(client).await
}

/// 运行嵌入式迁移（编译期从 `backend/migrations` 嵌入）。
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}

/// 探测 PostgreSQL 可用性（就绪检查）。
pub async fn ping_postgres(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

/// 探测 Redis 可用性（就绪检查）。
pub async fn ping_redis(cache: &ConnectionManager) -> Result<(), redis::RedisError> {
    let mut conn = cache.clone();
    let _: String = redis::cmd("PING").query_async(&mut conn).await?;
    Ok(())
}
