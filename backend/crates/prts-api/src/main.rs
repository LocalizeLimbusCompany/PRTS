//! PRTS API 服务入口。
//!
//! 启动流程：加载 `.env`（开发） → 初始化日志 → 加载分层配置 → 连接 PostgreSQL/Redis →
//! runtime 角色校验 → bootstrap 管理员 → 装配路由 → 监听并优雅停机。
//! 数据库迁移由独立的 `prts-api migrate` 进程使用 migration owner 执行。

mod appsettings;
mod auth;
mod dto;
mod embed_worker;
mod error;
mod job_retry;
mod job_worker;
mod jobs;
mod media;
mod openapi;
mod routes;
mod search_settings_worker;
mod state;
mod term_import;

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

    match std::env::args().nth(1).as_deref() {
        Some("migrate") => return run_migration_command(&settings).await,
        Some(other) => anyhow::bail!("unknown command: {other}"),
        None => {}
    }
    if settings
        .database
        .migration_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty())
    {
        anyhow::bail!("runtime process must not receive database.migration_url");
    }
    tracing::info!(addr = %settings.server.addr(), "starting PRTS API");

    // 连接依赖。P0：失败即快速退出并给出清晰错误。
    let db = prts_db::connect_postgres(&settings.database.url, settings.database.max_connections)
        .await
        .map_err(|e| anyhow::anyhow!("connect postgres failed: {e}"))?;
    prts_db::verify_runtime_role(&db, &settings.database.runtime_role)
        .await
        .map_err(|e| anyhow::anyhow!("runtime database role verification failed: {e}"))?;
    let cache = prts_db::connect_redis(&settings.redis.url)
        .await
        .map_err(|e| anyhow::anyhow!("connect redis failed: {e}"))?;

    // DB 是会话权威；后台 outbox 只负责把 committed state 收敛到 Redis cache。
    let mut auth_outbox_worker =
        crate::auth::session::spawn_outbox_worker(db.clone(), cache.clone());

    // bootstrap：把配置的用户名提升为 super_admin（若已存在且尚无平台角色）。
    bootstrap_admin(&db, &settings).await;

    #[cfg(feature = "zoot-oauth")]
    let zoot = build_zoot_provider(&settings);
    #[cfg(feature = "zoot-oauth")]
    if zoot.is_some() {
        tracing::info!("ZOOT OAuth provider enabled");
    }

    // 向量化 provider：仅当 env 配了 key 才构造（决定 Some/None）。
    let embedder = std::sync::Arc::new(if settings.embedding.qwen.is_configured() {
        Some(prts_search::qwen::QwenProvider::new(
            settings.embedding.qwen.api_key.clone(),
            settings.embedding.qwen.dimensions,
        ))
    } else {
        None
    });
    // 搜索运行时配置（从 settings 表加载，缺省默认）。
    let search_cfg = prts_db::search_settings::get(&db).await.unwrap_or_default();
    let search_rt = std::sync::Arc::new(tokio::sync::RwLock::new(search_cfg));
    let (search_settings_updater, mut search_settings_worker) =
        crate::search_settings_worker::spawn(db.clone(), search_rt.clone());
    let search_settings_shutdown = search_settings_updater.clone();

    // 实时协作 hub（启动 Redis 订阅中继）。
    let realtime = prts_realtime::Hub::new(&settings.redis.url)
        .await
        .map_err(|e| anyhow::anyhow!("realtime hub init failed: {e}"))?;

    // 启动后台嵌入 sweep（clones cheap: pool 引用计数，Arc 指针）。
    crate::embed_worker::spawn(db.clone(), embedder.clone(), search_rt.clone());

    let media: Arc<dyn crate::media::MediaStore> = Arc::new(crate::media::LocalMediaStore::new(
        settings.media.directory.clone(),
    ));
    // Foundation handlers：legacy 语言 repair 与主源两阶段重建均由同一可恢复 worker 领取。
    let job_registry = crate::jobs::JobRegistry::new(vec![
        Arc::new(crate::jobs::repair_languages::RepairLanguagesHandler::new(
            db.clone(),
        )),
        Arc::new(crate::jobs::reindex_project::ReindexProjectHandler::new(
            db.clone(),
        )),
        Arc::new(crate::jobs::reindex_project::EmbeddingBackfillHandler::new(
            db.clone(),
            embedder.clone(),
            search_rt.clone(),
        )),
        Arc::new(crate::jobs::cleanup_uploads::CleanupUploadsHandler::new(
            db.clone(),
            settings.media.upload_temp_directory.clone(),
        )),
        Arc::new(crate::jobs::process_upload::ProcessUploadHandler::new(
            db.clone(),
            settings.media.upload_temp_directory.clone(),
        )),
        Arc::new(crate::jobs::purge_deleted_files::PurgeDeletedFilesHandler::new(db.clone())),
        Arc::new(crate::jobs::purge_project::PurgeProjectHandler::new(
            db.clone(),
            media.clone(),
            settings.media.upload_temp_directory.clone().into(),
        )),
    ]);
    prts_db::jobs::ensure_file_retention_cleanup(&db, chrono::Utc::now())
        .await
        .map_err(|error| anyhow::anyhow!("schedule file retention cleanup failed: {error}"))?;
    sqlx::query(
        "UPDATE workspace_foundation_state
         SET lexical_worker_registered = TRUE, file_history_writer_ready = TRUE,
             updated_at = now()
         WHERE singleton",
    )
    .execute(&db)
    .await
    .map_err(|error| anyhow::anyhow!("workspace foundation readiness update failed: {error}"))?;
    let (job_worker, mut job_worker_task) = crate::job_worker::spawn(
        db.clone(),
        job_registry.clone(),
        Arc::new(crate::job_worker::DatabasePendingDeletions::new(db.clone())),
    );

    let addr = settings.server.addr();
    let state = AppState {
        db,
        cache,
        settings: Arc::new(settings),
        media,
        #[cfg(feature = "zoot-oauth")]
        zoot: Arc::new(zoot),
        realtime,
        embedder,
        search_rt,
        search_settings_updater,
        job_worker,
    };
    let app = routes::app(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}  (Swagger UI: /swagger-ui)");
    let (server_shutdown_tx, server_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut server_shutdown_tx = Some(server_shutdown_tx);
    let server = std::future::IntoFuture::into_future(
        axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = server_shutdown_rx.await;
        }),
    );
    tokio::pin!(server);

    tokio::select! {
        result = &mut server => {
            cancel_worker("auth outbox", &mut auth_outbox_worker).await;
            cancel_worker("durable job", &mut job_worker_task).await;
            drain_search_settings_worker(
                &search_settings_shutdown,
                &mut search_settings_worker,
            ).await;
            result?;
            Ok(())
        }
        _ = shutdown_signal() => {
            if let Some(sender) = server_shutdown_tx.take() {
                let _ = sender.send(());
            }
            cancel_worker("auth outbox", &mut auth_outbox_worker).await;
            cancel_worker("durable job", &mut job_worker_task).await;
            server.as_mut().await?;
            drain_search_settings_worker(
                &search_settings_shutdown,
                &mut search_settings_worker,
            ).await;
            Ok(())
        }
        outcome = &mut auth_outbox_worker => {
            let worker_error = worker_termination_error("auth outbox", outcome);
            if let Some(sender) = server_shutdown_tx.take() {
                let _ = sender.send(());
            }
            cancel_worker("durable job", &mut job_worker_task).await;
            if let Err(error) = server.as_mut().await {
                tracing::error!(%error, "server shutdown after auth outbox failure failed");
            }
            drain_search_settings_worker(
                &search_settings_shutdown,
                &mut search_settings_worker,
            ).await;
            Err(worker_error)
        }
        outcome = &mut job_worker_task => {
            let worker_error = worker_termination_error("durable job", outcome);
            if let Some(sender) = server_shutdown_tx.take() {
                let _ = sender.send(());
            }
            cancel_worker("auth outbox", &mut auth_outbox_worker).await;
            if let Err(error) = server.as_mut().await {
                tracing::error!(%error, "server shutdown after durable job failure failed");
            }
            drain_search_settings_worker(
                &search_settings_shutdown,
                &mut search_settings_worker,
            ).await;
            Err(worker_error)
        }
        outcome = &mut search_settings_worker => {
            let worker_error = worker_termination_error("search settings", outcome);
            if let Some(sender) = server_shutdown_tx.take() {
                let _ = sender.send(());
            }
            cancel_worker("auth outbox", &mut auth_outbox_worker).await;
            cancel_worker("durable job", &mut job_worker_task).await;
            if let Err(error) = server.as_mut().await {
                tracing::error!(%error, "server shutdown after search settings worker failure failed");
            }
            Err(worker_error)
        }
    }
}

