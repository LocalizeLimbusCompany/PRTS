//! 统一错误类型。
//!
//! 业务/基础设施层返回 [`Error`]，由 `prts-api` 在边界处映射为
//! HTTP 状态码 + `{ code, message }`（消息经 [`crate::i18n`] 本地化）。

use std::borrow::Cow;

/// 全局结果别名。
pub type Result<T> = std::result::Result<T, Error>;

/// PRTS 统一错误。
///
/// `code` 字段（见 [`Error::code`]）是稳定的机器可读错误码，前端据此做本地化与分支处理。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 请求参数 / 输入校验失败。
    #[error("bad request: {0}")]
    BadRequest(Cow<'static, str>),

    /// 未认证。
    #[error("unauthorized")]
    Unauthorized,

    /// 已认证但无权限（权限节点校验失败）。
    #[error("forbidden")]
    Forbidden,

    /// 资源不存在。
    #[error("not found")]
    NotFound,

    /// 乐观锁版本冲突。
    #[error("version conflict")]
    Conflict,

    /// 审计记录无法持久化；所有受审计操作必须 fail closed。
    #[error("audit unavailable")]
    AuditUnavailable,

    /// 数据库错误。
    #[error("database error")]
    Database(#[source] sqlx_error::SqlxError),

    /// 其它内部错误。
    #[error("internal error: {0}")]
    Internal(Cow<'static, str>),
}

impl Error {
    /// 稳定错误码（用于 i18n 与前端分支）。
    pub fn code(&self) -> &'static str {
        match self {
            Error::BadRequest(_) => "bad_request",
            Error::Unauthorized => "unauthorized",
            Error::Forbidden => "forbidden",
            Error::NotFound => "not_found",
            Error::Conflict => "conflict",
            Error::AuditUnavailable => "AUDIT_UNAVAILABLE",
            Error::Database(_) => "internal",
            Error::Internal(_) => "internal",
        }
    }

    /// 便捷构造内部错误。
    pub fn internal(msg: impl Into<Cow<'static, str>>) -> Self {
        Error::Internal(msg.into())
    }

    /// 便捷构造参数错误。
    pub fn bad_request(msg: impl Into<Cow<'static, str>>) -> Self {
        Error::BadRequest(msg.into())
    }
}

/// 将 `sqlx::Error` 桥接进来而不让 `prts-common` 直接依赖 sqlx 的全部特性。
/// 这里用一个薄包装，便于上层用 `?` 传播。
pub mod sqlx_error {
    /// 占位包装类型：实际工程中 `prts-db` 会把 `sqlx::Error` 转成 [`crate::Error`]。
    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    pub struct SqlxError(pub String);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(Error::NotFound.code(), "not_found");
        assert_eq!(Error::Unauthorized.code(), "unauthorized");
        assert_eq!(Error::bad_request("x").code(), "bad_request");
        assert_eq!(Error::Conflict.code(), "conflict");
        assert_eq!(Error::AuditUnavailable.code(), "AUDIT_UNAVAILABLE");
    }
}
