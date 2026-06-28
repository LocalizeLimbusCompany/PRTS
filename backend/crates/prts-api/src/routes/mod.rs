//! 路由装配。

pub mod health;
pub mod meta;

use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;

use crate::error::ApiError;
use crate::openapi::ApiDoc;
use crate::state::AppState;

/// 装配完整应用路由（含状态与中间件）。
///
/// 端点经 `utoipa-axum` 注册，既挂载到 axum，也写入 OpenAPI 文档；
/// Swagger UI 挂在 `/swagger-ui`，OpenAPI JSON 在 `/api-docs/openapi.json`。
pub fn app(state: AppState) -> Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health::liveness))
        .routes(routes!(health::readiness))
        .routes(routes!(meta::version))
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", api))
        .fallback(handler_404)
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
}