/// 取消仍在运行的 worker 并等待其 JoinHandle 收敛，避免停机时遗留 detached task。
async fn cancel_worker(name: &str, handle: &mut tokio::task::JoinHandle<()>) {
    handle.abort();
    match handle.await {
        Ok(()) => tracing::warn!(
            worker = name,
            "background worker exited before cancellation"
        ),
        Err(error) if error.is_cancelled() => {}
        Err(error) => tracing::error!(worker = name, %error, "background worker join failed"),
    }
}

/// 停止接收新搜索设置更新，并排空所有已入队事务后再完成停机。
async fn drain_search_settings_worker(
    updater: &crate::search_settings_worker::SearchSettingsUpdater,
    handle: &mut tokio::task::JoinHandle<()>,
) {
    updater.shutdown();
    match handle.await {
        Ok(()) => {}
        Err(error) => tracing::error!(
            worker = "search settings",
            %error,
            "background worker join failed"
        ),
    }
}

fn worker_termination_error(
    name: &str,
    outcome: Result<(), tokio::task::JoinError>,
) -> anyhow::Error {
    match outcome {
        Ok(()) => anyhow::anyhow!("{name} worker exited unexpectedly"),
        Err(error) if error.is_panic() => anyhow::anyhow!("{name} worker panicked: {error}"),
        Err(error) => anyhow::anyhow!("{name} worker stopped unexpectedly: {error}"),
    }
}

