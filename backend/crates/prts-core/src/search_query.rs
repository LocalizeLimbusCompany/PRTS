//! 结构化项目搜索的领域类型、规范化与稳定 fingerprint 输入。
//!
//! axum 只负责协议与资源鉴权；数据库只执行本模块产生的 canonical plan。字段、操作符、
//! scope、BCP-47 selector、路径和分页边界不得在 handler/SQL 中另定义第二套真值。

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::{canonicalize_language_tag, EntryState};

/// 搜索资源范围；每个带 payload 的 variant 都拒绝未知字段和缺失字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchScope {
    /// URL 项目内全部 active 文件。
    All,
    /// 一个 canonical active file path 或 active folder subtree。
    Path { path: String },
    /// 明确指定任意 active file。
    File { file_id: i64 },
    /// 编辑器明确传入的当前 active file；服务端不从 session 推断。
    CurrentFile { file_id: i64 },
    /// 明确指定任务当前 active files；不限 baseline snapshot。
    CurrentTask { task_id: i64 },
}

impl<'de> Deserialize<'de> for SearchScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        enum StrictSearchScope {
            All {},
            Path { path: String },
            File { file_id: i64 },
            CurrentFile { file_id: i64 },
            CurrentTask { task_id: i64 },
        }

        Ok(match StrictSearchScope::deserialize(deserializer)? {
            StrictSearchScope::All {} => Self::All,
            StrictSearchScope::Path { path } => Self::Path { path },
            StrictSearchScope::File { file_id } => Self::File { file_id },
            StrictSearchScope::CurrentFile { file_id } => Self::CurrentFile { file_id },
            StrictSearchScope::CurrentTask { task_id } => Self::CurrentTask { task_id },
        })
    }
}

/// 结构化条件仅支持的五种操作符。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchOperator {
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Equals,
}

impl SearchOperator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::NotContains => "not_contains",
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
            Self::Equals => "equals",
        }
    }
}

/// 客户端条件；所有条件按 AND 组合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchCondition {
    /// `source:<bcp47>`、`source_any`、`translation` 或 `key`。
    pub field: String,
    pub operator: SearchOperator,
    pub value: String,
}

/// POST 请求领域形状。协议层 OpenAPI 使用同形 shadow schema，但实际反序列化只用此类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredSearchRequest {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub conditions: Vec<SearchCondition>,
    pub scope: SearchScope,
    #[serde(default)]
    pub states: Vec<EntryState>,
    /// 独立的有疑问标签过滤；`None` 表示不过滤。
    #[serde(default)]
    pub questioned: Option<bool>,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default)]
    pub vector: bool,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: u16,
}

const fn default_search_limit() -> u16 {
    50
}

/// canonical condition 字段。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", content = "language", rename_all = "snake_case")]
pub enum SearchField {
    Source(String),
    SourceAny,
    Translation,
    Key,
}

impl SearchField {
    pub fn canonical_name(&self) -> String {
        match self {
            Self::Source(language) => format!("source:{language}"),
            Self::SourceAny => "source_any".to_string(),
            Self::Translation => "translation".to_string(),
            Self::Key => "key".to_string(),
        }
    }
}

/// 已规范化、可直接交给数据库 adapter 的 AND condition。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalSearchCondition {
    pub field: SearchField,
    pub operator: SearchOperator,
    pub value: String,
}

/// handler 完成资源归属解析前的 canonical typed plan。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredSearchPlan {
    pub query: Option<String>,
    pub conditions: Vec<CanonicalSearchCondition>,
    pub scope: SearchScope,
    pub states: Vec<EntryState>,
    pub questioned: Option<bool>,
    pub include_hidden: bool,
    pub vector: bool,
    pub limit: u16,
}

impl StructuredSearchPlan {
    /// 构造 opaque cursor fingerprint 的稳定 canonical bytes；不包含 `after` 或正文日志。
    pub fn fingerprint_material(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("canonical structured search plan must serialize")
    }
}

/// 稳定校验错误；API 只暴露 code，不回显 query/condition value。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchQueryError {
    InvalidLimit,
    InvalidField,
    InvalidSourceLanguage,
    SourceLanguageNotInProject,
    InvalidPath,
    InvalidResourceId,
}

impl SearchQueryError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidLimit => "SEARCH_LIMIT_INVALID",
            Self::InvalidField => "SEARCH_CONDITION_FIELD_INVALID",
            Self::InvalidSourceLanguage => "SEARCH_SOURCE_LANGUAGE_INVALID",
            Self::SourceLanguageNotInProject => "SEARCH_SOURCE_LANGUAGE_NOT_IN_PROJECT",
            Self::InvalidPath => "SEARCH_PATH_INVALID",
            Self::InvalidResourceId => "SEARCH_SCOPE_RESOURCE_INVALID",
        }
    }
}

