//! BCP-47 语言标签的唯一规范化入口。

use std::collections::HashSet;
use std::fmt;

use language_tags::LanguageTag;

/// 语言标签校验的稳定错误类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageTagError {
    /// 标签不是合法且已注册的 BCP-47 标签。
    Invalid,
    /// 多值输入在规范化后出现重复项。
    Duplicate,
    /// 项目没有源语言。
    EmptySourceLanguages,
    /// 主源语言不属于源语言集合。
    PrimaryNotInSourceLanguages,
}

impl LanguageTagError {
    /// API 使用的稳定错误码。
    pub const fn code(self) -> &'static str {
        match self {
            Self::Invalid => "INVALID_LANGUAGE_TAG",
            Self::Duplicate => "DUPLICATE_LANGUAGE_TAG",
            Self::EmptySourceLanguages => "SOURCE_LANGUAGES_REQUIRED",
            Self::PrimaryNotInSourceLanguages => "PRIMARY_SOURCE_LANGUAGE_INVALID",
        }
    }
}

impl fmt::Display for LanguageTagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for LanguageTagError {}

/// 解析、完整校验并输出稳定大小写的 BCP-47 标签。
pub fn canonicalize_language_tag(input: &str) -> Result<String, LanguageTagError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(LanguageTagError::Invalid);
    }
    let tag = LanguageTag::parse(trimmed).map_err(|_| LanguageTagError::Invalid)?;
    tag.validate().map_err(|_| LanguageTagError::Invalid)?;
    Ok(tag.into_string())
}

/// 按输入顺序规范化标签集合，并拒绝规范化后的重复项。
pub fn canonicalize_language_tags(inputs: &[String]) -> Result<Vec<String>, LanguageTagError> {
    let mut seen = HashSet::with_capacity(inputs.len());
    let mut canonical = Vec::with_capacity(inputs.len());
    for input in inputs {
        let tag = canonicalize_language_tag(input)?;
        if !seen.insert(tag.clone()) {
            return Err(LanguageTagError::Duplicate);
        }
        canonical.push(tag);
    }
    Ok(canonical)
}

/// 规范化项目语言并验证主源属于非空源语言集合。
pub fn canonicalize_project_languages(
    source_languages: &[String],
    primary_source_language: Option<&str>,
    target_language: &str,
) -> Result<(Vec<String>, String, String), LanguageTagError> {
    let source_languages = canonicalize_language_tags(source_languages)?;
    if source_languages.is_empty() {
        return Err(LanguageTagError::EmptySourceLanguages);
    }
    let primary_source_language = match primary_source_language {
        Some(value) => canonicalize_language_tag(value)?,
        None if source_languages.len() == 1 => source_languages[0].clone(),
        None => return Err(LanguageTagError::PrimaryNotInSourceLanguages),
    };
    if !source_languages.contains(&primary_source_language) {
        return Err(LanguageTagError::PrimaryNotInSourceLanguages);
    }
    let target_language = canonicalize_language_tag(target_language)?;
    Ok((source_languages, primary_source_language, target_language))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_registered_bcp47_casing() {
        assert_eq!(
            canonicalize_language_tag(" ZH-hans-cn ").unwrap(),
            "zh-Hans-CN"
        );
        assert_eq!(
            canonicalize_language_tag("de-de-u-co-phonebk").unwrap(),
            "de-DE-u-co-phonebk"
        );
    }

    #[test]
    fn rejects_invalid_and_canonical_duplicates() {
        assert_eq!(
            canonicalize_language_tag("not_a_language"),
            Err(LanguageTagError::Invalid)
        );
        assert_eq!(
            canonicalize_language_tags(&["zh-hans".into(), "zh-Hans".into()]),
            Err(LanguageTagError::Duplicate)
        );
    }

    #[test]
    fn single_source_defaults_primary_but_multi_source_requires_it() {
        let languages = canonicalize_project_languages(&["EN".into()], None, "zh-hans").unwrap();
        assert_eq!(
            languages,
            (vec!["en".into()], "en".into(), "zh-Hans".into())
        );
        assert_eq!(
            canonicalize_project_languages(&["en".into(), "ja".into()], None, "zh-Hans"),
            Err(LanguageTagError::PrimaryNotInSourceLanguages)
        );
    }
}