/// 用独立 migration owner 执行迁移后立即销毁连接；不会构造 AppState 或启动服务。
async fn run_migration_command(settings: &Settings) -> anyhow::Result<()> {
    let migration_url = settings
        .database
        .migration_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("database.migration_url is required for migrate"))?;
    let migration_pool = prts_db::connect_postgres(migration_url, 1)
        .await
        .map_err(|error| anyhow::anyhow!("connect migration postgres failed: {error}"))?;
    let mut connection = migration_pool
        .acquire()
        .await
        .map_err(|error| anyhow::anyhow!("acquire migration connection failed: {error}"))?;
    prts_db::run_migrations(&mut connection, &settings.database.runtime_role)
        .await
        .map_err(|error| anyhow::anyhow!("run migrations failed: {error}"))?;
    drop(connection);
    migration_pool.close().await;
    tracing::info!(runtime_role = %settings.database.runtime_role, "database migrations complete");
    Ok(())
}

/// 依配置构造 ZOOT OAuth provider（未配置则 None）。
#[cfg(feature = "zoot-oauth")]
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
    let result: Result<Option<String>, sqlx::Error> = async {
        let mut tx = db.begin().await?;
        let Some(user) = prts_db::users::find_by_username_for_update_tx(&mut tx, target).await?
        else {
            tx.rollback().await?;
            return Ok(None);
        };
        if user.platform_role.is_some() {
            tx.rollback().await?;
            return Ok(None);
        }
        prts_db::users::set_platform_role_tx(&mut tx, user.id, Some("super_admin")).await?;
        prts_db::audit::append_event_tx(
            &mut tx,
            prts_db::audit::AuditActor {
                id: None,
                kind: prts_db::audit::AuditActorKind::System,
                ip: None,
            },
            prts_db::audit::AuditEvent::AuthBootstrapRoleGranted {
                user_id: user.id,
                role: "super_admin",
            },
        )
        .await?;
        tx.commit().await?;
        Ok(Some(user.username))
    }
    .await;
    match result {
        Ok(Some(username)) => {
            tracing::info!(user = %username, "granted super_admin to bootstrap admin");
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "bootstrap admin failed"),
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

#[cfg(test)]
mod tests {
    #[test]
    fn session_breaking_rollout_is_documented_in_both_languages() {
        let zh = include_str!("../../../../README.md");
        assert!(zh.contains("维护窗口"));
        assert!(zh.contains("现有会话"));
        assert!(zh.contains("重新登录"));

        let en = include_str!("../../../../README.en.md");
        assert!(en.contains("maintenance window"));
        assert!(en.contains("existing sessions"));
        assert!(en.contains("signing in again"));
    }

    #[test]
    fn file_history_writer_readiness_follows_complete_worker_registration() {
        let source = include_str!("main.rs");
        let handler = source
            .find("PurgeDeletedFilesHandler::new")
            .expect("retention handler is registered");
        let durable_schedule = source
            .find("ensure_file_retention_cleanup")
            .expect("retention scan is durably scheduled");
        let readiness = source
            .find("file_history_writer_ready = TRUE")
            .expect("writer cutover marker is enabled");
        assert!(handler < durable_schedule && durable_schedule < readiness);
    }
}
