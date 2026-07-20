//! source-aware 项目术语的框架无关规则与主源切换计划。

use crate::language::{canonicalize_language_tag, LanguageTagError};
use regex::Regex;

/// 原文术语的匹配模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermMatchMode {
    /// 原文包含固定术语文本。
    Exact,
    /// `[]` 表示任意文本，仅在原文匹配时生效。
    Placeholder,
    /// Rust `regex` crate 的线性时间正则语义。
    Regex,
}

impl TermMatchMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Placeholder => "placeholder",
            Self::Regex => "regex",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "exact" => Some(Self::Exact),
            "placeholder" => Some(Self::Placeholder),
            "regex" => Some(Self::Regex),
            _ => None,
        }
    }
}

/// 匹配表达式的稳定校验错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermPatternError {
    InvalidMode,
    EmptyPattern,
    PlaceholderRequired,
    PlaceholderEmptyAnchor,
    InvalidRegex,
    PatternTooLong,
}

impl TermPatternError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidMode => "TERM_MATCH_MODE_INVALID",
            Self::EmptyPattern => "TERM_PATTERN_REQUIRED",
            Self::PlaceholderRequired => "TERM_PLACEHOLDER_REQUIRED",
            Self::PlaceholderEmptyAnchor => "TERM_PLACEHOLDER_EMPTY_ANCHOR",
            Self::InvalidRegex => "TERM_REGEX_INVALID",
            Self::PatternTooLong => "TERM_PATTERN_TOO_LONG",
        }
    }
}

/// 校验单个术语表达式。`placeholder` 至少含一个 `[]`，且每个固定片段均不能为空。
pub fn validate_term_pattern(mode: &str, pattern: &str) -> Result<TermMatchMode, TermPatternError> {
    let mode = TermMatchMode::parse(mode).ok_or(TermPatternError::InvalidMode)?;
    if pattern.is_empty() {
        return Err(TermPatternError::EmptyPattern);
    }
    if pattern.chars().count() > 2_000 {
        return Err(TermPatternError::PatternTooLong);
    }
    match mode {
        TermMatchMode::Exact => Ok(mode),
        TermMatchMode::Placeholder => {
            let parts = pattern.split("[]").collect::<Vec<_>>();
            if parts.len() < 2 {
                return Err(TermPatternError::PlaceholderRequired);
            }
            if parts.iter().any(|part| part.is_empty()) {
                return Err(TermPatternError::PlaceholderEmptyAnchor);
            }
            Ok(mode)
        }
        TermMatchMode::Regex => Regex::new(pattern)
            .map(|_| mode)
            .map_err(|_| TermPatternError::InvalidRegex),
    }
}

/// 判断原文是否命中术语；placeholder 的 `[]` 可跨行并匹配空文本。
pub fn term_matches_source(
    mode: &str,
    pattern: &str,
    source: &str,
) -> Result<bool, TermPatternError> {
    match validate_term_pattern(mode, pattern)? {
        TermMatchMode::Exact => Ok(source.contains(pattern)),
        TermMatchMode::Placeholder => {
            let expression = pattern
                .split("[]")
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join("(?s:.*?)");
            Regex::new(&expression)
                .map(|regex| regex.is_match(source))
                .map_err(|_| TermPatternError::InvalidRegex)
        }
        TermMatchMode::Regex => Regex::new(pattern)
            .map(|regex| regex.is_match(source))
            .map_err(|_| TermPatternError::InvalidRegex),
    }
}

/// 普通术语写入的稳定领域错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermRuleError {
    /// source_lang 或当前 primary 不是合法 BCP-47。
    InvalidLanguageTag,
    /// active term 的语言不是当前 primary。
    ActiveSourceMismatch,
    /// 项目语言仍需人工消歧。
    LanguageResolutionRequired,
    /// 项目已进入待删除只读状态。
    ProjectPendingDeletion,
}

impl TermRuleError {
    /// 稳定 API 错误码。
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidLanguageTag => "INVALID_LANGUAGE_TAG",
            Self::ActiveSourceMismatch => "TERM_ACTIVE_SOURCE_MISMATCH",
            Self::LanguageResolutionRequired => "PROJECT_LANGUAGE_RESOLUTION_REQUIRED",
            Self::ProjectPendingDeletion => "PROJECT_PENDING_DELETION",
        }
    }
}

/// 经过 canonicalization 与 active/archived 校验的写计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermWritePlan {
    pub source_lang: String,
    pub archived: bool,
}

/// 校验普通术语 mutation，且绝不把非法 active 请求静默改为 archived。
pub fn plan_term_write(
    source_lang: &str,
    primary_source_lang: &str,
    archived: bool,
    language_ready: bool,
    pending_deletion: bool,
) -> Result<TermWritePlan, TermRuleError> {
    if pending_deletion {
        return Err(TermRuleError::ProjectPendingDeletion);
    }
    if !language_ready {
        return Err(TermRuleError::LanguageResolutionRequired);
    }
    let source_lang = canonicalize_language_tag(source_lang).map_err(map_language_error)?;
    let primary_source_lang =
        canonicalize_language_tag(primary_source_lang).map_err(map_language_error)?;
    if !archived && source_lang != primary_source_lang {
        return Err(TermRuleError::ActiveSourceMismatch);
    }
    Ok(TermWritePlan {
        source_lang,
        archived,
    })
}

/// 一条既有 term 在主源切换时应采取的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimarySourceTermAction {
    Archive,
    Activate,
    Keep,
}

/// 主源切换的集合执行计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimarySourceTermsPlan {
    pub primary_source_lang: String,
}

/// 为 DB set-based executor 生成 canonical 主源切换计划。
pub fn plan_primary_source_terms(
    primary_source_lang: &str,
) -> Result<PrimarySourceTermsPlan, TermRuleError> {
    Ok(PrimarySourceTermsPlan {
        primary_source_lang: canonicalize_language_tag(primary_source_lang)
            .map_err(map_language_error)?,
    })
}

/// 纯规则投影，用于证明集合计划的逐行语义。
pub fn plan_primary_source_term_action(
    source_lang: &str,
    archived: bool,
    primary_source_lang: &str,
) -> Result<PrimarySourceTermAction, TermRuleError> {
    let source_lang = canonicalize_language_tag(source_lang).map_err(map_language_error)?;
    let primary_source_lang =
        canonicalize_language_tag(primary_source_lang).map_err(map_language_error)?;
    Ok(if source_lang == primary_source_lang && archived {
        PrimarySourceTermAction::Activate
    } else if source_lang != primary_source_lang && !archived {
        PrimarySourceTermAction::Archive
    } else {
        PrimarySourceTermAction::Keep
    })
}

fn map_language_error(_error: LanguageTagError) -> TermRuleError {
    TermRuleError::InvalidLanguageTag
}
