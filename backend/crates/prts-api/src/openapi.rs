//! OpenAPI 文档根。
//!
//! 具体 path 由 `utoipa-axum` 的 `OpenApiRouter::routes(routes!(..))` 在装配时注入，
//! 因此此处只声明全局信息与标签。

use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some(
                        "Access token from the login, refresh, or OAuth flow. Send as `Authorization: Bearer <token>`."
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "api_key_auth",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                "Authorization",
                "Personal PRTS API key. Send as `Authorization: Bearer prts_...`; plaintext is shown only once when created.",
            ))),
        );
    }
}

/// PRTS API 文档根。
#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    security(
        (),
        ("bearer_auth" = []),
        ("api_key_auth" = [])
    ),
    info(
        title = "PRTS API",
        version = env!("CARGO_PKG_VERSION"),
        description = "PRTS · Process-Review-Translation System public API. The full open-source API contract is exposed without role-based document trimming; each operation still enforces its runtime authorization and project visibility rules.",
    ),
    servers(
        (url = "/api", description = "Same-origin public API through the bundled nginx deployment"),
        (url = "/", description = "Direct prts-api service, including local development on port 3000")
    ),
    tags(
        (name = "health", description = "存活与就绪探测"),
        (name = "meta", description = "服务元信息"),
        (name = "auth", description = "注册 / 登录 / 刷新 / 登出 / OAuth"),
        (name = "user", description = "用户资料 / 关联账号 / API Key"),
        (name = "admin", description = "平台设置与角色任免"),
        (name = "project", description = "项目与成员"),
        (name = "job", description = "持久化任务进度与受控重试"),
        (name = "file", description = "文件夹 / 文件树"),
        (name = "file-history", description = "文件变更集、恢复与回滚"),
        (name = "task", description = "项目任务、snapshot baseline 与物化进度"),
        (name = "term", description = "source-aware 项目术语、键集列表、匹配、预览确认导入与 mixed 导出"),
        (name = "pos", description = "双语词性预设读取、平台管理员维护与预览确认导入导出"),
        (name = "upload", description = "流式上传批次、传输尝试与清理"),
        (name = "entry", description = "上传 / 词条 / 历史 / 导出"),
        (name = "entry-comment", description = "词条评论、项目级可见性策略与内容治理"),
        (name = "search", description = "混合搜索（FTS + pg_trgm + RRF 融合）"),
        (name = "notification", description = "通知（收件人自助列表/已读 + poke 发送）"),
        (name = "message", description = "私信（会话列表/会话/发送/已读/未读数，共享项目门限）"),
    ),
)]
pub struct ApiDoc;