impl fmt::Display for SearchQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for SearchQueryError {}

/// 规范化请求并冻结 fingerprint 顺序。AND conditions 与 states 排序去重，顺序变化不改变语义。
pub fn plan_structured_search(
    request: &StructuredSearchRequest,
    project_source_languages: &[String],
) -> Result<StructuredSearchPlan, SearchQueryError> {
    if !(1..=100).contains(&request.limit) {
        return Err(SearchQueryError::InvalidLimit);
    }
    let source_languages = project_source_languages
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut conditions = request
        .conditions
        .iter()
        .map(|condition| canonicalize_condition(condition, &source_languages))
        .collect::<Result<Vec<_>, _>>()?;
    conditions.sort_by(|left, right| {
        left.field
            .canonical_name()
            .cmp(&right.field.canonical_name())
            .then(left.operator.cmp(&right.operator))
            .then(left.value.cmp(&right.value))
    });
    conditions.dedup();

    let mut states = request.states.clone();
    states.sort_by_key(|state| match state {
        EntryState::Untranslated => 0,
        EntryState::Translated => 1,
        EntryState::Checked => 2,
        EntryState::Reviewed => 3,
    });
    states.dedup();
    let scope = match &request.scope {
        SearchScope::All => SearchScope::All,
        SearchScope::Path { path } => SearchScope::Path {
            path: canonicalize_file_path(path)?,
        },
        SearchScope::File { file_id } if *file_id > 0 => SearchScope::File { file_id: *file_id },
        SearchScope::CurrentFile { file_id } if *file_id > 0 => {
            SearchScope::CurrentFile { file_id: *file_id }
        }
        SearchScope::CurrentTask { task_id } if *task_id > 0 => {
            SearchScope::CurrentTask { task_id: *task_id }
        }
        SearchScope::File { .. }
        | SearchScope::CurrentFile { .. }
        | SearchScope::CurrentTask { .. } => return Err(SearchQueryError::InvalidResourceId),
    };
    Ok(StructuredSearchPlan {
        query: request
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_string),
        conditions,
        scope,
        states,
        questioned: request.questioned,
        include_hidden: request.include_hidden,
        vector: request.vector,
        limit: request.limit,
    })
}

fn canonicalize_condition(
    condition: &SearchCondition,
    project_source_languages: &HashSet<&str>,
) -> Result<CanonicalSearchCondition, SearchQueryError> {
    let raw_field = condition.field.trim();
    let field = match raw_field {
        "source_any" => SearchField::SourceAny,
        "translation" => SearchField::Translation,
        "key" => SearchField::Key,
        _ if raw_field.starts_with("source:") => {
            let raw_language = &raw_field["source:".len()..];
            let language = canonicalize_language_tag(raw_language)
                .map_err(|_| SearchQueryError::InvalidSourceLanguage)?;
            if !project_source_languages.contains(language.as_str()) {
                return Err(SearchQueryError::SourceLanguageNotInProject);
            }
            SearchField::Source(language)
        }
        _ => return Err(SearchQueryError::InvalidField),
    };
    Ok(CanonicalSearchCondition {
        field,
        operator: condition.operator,
        value: condition.value.clone(),
    })
}

/// 文件、文件夹与搜索 path 共用的 relative path canonicalizer；调用方可另加 `.json` 限制。
pub fn canonicalize_file_path(raw: &str) -> Result<String, SearchQueryError> {
    if raw != raw.trim() {
        return Err(SearchQueryError::InvalidPath);
    }
    let normalized = raw.replace('\\', "/");
    if normalized.is_empty() || normalized.len() > 1024 || normalized.starts_with('/') {
        return Err(SearchQueryError::InvalidPath);
    }
    let path = Path::new(&normalized);
    if path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().is_empty()
        })
        || normalized.split('/').any(is_reserved_path_segment)
    {
        return Err(SearchQueryError::InvalidPath);
    }
    Ok(normalized)
}

