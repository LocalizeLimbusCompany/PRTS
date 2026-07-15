//! 轻量国际化（后端消息本地化）。
//!
//! P0 采用内置静态消息表，覆盖错误码 → {zh-CN, en}。后续可平滑替换为
//! Fluent / rust-i18n 等方案，对外接口（[`localize`]、[`Locale`]）保持不变。
//!
//! 用法：在 API 边界从 `Accept-Language` 解析 [`Locale`]，再用错误码取本地化消息。

/// 受支持的界面语言。新增语言只需扩展此枚举与 [`message_for`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    /// 简体中文（默认）。
    #[default]
    ZhCn,
    /// 英语。
    En,
}

impl Locale {
    /// 从 HTTP `Accept-Language` 头解析首选语言。无法识别时回退到默认（zh-CN）。
    pub fn from_accept_language(header: &str) -> Self {
        // 仅做前缀匹配即可满足需求：取第一个能识别的语言标签。
        for part in header.split(',') {
            let tag = part
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if tag.starts_with("zh") {
                return Locale::ZhCn;
            }
            if tag.starts_with("en") {
                return Locale::En;
            }
        }
        Locale::default()
    }
}

/// 取错误码在指定语言下的消息；未知错误码回退到通用内部错误消息。
pub fn localize(code: &str, locale: Locale) -> &'static str {
    message_for(code, locale).unwrap_or_else(|| message_for("internal", locale).unwrap())
}

