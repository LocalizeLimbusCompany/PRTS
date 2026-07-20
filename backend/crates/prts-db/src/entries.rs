//! 词条数据访问：批量上传（带 key 覆盖与差异）、键集分页、乐观锁更新、历史。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use prts_core::upload_replacement::{
    plan_transition, EntryHistorySnapshot, EntryStatsDelta, ExistingEntry, OriginalText,
    ReplacementInput, ReplacementPlan, ReplacementPlanError, ReplacementSummary,
    ReplacementTransitionKind, UploadedEntry,
};
use prts_core::{EntryFlags, EntryState};
use serde::Deserialize;
use sqlx::{FromRow, PgConnection, PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::models::{Entry, EntryVersion};

/// 上传词条（来自上传 JSON 的单项）。
#[derive(Debug, Clone, Deserialize)]
pub struct UploadEntry {
    pub key: String,
    /// `{bcp47: 源文本}` 对象。
    #[serde(default)]
    pub original: serde_json::Value,
    #[serde(default)]
    pub translation: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

/// 上传统计。
#[derive(Debug, Default, Clone, Copy)]
pub struct UpsertStats {
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// 流式 parser 已完成 canonical BCP-47 与 duplicate 校验的 staging 行。
#[derive(Debug, Clone)]
pub struct ReplacementStagedEntry {
    pub ordinal: i64,
    pub key: String,
    pub original: OriginalText,
    pub translation: Option<String>,
    pub state: Option<EntryState>,
}

/// 一个 bounded DB page 经 core 分类并写入 plan temp table 后的结果。
#[derive(Debug)]
pub struct ReplacementPlanPage {
    pub has_rows: bool,
    pub plan: ReplacementPlan,
}

/// 单文件 replacement 的持久化结果。
#[derive(Debug, Clone)]
pub struct ReplacementApplyResult {
    pub change_set_id: Uuid,
    pub summary: ReplacementSummary,
    pub stats_delta: EntryStatsDelta,
}

/// full join staging/current entries 时使用的数据库行；分类逻辑不放在 SQL 中。
#[derive(Debug, FromRow)]
struct ReplacementJoinedRow {
    existing_id: Option<i64>,
    existing_key: Option<String>,
    existing_original: Option<serde_json::Value>,
    existing_translation: Option<String>,
    existing_state: Option<String>,
    existing_locked: Option<bool>,
    existing_hidden: Option<bool>,
    existing_questioned: Option<bool>,
    existing_deleted_at: Option<DateTime<Utc>>,
    upload_ordinal: Option<i64>,
    upload_key: Option<String>,
    upload_original: Option<serde_json::Value>,
    upload_translation: Option<String>,
    upload_state: Option<String>,
}

struct ReplacementPlanStagingRow {
    ordinal: i64,
    key: String,
    entry_id: Option<i64>,
    kind: &'static str,
    source_changed: bool,
    after_original: serde_json::Value,
    after_translation: String,
    after_state: &'static str,
    after_locked: bool,
    after_hidden: bool,
    after_questioned: bool,
    before_value: Option<serde_json::Value>,
    after_value: serde_json::Value,
    history_operation: &'static str,
    stats_delta: EntryStatsDelta,
}

#[derive(Debug, FromRow)]
struct ReplacementStagedTotals {
    inserted: i64,
    restored: i64,
    source_changed_rows: i64,
    source_changed: i64,
    tombstoned: i64,
    changed: i64,
    visible_total_delta: i64,
    untranslated_delta: i64,
    translated_delta: i64,
    questioned_delta: i64,
    checked_delta: i64,
    reviewed_delta: i64,
    hidden_total_delta: i64,
    hidden_untranslated_delta: i64,
    hidden_translated_delta: i64,
    hidden_questioned_delta: i64,
    hidden_checked_delta: i64,
    hidden_reviewed_delta: i64,
}

#[derive(Debug, PartialEq, Eq, FromRow)]
struct EditorStatsSnapshot {
    visible_total: i64,
    untranslated: i64,
    translated: i64,
    questioned: i64,
    checked: i64,
    reviewed: i64,
    hidden_total: i64,
    hidden_untranslated: i64,
    hidden_translated: i64,
    hidden_questioned: i64,
    hidden_checked: i64,
    hidden_reviewed: i64,
}

/// 建立当前文件事务专属的 upload staging 与 typed plan 表。
///
/// 两张表均 `ON COMMIT DROP`；解析、分类或应用任一步失败都会随文件事务整体回滚。
pub async fn create_replacement_temp_tables_tx(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TEMP TABLE prts_upload_replacement_entries (
             ordinal BIGINT PRIMARY KEY,
             key TEXT NOT NULL,
             original JSONB NOT NULL,
             translation TEXT,
             state TEXT
         ) ON COMMIT DROP",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "CREATE TEMP TABLE prts_upload_replacement_plan (
             ordinal BIGINT PRIMARY KEY,
             key TEXT NOT NULL UNIQUE,
             entry_id BIGINT,
             kind TEXT NOT NULL,
             source_changed BOOLEAN NOT NULL,
             after_original JSONB NOT NULL,
             after_translation TEXT NOT NULL,
             after_state TEXT NOT NULL,
             after_locked BOOLEAN NOT NULL,
             after_hidden BOOLEAN NOT NULL,
             after_questioned BOOLEAN NOT NULL,
             before_value JSONB,
             after_value JSONB NOT NULL,
             history_operation TEXT NOT NULL,
             visible_total_delta BIGINT NOT NULL,
             untranslated_delta BIGINT NOT NULL,
             translated_delta BIGINT NOT NULL,
             questioned_delta BIGINT NOT NULL,
             checked_delta BIGINT NOT NULL,
             reviewed_delta BIGINT NOT NULL,
             hidden_total_delta BIGINT NOT NULL,
             hidden_untranslated_delta BIGINT NOT NULL,
             hidden_translated_delta BIGINT NOT NULL,
             hidden_questioned_delta BIGINT NOT NULL,
             hidden_checked_delta BIGINT NOT NULL,
             hidden_reviewed_delta BIGINT NOT NULL
         ) ON COMMIT DROP",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "CREATE INDEX prts_upload_replacement_plan_kind_entry_idx
         ON prts_upload_replacement_plan (kind, entry_id)",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "CREATE INDEX prts_upload_replacement_plan_source_entry_idx
         ON prts_upload_replacement_plan (entry_id) WHERE source_changed",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// parser 完成后在 PostgreSQL 内定位最早的重复 key，并为后续 full join 固化唯一索引。
///
/// 查重不把全部 key 留在 worker RSS 中；返回值只包含首个与重复数组 ordinal，不把
/// key 或正文带入 job result。只有无重复时才创建唯一索引。
pub async fn finalize_replacement_staging_tx(
    conn: &mut PgConnection,
) -> Result<Option<(i64, i64)>, sqlx::Error> {
    let duplicate: Option<(i64, i64)> = sqlx::query_as(
        "WITH ranked AS (
             SELECT ordinal,
                    min(ordinal) OVER (PARTITION BY key) AS first_ordinal,
                    row_number() OVER (PARTITION BY key ORDER BY ordinal) AS occurrence
             FROM prts_upload_replacement_entries
         )
         SELECT first_ordinal, ordinal AS duplicate_ordinal
         FROM ranked
         WHERE occurrence = 2
         ORDER BY duplicate_ordinal
         LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await?;
    if duplicate.is_some() {
        return Ok(duplicate);
    }
    sqlx::query(
        "CREATE UNIQUE INDEX prts_upload_replacement_entries_key_idx
         ON prts_upload_replacement_entries (key)",
    )
    .execute(conn)
    .await?;
    Ok(None)
}

/// 把一个 parser 小批参数化写入 staging temp table。
pub async fn stage_replacement_entries_tx(
    conn: &mut PgConnection,
    entries: &[ReplacementStagedEntry],
) -> Result<(), sqlx::Error> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO prts_upload_replacement_entries (
             ordinal, key, original, translation, state
         ) ",
    );
    builder.push_values(entries, |mut row, entry| {
        row.push_bind(entry.ordinal)
            .push_bind(&entry.key)
            .push_bind(
                serde_json::to_value(&entry.original)
                    .expect("canonical original text map must serialize"),
            )
            .push_bind(&entry.translation)
            .push_bind(entry.state.map(|state| state.as_str()));
    });
    builder.build().execute(conn).await?;
    Ok(())
}

/// 在生成 typed plan 前锁定当前文件全部平台词条，防止编辑保存与 replacement 之间
/// 出现 TOCTOU，导致平台译文或 flags 被陈旧计划覆盖。
pub async fn lock_replacement_entries_tx(
    conn: &mut PgConnection,
    file_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT id FROM entries WHERE file_id = $1 ORDER BY id FOR UPDATE")
        .bind(file_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// 为当前文件声明一次性 server-side cursor；full join 只规划一次，后续固定批量 FETCH，
/// 避免 `FULL OUTER JOIN + COALESCE` keyset 每页重复扫描导致 O(N²)。
pub async fn declare_replacement_input_cursor_tx(
    conn: &mut PgConnection,
    file_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DECLARE prts_upload_replacement_cursor NO SCROLL CURSOR FOR
         SELECT entry.id AS existing_id,
                entry.key AS existing_key,
                entry.original AS existing_original,
                entry.translation AS existing_translation,
                entry.state AS existing_state,
                entry.locked AS existing_locked,
                entry.hidden AS existing_hidden,
                entry.questioned AS existing_questioned,
                entry.deleted_at AS existing_deleted_at,
                upload.ordinal AS upload_ordinal,
                upload.key AS upload_key,
                upload.original AS upload_original,
                upload.translation AS upload_translation,
                upload.state AS upload_state
         FROM entries AS entry
         FULL OUTER JOIN prts_upload_replacement_entries AS upload
           ON entry.file_id = $1 AND entry.key = upload.key
         WHERE entry.file_id = $1 OR entry.id IS NULL
         ORDER BY COALESCE(entry.key, upload.key)",
    )
    .bind(file_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// FETCH 一个 bounded cursor page，调用 prts-core 分类，并把 changed transitions 写入 plan 表。
///
/// `missing_ordinal` 从上传数组长度开始递增，保证缺失旧 key 的历史 ordinal 与上传
/// ordinals 不冲突且跨 page 稳定。
pub async fn plan_and_stage_replacement_page_tx(
    conn: &mut PgConnection,
    missing_ordinal: &mut i64,
    effective_at: DateTime<Utc>,
    limit: i64,
) -> Result<ReplacementPlanPage, sqlx::Error> {
    let fetch = format!(
        "FETCH FORWARD {} FROM prts_upload_replacement_cursor",
        limit.clamp(1, 1000)
    );
    let rows: Vec<ReplacementJoinedRow> = sqlx::query_as(&fetch).fetch_all(&mut *conn).await?;
    if rows.is_empty() {
        return Ok(ReplacementPlanPage {
            has_rows: false,
            plan: ReplacementPlan::default(),
        });
    }

    let mut plan = ReplacementPlan::default();
    let mut staged_transitions = Vec::with_capacity(rows.len());
    for row in rows {
        let existing_deleted_at = row.existing_deleted_at;
        let input = joined_row_to_input(row, missing_ordinal)?;
        let transition = plan_transition(input).map_err(replacement_plan_error)?;
        let mut item_plan = ReplacementPlan::default();
        item_plan.transitions.push(transition.clone());
        match transition.kind {
            ReplacementTransitionKind::Insert => item_plan.summary.inserted = 1,
            ReplacementTransitionKind::Restore { source_changed } => {
                item_plan.summary.restored = 1;
                item_plan.summary.source_changed = usize::from(source_changed);
            }
            ReplacementTransitionKind::SourceChanged => item_plan.summary.source_changed = 1,
            ReplacementTransitionKind::Tombstone => item_plan.summary.tombstoned = 1,
            ReplacementTransitionKind::Unchanged => item_plan.summary.unchanged = 1,
        }
        item_plan.stats_delta = transition.stats_delta;
        if transition.history.is_some() {
            staged_transitions.push(planned_transition_staging_row(
                &transition,
                existing_deleted_at,
                effective_at,
            )?);
        }
        plan.merge(item_plan);
    }
    stage_planned_transitions_tx(&mut *conn, &staged_transitions).await?;
    Ok(ReplacementPlanPage {
        has_rows: true,
        plan,
    })
}

fn joined_row_to_input(
    row: ReplacementJoinedRow,
    missing_ordinal: &mut i64,
) -> Result<ReplacementInput, sqlx::Error> {
    let existing = match row.existing_id {
        Some(id) => Some(ExistingEntry {
            id,
            key: row.existing_key.ok_or_else(|| {
                sqlx::Error::Protocol("replacement existing row has no key".to_string())
            })?,
            original: decode_original(row.existing_original.ok_or_else(|| {
                sqlx::Error::Protocol("replacement existing row has no original".to_string())
            })?)?,
            translation: row.existing_translation.ok_or_else(|| {
                sqlx::Error::Protocol("replacement existing row has no translation".to_string())
            })?,
            state: decode_state(row.existing_state.as_deref())?,
            flags: EntryFlags {
                locked: row.existing_locked.unwrap_or(false),
                hidden: row.existing_hidden.unwrap_or(false),
                questioned: row.existing_questioned.unwrap_or(false),
            },
            deleted: row.existing_deleted_at.is_some(),
        }),
        None => None,
    };
    let uploaded = match row.upload_ordinal {
        Some(ordinal) => Some(UploadedEntry {
            ordinal,
            key: row.upload_key.ok_or_else(|| {
                sqlx::Error::Protocol("replacement upload row has no key".to_string())
            })?,
            original: decode_original(row.upload_original.ok_or_else(|| {
                sqlx::Error::Protocol("replacement upload row has no original".to_string())
            })?)?,
            translation: row.upload_translation,
            state: match row.upload_state.as_deref() {
                Some(state) => Some(decode_state(Some(state))?),
                None => None,
            },
        }),
        None => None,
    };
    let current_missing_ordinal = if uploaded.is_none() {
        let ordinal = *missing_ordinal;
        *missing_ordinal += 1;
        ordinal
    } else {
        0
    };
    Ok(ReplacementInput {
        existing,
        uploaded,
        missing_ordinal: current_missing_ordinal,
    })
}

fn decode_original(value: serde_json::Value) -> Result<OriginalText, sqlx::Error> {
    serde_json::from_value(value)
        .map_err(|_| sqlx::Error::Protocol("replacement original is not a string map".to_string()))
}

fn decode_state(value: Option<&str>) -> Result<EntryState, sqlx::Error> {
    value
        .and_then(EntryState::parse)
        .ok_or_else(|| sqlx::Error::Protocol("replacement entry state is invalid".to_string()))
}

fn replacement_plan_error(error: ReplacementPlanError) -> sqlx::Error {
    sqlx::Error::Protocol(format!("replacement plan input is invalid: {error:?}"))
}

fn planned_transition_staging_row(
    transition: &prts_core::upload_replacement::PlannedEntryTransition,
    existing_deleted_at: Option<DateTime<Utc>>,
    effective_at: DateTime<Utc>,
) -> Result<ReplacementPlanStagingRow, sqlx::Error> {
    let history = transition.history.as_ref().ok_or_else(|| {
        sqlx::Error::Protocol("changed replacement transition has no history".to_string())
    })?;
    let kind = match transition.kind {
        ReplacementTransitionKind::Insert => "insert",
        ReplacementTransitionKind::Restore { .. } => "restore",
        ReplacementTransitionKind::SourceChanged => "source_changed",
        ReplacementTransitionKind::Tombstone => "tombstone",
        ReplacementTransitionKind::Unchanged => {
            return Err(sqlx::Error::Protocol(
                "unchanged transition cannot be staged".to_string(),
            ));
        }
    };
    let source_changed = matches!(
        transition.kind,
        ReplacementTransitionKind::SourceChanged
            | ReplacementTransitionKind::Restore {
                source_changed: true
            }
    );
    let before_value = history
        .before
        .as_ref()
        .map(|snapshot| history_snapshot_json(snapshot, existing_deleted_at));
    let after_deleted_at = transition.after.deleted.then_some(effective_at);
    let after_value = history_snapshot_json(&transition.after, after_deleted_at);
    let history_operation = match history.operation {
        prts_core::upload_replacement::ReplacementHistoryOperation::Create => "create",
        prts_core::upload_replacement::ReplacementHistoryOperation::Update => "update",
        prts_core::upload_replacement::ReplacementHistoryOperation::Restore => "restore",
        prts_core::upload_replacement::ReplacementHistoryOperation::Tombstone => "tombstone",
    };
    Ok(ReplacementPlanStagingRow {
        ordinal: transition.ordinal,
        key: transition.key.clone(),
        entry_id: transition.entry_id,
        kind,
        source_changed,
        after_original: serde_json::to_value(&transition.after.original)
            .expect("planned original map must serialize"),
        after_translation: transition.after.translation.clone(),
        after_state: transition.after.state.as_str(),
        after_locked: transition.after.locked,
        after_hidden: transition.after.hidden,
        after_questioned: transition.after.questioned,
        before_value,
        after_value,
        history_operation,
        stats_delta: transition.stats_delta,
    })
}

async fn stage_planned_transitions_tx(
    conn: &mut PgConnection,
    transitions: &[ReplacementPlanStagingRow],
) -> Result<(), sqlx::Error> {
    if transitions.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO prts_upload_replacement_plan (
             ordinal, key, entry_id, kind, source_changed,
             after_original, after_translation, after_state, after_locked, after_hidden,
             after_questioned,
             before_value, after_value, history_operation,
             visible_total_delta, untranslated_delta, translated_delta,
             questioned_delta, checked_delta, reviewed_delta,
             hidden_total_delta, hidden_untranslated_delta, hidden_translated_delta,
             hidden_questioned_delta, hidden_checked_delta, hidden_reviewed_delta
         ) ",
    );
    builder.push_values(transitions, |mut row, transition| {
        row.push_bind(transition.ordinal)
            .push_bind(&transition.key)
            .push_bind(transition.entry_id)
            .push_bind(transition.kind)
            .push_bind(transition.source_changed)
            .push_bind(&transition.after_original)
            .push_bind(&transition.after_translation)
            .push_bind(transition.after_state)
            .push_bind(transition.after_locked)
            .push_bind(transition.after_hidden)
            .push_bind(transition.after_questioned)
            .push_bind(&transition.before_value)
            .push_bind(&transition.after_value)
            .push_bind(transition.history_operation)
            .push_bind(transition.stats_delta.visible_total)
            .push_bind(transition.stats_delta.untranslated)
            .push_bind(transition.stats_delta.translated)
            .push_bind(transition.stats_delta.questioned)
            .push_bind(transition.stats_delta.checked)
            .push_bind(transition.stats_delta.reviewed)
            .push_bind(transition.stats_delta.hidden_total)
            .push_bind(transition.stats_delta.hidden_untranslated)
            .push_bind(transition.stats_delta.hidden_translated)
            .push_bind(transition.stats_delta.hidden_questioned)
            .push_bind(transition.stats_delta.hidden_checked)
            .push_bind(transition.stats_delta.hidden_reviewed);
    });
    builder.build().execute(conn).await?;
    Ok(())
}

fn history_snapshot_json(
    snapshot: &EntryHistorySnapshot,
    deleted_at: Option<DateTime<Utc>>,
) -> serde_json::Value {
    serde_json::json!({
        "key": snapshot.key,
        "original": snapshot.original,
        "translation": snapshot.translation,
        "state": snapshot.state.as_str(),
        "locked": snapshot.locked,
        "hidden": snapshot.hidden,
        "questioned": snapshot.questioned,
        "deleted_at": deleted_at,
    })
}

/// 按已 staging 的 typed plan 执行集合 SQL、写 file history 并验证统计后置条件。
#[allow(clippy::too_many_arguments)]
pub async fn apply_staged_replacement_tx(
    conn: &mut PgConnection,
    project_id: i64,
    file_id: i64,
    path: &str,
    actor_id: i64,
    summary: ReplacementSummary,
    stats_delta: EntryStatsDelta,
    effective_at: DateTime<Utc>,
) -> Result<ReplacementApplyResult, sqlx::Error> {
    let staged: ReplacementStagedTotals = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE kind = 'insert')::BIGINT AS inserted,
                count(*) FILTER (WHERE kind = 'restore')::BIGINT AS restored,
                count(*) FILTER (WHERE kind = 'source_changed')::BIGINT
                    AS source_changed_rows,
                count(*) FILTER (WHERE source_changed)::BIGINT AS source_changed,
                count(*) FILTER (WHERE kind = 'tombstone')::BIGINT AS tombstoned,
                count(*)::BIGINT AS changed,
                COALESCE(sum(visible_total_delta), 0)::BIGINT AS visible_total_delta,
                COALESCE(sum(untranslated_delta), 0)::BIGINT AS untranslated_delta,
                COALESCE(sum(translated_delta), 0)::BIGINT AS translated_delta,
                COALESCE(sum(questioned_delta), 0)::BIGINT AS questioned_delta,
                COALESCE(sum(checked_delta), 0)::BIGINT AS checked_delta,
                COALESCE(sum(reviewed_delta), 0)::BIGINT AS reviewed_delta,
                COALESCE(sum(hidden_total_delta), 0)::BIGINT AS hidden_total_delta,
                COALESCE(sum(hidden_untranslated_delta), 0)::BIGINT
                    AS hidden_untranslated_delta,
                COALESCE(sum(hidden_translated_delta), 0)::BIGINT
                    AS hidden_translated_delta,
                COALESCE(sum(hidden_questioned_delta), 0)::BIGINT
                    AS hidden_questioned_delta,
                COALESCE(sum(hidden_checked_delta), 0)::BIGINT AS hidden_checked_delta,
                COALESCE(sum(hidden_reviewed_delta), 0)::BIGINT AS hidden_reviewed_delta
         FROM prts_upload_replacement_plan",
    )
    .fetch_one(&mut *conn)
    .await?;
    let expected_summary = (
        i64::try_from(summary.inserted),
        i64::try_from(summary.restored),
        i64::try_from(summary.source_changed),
        i64::try_from(summary.tombstoned),
    );
    let (Ok(inserted), Ok(restored), Ok(source_changed), Ok(tombstoned)) = expected_summary else {
        return Err(sqlx::Error::Protocol(
            "replacement summary exceeds database count range".to_string(),
        ));
    };
    if (
        staged.inserted,
        staged.restored,
        staged.source_changed,
        staged.tombstoned,
    ) != (inserted, restored, source_changed, tombstoned)
    {
        return Err(sqlx::Error::Protocol(format!(
            "replacement staged summary mismatch: staged={staged:?}, summary={summary:?}"
        )));
    }
    let staged_stats = EntryStatsDelta {
        visible_total: staged.visible_total_delta,
        untranslated: staged.untranslated_delta,
        translated: staged.translated_delta,
        questioned: staged.questioned_delta,
        checked: staged.checked_delta,
        reviewed: staged.reviewed_delta,
        hidden_total: staged.hidden_total_delta,
        hidden_untranslated: staged.hidden_untranslated_delta,
        hidden_translated: staged.hidden_translated_delta,
        hidden_questioned: staged.hidden_questioned_delta,
        hidden_checked: staged.hidden_checked_delta,
        hidden_reviewed: staged.hidden_reviewed_delta,
    };
    if staged_stats != stats_delta {
        return Err(sqlx::Error::Protocol(format!(
            "replacement staged stats mismatch: staged={staged_stats:?}, summary={stats_delta:?}"
        )));
    }
    let before_stats: EditorStatsSnapshot = sqlx::query_as(
        "SELECT visible_total,
                untranslated_count AS untranslated,
                translated_count AS translated,
                questioned_count AS questioned,
                checked_count AS checked,
                reviewed_count AS reviewed,
                hidden_total,
                hidden_untranslated_count AS hidden_untranslated,
                hidden_translated_count AS hidden_translated,
                hidden_questioned_count AS hidden_questioned,
                hidden_checked_count AS hidden_checked,
                hidden_reviewed_count AS hidden_reviewed
         FROM file_stats WHERE file_id = $1 FOR UPDATE",
    )
    .bind(file_id)
    .fetch_one(&mut *conn)
    .await?;
    let change_set_id = Uuid::new_v4();
    let metadata = serde_json::json!({
        "inserted": summary.inserted,
        "restored": summary.restored,
        "source_changed": summary.source_changed,
        "tombstoned": summary.tombstoned,
        "unchanged": summary.unchanged,
        "stats_delta": {
            "visible_total": stats_delta.visible_total,
            "untranslated": stats_delta.untranslated,
            "translated": stats_delta.translated,
            "questioned": stats_delta.questioned,
            "checked": stats_delta.checked,
            "reviewed": stats_delta.reviewed,
            "hidden_total": stats_delta.hidden_total,
            "hidden_untranslated": stats_delta.hidden_untranslated,
            "hidden_translated": stats_delta.hidden_translated,
            "hidden_questioned": stats_delta.hidden_questioned,
            "hidden_checked": stats_delta.hidden_checked,
            "hidden_reviewed": stats_delta.hidden_reviewed,
        },
    });
    sqlx::query(
        "INSERT INTO file_change_sets (
             id, project_id, file_id, actor_id, operation, path_snapshot, metadata
         ) VALUES ($1, $2, $3, $4, 'upload_replace', $5, $6)",
    )
    .bind(change_set_id)
    .bind(project_id)
    .bind(file_id)
    .bind(actor_id)
    .bind(path)
    .bind(metadata)
    .execute(&mut *conn)
    .await?;

    // 同一 replacement 可能触碰 20 万行。foundation 的通用 row trigger 会为每条
    // visible entry 反复更新相同两行 stats，形成超长 MVCC chain。事务内暂时把已锁定
    // target file 标记为 deleted，使 entry trigger 的共享 effective-visible 谓词返回 false；
    // 该状态不会提交、外部 MVCC 读仍看见 active file，最终统计只应用 core 的集合 delta。
    let isolated = sqlx::query(
        "UPDATE files
         SET deleted_at = $2, deleted_by = $3, purge_after = $2,
             deletion_change_set_id = $4
         WHERE id = $1 AND project_id = $5 AND deleted_at IS NULL",
    )
    .bind(file_id)
    .bind(effective_at)
    .bind(actor_id)
    .bind(change_set_id)
    .bind(project_id)
    .execute(&mut *conn)
    .await?;
    if isolated.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "replacement target file is not active".to_string(),
        ));
    }

    let inserted_rows = sqlx::query(
        "WITH inserted AS (
             INSERT INTO entries (
                 file_id, project_id, key, original, translation, state, locked, hidden,
                 questioned,
                 updated_by
             )
             SELECT $1, $2, plan.key, plan.after_original, plan.after_translation,
                    plan.after_state, plan.after_locked, plan.after_hidden,
                    plan.after_questioned, $3
             FROM prts_upload_replacement_plan AS plan
             WHERE plan.kind = 'insert'
             ORDER BY plan.ordinal
             RETURNING id, key
         )
         UPDATE prts_upload_replacement_plan AS plan
         SET entry_id = inserted.id
         FROM inserted
         WHERE plan.kind = 'insert' AND plan.key = inserted.key",
    )
    .bind(file_id)
    .bind(project_id)
    .bind(actor_id)
    .execute(&mut *conn)
    .await?;
    verify_replacement_rows("insert", inserted_rows.rows_affected(), staged.inserted)?;

    // Existing entries created by older upload paths may not have a snapshot for their current
    // version. Preserve that pre-upload baseline once so the first source diff has a left side;
    // entries already saved/flagged at this version reuse their existing complete snapshot.
    sqlx::query(
        "INSERT INTO entry_versions (
             entry_id, version, kind, translation, state, questioned, original, editor_id,
             editor_name, editor_avatar_url
         )
         SELECT entry.id, entry.version, 'baseline', entry.translation,
                entry.state, entry.questioned, entry.original, actor.id, actor.username, actor.avatar_url
         FROM entries AS entry
         JOIN prts_upload_replacement_plan AS plan ON plan.entry_id = entry.id
         LEFT JOIN users AS actor ON actor.id = entry.updated_by
         WHERE plan.source_changed
           AND NOT EXISTS (
               SELECT 1 FROM entry_versions AS existing
               WHERE existing.entry_id = entry.id AND existing.version = entry.version
           )",
    )
    .execute(&mut *conn)
    .await?;

    let source_rows = sqlx::query(
        "UPDATE entries AS entry
         SET original = plan.after_original,
             translation = plan.after_translation,
             state = plan.after_state,
             locked = plan.after_locked,
             hidden = plan.after_hidden,
             questioned = plan.after_questioned,
             version = entry.version + 1,
             updated_by = $1
         FROM prts_upload_replacement_plan AS plan
         WHERE plan.entry_id = entry.id AND plan.kind = 'source_changed'",
    )
    .bind(actor_id)
    .execute(&mut *conn)
    .await?;
    verify_replacement_rows(
        "source update",
        source_rows.rows_affected(),
        staged.source_changed_rows,
    )?;

    let restored_rows = sqlx::query(
        "UPDATE entries AS entry
         SET original = plan.after_original,
             translation = plan.after_translation,
             state = plan.after_state,
             locked = plan.after_locked,
             hidden = plan.after_hidden,
             questioned = plan.after_questioned,
             deleted_at = NULL,
             deleted_by = NULL,
             deletion_change_set_id = NULL,
             version = entry.version + 1,
             updated_by = $1
         FROM prts_upload_replacement_plan AS plan
         WHERE plan.entry_id = entry.id AND plan.kind = 'restore'",
    )
    .bind(actor_id)
    .execute(&mut *conn)
    .await?;
    verify_replacement_rows("restore", restored_rows.rows_affected(), staged.restored)?;

    // Record the post-upload state at the new version. This snapshot must be written after both
    // active source updates and source-changing restores, otherwise a later translator edit would
    // make the source diff appear under the translator instead of the uploader.
    let source_version_rows = sqlx::query(
        "INSERT INTO entry_versions (
             entry_id, version, kind, translation, state, questioned, original, editor_id,
             editor_name, editor_avatar_url
         )
         SELECT entry.id, entry.version, 'source_update', entry.translation,
                entry.state, entry.questioned, entry.original, actor.id, actor.username, actor.avatar_url
         FROM entries AS entry
         JOIN prts_upload_replacement_plan AS plan ON plan.entry_id = entry.id
         LEFT JOIN users AS actor ON actor.id = $1
         WHERE plan.source_changed",
    )
    .bind(actor_id)
    .execute(&mut *conn)
    .await?;
    verify_replacement_rows(
        "source version",
        source_version_rows.rows_affected(),
        staged.source_changed,
    )?;

    let tombstoned_rows = sqlx::query(
        "UPDATE entries AS entry
         SET deleted_at = $1,
             deleted_by = $2,
             deletion_change_set_id = $3,
             version = entry.version + 1,
             updated_by = $2
         FROM prts_upload_replacement_plan AS plan
         WHERE plan.entry_id = entry.id AND plan.kind = 'tombstone'",
    )
    .bind(effective_at)
    .bind(actor_id)
    .bind(change_set_id)
    .execute(&mut *conn)
    .await?;
    verify_replacement_rows(
        "tombstone",
        tombstoned_rows.rows_affected(),
        staged.tombstoned,
    )?;

    let history_rows = sqlx::query(
        "INSERT INTO file_change_items (
             change_set_id, entity_type, entity_id_snapshot, operation,
             before_value, after_value, ordinal
         )
         SELECT $1, 'entry', entry_id, history_operation,
                before_value, after_value, ordinal
         FROM prts_upload_replacement_plan
         ORDER BY ordinal",
    )
    .bind(change_set_id)
    .execute(&mut *conn)
    .await?;
    verify_replacement_rows("history", history_rows.rows_affected(), staged.changed)?;

    sqlx::query(
        "UPDATE file_stats SET
             visible_total = visible_total + $2,
             untranslated_count = untranslated_count + $3,
             translated_count = translated_count + $4,
             questioned_count = questioned_count + $5,
             checked_count = checked_count + $6,
             reviewed_count = reviewed_count + $7,
             hidden_total = hidden_total + $8,
             hidden_untranslated_count = hidden_untranslated_count + $9,
             hidden_translated_count = hidden_translated_count + $10,
             hidden_questioned_count = hidden_questioned_count + $11,
             hidden_checked_count = hidden_checked_count + $12,
             hidden_reviewed_count = hidden_reviewed_count + $13,
             updated_at = now()
         WHERE file_id = $1",
    )
    .bind(file_id)
    .bind(stats_delta.visible_total)
    .bind(stats_delta.untranslated)
    .bind(stats_delta.translated)
    .bind(stats_delta.questioned)
    .bind(stats_delta.checked)
    .bind(stats_delta.reviewed)
    .bind(stats_delta.hidden_total)
    .bind(stats_delta.hidden_untranslated)
    .bind(stats_delta.hidden_translated)
    .bind(stats_delta.hidden_questioned)
    .bind(stats_delta.hidden_checked)
    .bind(stats_delta.hidden_reviewed)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE project_stats SET
             visible_total = visible_total + $2,
             untranslated_count = untranslated_count + $3,
             translated_count = translated_count + $4,
             questioned_count = questioned_count + $5,
             checked_count = checked_count + $6,
             reviewed_count = reviewed_count + $7,
             hidden_total = hidden_total + $8,
             hidden_untranslated_count = hidden_untranslated_count + $9,
             hidden_translated_count = hidden_translated_count + $10,
             hidden_questioned_count = hidden_questioned_count + $11,
             hidden_checked_count = hidden_checked_count + $12,
             hidden_reviewed_count = hidden_reviewed_count + $13,
             updated_at = now()
         WHERE project_id = $1",
    )
    .bind(project_id)
    .bind(stats_delta.visible_total)
    .bind(stats_delta.untranslated)
    .bind(stats_delta.translated)
    .bind(stats_delta.questioned)
    .bind(stats_delta.checked)
    .bind(stats_delta.reviewed)
    .bind(stats_delta.hidden_total)
    .bind(stats_delta.hidden_untranslated)
    .bind(stats_delta.hidden_translated)
    .bind(stats_delta.hidden_questioned)
    .bind(stats_delta.hidden_checked)
    .bind(stats_delta.hidden_reviewed)
    .execute(&mut *conn)
    .await?;
    let restored_file = sqlx::query(
        "UPDATE files
         SET deleted_at = NULL, deleted_by = NULL, purge_after = NULL,
             deletion_change_set_id = NULL
         WHERE id = $1 AND deletion_change_set_id = $2",
    )
    .bind(file_id)
    .bind(change_set_id)
    .execute(&mut *conn)
    .await?;
    if restored_file.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "replacement target file isolation was not cleared".to_string(),
        ));
    }
    super::files::refresh_entry_count_tx(&mut *conn, file_id).await?;
    let after_stats: EditorStatsSnapshot = sqlx::query_as(
        "SELECT visible_total,
                untranslated_count AS untranslated,
                translated_count AS translated,
                questioned_count AS questioned,
                checked_count AS checked,
                reviewed_count AS reviewed,
                hidden_total,
                hidden_untranslated_count AS hidden_untranslated,
                hidden_translated_count AS hidden_translated,
                hidden_questioned_count AS hidden_questioned,
                hidden_checked_count AS hidden_checked,
                hidden_reviewed_count AS hidden_reviewed
         FROM file_stats WHERE file_id = $1",
    )
    .bind(file_id)
    .fetch_one(&mut *conn)
    .await?;
    let expected = EditorStatsSnapshot {
        visible_total: before_stats.visible_total + stats_delta.visible_total,
        untranslated: before_stats.untranslated + stats_delta.untranslated,
        translated: before_stats.translated + stats_delta.translated,
        questioned: before_stats.questioned + stats_delta.questioned,
        checked: before_stats.checked + stats_delta.checked,
        reviewed: before_stats.reviewed + stats_delta.reviewed,
        hidden_total: before_stats.hidden_total + stats_delta.hidden_total,
        hidden_untranslated: before_stats.hidden_untranslated + stats_delta.hidden_untranslated,
        hidden_translated: before_stats.hidden_translated + stats_delta.hidden_translated,
        hidden_questioned: before_stats.hidden_questioned + stats_delta.hidden_questioned,
        hidden_checked: before_stats.hidden_checked + stats_delta.hidden_checked,
        hidden_reviewed: before_stats.hidden_reviewed + stats_delta.hidden_reviewed,
    };
    if after_stats != expected {
        return Err(sqlx::Error::Protocol(format!(
            "replacement stats postcondition failed: expected {expected:?}, got {after_stats:?}"
        )));
    }
    super::tasks::recompute_for_file_ids_tx(conn, &[file_id]).await?;
    Ok(ReplacementApplyResult {
        change_set_id,
        summary,
        stats_delta,
    })
}

