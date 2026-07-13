//! `upload_process` 单文件原子完整替换处理器。
//!
//! JSON 在 blocking parser 中逐项验证并通过 bounded channel 写入 PostgreSQL temp table；
//! 当前词条与 staging 行再按 bounded server-side cursor page 交给
//! `prts-core::upload_replacement` 分类；最终集合 SQL 只消费 typed plan，任何 parser、
//! 权限、历史或统计后置条件失败都会回滚整个文件。

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};

use futures_util::future::BoxFuture;
use prts_core::upload_replacement::{EntryStatsDelta, OriginalText, ReplacementSummary};
use prts_core::EntryState;
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use sqlx::PgConnection;
use tokio::sync::mpsc;

use super::{JobErrorCode, JobExecutionError, JobHandler, JobResult};

const PARSER_CHANNEL_CAPACITY: usize = 64;
const STAGING_BATCH_SIZE: usize = 250;
const PLAN_PAGE_SIZE: i64 = 500;

/// durable worker 注册的真实 upload_process 执行器。
pub struct ProcessUploadHandler {
    db: prts_db::Db,
    temp_root: PathBuf,
}

impl ProcessUploadHandler {
    pub fn new(db: prts_db::Db, temp_root: impl Into<PathBuf>) -> Self {
        Self {
            db,
            temp_root: temp_root.into(),
        }
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, JobExecutionError> {
        let relative = Path::new(key);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(invalid_payload("upload process temp key is invalid"));
        }
        Ok(self.temp_root.join(relative))
    }
}

impl JobHandler for ProcessUploadHandler {
    fn kind(&self) -> &'static str {
        "upload_process"
    }

    fn execute<'a>(
        &'a self,
        job: &'a prts_db::models::Job,
    ) -> BoxFuture<'a, Result<JobResult, JobExecutionError>> {
        Box::pin(async move {
            let project_id = job
                .project_id
                .ok_or_else(|| invalid_payload("upload process lacks project id"))?;
            let payload: ProcessUploadPayload = serde_json::from_value(job.payload.clone())
                .map_err(|_| invalid_payload("upload process payload is invalid"))?;
            if job.upload_batch_file_id != Some(payload.batch_file_id) {
                return Err(invalid_payload(
                    "upload process payload does not match logical file",
                ));
            }

            let mut tx = self.db.begin().await.map_err(database_error)?;
            let context = prts_db::uploads::begin_processing_tx(
                &mut tx,
                job.id,
                project_id,
                payload.batch_id,
                payload.batch_file_id,
                payload.attempt_id,
            )
            .await
            .map_err(database_error)?
            .ok_or_else(|| invalid_payload("upload process state is stale"))?;
            if context.language_repair_state != "ready" {
                return Err(JobExecutionError {
                    code: JobErrorCode::LanguageResolutionRequired,
                    message: "upload project language repair is not ready".to_string(),
                    retryable: false,
                    details: None,
                });
            }
            let temp_path = self.path_for(&context.temp_key)?;
            let target_file =
                prts_db::files::ensure_file_at_path_tx(&mut tx, context.project_id, &context.path)
                    .await
                    .map_err(database_error)?;
            prts_db::entries::create_replacement_temp_tables_tx(&mut tx)
                .await
                .map_err(database_error)?;

            let uploaded_count =
                stage_parsed_upload(&mut tx, temp_path.clone(), context.source_langs.clone())
                    .await?;
            prts_db::entries::lock_replacement_entries_tx(&mut tx, target_file.id)
                .await
                .map_err(database_error)?;
            prts_db::entries::declare_replacement_input_cursor_tx(&mut tx, target_file.id)
                .await
                .map_err(database_error)?;
            let effective_at: chrono::DateTime<chrono::Utc> =
                sqlx::query_scalar("SELECT transaction_timestamp()")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(database_error)?;
            let mut missing_ordinal = uploaded_count;
            let mut summary = ReplacementSummary::default();
            let mut stats_delta = EntryStatsDelta::default();
            loop {
                let page = prts_db::entries::plan_and_stage_replacement_page_tx(
                    &mut tx,
                    &mut missing_ordinal,
                    effective_at,
                    PLAN_PAGE_SIZE,
                )
                .await
                .map_err(database_error)?;
                if !page.has_rows {
                    break;
                }
                add_summary(&mut summary, page.plan.summary);
                stats_delta += page.plan.stats_delta;
            }

            let applied = prts_db::entries::apply_staged_replacement_tx(
                &mut tx,
                context.project_id,
                target_file.id,
                &context.path,
                context.actor_id,
                summary,
                stats_delta,
                effective_at,
            )
            .await
            .map_err(database_error)?;
            let changed_count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM prts_upload_replacement_plan")
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(database_error)?;
            prts_db::audit::append_event_tx(
                &mut tx,
                prts_db::audit::AuditActor {
                    id: Some(context.actor_id),
                    kind: prts_db::audit::AuditActorKind::User,
                    ip: None,
                },
                prts_db::audit::AuditEvent::EntriesUploaded {
                    project_id: context.project_id,
                    file_id: target_file.id,
                    path: &context.path,
                    created: applied.summary.inserted,
                    updated: usize::try_from(changed_count)
                        .unwrap_or(usize::MAX)
                        .saturating_sub(applied.summary.inserted),
                    unchanged: applied.summary.unchanged,
                },
            )
            .await
            .map_err(|_| JobExecutionError {
                code: JobErrorCode::DatabaseUnavailable,
                message: "upload replacement audit failed".to_string(),
                retryable: false,
                details: None,
            })?;
            prts_db::uploads::mark_processing_succeeded_tx(
                &mut tx,
                job.id,
                &context,
                target_file.id,
            )
            .await
            .map_err(database_error)?;
            tx.commit().await.map_err(database_error)?;

            // DB commit 后立即尽力删除 raw temp；失败时 attempt.cleanup_after 仍由 durable
            // cleanup handler 幂等收敛，不能把已提交 replacement 反标失败。
            if let Err(error) = tokio::fs::remove_file(&temp_path).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(%error, job_id = job.id, "upload raw temp cleanup deferred");
                }
            }
            Ok(JobResult::Completed)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessUploadPayload {
    batch_id: i64,
    batch_file_id: i64,
    attempt_id: i64,
}

