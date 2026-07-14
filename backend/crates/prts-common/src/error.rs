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

    /// 带稳定机器码的参数/状态校验失败；不得把用户 query/value 放进 code。
    #[error("validation failed: {0}")]
    Validation(&'static str),

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

    /// 语言标签不是合法 canonical BCP-47。
    #[error("invalid language tag")]
    InvalidLanguageTag,

    /// 语言标签规范化后重复。
    #[error("duplicate language tag")]
    DuplicateLanguageTag,

    /// 项目存在需要拥有者处理的语言歧义。
    #[error("project language resolution required")]
    ProjectLanguageResolutionRequired,

    /// active term 的 source_lang 不是项目当前主源。
    #[error("active term source language mismatch")]
    TermActiveSourceMismatch,

    /// canonical term identity 已存在。
    #[error("duplicate term")]
    DuplicateTerm,

    /// POS 至少需要一个本地化名称。
    #[error("pos name required")]
    PosNameRequired,

    /// POS 的中文名或英文名与既有预设冲突。
    #[error("duplicate pos name")]
    DuplicatePosName,

    /// POS 删除会使被引用术语的 NULL-safe identity 冲突。
    #[error("pos is in use")]
    PosInUse,

    /// 项目已经进入待删除只读状态。
    #[error("project pending deletion")]
    ProjectPendingDeletion,

    /// 导入正文不是稳定 CSV/JSON 格式或缺少必填字段。
    #[error("invalid import format")]
    ImportFormatInvalid,

    /// 导入行在 canonical identity 上重复。
    #[error("duplicate import row")]
    ImportDuplicateRow,

    /// preview token 已过期、已消费或绑定校验失败。
    #[error("invalid import preview token")]
    ImportPreviewTokenInvalid,

    /// POS 名称同时匹配多个预设，无法安全解析。
    #[error("ambiguous imported part of speech")]
    ImportPosAmbiguous,

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
            Error::Validation(code) => code,
            Error::Unauthorized => "unauthorized",
            Error::Forbidden => "forbidden",
            Error::NotFound => "not_found",
            Error::Conflict => "conflict",
            Error::AuditUnavailable => "AUDIT_UNAVAILABLE",
            Error::InvalidLanguageTag => "INVALID_LANGUAGE_TAG",
            Error::DuplicateLanguageTag => "DUPLICATE_LANGUAGE_TAG",
            Error::ProjectLanguageResolutionRequired => "PROJECT_LANGUAGE_RESOLUTION_REQUIRED",
            Error::TermActiveSourceMismatch => "TERM_ACTIVE_SOURCE_MISMATCH",
            Error::DuplicateTerm => "TERM_DUPLICATE",
            Error::PosNameRequired => "POS_NAME_REQUIRED",
            Error::DuplicatePosName => "POS_NAME_DUPLICATE",
            Error::PosInUse => "POS_IN_USE",
            Error::ProjectPendingDeletion => "PROJECT_PENDING_DELETION",
            Error::ImportFormatInvalid => "IMPORT_FORMAT_INVALID",
            Error::ImportDuplicateRow => "IMPORT_DUPLICATE_ROW",
            Error::ImportPreviewTokenInvalid => "IMPORT_PREVIEW_TOKEN_INVALID",
            Error::ImportPosAmbiguous => "IMPORT_POS_AMBIGUOUS",
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

    /// 构造不携带正文的稳定校验错误码。
    pub const fn validation(code: &'static str) -> Self {
        Error::Validation(code)
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
        assert_eq!(
            Error::validation("SEARCH_CURSOR_INVALID").code(),
            "SEARCH_CURSOR_INVALID"
        );
        assert_eq!(Error::Conflict.code(), "conflict");
        assert_eq!(Error::AuditUnavailable.code(), "AUDIT_UNAVAILABLE");
    }
}
