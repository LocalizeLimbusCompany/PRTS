//! 术语/POS 导入导出的纯格式与 preview-token 规则。

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use base64::Engine as _;
use rand::RngCore as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use utoipa::{IntoParams, ToSchema};

/// Preview token 固定存活十五分钟。
pub const PREVIEW_TTL_SECONDS: u64 = 15 * 60;
const PREVIEW_KEY_PREFIX: &str = "terminology_import_preview:";

fn default_match_mode() -> String {
    "exact".to_string()
}

/// 支持的稳定导入导出文档格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Csv,
    Json,
}

/// 术语文档固定字段。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TermDocumentRow {
    pub source_lang: String,
    pub source_text: String,
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
    pub translation: String,
    pub pos: Option<String>,
    pub notes: String,
    pub archived: bool,
}

/// POS 文档固定字段。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PosDocumentRow {
    pub name_zh_cn: Option<String>,
    pub name_en: Option<String>,
    pub sort_order: i32,
}

/// 纯解析/规范化规则的稳定失败类型；不携带任何正文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRuleError {
    InvalidFormat,
    InvalidLanguageTag { row: usize },
    DuplicateRow { first: usize, duplicate: usize },
    ActiveSourceMismatch { row: usize },
    SourceTextRequired { row: usize },
    PosNameRequired { row: usize },
}

/// Preview 请求只在 Redis 暂存正文，不写 PostgreSQL 业务表。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ImportPreviewRequest {
    pub format: DocumentFormat,
    pub content: String,
}

/// Confirm 只回传 canonical digest，不再次发送导入正文。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ImportConfirmRequest {
    pub digest: String,
}

/// 导出格式查询。
#[derive(Debug, Deserialize, IntoParams)]
pub struct ExportQuery {
    pub format: Option<DocumentFormat>,
}

/// 不包含正文的行级 warning。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ImportWarningDto {
    pub row: usize,
    pub code: String,
}

/// Preview 中可供用户确认的术语行。
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct TermPreviewRowDto {
    pub row: usize,
    pub source_lang: String,
    pub source_text: String,
    pub match_mode: String,
    pub translation: String,
    pub pos: Option<String>,
    pub notes: String,
    pub archived: bool,
    pub action: String,
}

/// 术语 preview 结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct TermImportPreviewDto {
    pub token: String,
    pub digest: String,
    pub expires_in_seconds: u64,
    pub created: usize,
    pub updated: usize,
    pub rows: Vec<TermPreviewRowDto>,
    pub warnings: Vec<ImportWarningDto>,
}

/// Preview 中可供管理员确认的 POS 行。
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PosPreviewRowDto {
    pub row: usize,
    pub name_zh_cn: Option<String>,
    pub name_en: Option<String>,
    pub sort_order: i32,
    pub action: String,
}

/// POS preview 结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct PosImportPreviewDto {
    pub token: String,
    pub digest: String,
    pub expires_in_seconds: u64,
    pub created: usize,
    pub updated: usize,
    pub rows: Vec<PosPreviewRowDto>,
    pub warnings: Vec<ImportWarningDto>,
}

/// Confirm 成功后的脱敏计数。
#[derive(Debug, Serialize, ToSchema)]
pub struct ImportConfirmDto {
    pub created: usize,
    pub updated: usize,
    pub warnings: Vec<ImportWarningDto>,
}

/// Redis token 绑定的导入种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    Term,
    Pos,
}

/// Confirm 实际 upsert 使用的已解析术语行。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolvedTermImportRow {
    pub row: usize,
    pub source_lang: String,
    pub source_text: String,
    pub match_mode: String,
    pub translation: String,
    pub pos: Option<String>,
    pub pos_id: Option<i64>,
    pub notes: String,
    pub archived: bool,
    pub warning_codes: Vec<String>,
}

/// Confirm 实际 upsert 使用的已解析 POS 行。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResolvedPosImportRow {
    pub row: usize,
    pub id: Option<i64>,
    pub name_zh_cn: Option<String>,
    pub name_en: Option<String>,
    pub sort_order: i32,
}