/// parser 与 async DB consumer 的 bounded bridge；内存只保留固定数量的单项记录。
async fn stage_parsed_upload(
    conn: &mut PgConnection,
    path: PathBuf,
    source_langs: Vec<String>,
) -> Result<i64, JobExecutionError> {
    let (sender, mut receiver) = mpsc::channel(PARSER_CHANNEL_CAPACITY);
    let parser = tokio::task::spawn_blocking(move || parse_upload_file(path, source_langs, sender));
    let mut staged = Vec::with_capacity(STAGING_BATCH_SIZE);
    let mut database_failure: Option<JobExecutionError> = None;
    while let Some(entry) = receiver.recv().await {
        if database_failure.is_some() {
            continue;
        }
        staged.push(entry);
        if staged.len() == STAGING_BATCH_SIZE {
            if let Err(error) = prts_db::entries::stage_replacement_entries_tx(conn, &staged).await
            {
                database_failure = Some(database_error(error));
                receiver.close();
            }
            staged.clear();
        }
    }
    if database_failure.is_none() && !staged.is_empty() {
        if let Err(error) = prts_db::entries::stage_replacement_entries_tx(conn, &staged).await {
            database_failure = Some(database_error(error));
        }
    }
    let parsed = parser.await.map_err(|_| JobExecutionError {
        code: JobErrorCode::UploadInvalidJson,
        message: "upload parser task panicked".to_string(),
        retryable: false,
        details: None,
    })?;
    if let Some(error) = database_failure {
        return Err(error);
    }
    let parsed = parsed.map_err(UploadParseFailure::into_job_error)?;
    if let Some((first_ordinal, duplicate_ordinal)) =
        prts_db::entries::finalize_replacement_staging_tx(conn)
            .await
            .map_err(database_error)?
    {
        return Err(UploadParseFailure::DuplicateKey {
            first_ordinal,
            duplicate_ordinal,
        }
        .into_job_error());
    }
    Ok(parsed)
}

