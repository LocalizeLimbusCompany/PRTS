//! HTTP 边界错误映射。
//!
//! 把 [`prts_common::Error`] 映射为 HTTP 状态码 + JSON `{ code, message }`，
//! 其中 `message` 经 [`prts_common::i18n`] 按请求语言本地化。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use prts_common::i18n::{localize, Locale};
use prts_common::Error;
use serde::Serialize;

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

/// 线上线错误响应体。
#[derive(Serialize)]
struct ErrorBody {
    /// 稳定错误码（前端据此分支与本地化）。
    code: &'static str,
    /// 已本地化的人类可读消息。
    message: &'static str,
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
        let body = ErrorBody {
            code,
            message: localize(code, self.locale),
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