fn verify_replacement_rows(operation: &str, actual: u64, expected: i64) -> Result<(), sqlx::Error> {
    if expected < 0 || actual != expected as u64 {
        return Err(sqlx::Error::Protocol(format!(
            "replacement {operation} affected {actual} rows; expected {expected}"
        )));
    }
    Ok(())
}

/// 词条列表筛选条件。
#[derive(Debug, Default, Clone)]
pub struct EntryFilter {
    pub file_id: Option<i64>,
    pub task_id: Option<i64>,
    pub states: Vec<String>,
    /// 独立于 workflow state 的有疑问标签过滤。
    pub questioned: Option<bool>,
    /// 关键字（P2 用 ILIKE；语义/全文检索见 P4）。
    pub query: Option<String>,
    pub include_hidden: bool,
}

/// 在低频项目重建事务内统计全部词条，作为一次性 job 进度基线。
pub async fn count_project_entries_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM entries WHERE project_id = $1")
        .bind(project_id)
        .fetch_one(conn)
        .await
}

fn normalize_state(s: Option<&str>) -> &'static str {
    match s {
        Some("translated") => "translated",
        Some("checked") => "checked",
        Some("reviewed") => "reviewed",
        _ => "untranslated",
    }
}

/// 测试夹具与性能基准专用的增量造数 helper；生产上传不得调用。
///
/// 用户可见的旧/新上传入口都必须走 `prts-core::upload_replacement` typed plan，
/// 以保证 replacement/restore/tombstone/state reset 只有一套领域真值。这里保留
/// patch 语义，仅用于测试分批构造大数据集，避免每个 seed batch 把前批词条 tombstone。
///
/// 造数规则：
/// - key 不存在 → 插入；
/// - key 存在且源文(original)有变化 → 覆盖源文、置未翻译、版本+1、**保留旧译文**，并记一条历史快照；
/// - key 存在且源文未变 → 不动。
///
/// 分批事务处理，便于承载大文件（性能可后续以 UNNEST/COPY 进一步优化）。
pub async fn bulk_upsert(
    pool: &PgPool,
    file_id: i64,
    project_id: i64,
    entries: &[UploadEntry],
    editor_id: Option<i64>,
) -> Result<UpsertStats, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let stats = bulk_upsert_tx(&mut tx, file_id, project_id, entries, editor_id).await?;
    tx.commit().await?;
    Ok(stats)
}

