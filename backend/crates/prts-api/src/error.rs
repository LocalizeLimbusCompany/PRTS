//! HTTP 边界错误映射。
//!
//! 把 [`prts_common::Error`] 映射为 HTTP 状态码 + JSON `{ code, message }`，
//! 其中 `message` 经 [`prts_common::i18n`] 按请求语言本地化。

use axum::extract::FromRequestParts;
use axum::http::{request::Parts, StatusCode};
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
}

impl From<Error> for ApiError {
    /// 默认使用 zh-CN；需要随请求语言时用 [`ApiError::new`]。
    fn from(error: Error) -> Self {
        Self {
            error,
            locale: Locale::default(),
        }
    }
}

impl ApiError {
    /// 在路由边界覆盖 locale，保留稳定错误码。
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
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
}

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
        "unauthorized" => StatusCode::UNAUTHORIZED,
        "forbidden" => StatusCode::FORBIDDEN,
        "not_found" => StatusCode::NOT_FOUND,
        "conflict" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = self.error.code();
        let body = ErrorResponse {
            code: code.to_string(),
            message: localize(code, self.locale).to_string(),
        };
        (status_for(code), Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_codes_to_status() {
        assert_eq!(status_for("not_found"), StatusCode::NOT_FOUND);
        assert_eq!(status_for("conflict"), StatusCode::CONFLICT);
        assert_eq!(status_for("whatever"), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