/// Redis 中保存的 canonical preview；token 本身只作为随机查找句柄。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoredPreview {
    pub actor_id: String,
    pub project_id: Option<String>,
    pub kind: ImportKind,
    pub digest: String,
    pub primary_source_lang: Option<String>,
    pub terms: Vec<ResolvedTermImportRow>,
    pub pos: Vec<ResolvedPosImportRow>,
}

/// Redis/序列化失败的脱敏分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewStoreError {
    Unavailable,
    InvalidPayload,
}

/// 解析并规范化术语文档；语言码在唯一性与 active 校验前 canonicalize。
pub fn parse_term_document(
    format: DocumentFormat,
    content: &str,
    primary_source_lang: &str,
) -> Result<Vec<TermDocumentRow>, ImportRuleError> {
    let mut rows: Vec<TermDocumentRow> = decode_document(
        format,
        content,
        &[
            "source_lang",
            "source_text",
            "match_mode",
            "translation",
            "pos",
            "notes",
            "archived",
        ],
        &["match_mode"],
    )?;
    let mut seen: HashMap<(String, String, String, Option<String>), usize> = HashMap::new();
    for (index, row) in rows.iter_mut().enumerate() {
        let row_number = index + 1;
        let plan = prts_core::terms::plan_term_write(
            &row.source_lang,
            primary_source_lang,
            row.archived,
            true,
            false,
        )
        .map_err(|error| match error {
            prts_core::terms::TermRuleError::InvalidLanguageTag => {
                ImportRuleError::InvalidLanguageTag { row: row_number }
            }
            prts_core::terms::TermRuleError::ActiveSourceMismatch => {
                ImportRuleError::ActiveSourceMismatch { row: row_number }
            }
            prts_core::terms::TermRuleError::LanguageResolutionRequired
            | prts_core::terms::TermRuleError::ProjectPendingDeletion => {
                ImportRuleError::InvalidFormat
            }
        })?;
        row.source_lang = plan.source_lang;
        row.source_text = row.source_text.trim().to_string();
        prts_core::terms::validate_term_pattern(&row.match_mode, &row.source_text).map_err(
            |error| match error {
                prts_core::terms::TermPatternError::EmptyPattern => {
                    ImportRuleError::SourceTextRequired { row: row_number }
                }
                _ => ImportRuleError::InvalidFormat,
            },
        )?;
        row.pos = normalize_optional(row.pos.take());
        let key = (
            row.source_lang.clone(),
            row.source_text.clone(),
            row.match_mode.clone(),
            row.pos.as_ref().map(|value| value.to_lowercase()),
        );
        if let Some(first) = seen.insert(key, row_number) {
            return Err(ImportRuleError::DuplicateRow {
                first,
                duplicate: row_number,
            });
        }
    }
    Ok(rows)
}

/// 解析 POS 文档，trim 名称并按各语言名称的大小写无关唯一性拒绝重复行。
pub fn parse_pos_document(
    format: DocumentFormat,
    content: &str,
) -> Result<Vec<PosDocumentRow>, ImportRuleError> {
    let mut rows: Vec<PosDocumentRow> = decode_document(
        format,
        content,
        &["name_zh_cn", "name_en", "sort_order"],
        &[],
    )?;
    let mut zh_names = HashMap::new();
    let mut en_names = HashMap::new();
    for (index, row) in rows.iter_mut().enumerate() {
        let row_number = index + 1;
        row.name_zh_cn = normalize_optional(row.name_zh_cn.take());
        row.name_en = normalize_optional(row.name_en.take());
        if row.name_zh_cn.is_none() && row.name_en.is_none() {
            return Err(ImportRuleError::PosNameRequired { row: row_number });
        }
        for (value, names) in [
            (row.name_zh_cn.as_ref(), &mut zh_names),
            (row.name_en.as_ref(), &mut en_names),
        ] {
            if let Some(value) = value {
                let key = value.to_lowercase();
                if let Some(first) = names.insert(key, row_number) {
                    return Err(ImportRuleError::DuplicateRow {
                        first,
                        duplicate: row_number,
                    });
                }
            }
        }
    }
    Ok(rows)
}

