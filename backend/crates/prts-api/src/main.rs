//! PRTS API 服务入口。
//!
//! 启动流程：加载 `.env`（开发） → 初始化日志 → 加载分层配置 → 连接 PostgreSQL/Redis →
//! 执行迁移 → bootstrap 管理员 → 装配路由 → 监听并优雅停机。

mod appsettings;
mod auth;
mod dto;
mod error;
mod openapi;
mod routes;
mod state;

use std::sync::Arc;

use prts_common::config::Settings;
use state::AppState;

/// 将数据库错误映射为统一的内部错误响应。
pub(crate) fn db_err(e: prts_db::DbError) -> error::ApiError {
    prts_common::Error::internal(format!("db error: {e}")).into()
}

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

    // bootstrap：把配置的用户名提升为 super_admin（若已存在且尚无平台角色）。
    bootstrap_admin(&db, &settings).await;

    let zoot = build_zoot_provider(&settings);
    if zoot.is_some() {
        tracing::info!("ZOOT OAuth provider enabled");
    }

    let addr = settings.server.addr();
    let state = AppState {
        db,
        cache,
        settings: Arc::new(settings),
        zoot: Arc::new(zoot),
    };
    let app = routes::app(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}  (Swagger UI: /swagger-ui)");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// 依配置构造 ZOOT OAuth provider（未配置则 None）。
fn build_zoot_provider(settings: &Settings) -> Option<prts_auth::OAuth2Provider> {
    let z = &settings.auth.zoot;
    if !z.is_configured() {
        return None;
    }
    let base = settings.auth.public_base_url.trim_end_matches('/');
    Some(prts_auth::OAuth2Provider::new(prts_auth::OAuth2Config {
        provider_id: "zoot".to_string(),
        client_id: z.client_id.clone(),
        client_secret: z.client_secret.clone(),
        authorize_url: z.authorize_url.clone(),
        token_url: z.token_url.clone(),
        userinfo_url: z.userinfo_url.clone(),
        redirect_uri: format!("{base}/api/auth/oauth/zoot/callback"),
        scopes: z.scopes.clone(),
    }))
}

/// 启动期 bootstrap 管理员。
async fn bootstrap_admin(db: &prts_db::Db, settings: &Settings) {
    let target = settings.auth.bootstrap_admin.trim();
    if target.is_empty() {
        return;
    }
    match prts_db::users::find_by_username(db, target).await {
        Ok(Some(u)) if u.platform_role.is_none() => {
            if let Err(e) = prts_db::users::set_platform_role(db, u.id, Some("super_admin")).await {
                tracing::warn!("bootstrap admin failed: {e}");
            } else {
                tracing::info!(user = %u.username, "granted super_admin to bootstrap admin");
            }
        }
        _ => {}
    }
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
