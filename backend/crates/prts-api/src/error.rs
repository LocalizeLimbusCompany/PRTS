//! HTTP 边界错误映射。
//!
//! 把 [`prts_common::Error`] 映射为 HTTP 状态码 + JSON `{ code, message }`，
//! 其中 `message` 经 [`prts_common::i18n`] 按请求语言本地化。

use axum::extract::FromRequestParts;
use axum::extract::Request;
use axum::http::{request::Parts, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use prts_common::i18n::{localize, Locale};
use prts_common::Error;
use serde::Serialize;
use utoipa::ToSchema;

/// API 错误响应包装：携带底层错误与目标语言。
pub struct ApiError {
    error: Error,
    locale: Locale,
    job_id: Option<i64>,
}

impl From<Error> for ApiError {
    /// 默认使用 zh-CN；需要随请求语言时用 [`ApiError::new`]。
    fn from(error: Error) -> Self {
        Self {
            error,
            locale: Locale::default(),
            job_id: None,
        }
    }
}

impl ApiError {
    /// 稳定错误码，供同一 HTTP 边界内决定可否安全降级。
    pub(crate) fn code(&self) -> &'static str {
        self.error.code()
    }

    /// 在路由边界覆盖 locale，保留稳定错误码。
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// 为持久化阶段错误附加可安全公开的 job 引用，不附带内部错误文本。
    pub fn with_job_id(mut self, job_id: Option<i64>) -> Self {
        self.job_id = job_id;
        self
    }
}

/// 共享错误响应体；所有 OpenAPI error response 复用该 schema。
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// 稳定错误码（前端据此分支与本地化）。
    pub code: String,
    /// 已本地化的人类可读消息。
    pub message: String,
    /// 可重试/查看进度的持久化 job；普通错误省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<i64>,
}

/// 传递给 route-boundary middleware 的稳定错误码。
///
/// 响应扩展不会写入网络；它只避免 middleware 反序列化已经构造好的 JSON body。
#[derive(Debug, Clone, Copy)]
struct ApiErrorCode(&'static str);

/// 从 `Accept-Language` 提取请求语言，并写入 extensions 供鉴权 extractor 复用。
#[derive(Debug, Clone, Copy)]
pub struct RequestLocale(pub Locale);

impl<S> FromRequestParts<S> for RequestLocale
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let locale = parts
            .headers
            .get(axum::http::header::ACCEPT_LANGUAGE)
            .and_then(|value| value.to_str().ok())
            .map(Locale::from_accept_language)
            .unwrap_or_default();
        parts.extensions.insert(locale);
        Ok(Self(locale))
    }
}

fn status_for(code: &str) -> StatusCode {
    match code {
        "bad_request" => StatusCode::BAD_REQUEST,
        "SEARCH_REQUEST_INVALID"
        | "SEARCH_LIMIT_INVALID"
        | "SEARCH_CONDITION_FIELD_INVALID"
        | "SEARCH_SOURCE_LANGUAGE_INVALID"
        | "SEARCH_SOURCE_LANGUAGE_NOT_IN_PROJECT"
        | "SEARCH_PATH_INVALID"
        | "SEARCH_SCOPE_RESOURCE_INVALID"
        | "SEARCH_SCOPE_AMBIGUOUS"
        | "SEARCH_CURSOR_INVALID"
        | "ADMIN_USER_CURSOR_INVALID"
        | "PROJECT_MEMBER_ROLE_INVALID"
        | "PROJECT_OWNER_TRANSFER_FORBIDDEN"
        | "PROJECT_DELETE_CHALLENGE_INVALID" => StatusCode::BAD_REQUEST,
        "unauthorized" => StatusCode::UNAUTHORIZED,
        "forbidden" => StatusCode::FORBIDDEN,
        "not_found" => StatusCode::NOT_FOUND,
        "conflict" => StatusCode::CONFLICT,
        "AUDIT_UNAVAILABLE" => StatusCode::SERVICE_UNAVAILABLE,
        "INVALID_LANGUAGE_TAG" | "DUPLICATE_LANGUAGE_TAG" => StatusCode::BAD_REQUEST,
        "PROJECT_LANGUAGE_RESOLUTION_REQUIRED" => StatusCode::CONFLICT,
        "PROJECT_SEARCH_REBUILDING" | "PROJECT_SEARCH_FAILED" => StatusCode::CONFLICT,
        "TERM_ACTIVE_SOURCE_MISMATCH" | "POS_NAME_REQUIRED" => StatusCode::BAD_REQUEST,
        "TERM_DUPLICATE" | "POS_NAME_DUPLICATE" | "POS_IN_USE" | "PROJECT_PENDING_DELETION" => {
            StatusCode::CONFLICT
        }
        "IMPORT_FORMAT_INVALID" | "IMPORT_DUPLICATE_ROW" | "IMPORT_POS_AMBIGUOUS" => {
            StatusCode::BAD_REQUEST
        }
        "IMPORT_PREVIEW_TOKEN_INVALID" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = self.error.code();
        let body = ErrorResponse {
            code: code.to_string(),
            message: localize(code, self.locale).to_string(),
            job_id: self.job_id,
        };
        let mut response = (status_for(code), Json(body)).into_response();
        response.extensions_mut().insert(ApiErrorCode(code));
        response
    }
}

/// 在 HTTP route 边界本地化 Task 1.2 的 fail-closed 审计错误。
///
/// 现有 handler 仍可直接被 DB 合同测试调用；真实 HTTP 请求则在这里统一读取
/// `Accept-Language`。同时把 locale 放进 request extensions，让认证 extractor 无需
/// 重复解析 header。除 `AUDIT_UNAVAILABLE` 外不重写其它 handler 响应。
pub async fn localize_audit_errors(mut request: Request, next: Next) -> Response {
    let locale = request
        .headers()
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .map(Locale::from_accept_language)
        .unwrap_or_default();
    request.extensions_mut().insert(locale);

    let response = next.run(request).await;
    let is_audit_unavailable = response
        .extensions()
        .get::<ApiErrorCode>()
        .is_some_and(|code| code.0 == "AUDIT_UNAVAILABLE");
    if !is_audit_unavailable {
        return response;
    }

    let mut localized = (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            code: "AUDIT_UNAVAILABLE".to_string(),
            message: localize("AUDIT_UNAVAILABLE", locale).to_string(),
            job_id: None,
        }),
    )
        .into_response();
    localized
        .extensions_mut()
        .insert(ApiErrorCode("AUDIT_UNAVAILABLE"));
    localized
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[test]
    fn maps_codes_to_status() {
        assert_eq!(status_for("not_found"), StatusCode::NOT_FOUND);
        assert_eq!(status_for("conflict"), StatusCode::CONFLICT);
        assert_eq!(status_for("SEARCH_CURSOR_INVALID"), StatusCode::BAD_REQUEST);
        assert_eq!(
            status_for("PROJECT_SEARCH_REBUILDING"),
            StatusCode::CONFLICT
        );
        assert_eq!(
            status_for("AUDIT_UNAVAILABLE"),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(status_for("whatever"), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// 审计失败是本任务新增的 fail-closed 边界，必须遵循请求语言而非固定中文。
    #[tokio::test]
    async fn audit_unavailable_uses_accept_language_at_route_boundary() {
        async fn unavailable() -> ApiError {
            Error::AuditUnavailable.into()
        }

        let response = Router::new()
            .route("/", get(unavailable))
            .layer(axum::middleware::from_fn(localize_audit_errors))
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "AUDIT_UNAVAILABLE");
        assert_eq!(body["message"], "Audit service unavailable");
    }
}