fn parse_upload_file(
    path: PathBuf,
    source_langs: Vec<String>,
    sender: mpsc::Sender<prts_db::entries::ReplacementStagedEntry>,
) -> Result<i64, UploadParseFailure> {
    let file = File::open(path).map_err(|_| UploadParseFailure::TempUnavailable)?;
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    let source_langs = source_langs.into_iter().collect::<HashSet<_>>();
    let mut ordinal = 0_i64;
    let mut validation_failure = None;
    let visitor = UploadArrayVisitor {
        sender,
        source_langs: &source_langs,
        ordinal: &mut ordinal,
        validation_failure: &mut validation_failure,
    };
    let parsed = serde::de::Deserializer::deserialize_seq(&mut deserializer, visitor);
    if let Some(failure) = validation_failure {
        return Err(failure);
    }
    if let Err(error) = parsed {
        return Err(UploadParseFailure::from_serde(error, ordinal));
    }
    if let Err(error) = deserializer.end() {
        return Err(UploadParseFailure::from_serde(error, ordinal));
    }
    Ok(ordinal)
}

struct UploadArrayVisitor<'a> {
    sender: mpsc::Sender<prts_db::entries::ReplacementStagedEntry>,
    source_langs: &'a HashSet<String>,
    ordinal: &'a mut i64,
    validation_failure: &'a mut Option<UploadParseFailure>,
}

impl<'de> Visitor<'de> for UploadArrayVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON array of PRTS upload entries")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(raw) = sequence.next_element::<RawUploadEntry>()? {
            let current = *self.ordinal;
            let entry = match validate_raw_entry(raw, current, self.source_langs) {
                Ok(entry) => entry,
                Err(error) => {
                    *self.validation_failure = Some(error);
                    return Err(serde::de::Error::custom("upload validation failed"));
                }
            };
            if self.sender.blocking_send(entry).is_err() {
                *self.validation_failure = Some(UploadParseFailure::ConsumerClosed);
                return Err(serde::de::Error::custom("upload consumer closed"));
            }
            *self.ordinal += 1;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUploadEntry {
    key: String,
    #[serde(default)]
    original: RawOriginal,
    #[serde(default)]
    translation: Option<String>,
    #[serde(default)]
    state: Option<String>,
    /// 0013 前的兼容字段；接收但从不进入 staging/history。
    #[serde(default, rename = "context")]
    _context: Option<IgnoredAny>,
}

#[derive(Default)]
struct RawOriginal(Vec<(String, String)>);

impl<'de> Deserialize<'de> for RawOriginal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(RawOriginalVisitor)
    }
}

struct RawOriginalVisitor;

impl<'de> Visitor<'de> for RawOriginalVisitor {
    type Value = RawOriginal;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an object mapping BCP-47 tags to source strings")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some((tag, text)) = map.next_entry::<String, String>()? {
            values.push((tag, text));
        }
        Ok(RawOriginal(values))
    }
}

fn validate_raw_entry(
    raw: RawUploadEntry,
    ordinal: i64,
    project_source_langs: &HashSet<String>,
) -> Result<prts_db::entries::ReplacementStagedEntry, UploadParseFailure> {
    if raw.key.is_empty() {
        return Err(UploadParseFailure::InvalidEntry {
            ordinal,
            field: "key",
            line: None,
            column: None,
        });
    }
    let mut original: OriginalText = BTreeMap::new();
    for (raw_tag, text) in raw.original.0 {
        let canonical = prts_core::canonicalize_language_tag(&raw_tag).map_err(|_| {
            UploadParseFailure::InvalidLanguage {
                ordinal,
                reason: "invalid_tag",
            }
        })?;
        if !project_source_langs.contains(&canonical) {
            return Err(UploadParseFailure::SourceLanguageMismatch { ordinal });
        }
        if original.insert(canonical, text).is_some() {
            return Err(UploadParseFailure::InvalidLanguage {
                ordinal,
                reason: "canonical_duplicate",
            });
        }
    }
    let state = match raw.state {
        Some(value) => Some(
            EntryState::parse(&value).ok_or(UploadParseFailure::InvalidEntry {
                ordinal,
                field: "state",
                line: None,
                column: None,
            })?,
        ),
        None => None,
    };
    Ok(prts_db::entries::ReplacementStagedEntry {
        ordinal,
        key: raw.key,
        original,
        translation: raw.translation,
        state,
    })
}