/// 静态消息表。`None` 表示该错误码无专属消息。
fn message_for(code: &str, locale: Locale) -> Option<&'static str> {
    let msg = match (code, locale) {
        ("bad_request", Locale::ZhCn) => "请求参数有误",
        ("bad_request", Locale::En) => "Bad request",
        ("AUTH_PASSWORD_DISABLED", Locale::ZhCn) => "当前仅允许使用 OAuth 登录",
        ("AUTH_PASSWORD_DISABLED", Locale::En) => {
            "Password authentication is disabled; use an OAuth provider"
        }
        ("AUTH_REGISTRATION_CLOSED", Locale::ZhCn) => "当前未开放账号注册",
        ("AUTH_REGISTRATION_CLOSED", Locale::En) => "Account registration is currently closed",
        ("SEARCH_REQUEST_INVALID", Locale::ZhCn) => "搜索请求格式无效",
        ("SEARCH_REQUEST_INVALID", Locale::En) => "Invalid search request",
        ("SEARCH_LIMIT_INVALID", Locale::ZhCn) => "搜索每页数量必须在 1 到 100 之间",
        ("SEARCH_LIMIT_INVALID", Locale::En) => "Search limit must be between 1 and 100",
        ("SEARCH_CONDITION_FIELD_INVALID", Locale::ZhCn) => "搜索条件字段无效",
        ("SEARCH_CONDITION_FIELD_INVALID", Locale::En) => "Invalid search condition field",
        ("SEARCH_SOURCE_LANGUAGE_INVALID", Locale::ZhCn) => "搜索源语言标签无效",
        ("SEARCH_SOURCE_LANGUAGE_INVALID", Locale::En) => "Invalid search source language",
        ("SEARCH_SOURCE_LANGUAGE_NOT_IN_PROJECT", Locale::ZhCn) => "搜索源语言不属于该项目",
        ("SEARCH_SOURCE_LANGUAGE_NOT_IN_PROJECT", Locale::En) => {
            "Search source language is not part of this project"
        }
        ("SEARCH_PATH_INVALID", Locale::ZhCn) => "搜索路径无效",
        ("SEARCH_PATH_INVALID", Locale::En) => "Invalid search path",
        ("SEARCH_SCOPE_RESOURCE_INVALID", Locale::ZhCn) => "搜索范围资源无效",
        ("SEARCH_SCOPE_RESOURCE_INVALID", Locale::En) => "Invalid search scope resource",
        ("SEARCH_SCOPE_AMBIGUOUS", Locale::ZhCn) => "搜索路径同时匹配文件和文件夹",
        ("SEARCH_SCOPE_AMBIGUOUS", Locale::En) => "Search path is ambiguous",
        ("SEARCH_CURSOR_INVALID", Locale::ZhCn) => "搜索游标无效或与当前请求不匹配",
        ("SEARCH_CURSOR_INVALID", Locale::En) => {
            "Search cursor is invalid or does not match this request"
        }
        ("ADMIN_USER_CURSOR_INVALID", Locale::ZhCn) => "管理员用户游标无效或与当前筛选排序不匹配",
        ("ADMIN_USER_CURSOR_INVALID", Locale::En) => {
            "Admin user cursor is invalid or does not match this request"
        }
        ("PROJECT_MEMBER_ROLE_INVALID", Locale::ZhCn) => "项目成员角色无效",
        ("PROJECT_MEMBER_ROLE_INVALID", Locale::En) => "Invalid project member role",
        ("PROJECT_OWNER_TRANSFER_FORBIDDEN", Locale::ZhCn) => "不能通过成员管理变更项目拥有者",
        ("PROJECT_OWNER_TRANSFER_FORBIDDEN", Locale::En) => {
            "Project ownership cannot be changed through membership management"
        }
        ("PROJECT_DELETE_CHALLENGE_INVALID", Locale::ZhCn) => {
            "删除验证题已过期、已使用或答案不正确"
        }
        ("PROJECT_DELETE_CHALLENGE_INVALID", Locale::En) => {
            "The deletion challenge expired, was already used, or has an incorrect answer"
        }
        ("PROJECT_SEARCH_REBUILDING", Locale::ZhCn) => "项目词法搜索正在重建",
        ("PROJECT_SEARCH_REBUILDING", Locale::En) => "Project lexical search is rebuilding",
        ("PROJECT_SEARCH_FAILED", Locale::ZhCn) => "项目词法搜索重建失败",
        ("PROJECT_SEARCH_FAILED", Locale::En) => "Project lexical search rebuild failed",
        ("unauthorized", Locale::ZhCn) => "未登录或登录已失效",
        ("unauthorized", Locale::En) => "Unauthorized",
        ("forbidden", Locale::ZhCn) => "无权限执行此操作",
        ("forbidden", Locale::En) => "Forbidden",
        ("not_found", Locale::ZhCn) => "资源不存在",
        ("not_found", Locale::En) => "Resource not found",
        ("conflict", Locale::ZhCn) => "版本冲突，请刷新后重试",
        ("conflict", Locale::En) => "Version conflict, please refresh and retry",
        ("AUDIT_UNAVAILABLE", Locale::ZhCn) => "审计服务暂不可用",
        ("AUDIT_UNAVAILABLE", Locale::En) => "Audit service unavailable",
        ("INVALID_LANGUAGE_TAG", Locale::ZhCn) => "语言标签不是有效的 BCP-47 格式",
        ("INVALID_LANGUAGE_TAG", Locale::En) => "Invalid BCP-47 language tag",
        ("LEADERBOARD_PERIOD_INVALID", Locale::ZhCn) => "排行榜周期必须是总榜、月榜或周榜",
        ("LEADERBOARD_PERIOD_INVALID", Locale::En) => {
            "Leaderboard period must be all, month, or week"
        }
        ("DUPLICATE_LANGUAGE_TAG", Locale::ZhCn) => "语言标签规范化后重复",
        ("DUPLICATE_LANGUAGE_TAG", Locale::En) => "Duplicate canonical language tag",
        ("PROJECT_LANGUAGE_RESOLUTION_REQUIRED", Locale::ZhCn) => {
            "项目语言存在歧义，需要拥有者先完成处理"
        }
        ("PROJECT_LANGUAGE_RESOLUTION_REQUIRED", Locale::En) => {
            "Project language resolution is required"
        }
        ("TERM_ACTIVE_SOURCE_MISMATCH", Locale::ZhCn) => "启用术语的源语言必须是项目当前主源",
        ("TERM_ACTIVE_SOURCE_MISMATCH", Locale::En) => {
            "An active term must use the project's current primary source language"
        }
        ("TERM_DUPLICATE", Locale::ZhCn) => "相同语言、原文与词性的术语已存在",
        ("TERM_DUPLICATE", Locale::En) => {
            "A term with the same language, source text, and part of speech already exists"
        }
        ("POS_NAME_REQUIRED", Locale::ZhCn) => "词性至少需要一个中文或英文名称",
        ("POS_NAME_REQUIRED", Locale::En) => {
            "At least one Chinese or English part-of-speech name is required"
        }
        ("POS_NAME_DUPLICATE", Locale::ZhCn) => "词性名称已存在",
        ("POS_NAME_DUPLICATE", Locale::En) => "The part-of-speech name already exists",
        ("POS_IN_USE", Locale::ZhCn) => "该词性仍被会产生术语标识冲突的记录引用",
        ("POS_IN_USE", Locale::En) => {
            "The part of speech is still referenced by terms that would conflict"
        }
        ("PROJECT_PENDING_DELETION", Locale::ZhCn) => "项目正在等待删除，目前为只读状态",
        ("PROJECT_PENDING_DELETION", Locale::En) => {
            "The project is pending deletion and is currently read-only"
        }
        ("IMPORT_FORMAT_INVALID", Locale::ZhCn) => "导入文件格式无效或缺少必填字段",
        ("IMPORT_FORMAT_INVALID", Locale::En) => {
            "The import file format is invalid or required fields are missing"
        }
        ("IMPORT_DUPLICATE_ROW", Locale::ZhCn) => "导入文件包含规范化后重复的行",
        ("IMPORT_DUPLICATE_ROW", Locale::En) => "The import file contains duplicate canonical rows",
        ("IMPORT_PREVIEW_TOKEN_INVALID", Locale::ZhCn) => {
            "导入预览已过期、已使用或与当前请求不匹配"
        }
        ("IMPORT_PREVIEW_TOKEN_INVALID", Locale::En) => {
            "The import preview expired, was already used, or does not match this request"
        }
        ("IMPORT_POS_AMBIGUOUS", Locale::ZhCn) => "导入的词性名称匹配多个预设",
        ("IMPORT_POS_AMBIGUOUS", Locale::En) => {
            "The imported part-of-speech name matches multiple presets"
        }
        ("internal", Locale::ZhCn) => "服务器内部错误",
        ("internal", Locale::En) => "Internal server error",
        _ => return None,
    };
    Some(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_accept_language() {
        assert_eq!(
            Locale::from_accept_language("zh-CN,zh;q=0.9,en;q=0.8"),
            Locale::ZhCn
        );
        assert_eq!(Locale::from_accept_language("en-US,en;q=0.9"), Locale::En);
        assert_eq!(Locale::from_accept_language("fr-FR"), Locale::ZhCn); // 回退默认
        assert_eq!(Locale::from_accept_language(""), Locale::ZhCn);
    }

    #[test]
    fn localizes_known_and_unknown_codes() {
        assert_eq!(localize("not_found", Locale::En), "Resource not found");
        assert_eq!(localize("not_found", Locale::ZhCn), "资源不存在");
        assert_eq!(
            localize("AUTH_PASSWORD_DISABLED", Locale::En),
            "Password authentication is disabled; use an OAuth provider"
        );
        assert_eq!(
            localize("AUTH_REGISTRATION_CLOSED", Locale::ZhCn),
            "当前未开放账号注册"
        );
        // 未知错误码回退到内部错误消息。
        assert_eq!(localize("nope", Locale::En), "Internal server error");
    }
}
