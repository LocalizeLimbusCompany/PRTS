//! 路由装配。

pub mod admin;
pub mod admin_settings;
pub mod auth;
pub mod entries;
pub mod files;
pub mod health;
pub mod jobs;
pub mod language_resolution;
pub mod messages;
pub mod meta;
pub mod notifications;
pub mod projects;
pub mod search;
pub mod suggestions;
pub mod users;
pub mod ws;

use axum::routing::get;
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;

use prts_core::EntryState;

use crate::error::ApiError;
use crate::openapi::ApiDoc;
use crate::state::AppState;

// ============================= 公用工具 =============================

/// 解析逗号分隔的词条状态字符串，过滤非法值，返回合法状态列表。
///
/// 供 `list_entries` 与 `search_entries` 共用，行为保持一致。
pub(crate) fn parse_states(s: Option<&str>) -> Vec<String> {
    s.map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|x| EntryState::parse(x).is_some())
            .map(|x| x.to_string())
            .collect()
    })
    .unwrap_or_default()
}

/// 构建 OpenAPI 路由（注册全部业务端点，不含状态/中间件）。
///
/// 抽出以便单测「路由装配不 panic（无路径/方法重叠）」——`app()` 的路由注册发生在
/// `with_state` 之前，测试无需真实 AppState/DB/Redis 即可装配校验。历史上单测仅覆盖
/// `public_router()`，未装配完整路由，致使「不同路径 handler 误并入一次 `routes!()`」
/// 造成的路由重叠在服务启动时才 panic。
fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health::liveness))
        .routes(routes!(health::readiness))
        .routes(routes!(meta::version))
        // 认证
        .routes(routes!(auth::register))
        .routes(routes!(auth::login))
        .routes(routes!(auth::refresh))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::oauth_start))
        .routes(routes!(auth::oauth_callback))
        // 用户自助
        .routes(routes!(users::me, users::update_me))
        .routes(routes!(users::get_user))
        .routes(routes!(users::my_accounts))
        .routes(routes!(users::create_api_key, users::list_api_keys))
        .routes(routes!(users::revoke_api_key))
        // 平台管理
        .routes(routes!(admin::get_settings, admin::update_settings))
        .routes(routes!(admin::grant_role))
        .routes(routes!(
            admin_settings::get_search_settings,
            admin_settings::put_search_settings
        ))
        // 项目
        .routes(routes!(projects::create_project, projects::list_projects))
        .routes(routes!(
            projects::get_project,
            projects::update_project,
            projects::delete_project
        ))
        .routes(routes!(projects::list_members, projects::add_member))
        .routes(routes!(projects::remove_member))
        .routes(routes!(
            language_resolution::get_project_language_resolution,
            language_resolution::resolve_project_languages
        ))
        .routes(routes!(
            language_resolution::list_admin_language_resolutions
        ))
        .routes(routes!(language_resolution::retry_admin_language_repair))
        // 持久化任务进度与受控重试
        .routes(routes!(jobs::get_job))
        .routes(routes!(jobs::retry_job))
        .routes(routes!(jobs::list_project_jobs))
        // 文件树
        .routes(routes!(files::get_tree))
        .routes(routes!(files::delete_file))
        .routes(routes!(files::delete_folder))
        // 上传 / 词条 / 导出
        .routes(routes!(entries::upload))
        .routes(routes!(entries::list_entries))
        .routes(routes!(entries::get_entry, entries::update_entry))
        .routes(routes!(entries::set_entry_flags))
        .routes(routes!(entries::entry_history))
        .routes(routes!(entries::export_project))
        // 混合搜索
        .routes(routes!(search::search_entries))
        // TM 翻译建议
        .routes(routes!(suggestions::entry_suggestions))
        // 通知（收件人自助 + poke 发送）
        // list 与 unread_count 路径不同（/notifications vs /notifications/unread_count），
        // 必须各自 .routes()：utoipa-axum 的 routes!(a, b) 会把多个 handler 合并到**同一
        // 路径**当作方法路由，两个 GET 同路径会「Overlapping method route」启动即 panic。
        .routes(routes!(notifications::list))
        .routes(routes!(notifications::unread_count))
        .routes(routes!(notifications::mark_read))
        .routes(routes!(notifications::poke))
        // 私信（会话列表 / 会话 / 发送 / 已读 / 未读数）
        .routes(routes!(messages::list_threads, messages::send))
        .routes(routes!(messages::unread_count))
        .routes(routes!(messages::conversation))
        .routes(routes!(messages::mark_read))
}