/// 测试夹具在调用方事务内执行增量 seed；不得作为生产上传 writer 使用。
pub async fn bulk_upsert_tx(
    conn: &mut PgConnection,
    file_id: i64,
    project_id: i64,
    entries: &[UploadEntry],
    editor_id: Option<i64>,
) -> Result<UpsertStats, sqlx::Error> {
    let mut stats = UpsertStats::default();

    for chunk in entries.chunks(500) {
        let keys: Vec<String> = chunk.iter().map(|e| e.key.clone()).collect();
        let existing_rows: Vec<(i64, String, serde_json::Value)> = sqlx::query_as(
            "SELECT id, key, original FROM entries WHERE file_id = $1 AND key = ANY($2)",
        )
        .bind(file_id)
        .bind(&keys)
        .fetch_all(&mut *conn)
        .await?;

        let mut existing: HashMap<String, (i64, serde_json::Value)> = HashMap::new();
        for (id, key, original) in existing_rows {
            existing.insert(key, (id, original));
        }

        let mut to_insert: Vec<&UploadEntry> = Vec::new();
        for e in chunk {
            match existing.get(&e.key) {
                None => to_insert.push(e),
                Some((id, old_original)) => {
                    if old_original != &e.original {
                        // 旧上传路径可能尚无当前版本快照；只在缺失时补基线。
                        sqlx::query(
                            "INSERT INTO entry_versions (
                                 entry_id, version, kind, translation, state, questioned, original, editor_id,
                                 editor_name, editor_avatar_url
                             )
                             SELECT entry.id, entry.version, 'baseline', entry.translation,
                                    entry.state, entry.questioned, entry.original, actor.id, actor.username,
                                    actor.avatar_url
                             FROM entries AS entry
                             LEFT JOIN users AS actor ON actor.id = entry.updated_by
                             WHERE entry.id = $1
                               AND NOT EXISTS (
                                   SELECT 1 FROM entry_versions AS existing
                                   WHERE existing.entry_id = entry.id
                                     AND existing.version = entry.version
                               )",
                        )
                        .bind(id)
                        .execute(&mut *conn)
                        .await?;
                        // 覆盖源文、置未翻译、版本+1（保留 translation）
                        sqlx::query(
                            "UPDATE entries SET original = $2, state = 'untranslated',
                                 version = version + 1, updated_by = $3 WHERE id = $1",
                        )
                        .bind(id)
                        .bind(&e.original)
                        .bind(editor_id)
                        .execute(&mut *conn)
                        .await?;
                        // 立即记录上传后的新版本，避免后续编辑者被误标为源文变更操作者。
                        sqlx::query(
                            "INSERT INTO entry_versions (
                                 entry_id, version, kind, translation, state, questioned, original, editor_id,
                                 editor_name, editor_avatar_url
                             )
                             SELECT entry.id, entry.version, 'source_update', entry.translation,
                                    entry.state, entry.questioned, entry.original, actor.id, actor.username,
                                    actor.avatar_url
                             FROM entries AS entry
                             LEFT JOIN users AS actor ON actor.id = $2
                             WHERE entry.id = $1",
                        )
                        .bind(id)
                        .bind(editor_id)
                        .execute(&mut *conn)
                        .await?;
                        stats.updated += 1;
                    } else {
                        stats.unchanged += 1;
                    }
                }
            }
        }

        if !to_insert.is_empty() {
            let mut qb = QueryBuilder::<Postgres>::new(
                "INSERT INTO entries (file_id, project_id, key, original, translation, state) ",
            );
            qb.push_values(to_insert.iter(), |mut b, e| {
                b.push_bind(file_id)
                    .push_bind(project_id)
                    .push_bind(&e.key)
                    .push_bind(&e.original)
                    .push_bind(e.translation.clone().unwrap_or_default())
                    .push_bind(normalize_state(e.state.as_deref()));
            });
            qb.build().execute(&mut *conn).await?;
            stats.created += to_insert.len();
        }
    }

    super::tasks::recompute_for_file_ids_tx(conn, &[file_id]).await?;

    Ok(stats)
}

