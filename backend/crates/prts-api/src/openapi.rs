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
        (name = "auth", description = "注册 / 登录 / 刷新 / 登出 / OAuth"),
        (name = "user", description = "用户资料 / 关联账号 / API Key"),
        (name = "admin", description = "平台设置与角色任免"),
    ),
)]
pub struct ApiDoc;