/// 按固定列顺序编码术语文档。
pub fn encode_term_document(
    format: DocumentFormat,
    rows: &[TermDocumentRow],
) -> Result<String, ImportRuleError> {
    match format {
        DocumentFormat::Json => {
            serde_json::to_string_pretty(rows).map_err(|_| ImportRuleError::InvalidFormat)
        }
        DocumentFormat::Csv => encode_csv(rows),
    }
}

/// 按固定列顺序编码 POS 文档。
pub fn encode_pos_document(
    format: DocumentFormat,
    rows: &[PosDocumentRow],
) -> Result<String, ImportRuleError> {
    match format {
        DocumentFormat::Json => {
            serde_json::to_string_pretty(rows).map_err(|_| ImportRuleError::InvalidFormat)
        }
        DocumentFormat::Csv => encode_csv(rows),
    }
}

/// 对 canonical payload 生成稳定 SHA-256 十六进制摘要。
pub fn canonical_digest<T: Serialize + ?Sized>(value: &T) -> Result<String, ImportRuleError> {
    let encoded = serde_json::to_vec(value).map_err(|_| ImportRuleError::InvalidFormat)?;
    let digest = Sha256::digest(encoded);
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(result, "{byte:02x}");
    }
    Ok(result)
}

/// 生成 256-bit CSPRNG URL-safe token（高于 128-bit 下限）。
pub fn generate_preview_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Redis key 仅含随机 token，不含 actor/project/正文。
pub fn preview_redis_key(token: &str) -> String {
    format!("{PREVIEW_KEY_PREFIX}{token}")
}

/// 以 NX + 固定 TTL 保存 preview；随机碰撞时有限重试。
pub async fn store_preview(
    cache: &prts_db::Cache,
    preview: &StoredPreview,
) -> Result<String, PreviewStoreError> {
    let payload = serde_json::to_string(preview).map_err(|_| PreviewStoreError::InvalidPayload)?;
    for _ in 0..3 {
        let token = generate_preview_token();
        let mut connection = cache.clone();
        let stored: Option<String> = redis::cmd("SET")
            .arg(preview_redis_key(&token))
            .arg(&payload)
            .arg("NX")
            .arg("EX")
            .arg(PREVIEW_TTL_SECONDS)
            .query_async(&mut connection)
            .await
            .map_err(|_| PreviewStoreError::Unavailable)?;
        if stored.is_some() {
            return Ok(token);
        }
    }
    Err(PreviewStoreError::Unavailable)
}