/// 键集分页列出词条（按 id 游标）。
pub async fn list(
    pool: &PgPool,
    project_id: i64,
    filter: &EntryFilter,
    after_id: Option<i64>,
    limit: i64,
) -> Result<Vec<Entry>, sqlx::Error> {
    let mut qb = QueryBuilder::<Postgres>::new(
        "SELECT entry.* FROM entries AS entry
         JOIN files AS file ON file.id = entry.file_id
         WHERE entry.project_id = ",
    );
    qb.push_bind(project_id);
    qb.push(" AND prts_entry_effective_visible(entry.id, ")
        .push_bind(filter.include_hidden)
        .push(")");

    if let Some(fid) = filter.file_id {
        qb.push(" AND entry.file_id = ");
        qb.push_bind(fid);
    }
    if let Some(task_id) = filter.task_id {
        qb.push(
            " AND EXISTS (
                 SELECT 1 FROM task_files AS task_file
                 WHERE task_file.task_id = ",
        );
        qb.push_bind(task_id);
        qb.push(" AND task_file.live_file_id = entry.file_id)");
    }
    if !filter.states.is_empty() {
        qb.push(" AND entry.state = ANY(");
        qb.push_bind(filter.states.clone());
        qb.push(")");
    }
    if let Some(questioned) = filter.questioned {
        qb.push(" AND entry.questioned = ");
        qb.push_bind(questioned);
    }
    if let Some(q) = filter.query.as_deref().filter(|s| !s.is_empty()) {
        let pat = format!("%{q}%");
        qb.push(" AND (entry.key ILIKE ");
        qb.push_bind(pat.clone());
        qb.push(" OR entry.translation ILIKE ");
        qb.push_bind(pat.clone());
        qb.push(" OR entry.original::text ILIKE ");
        qb.push_bind(pat);
        qb.push(")");
    }
    if let Some(after) = after_id {
        qb.push(" AND entry.id > ");
        qb.push_bind(after);
    }
    qb.push(" ORDER BY entry.id LIMIT ");
    qb.push_bind(limit);

    qb.build_query_as::<Entry>().fetch_all(pool).await
}