fn is_reserved_path_segment(segment: &str) -> bool {
    if segment.is_empty()
        || matches!(segment, "." | "..")
        || segment.starts_with('.')
        || segment.ends_with('.')
        || segment.ends_with(' ')
        || segment.chars().any(|character| {
            character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
    {
        return true;
    }
    let device_name = segment
        .split_once('.')
        .map_or(segment, |(stem, _extension)| stem)
        .to_ascii_uppercase();
    matches!(device_name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || device_name
            .strip_prefix("COM")
            .or_else(|| device_name.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(scope: SearchScope) -> StructuredSearchRequest {
        StructuredSearchRequest {
            query: Some("  hello  ".to_string()),
            conditions: Vec::new(),
            scope,
            states: Vec::new(),
            questioned: None,
            include_hidden: false,
            vector: false,
            after: None,
            limit: 50,
        }
    }

    #[test]
    fn tagged_scope_accepts_each_exact_shape_and_rejects_invalid_payloads() {
        for json in [
            r#"{"type":"all"}"#,
            r#"{"type":"path","path":"chapter/01"}"#,
            r#"{"type":"file","file_id":41}"#,
            r#"{"type":"current_file","file_id":41}"#,
            r#"{"type":"current_task","task_id":73}"#,
        ] {
            serde_json::from_str::<SearchScope>(json).unwrap();
        }
        for json in [
            r#"{"type":"path"}"#,
            r#"{"type":"file"}"#,
            r#"{"type":"current_task"}"#,
            r#"{"type":"unknown"}"#,
            r#"{"type":"all","file_id":41}"#,
            r#"{"type":"path","path":"x","task_id":73}"#,
            r#"{"type":"file","file_id":"41"}"#,
        ] {
            assert!(serde_json::from_str::<SearchScope>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn request_defaults_limit_and_rejects_unknown_fields() {
        let parsed: StructuredSearchRequest =
            serde_json::from_str(r#"{"scope":{"type":"all"}}"#).unwrap();
        assert_eq!(parsed.limit, 50);
        assert!(serde_json::from_str::<StructuredSearchRequest>(
            r#"{"scope":{"type":"all"},"unexpected":true}"#
        )
        .is_err());
        for limit in [0, 101] {
            let mut parsed = parsed.clone();
            parsed.limit = limit;
            assert_eq!(
                plan_structured_search(&parsed, &["en".to_string()]),
                Err(SearchQueryError::InvalidLimit)
            );
        }
    }

    #[test]
    fn canonicalizes_source_selector_scope_conditions_states_and_fingerprint() {
        let mut input = request(SearchScope::Path {
            path: "chapter\\01".to_string(),
        });
        input.conditions = vec![
            SearchCondition {
                field: "translation".to_string(),
                operator: SearchOperator::Contains,
                value: "T".to_string(),
            },
            SearchCondition {
                field: "source:EN".to_string(),
                operator: SearchOperator::Equals,
                value: "Source".to_string(),
            },
        ];
        input.states = vec![
            EntryState::Reviewed,
            EntryState::Translated,
            EntryState::Reviewed,
        ];
        let plan = plan_structured_search(&input, &["en".to_string()]).unwrap();
        assert_eq!(plan.query.as_deref(), Some("hello"));
        assert_eq!(
            plan.scope,
            SearchScope::Path {
                path: "chapter/01".into()
            }
        );
        assert_eq!(plan.states, [EntryState::Translated, EntryState::Reviewed]);
        assert_eq!(plan.conditions[0].field, SearchField::Source("en".into()));

        let mut reordered = input;
        reordered.conditions.reverse();
        reordered.states.reverse();
        assert_eq!(
            plan.fingerprint_material(),
            plan_structured_search(&reordered, &["en".to_string()])
                .unwrap()
                .fingerprint_material()
        );
    }

    #[test]
    fn rejects_invalid_or_non_project_source_and_invalid_resource_ids() {
        for field in ["source:not_a_tag", "source:ja", "regex"] {
            let mut input = request(SearchScope::All);
            input.conditions.push(SearchCondition {
                field: field.to_string(),
                operator: SearchOperator::Contains,
                value: "x".to_string(),
            });
            assert!(plan_structured_search(&input, &["en".to_string()]).is_err());
        }
        assert_eq!(
            plan_structured_search(
                &request(SearchScope::CurrentTask { task_id: 0 }),
                &["en".to_string()]
            ),
            Err(SearchQueryError::InvalidResourceId)
        );
    }

    #[test]
    fn path_canonicalizer_preserves_segment_boundaries_and_rejects_reserved_paths() {
        assert_eq!(
            canonicalize_file_path("dir\\file.json").unwrap(),
            "dir/file.json"
        );
        for invalid in [
            " dir",
            "/dir",
            "dir//file",
            "dir/../file",
            "dir/.hidden",
            "dir/CON",
        ] {
            assert_eq!(
                canonicalize_file_path(invalid),
                Err(SearchQueryError::InvalidPath)
            );
        }
        assert_ne!(
            canonicalize_file_path("dir").unwrap(),
            canonicalize_file_path("dir2").unwrap()
        );
    }
}