#[derive(Debug)]
enum UploadParseFailure {
    TempUnavailable,
    InvalidJson {
        ordinal: i64,
        line: usize,
        column: usize,
    },
    InvalidEntry {
        ordinal: i64,
        field: &'static str,
        line: Option<usize>,
        column: Option<usize>,
    },
    DuplicateKey {
        first_ordinal: i64,
        duplicate_ordinal: i64,
    },
    InvalidLanguage {
        ordinal: i64,
        reason: &'static str,
    },
    SourceLanguageMismatch {
        ordinal: i64,
    },
    ConsumerClosed,
}

impl UploadParseFailure {
    fn from_serde(error: serde_json::Error, ordinal: i64) -> Self {
        match error.classify() {
            serde_json::error::Category::Data => Self::InvalidEntry {
                ordinal,
                field: "entry",
                line: Some(error.line()),
                column: Some(error.column()),
            },
            serde_json::error::Category::Io => Self::TempUnavailable,
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                Self::InvalidJson {
                    ordinal,
                    line: error.line(),
                    column: error.column(),
                }
            }
        }
    }

    fn into_job_error(self) -> JobExecutionError {
        match self {
            Self::TempUnavailable => JobExecutionError {
                code: JobErrorCode::UploadTempUnavailable,
                message: "upload temp file could not be read".to_string(),
                retryable: false,
                details: None,
            },
            Self::InvalidJson {
                ordinal,
                line,
                column,
            } => JobExecutionError {
                code: JobErrorCode::UploadInvalidJson,
                message: "upload JSON syntax is invalid".to_string(),
                retryable: false,
                details: Some(serde_json::json!({
                    "ordinal": ordinal,
                    "line": line,
                    "column": column,
                })),
            },
            Self::InvalidEntry {
                ordinal,
                field,
                line,
                column,
            } => JobExecutionError {
                code: JobErrorCode::UploadInvalidEntry,
                message: "upload entry shape or state is invalid".to_string(),
                retryable: false,
                details: Some(serde_json::json!({
                    "ordinal": ordinal,
                    "field": field,
                    "line": line,
                    "column": column,
                })),
            },
            Self::DuplicateKey {
                first_ordinal,
                duplicate_ordinal,
            } => JobExecutionError {
                code: JobErrorCode::UploadDuplicateKey,
                message: "upload contains duplicate entry keys".to_string(),
                retryable: false,
                details: Some(serde_json::json!({
                    "first_ordinal": first_ordinal,
                    "duplicate_ordinal": duplicate_ordinal,
                })),
            },
            Self::InvalidLanguage { ordinal, reason } => JobExecutionError {
                code: JobErrorCode::UploadInvalidLanguage,
                message: "upload original language tag is invalid".to_string(),
                retryable: false,
                details: Some(serde_json::json!({
                    "ordinal": ordinal,
                    "reason": reason,
                })),
            },
            Self::SourceLanguageMismatch { ordinal } => JobExecutionError {
                code: JobErrorCode::UploadSourceLanguageMismatch,
                message: "upload original language is outside the project source set".to_string(),
                retryable: false,
                details: Some(serde_json::json!({"ordinal": ordinal})),
            },
            Self::ConsumerClosed => database_error(sqlx::Error::Protocol(
                "upload staging consumer closed".to_string(),
            )),
        }
    }
}

fn add_summary(total: &mut ReplacementSummary, page: ReplacementSummary) {
    total.inserted += page.inserted;
    total.restored += page.restored;
    total.source_changed += page.source_changed;
    total.tombstoned += page.tombstoned;
    total.unchanged += page.unchanged;
}

fn invalid_payload(message: &str) -> JobExecutionError {
    JobExecutionError {
        code: JobErrorCode::InvalidPayload,
        message: message.to_string(),
        retryable: false,
        details: None,
    }
}