/// 按 id 查词条（限定项目）。
pub async fn get(
    pool: &PgPool,
    project_id: i64,
    entry_id: i64,
) -> Result<Option<Entry>, sqlx::Error> {
    sqlx::query_as::<_, Entry>(
        "SELECT entry.* FROM entries AS entry
         JOIN files AS file ON file.id = entry.file_id
         WHERE entry.id = $1 AND entry.project_id = $2
           AND entry.deleted_at IS NULL AND file.deleted_at IS NULL",
    )
    .bind(entry_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

/// 在调用方事务内锁定项目所属词条并返回一致快照。
pub async fn get_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
) -> Result<Option<Entry>, sqlx::Error> {
    sqlx::query_as::<_, Entry>("SELECT * FROM entries WHERE id = $1 AND project_id = $2 FOR UPDATE")
        .bind(entry_id)
        .bind(project_id)
        .fetch_optional(conn)
        .await
}

/// 乐观锁更新译文与状态：仅当 `expected_version` 匹配才更新。
/// 返回 `Ok(None)` 表示版本冲突（已被他人修改）。同时记录一条历史。
#[allow(clippy::too_many_arguments)]
pub async fn update_translation(
    pool: &PgPool,
    entry_id: i64,
    expected_version: i64,
    translation: &str,
    state: &str,
    questioned: Option<bool>,
    kind: &str,
    editor_id: Option<i64>,
) -> Result<Option<Entry>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let updated = update_translation_tx(
        &mut tx,
        entry_id,
        expected_version,
        translation,
        state,
        questioned,
        kind,
        editor_id,
    )
    .await?;
    tx.commit().await?;
    Ok(updated)
}

