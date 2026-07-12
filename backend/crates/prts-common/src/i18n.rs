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
        ("DUPLICATE_LANGUAGE_TAG", Locale::ZhCn) => "语言标签规范化后重复",
        ("DUPLICATE_LANGUAGE_TAG", Locale::En) => "Duplicate canonical language tag",
        ("PROJECT_LANGUAGE_RESOLUTION_REQUIRED", Locale::ZhCn) => {
            "项目语言存在歧义，需要拥有者先完成处理"
        }
        ("PROJECT_LANGUAGE_RESOLUTION_REQUIRED", Locale::En) => {
            "Project language resolution is required"
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
        // 未知错误码回退到内部错误消息。
        assert_eq!(localize("nope", Locale::En), "Internal server error");
    }
}