/// 装配完整应用路由（含状态与中间件）。
///
/// 端点经 `utoipa-axum` 注册，既挂载到 axum，也写入 OpenAPI 文档；
/// Swagger UI 挂在 `/swagger-ui`，OpenAPI JSON 在 `/api-docs/openapi.json`。
pub fn app(state: AppState) -> Router {
    let (router, api) = api_router().split_for_parts();

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api))
        // 实时协作 WebSocket（不在 OpenAPI 文档内）
        .route("/ws/projects/{id}", get(ws::ws_handler))
        // 用户通知流 WebSocket（不在 OpenAPI 文档内）
        .route("/ws/user", get(ws::user_ws_handler))
        .fallback(handler_404)
        // 仅统一 Task 1.2 的审计失败本地化，并向认证 extractor 提供请求 locale。
        .layer(axum::middleware::from_fn(
            crate::error::localize_audit_errors,
        ))
        // P0：CORS 放开便于联调；生产环境按需收紧（见 plan §15）。
        .layer(CorsLayer::permissive())
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// 未匹配路由的兜底，返回统一的 404 错误响应。
async fn handler_404() -> ApiError {
    prts_common::Error::NotFound.into()
}

/// 仅包含「无状态」公共端点的路由，供无 DB/Redis 环境下的单元测试使用。
#[cfg(test)]
fn public_router() -> Router {
    use axum::routing::get;
    Router::new()
        .route("/health", get(health::liveness))
        .route("/version", get(meta::version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // 提供 oneshot

    #[tokio::test]
    async fn health_returns_ok() {
        let resp = public_router()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn version_reports_crate_name() {
        let resp = public_router()
            .oneshot(
                Request::builder()
                    .uri("/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["name"], "prts-api");
    }

    /// 装配全部业务路由，确保无路径/方法重叠导致 `app()` 启动即 panic。
    ///
    /// 路由注册在 `with_state` 之前，故无需真实 AppState/DB/Redis 即可校验。
    /// 补此测试的动因：通知路由曾把 `/notifications` 与 `/notifications/unread_count`
    /// 两个**不同路径** handler 并入一次 `routes!()`（被当作同路径两个 GET →
    /// axum「Overlapping method route」），`public_router()` 未覆盖，致启动时才 panic
    /// 而 CI 未捕获。
    #[test]
    fn full_router_assembles_without_route_conflicts() {
        let _ = api_router().split_for_parts();
    }

    #[test]
    fn jobs_openapi_errors_share_code_message_schema() {
        let (_, api) = api_router().split_for_parts();
        let document = serde_json::to_value(api).unwrap();
        for (path, method, statuses) in [
            ("/jobs/{id}", "get", &["401", "403", "404"][..]),
            (
                "/projects/{project_id}/jobs",
                "get",
                &["400", "401", "403", "404"][..],
            ),
            (
                "/jobs/{id}/retry",
                "post",
                &["400", "401", "403", "404"][..],
            ),
        ] {
            for status in statuses {
                assert_eq!(
                    document["paths"][path][method]["responses"][status]["content"]
                        ["application/json"]["schema"]["$ref"],
                    "#/components/schemas/ErrorResponse",
                    "{method} {path} 的 {status} 应复用稳定错误 schema"
                );
            }
        }
    }

    /// Task 1.2 的认证与受审计 mutation 都公开稳定的 fail-closed 503 schema。
    #[test]
    fn audited_mutations_document_audit_unavailable() {
        let (_, api) = api_router().split_for_parts();
        let document = serde_json::to_value(api).unwrap();
        for (path, method) in [
            ("/auth/register", "post"),
            ("/auth/login", "post"),
            ("/auth/refresh", "post"),
            ("/auth/logout", "post"),
            ("/auth/oauth/{provider}/callback", "get"),
            ("/me", "put"),
            ("/me/api-keys", "post"),
            ("/me/api-keys/{id}", "delete"),
            ("/admin/settings", "put"),
            ("/admin/users/{id}/role", "post"),
            ("/admin/settings/search", "put"),
            ("/projects", "post"),
            ("/projects/{id}", "put"),
            ("/projects/{id}", "delete"),
            ("/projects/{id}/members", "post"),
            ("/projects/{id}/members/{user_id}", "delete"),
            ("/projects/{id}/files/{file_id}", "delete"),
            ("/projects/{id}/folders/{folder_id}", "delete"),
            ("/projects/{id}/upload", "post"),
            ("/projects/{id}/entries/{entry_id}", "put"),
            ("/projects/{id}/entries/{entry_id}/flags", "patch"),
            ("/projects/{id}/export", "get"),
            ("/notifications/read", "post"),
            ("/projects/{id}/poke", "post"),
            ("/messages", "post"),
            ("/messages/{user_id}/read", "post"),
            ("/jobs/{id}/retry", "post"),
        ] {
            assert_eq!(
                document["paths"][path][method]["responses"]["503"]["content"]["application/json"]
                    ["schema"]["$ref"],
                "#/components/schemas/ErrorResponse",
                "{method} {path} 应公开 AUDIT_UNAVAILABLE 的稳定错误 schema"
            );
        }
    }
}