/// 在调用方事务内乐观锁更新译文/状态并写 entry version。
#[allow(clippy::too_many_arguments)]
pub async fn update_translation_tx(
    conn: &mut PgConnection,
    entry_id: i64,
    expected_version: i64,
    translation: &str,
    state: &str,
    questioned: Option<bool>,
    kind: &str,
    editor_id: Option<i64>,
) -> Result<Option<Entry>, sqlx::Error> {
    let before: Option<(String, bool)> = sqlx::query_as(
        "SELECT state, prts_entry_is_effectively_visible(entry)
         FROM entries AS entry WHERE id = $1 AND version = $2 FOR UPDATE",
    )
    .bind(entry_id)
    .bind(expected_version)
    .fetch_optional(&mut *conn)
    .await?;
    let updated: Option<Entry> = sqlx::query_as::<_, Entry>(
        "UPDATE entries SET translation = $3, state = $4,
             questioned = COALESCE($5, questioned), version = version + 1, updated_by = $6
         WHERE id = $1 AND version = $2 RETURNING *",
    )
    .bind(entry_id)
    .bind(expected_version)
    .bind(translation)
    .bind(state)
    .bind(questioned)
    .bind(editor_id)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(ref e) = updated {
        sqlx::query(
            "INSERT INTO entry_versions (
                 entry_id, version, kind, translation, state, questioned, original, editor_id,
                 editor_name, editor_avatar_url
             )
             SELECT $1, $2, $3, $4, $5, $6, $7, actor.id, actor.username, actor.avatar_url
             FROM (SELECT 1) AS seed LEFT JOIN users AS actor ON actor.id = $8",
        )
        .bind(e.id)
        .bind(e.version)
        .bind(kind)
        .bind(&e.translation)
        .bind(&e.state)
        .bind(e.questioned)
        .bind(&e.original)
        .bind(editor_id)
        .execute(&mut *conn)
        .await?;
        let (before_state, before_visible) = before.ok_or_else(|| {
            sqlx::Error::Protocol("entry transition snapshot is missing".to_string())
        })?;
        let before = super::tasks::entry_snapshot(&before_state, before_visible)?;
        let after = super::tasks::entry_snapshot(&e.state, before_visible)?;
        super::tasks::apply_entry_transition_tx(conn, e.id, before, after).await?;
    }
    Ok(updated)
}