fn database_error(error: sqlx::Error) -> JobExecutionError {
    JobExecutionError {
        code: JobErrorCode::DatabaseUnavailable,
        message: format!("upload replacement database operation failed: {error}"),
        retryable: false,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_bytes(
        bytes: &[u8],
        source_langs: &[&str],
    ) -> Result<Vec<prts_db::entries::ReplacementStagedEntry>, UploadParseFailure> {
        let path = std::env::temp_dir().join(format!(
            "prts-upload-parser-{}.json",
            prts_auth::token::random_token(12).to_lowercase()
        ));
        std::fs::write(&path, bytes).unwrap();
        let (sender, mut receiver) = mpsc::channel(16);
        let languages = source_langs
            .iter()
            .map(|value| (*value).to_string())
            .collect();
        let result = parse_upload_file(path.clone(), languages, sender);
        let mut entries = Vec::new();
        while let Ok(entry) = receiver.try_recv() {
            entries.push(entry);
        }
        let _ = std::fs::remove_file(path);
        result.map(|_| entries)
    }

    #[test]
    fn parser_streams_entries_and_ignores_legacy_context() {
        let entries = parse_bytes(
            br#"[{"key":"a","original":{"EN":"Hello"},"translation":"T","state":"translated","context":"legacy"}]"#,
            &["en"],
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].original["en"], "Hello");
        assert_eq!(entries[0].state, Some(EntryState::Translated));
    }

    #[test]
    fn parser_preserves_duplicate_ordinals_for_database_validation() {
        let entries = parse_bytes(
            br#"[{"key":"a","original":{"en":"A"}},{"key":"a","original":{"en":"B"}}]"#,
            &["en"],
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].ordinal, entries[1].ordinal), (0, 1));
        assert_eq!(entries[0].key, entries[1].key);
    }

    #[test]
    fn parser_rejects_canonical_language_duplicates_and_project_mismatch() {
        let duplicate = parse_bytes(
            br#"[{"key":"a","original":{"en-US":"A","EN-us":"B"}}]"#,
            &["en-US"],
        )
        .unwrap_err();
        assert!(matches!(
            duplicate,
            UploadParseFailure::InvalidLanguage {
                ordinal: 0,
                reason: "canonical_duplicate"
            }
        ));

        let mismatch = parse_bytes(br#"[{"key":"a","original":{"ja":"A"}}]"#, &["en"]).unwrap_err();
        assert!(matches!(
            mismatch,
            UploadParseFailure::SourceLanguageMismatch { ordinal: 0 }
        ));
    }

    #[test]
    fn parser_reports_json_line_and_column_without_body_text() {
        let error = parse_bytes(br#"[{"key":"a","original":{"en":"A"}},]"#, &["en"]).unwrap_err();
        let job_error = error.into_job_error();
        assert_eq!(job_error.code, JobErrorCode::UploadInvalidJson);
        let details = job_error.details.unwrap();
        assert!(details["line"].as_u64().is_some());
        assert!(details["column"].as_u64().is_some());
        assert!(!details.to_string().contains("original"));
    }

    #[tokio::test]
    #[ignore = "100MB streaming verify，手动运行"]
    async fn parser_streams_large_file_through_bounded_channel() {
        use std::io::{BufWriter, Write};

        let target_mb: usize = std::env::var("PRTS_UPLOAD_PERF_MB")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(100);
        let source = "x".repeat(1024);
        let entry_count = target_mb * 1024 * 1024 / (source.len() + 80);
        let path = std::env::temp_dir().join(format!(
            "prts-upload-stream-{}.json",
            prts_auth::token::random_token(12).to_lowercase()
        ));
        let mut output = BufWriter::new(File::create(&path).unwrap());
        output.write_all(b"[").unwrap();
        for index in 0..entry_count {
            if index > 0 {
                output.write_all(b",").unwrap();
            }
            write!(
                output,
                "{{\"key\":\"k{index}\",\"original\":{{\"en\":\"{source}\"}}}}"
            )
            .unwrap();
        }
        output.write_all(b"]").unwrap();
        output.flush().unwrap();
        drop(output);

        let (sender, mut receiver) = mpsc::channel(PARSER_CHANNEL_CAPACITY);
        let parse_path = path.clone();
        let parser = tokio::task::spawn_blocking(move || {
            parse_upload_file(parse_path, vec!["en".to_string()], sender)
        });
        let mut received = 0_i64;
        let mut max_buffered = 0_usize;
        while receiver.recv().await.is_some() {
            received += 1;
            max_buffered = max_buffered.max(receiver.len());
        }
        let parsed = parser.await.unwrap().unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(received, parsed);
        assert_eq!(received, entry_count as i64);
        assert!(max_buffered <= PARSER_CHANNEL_CAPACITY);
    }
}