/// Lua 原子校验绑定并消费 preview；并发 confirm 只能一个得到 payload。
pub async fn take_bound_preview(
    cache: &prts_db::Cache,
    token: &str,
    actor_id: i64,
    project_id: Option<i64>,
    kind: ImportKind,
    digest: &str,
) -> Result<Option<StoredPreview>, PreviewStoreError> {
    const INVALID_PAYLOAD: &str = "__PRTS_INVALID_PREVIEW_PAYLOAD__";
    const LUA: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then return false end
local ok, preview = pcall(cjson.decode, raw)
if not ok then return '__PRTS_INVALID_PREVIEW_PAYLOAD__' end
if preview.actor_id ~= ARGV[1] then return false end
if ARGV[2] == 'none' then
  if preview.project_id ~= cjson.null then return false end
elseif preview.project_id ~= ARGV[2] then
  return false
end
if preview.kind ~= ARGV[3] or preview.digest ~= ARGV[4] then return false end
redis.call('DEL', KEYS[1])
return raw
"#;
    let mut connection = cache.clone();
    let payload: Option<String> = redis::Script::new(LUA)
        .key(preview_redis_key(token))
        .arg(actor_id)
        .arg(project_id.map_or_else(|| "none".to_string(), |id| id.to_string()))
        .arg(match kind {
            ImportKind::Term => "term",
            ImportKind::Pos => "pos",
        })
        .arg(digest)
        .invoke_async(&mut connection)
        .await
        .map_err(|_| PreviewStoreError::Unavailable)?;
    if payload.as_deref() == Some(INVALID_PAYLOAD) {
        return Err(PreviewStoreError::InvalidPayload);
    }
    payload
        .map(|value| serde_json::from_str(&value).map_err(|_| PreviewStoreError::InvalidPayload))
        .transpose()
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn decode_document<T: DeserializeOwned>(
    format: DocumentFormat,
    content: &str,
    expected_headers: &[&str],
    optional_default_headers: &[&str],
) -> Result<Vec<T>, ImportRuleError> {
    let legacy_headers = expected_headers
        .iter()
        .copied()
        .filter(|header| !optional_default_headers.contains(header))
        .collect::<Vec<_>>();
    match format {
        DocumentFormat::Json => decode_json(content, expected_headers, &legacy_headers),
        DocumentFormat::Csv => {
            let mut reader = csv::ReaderBuilder::new()
                .trim(csv::Trim::Headers)
                .from_reader(content.as_bytes());
            let headers = reader
                .headers()
                .map_err(|_| ImportRuleError::InvalidFormat)?;
            let actual_headers = headers.iter().collect::<Vec<_>>();
            if actual_headers != expected_headers && actual_headers != legacy_headers {
                return Err(ImportRuleError::InvalidFormat);
            }
            reader
                .deserialize()
                .collect::<Result<Vec<T>, _>>()
                .map_err(|_| ImportRuleError::InvalidFormat)
        }
    }
}

fn decode_json<T: DeserializeOwned>(
    content: &str,
    expected_fields: &[&str],
    legacy_fields: &[&str],
) -> Result<Vec<T>, ImportRuleError> {
    let value: serde_json::Value =
        serde_json::from_str(content).map_err(|_| ImportRuleError::InvalidFormat)?;
    let rows = value.as_array().ok_or(ImportRuleError::InvalidFormat)?;
    let expected = expected_fields.iter().copied().collect::<HashSet<_>>();
    let legacy = legacy_fields.iter().copied().collect::<HashSet<_>>();
    for row in rows {
        let object = row.as_object().ok_or(ImportRuleError::InvalidFormat)?;
        let actual = object.keys().map(String::as_str).collect::<HashSet<_>>();
        if actual != expected && actual != legacy {
            return Err(ImportRuleError::InvalidFormat);
        }
    }
    serde_json::from_value(value).map_err(|_| ImportRuleError::InvalidFormat)
}

fn encode_csv<T: Serialize>(rows: &[T]) -> Result<String, ImportRuleError> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for row in rows {
        writer
            .serialize(row)
            .map_err(|_| ImportRuleError::InvalidFormat)?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|_| ImportRuleError::InvalidFormat)?;
    String::from_utf8(bytes).map_err(|_| ImportRuleError::InvalidFormat)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn sample_rows() -> Vec<TermDocumentRow> {
        vec![
            TermDocumentRow {
                source_lang: "en".to_string(),
                source_text: "Archive".to_string(),
                match_mode: "exact".to_string(),
                translation: "档案".to_string(),
                pos: Some("Noun".to_string()),
                notes: "keep, commas and \"quotes\"".to_string(),
                archived: false,
            },
            TermDocumentRow {
                source_lang: "de-DE-u-co-phonebk".to_string(),
                source_text: "Quelle".to_string(),
                match_mode: "regex".to_string(),
                translation: "来源".to_string(),
                pos: None,
                notes: "legacy".to_string(),
                archived: true,
            },
        ]
    }

    #[test]
    fn term_csv_and_json_round_trip_all_stable_fields() {
        for format in [DocumentFormat::Csv, DocumentFormat::Json] {
            let encoded = encode_term_document(format, &sample_rows()).unwrap();
            let decoded = parse_term_document(format, &encoded, "en").unwrap();
            assert_eq!(decoded, sample_rows());
        }
    }

    #[test]
    fn mixed_term_csv_always_has_source_language_and_archived_columns() {
        let encoded = encode_term_document(DocumentFormat::Csv, &sample_rows()).unwrap();
        let header = encoded.lines().next().unwrap();
        assert_eq!(
            header,
            "source_lang,source_text,match_mode,translation,pos,notes,archived"
        );
        assert!(encoded.contains("de-DE-u-co-phonebk"));
        assert!(encoded.contains(",true"));
    }

    #[test]
    fn term_parser_canonicalizes_before_duplicate_and_active_validation() {
        let duplicate = r#"[
          {"source_lang":"EN","source_text":"same","match_mode":"exact","translation":"a","pos":null,"notes":"","archived":false},
          {"source_lang":"en","source_text":"same","match_mode":"exact","translation":"b","pos":null,"notes":"","archived":false}
        ]"#;
        assert_eq!(
            parse_term_document(DocumentFormat::Json, duplicate, "en"),
            Err(ImportRuleError::DuplicateRow {
                first: 1,
                duplicate: 2
            })
        );

        let invalid =
            "source_lang,source_text,match_mode,translation,pos,notes,archived\nnot_a_tag,x,exact,y,,,true\n";
        assert_eq!(
            parse_term_document(DocumentFormat::Csv, invalid, "en"),
            Err(ImportRuleError::InvalidLanguageTag { row: 1 })
        );

        let non_primary_active =
            "source_lang,source_text,match_mode,translation,pos,notes,archived\nja,x,exact,y,,,false\n";
        assert_eq!(
            parse_term_document(DocumentFormat::Csv, non_primary_active, "en"),
            Err(ImportRuleError::ActiveSourceMismatch { row: 1 })
        );

        let wrong_headers =
            "source_text,source_lang,match_mode,translation,pos,notes,archived\nx,en,exact,y,,,false\n";
        assert_eq!(
            parse_term_document(DocumentFormat::Csv, wrong_headers, "en"),
            Err(ImportRuleError::InvalidFormat)
        );

        let missing_json_field = r#"[{"source_lang":"en","source_text":"x","match_mode":"exact","translation":"y","pos":null,"notes":""}]"#;
        assert_eq!(
            parse_term_document(DocumentFormat::Json, missing_json_field, "en"),
            Err(ImportRuleError::InvalidFormat)
        );
    }

    #[test]
    fn legacy_term_documents_default_missing_match_mode_to_exact() {
        let csv =
            "source_lang,source_text,translation,pos,notes,archived\nen,Archive,档案,Noun,,false\n";
        let json = r#"[{"source_lang":"en","source_text":"Archive","translation":"档案","pos":"Noun","notes":"","archived":false}]"#;
        for (format, input) in [(DocumentFormat::Csv, csv), (DocumentFormat::Json, json)] {
            let rows = parse_term_document(format, input, "en").unwrap();
            assert_eq!(rows[0].match_mode, "exact");
        }
    }

    #[test]
    fn archived_non_project_language_is_preserved_canonically() {
        let input = "source_lang,source_text,match_mode,translation,pos,notes,archived\nde-de-u-co-phonebk,Quelle,exact,来源,,legacy,true\n";
        let rows = parse_term_document(DocumentFormat::Csv, input, "en").unwrap();
        assert_eq!(rows[0].source_lang, "de-DE-u-co-phonebk");
        assert!(rows[0].archived);
    }

    #[test]
    fn pos_csv_and_json_round_trip_bilingual_names_and_order() {
        let rows = vec![
            PosDocumentRow {
                name_zh_cn: Some("名词".to_string()),
                name_en: Some("Noun".to_string()),
                sort_order: 10,
            },
            PosDocumentRow {
                name_zh_cn: None,
                name_en: Some("Verb".to_string()),
                sort_order: 20,
            },
        ];
        for format in [DocumentFormat::Csv, DocumentFormat::Json] {
            let encoded = encode_pos_document(format, &rows).unwrap();
            assert_eq!(parse_pos_document(format, &encoded).unwrap(), rows);
        }
    }

    #[test]
    fn preview_tokens_have_at_least_128_bits_and_digest_is_canonical() {
        let tokens = (0..64)
            .map(|_| generate_preview_token())
            .collect::<HashSet<_>>();
        assert_eq!(tokens.len(), 64);
        assert!(tokens.iter().all(|token| token.len() >= 22));
        assert_eq!(PREVIEW_TTL_SECONDS, 15 * 60);

        let first = canonical_digest(&sample_rows()).unwrap();
        let second = canonical_digest(&sample_rows()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert_ne!(first, canonical_digest(&sample_rows()[..1]).unwrap());
    }
}