/// 设置锁定/隐藏标志（None 表示不变）。
pub async fn set_flags(
    pool: &PgPool,
    project_id: i64,
    entry_id: i64,
    locked: Option<bool>,
    hidden: Option<bool>,
) -> Result<Option<Entry>, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    set_flags_tx(&mut connection, project_id, entry_id, locked, hidden, None).await
}

/// 在调用方事务内设置词条正交 flags。
pub async fn set_flags_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
    locked: Option<bool>,
    hidden: Option<bool>,
    actor_id: Option<i64>,
) -> Result<Option<Entry>, sqlx::Error> {
    let before: Option<(String, bool)> = sqlx::query_as(
        "SELECT state, prts_entry_is_effectively_visible(entry)
         FROM entries AS entry WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(entry_id)
    .bind(project_id)
    .fetch_optional(&mut *conn)
    .await?;
    let updated = sqlx::query_as::<_, Entry>(
        "UPDATE entries SET locked = COALESCE($3, locked), hidden = COALESCE($4, hidden),
             version = version + 1, updated_by = $5
         WHERE id = $1 AND project_id = $2 RETURNING *",
    )
    .bind(entry_id)
    .bind(project_id)
    .bind(locked)
    .bind(hidden)
    .bind(actor_id)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(entry) = &updated {
        sqlx::query(
            "INSERT INTO entry_versions (
                 entry_id, version, kind, translation, state, questioned, original, editor_id,
                 editor_name, editor_avatar_url
             )
             SELECT $1, $2, 'flags', $3, $4, $5, $6, actor.id, actor.username, actor.avatar_url
             FROM (SELECT 1) AS seed LEFT JOIN users AS actor ON actor.id = $7",
        )
        .bind(entry.id)
        .bind(entry.version)
        .bind(&entry.translation)
        .bind(&entry.state)
        .bind(entry.questioned)
        .bind(&entry.original)
        .bind(actor_id)
        .execute(&mut *conn)
        .await?;
        let (before_state, before_visible) = before.ok_or_else(|| {
            sqlx::Error::Protocol("entry flag transition snapshot is missing".to_string())
        })?;
        let after_visible: bool = sqlx::query_scalar(
            "SELECT prts_entry_is_effectively_visible(entry)
             FROM entries AS entry WHERE id = $1",
        )
        .bind(entry.id)
        .fetch_one(&mut *conn)
        .await?;
        let before = super::tasks::entry_snapshot(&before_state, before_visible)?;
        let after = super::tasks::entry_snapshot(&entry.state, after_visible)?;
        super::tasks::apply_entry_transition_tx(conn, entry.id, before, after).await?;
    }
    Ok(updated)
}

