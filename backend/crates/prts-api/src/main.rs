//! PRTS API 服务入口。
//!
//! 启动流程：加载 `.env`（开发） → 初始化日志 → 加载分层配置 → 连接 PostgreSQL/Redis →
//! 执行迁移 → 装配路由 → 监听并优雅停机。

mod error;
mod openapi;
mod routes;
mod state;

use std::sync::Arc;

use prts_common::config::Settings;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 开发环境从 .env 读取；生产环境使用真实环境变量。
    let _ = dotenvy::dotenv();
    prts_common::logging::init();

    let settings = Settings::load()?;
    tracing::info!(addr = %settings.server.addr(), "starting PRTS API");

    // 连接依赖。P0：失败即快速退出并给出清晰错误。
    let db = prts_db::connect_postgres(&settings.database.url, settings.database.max_connections)
        .await
        .map_err(|e| anyhow::anyhow!("connect postgres failed: {e}"))?;
    let cache = prts_db::connect_redis(&settings.redis.url)
        .await
        .map_err(|e| anyhow::anyhow!("connect redis failed: {e}"))?;

    // 执行嵌入式迁移。
    prts_db::run_migrations(&db)
        .await
        .map_err(|e| anyhow::anyhow!("run migrations failed: {e}"))?;

    let addr = settings.server.addr();
    let state = AppState {
        db,
        cache,
        settings: Arc::new(settings),
    };
    let app = routes::app(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}  (Swagger UI: /swagger-ui)");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// 等待 Ctrl+C 或（类 Unix 上）SIGTERM，用于优雅停机。
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
