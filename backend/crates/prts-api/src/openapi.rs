//! OpenAPI 文档根。
//!
//! 具体 path 由 `utoipa-axum` 的 `OpenApiRouter::routes(routes!(..))` 在装配时注入，
//! 因此此处只声明全局信息与标签。

use utoipa::OpenApi;

/// PRTS API 文档根。
#[derive(OpenApi)]
#[openapi(
    info(
        title = "PRTS API",
        version = env!("CARGO_PKG_VERSION"),
        description = "PRTS · Process-Review-Translation System —— 内部协作 API 文档。",
    ),
    tags(
        (name = "health", description = "存活与就绪探测"),
        (name = "meta", description = "服务元信息"),
    ),
)]
pub struct ApiDoc;