/// 列出词条历史。
pub async fn list_versions(
    pool: &PgPool,
    entry_id: i64,
    limit: i64,
) -> Result<Vec<EntryVersion>, sqlx::Error> {
    sqlx::query_as::<_, EntryVersion>(
        "SELECT * FROM entry_versions WHERE entry_id = $1 ORDER BY version DESC, id DESC LIMIT $2",
    )
    .bind(entry_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 列出词条历史并附带仍存在账号的展示资料。
pub async fn list_versions_with_editor(
    pool: &PgPool,
    entry_id: i64,
    limit: i64,
) -> Result<Vec<EntryVersion>, sqlx::Error> {
    sqlx::query_as::<_, EntryVersion>(
        "SELECT * FROM entry_versions WHERE entry_id = $1
         ORDER BY version DESC, id DESC LIMIT $2",
    )
    .bind(entry_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 统计项目内各状态词条数。
pub async fn count_by_state(
    pool: &PgPool,
    project_id: i64,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    sqlx::query_as::<_, (String, i64)>(
        "SELECT state, COUNT(*) FROM entries WHERE project_id = $1 GROUP BY state",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// 导出用：取项目全部词条（按文件、key 排序）。
pub async fn list_for_export(pool: &PgPool, project_id: i64) -> Result<Vec<Entry>, sqlx::Error> {
    sqlx::query_as::<_, Entry>(
        "SELECT entry.* FROM entries AS entry
         JOIN files AS file ON file.id = entry.file_id
         WHERE entry.project_id = $1 AND entry.deleted_at IS NULL
           AND file.deleted_at IS NULL AND NOT entry.hidden
         ORDER BY entry.file_id, entry.key",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}
