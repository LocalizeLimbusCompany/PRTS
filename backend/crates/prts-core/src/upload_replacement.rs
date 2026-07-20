//! 同路径上传的完整文件替换领域规则。
//!
//! 本模块只根据“平台当前词条 + 已校验上传词条”生成 typed transition plan，
//! 不依赖数据库、Web 或临时文件。数据库适配器必须执行这里产出的分类，不能在
//! SQL 中重新定义译文保留、状态重置、恢复或 tombstone 规则。

use std::collections::BTreeMap;
use std::ops::AddAssign;

use serde::{Deserialize, Serialize};

use crate::{EntryFlags, EntryState};

/// 已经 canonicalize 并验证属于项目源语言集合的源文对象。
pub type OriginalText = BTreeMap<String, String>;

/// 平台中现有词条的领域快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingEntry {
    /// 永久平台词条 ID；tombstone 恢复时必须复用。
    pub id: i64,
    /// 文件内唯一 key。
    pub key: String,
    /// 当前平台源文。
    pub original: OriginalText,
    /// 当前平台译文；replacement 不覆盖现有译文。
    pub translation: String,
    /// 当前平台工作流状态。
    pub state: EntryState,
    /// 与工作流正交的平台 flags。
    pub flags: EntryFlags,
    /// 是否为同路径 replacement 产生的 entry tombstone。
    pub deleted: bool,
}

/// 流式解析并写入 staging table 后的上传词条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedEntry {
    /// JSON 数组中的零基 ordinal；用于稳定错误与历史顺序。
    pub ordinal: i64,
    /// 文件内唯一 key。
    pub key: String,
    /// 已规范化且无 canonical duplicate 的源文对象。
    pub original: OriginalText,
    /// 只用于从未存在的新 key；existing/restore 均忽略。
    pub translation: Option<String>,
    /// 只用于从未存在的新 key；缺省时为 untranslated。
    pub state: Option<EntryState>,
}

/// 历史 allowlist 所需的领域快照。
///
/// 数据库适配器负责把 `deleted` 映射为实际 `deleted_at` 时间；其它字段可直接写入
/// `file_change_items.before_value/after_value`，不得用通用实体序列化扩张字段集合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryHistorySnapshot {
    /// 文件内 key。
    pub key: String,
    /// 多源语言源文。
    pub original: OriginalText,
    /// 平台译文。
    pub translation: String,
    /// 工作流状态。
    pub state: EntryState,
    /// 锁定标志。
    pub locked: bool,
    /// 隐藏标志。
    pub hidden: bool,
    /// 是否应持有 `deleted_at`。
    pub deleted: bool,
}

impl From<&ExistingEntry> for EntryHistorySnapshot {
    fn from(entry: &ExistingEntry) -> Self {
        Self {
            key: entry.key.clone(),
            original: entry.original.clone(),
            translation: entry.translation.clone(),
            state: entry.state,
            locked: entry.flags.locked,
            hidden: entry.flags.hidden,
            deleted: entry.deleted,
        }
    }
}

/// 单个词条对 effective-visible 物化统计的有符号差异。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntryStatsDelta {
    /// 可见词条总数差异。
    pub visible_total: i64,
    /// untranslated 数量差异。
    pub untranslated: i64,
    /// translated 数量差异。
    pub translated: i64,
    /// questioned 数量差异。
    pub questioned: i64,
    /// checked 数量差异。
    pub checked: i64,
    /// reviewed 数量差异。
    pub reviewed: i64,
    /// 隐藏但未 tombstone 的词条总数差异。
    pub hidden_total: i64,
    /// 隐藏未翻译数差异。
    pub hidden_untranslated: i64,
    /// 隐藏已翻译数差异。
    pub hidden_translated: i64,
    /// 隐藏有疑问数差异。
    pub hidden_questioned: i64,
    /// 隐藏已检查数差异。
    pub hidden_checked: i64,
    /// 隐藏已审核数差异。
    pub hidden_reviewed: i64,
}

impl AddAssign for EntryStatsDelta {
    fn add_assign(&mut self, rhs: Self) {
        self.visible_total += rhs.visible_total;
        self.untranslated += rhs.untranslated;
        self.translated += rhs.translated;
        self.questioned += rhs.questioned;
        self.checked += rhs.checked;
        self.reviewed += rhs.reviewed;
        self.hidden_total += rhs.hidden_total;
        self.hidden_untranslated += rhs.hidden_untranslated;
        self.hidden_translated += rhs.hidden_translated;
        self.hidden_questioned += rhs.hidden_questioned;
        self.hidden_checked += rhs.hidden_checked;
        self.hidden_reviewed += rhs.hidden_reviewed;
    }
}

impl EntryStatsDelta {
    /// 根据 replacement 前后快照计算统计差异。
    ///
    /// 文件本身由 worker 锁定为 active，因此这里只处理 entry tombstone 与 hidden；
    /// 文件/祖先文件夹删除的 exposure 属于文件历史领域，不由上传 replacement 穿透。
    fn between(
        before: Option<&EntryHistorySnapshot>,
        after: Option<&EntryHistorySnapshot>,
    ) -> Self {
        let mut delta = Self::default();
        if let Some(before) = before.filter(|entry| !entry.deleted) {
            delta.add_entry(before.state, before.hidden, -1);
        }
        if let Some(after) = after.filter(|entry| !entry.deleted) {
            delta.add_entry(after.state, after.hidden, 1);
        }
        delta
    }

    fn add_entry(&mut self, state: EntryState, hidden: bool, amount: i64) {
        if hidden {
            self.hidden_total += amount;
            match state {
                EntryState::Untranslated => self.hidden_untranslated += amount,
                EntryState::Translated => self.hidden_translated += amount,
                EntryState::Questioned => self.hidden_questioned += amount,
                EntryState::Checked => self.hidden_checked += amount,
                EntryState::Reviewed => self.hidden_reviewed += amount,
            }
        } else {
            self.visible_total += amount;
            match state {
                EntryState::Untranslated => self.untranslated += amount,
                EntryState::Translated => self.translated += amount,
                EntryState::Questioned => self.questioned += amount,
                EntryState::Checked => self.checked += amount,
                EntryState::Reviewed => self.reviewed += amount,
            }
        }
    }
}

/// `file_change_items.operation` 的 typed 领域值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementHistoryOperation {
    /// 新建从未存在的 key。
    Create,
    /// 现有 active key 的源文变化。
    Update,
    /// 恢复已有 tombstone；可同时包含源文变化。
    Restore,
    /// 上传中缺失的 active key 形成 tombstone。
    Tombstone,
}

/// replacement 需要持久化的一条明确历史 delta。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementHistoryDelta {
    /// 已存在词条 ID；新建时为 `None`，由 adapter 插入后回填历史 target。
    pub entry_id: Option<i64>,
    /// 稳定的上传 ordinal；缺失 tombstone 沿用 DB join 的稳定顺序。
    pub ordinal: i64,
    /// 历史操作类型。
    pub operation: ReplacementHistoryOperation,
    /// 变更前 allowlisted 快照。
    pub before: Option<EntryHistorySnapshot>,
    /// 变更后 allowlisted 快照。
    pub after: Option<EntryHistorySnapshot>,
}

/// 每个 key 的互斥执行分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementTransitionKind {
    /// 从未存在的新 key。
    Insert,
    /// 恢复 tombstone；`source_changed` 表示恢复同时需要重置状态。
    Restore { source_changed: bool },
    /// active key 的源文变化。
    SourceChanged,
    /// 上传完整集合中缺失的 active key。
    Tombstone,
    /// 平台状态无需变化；上传 translation/state 被忽略。
    Unchanged,
}

/// 单个 key 的完整执行计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedEntryTransition {
    /// typed 分类；adapter 只按该值执行。
    pub kind: ReplacementTransitionKind,
    /// 已存在词条 ID；insert 时为 `None`。
    pub entry_id: Option<i64>,
    /// 上传 ordinal；仅缺失旧 key 时由调用方提供稳定 DB ordinal。
    pub ordinal: i64,
    /// 文件内 key。
    pub key: String,
    /// replacement 前的平台快照。
    pub before: Option<EntryHistorySnapshot>,
    /// replacement 后的平台快照。
    pub after: EntryHistorySnapshot,
    /// 该 key 对物化统计的纯领域差异。
    pub stats_delta: EntryStatsDelta,
    /// `None` 仅用于 unchanged；其它变更均有 allowlisted history delta。
    pub history: Option<ReplacementHistoryDelta>,
}

/// replacement 各类操作计数；恢复且源文变化会同时计入两项。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplacementSummary {
    /// 新建数。
    pub inserted: usize,
    /// 恢复数。
    pub restored: usize,
    /// 源文变化数（含恢复同时变化）。
    pub source_changed: usize,
    /// 新增 tombstone 数。
    pub tombstoned: usize,
    /// 无需写入数。
    pub unchanged: usize,
}

/// 一个流式批次的 typed replacement plan。
///
/// worker 可按固定小批反复调用 [`ReplacementPlan::from_inputs`]，将 transitions 写入
/// 数据库 plan temp table，并把 summary/stats_delta 累加到整个文件的结果中。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplacementPlan {
    /// adapter 需要执行的逐 key 分类。
    pub transitions: Vec<PlannedEntryTransition>,
    /// 分类计数。
    pub summary: ReplacementSummary,
    /// 该批全部词条的统计差异。
    pub stats_delta: EntryStatsDelta,
}

impl ReplacementPlan {
    /// 从已由数据库 full join 对齐的输入生成纯领域计划。
    pub fn from_inputs(
        inputs: impl IntoIterator<Item = ReplacementInput>,
    ) -> Result<Self, ReplacementPlanError> {
        let mut plan = Self::default();
        for input in inputs {
            plan.push(plan_transition(input)?);
        }
        Ok(plan)
    }

    /// 累加另一个小批计划，供流式 worker 汇总整个文件。
    pub fn merge(&mut self, other: Self) {
        self.summary.inserted += other.summary.inserted;
        self.summary.restored += other.summary.restored;
        self.summary.source_changed += other.summary.source_changed;
        self.summary.tombstoned += other.summary.tombstoned;
        self.summary.unchanged += other.summary.unchanged;
        self.stats_delta += other.stats_delta;
        self.transitions.extend(other.transitions);
    }

    fn push(&mut self, transition: PlannedEntryTransition) {
        match transition.kind {
            ReplacementTransitionKind::Insert => self.summary.inserted += 1,
            ReplacementTransitionKind::Restore { source_changed } => {
                self.summary.restored += 1;
                if source_changed {
                    self.summary.source_changed += 1;
                }
            }
            ReplacementTransitionKind::SourceChanged => self.summary.source_changed += 1,
            ReplacementTransitionKind::Tombstone => self.summary.tombstoned += 1,
            ReplacementTransitionKind::Unchanged => self.summary.unchanged += 1,
        }
        self.stats_delta += transition.stats_delta;
        self.transitions.push(transition);
    }
}

/// 数据库 full join 中一个 key 的现状与上传侧输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementInput {
    /// 当前平台行；新 key 时为 `None`。
    pub existing: Option<ExistingEntry>,
    /// 上传 staging 行；缺失旧 key 时为 `None`。
    pub uploaded: Option<UploadedEntry>,
    /// uploaded 缺失时使用的稳定历史 ordinal。
    pub missing_ordinal: i64,
}

/// 不可能由正确 full join 产生的输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplacementPlanError {
    /// 两侧同时为空。
    EmptyInput,
    /// 两侧 key 不相等，说明 adapter 对齐错误。
    MismatchedKey { existing: String, uploaded: String },
}

/// 为一个已对齐 key 生成 typed transition。
pub fn plan_transition(
    input: ReplacementInput,
) -> Result<PlannedEntryTransition, ReplacementPlanError> {
    match (input.existing, input.uploaded) {
        (None, None) => Err(ReplacementPlanError::EmptyInput),
        (None, Some(uploaded)) => Ok(plan_insert(uploaded)),
        (Some(existing), None) => Ok(plan_missing(existing, input.missing_ordinal)),
        (Some(existing), Some(uploaded)) => {
            if existing.key != uploaded.key {
                return Err(ReplacementPlanError::MismatchedKey {
                    existing: existing.key,
                    uploaded: uploaded.key,
                });
            }
            Ok(plan_existing(existing, uploaded))
        }
    }
}

fn plan_insert(uploaded: UploadedEntry) -> PlannedEntryTransition {
    let after = EntryHistorySnapshot {
        key: uploaded.key.clone(),
        original: uploaded.original,
        translation: uploaded.translation.unwrap_or_default(),
        state: uploaded.state.unwrap_or_default(),
        locked: false,
        hidden: false,
        deleted: false,
    };
    changed_transition(
        ReplacementTransitionKind::Insert,
        None,
        uploaded.ordinal,
        uploaded.key,
        None,
        after,
        ReplacementHistoryOperation::Create,
    )
}

fn plan_missing(existing: ExistingEntry, ordinal: i64) -> PlannedEntryTransition {
    let before = EntryHistorySnapshot::from(&existing);
    if existing.deleted {
        return unchanged_transition(existing.id, ordinal, before);
    }
    let mut after = before.clone();
    after.deleted = true;
    changed_transition(
        ReplacementTransitionKind::Tombstone,
        Some(existing.id),
        ordinal,
        existing.key,
        Some(before),
        after,
        ReplacementHistoryOperation::Tombstone,
    )
}

fn plan_existing(existing: ExistingEntry, uploaded: UploadedEntry) -> PlannedEntryTransition {
    let before = EntryHistorySnapshot::from(&existing);
    let source_changed = existing.original != uploaded.original;
    if existing.deleted {
        let mut after = before.clone();
        after.deleted = false;
        if source_changed {
            after.original = uploaded.original;
            after.state = EntryState::Untranslated;
        }
        return changed_transition(
            ReplacementTransitionKind::Restore { source_changed },
            Some(existing.id),
            uploaded.ordinal,
            existing.key,
            Some(before),
            after,
            ReplacementHistoryOperation::Restore,
        );
    }
    if source_changed {
        let mut after = before.clone();
        after.original = uploaded.original;
        after.state = EntryState::Untranslated;
        return changed_transition(
            ReplacementTransitionKind::SourceChanged,
            Some(existing.id),
            uploaded.ordinal,
            existing.key,
            Some(before),
            after,
            ReplacementHistoryOperation::Update,
        );
    }
    unchanged_transition(existing.id, uploaded.ordinal, before)
}

fn changed_transition(
    kind: ReplacementTransitionKind,
    entry_id: Option<i64>,
    ordinal: i64,
    key: String,
    before: Option<EntryHistorySnapshot>,
    after: EntryHistorySnapshot,
    operation: ReplacementHistoryOperation,
) -> PlannedEntryTransition {
    let stats_delta = EntryStatsDelta::between(before.as_ref(), Some(&after));
    let history = ReplacementHistoryDelta {
        entry_id,
        ordinal,
        operation,
        before: before.clone(),
        after: Some(after.clone()),
    };
    PlannedEntryTransition {
        kind,
        entry_id,
        ordinal,
        key,
        before,
        after,
        stats_delta,
        history: Some(history),
    }
}

fn unchanged_transition(
    entry_id: i64,
    ordinal: i64,
    snapshot: EntryHistorySnapshot,
) -> PlannedEntryTransition {
    PlannedEntryTransition {
        kind: ReplacementTransitionKind::Unchanged,
        entry_id: Some(entry_id),
        ordinal,
        key: snapshot.key.clone(),
        before: Some(snapshot.clone()),
        after: snapshot,
        stats_delta: EntryStatsDelta::default(),
        history: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original(lang: &str, text: &str) -> OriginalText {
        BTreeMap::from([(lang.to_string(), text.to_string())])
    }

    fn existing(state: EntryState, deleted: bool, flags: EntryFlags) -> ExistingEntry {
        ExistingEntry {
            id: 41,
            key: "greeting".to_string(),
            original: original("en", "Hello"),
            translation: "你好".to_string(),
            state,
            flags,
            deleted,
        }
    }

    fn uploaded(text: &str) -> UploadedEntry {
        UploadedEntry {
            ordinal: 7,
            key: "greeting".to_string(),
            original: original("en", text),
            translation: Some("上传译文".to_string()),
            state: Some(EntryState::Reviewed),
        }
    }

    #[test]
    fn unchanged_existing_key_ignores_uploaded_translation_and_state() {
        let current = existing(
            EntryState::Checked,
            false,
            EntryFlags {
                locked: true,
                hidden: true,
            },
        );
        let transition = plan_transition(ReplacementInput {
            existing: Some(current.clone()),
            uploaded: Some(uploaded("Hello")),
            missing_ordinal: 0,
        })
        .unwrap();

        assert_eq!(transition.kind, ReplacementTransitionKind::Unchanged);
        assert_eq!(transition.after.translation, current.translation);
        assert_eq!(transition.after.state, EntryState::Checked);
        assert!(transition.after.locked && transition.after.hidden);
        assert_eq!(transition.stats_delta, EntryStatsDelta::default());
        assert!(transition.history.is_none());
    }

    #[test]
    fn source_change_preserves_platform_values_and_resets_state() {
        let transition = plan_transition(ReplacementInput {
            existing: Some(existing(
                EntryState::Reviewed,
                false,
                EntryFlags {
                    locked: true,
                    hidden: false,
                },
            )),
            uploaded: Some(uploaded("Hello again")),
            missing_ordinal: 0,
        })
        .unwrap();

        assert_eq!(transition.kind, ReplacementTransitionKind::SourceChanged);
        assert_eq!(transition.after.translation, "你好");
        assert_eq!(transition.after.original, original("en", "Hello again"));
        assert_eq!(transition.after.state, EntryState::Untranslated);
        assert!(transition.after.locked);
        assert_eq!(
            transition.stats_delta,
            EntryStatsDelta {
                untranslated: 1,
                reviewed: -1,
                ..EntryStatsDelta::default()
            }
        );
        assert_eq!(
            transition.history.as_ref().unwrap().operation,
            ReplacementHistoryOperation::Update
        );
    }

    #[test]
    fn missing_active_key_becomes_tombstone_without_changing_platform_fields() {
        let transition = plan_transition(ReplacementInput {
            existing: Some(existing(
                EntryState::Translated,
                false,
                EntryFlags::default(),
            )),
            uploaded: None,
            missing_ordinal: 99,
        })
        .unwrap();

        assert_eq!(transition.kind, ReplacementTransitionKind::Tombstone);
        assert!(transition.after.deleted);
        assert_eq!(transition.after.translation, "你好");
        assert_eq!(
            transition.stats_delta,
            EntryStatsDelta {
                visible_total: -1,
                translated: -1,
                ..EntryStatsDelta::default()
            }
        );
    }

    #[test]
    fn already_deleted_missing_key_is_unchanged() {
        let transition = plan_transition(ReplacementInput {
            existing: Some(existing(
                EntryState::Translated,
                true,
                EntryFlags::default(),
            )),
            uploaded: None,
            missing_ordinal: 99,
        })
        .unwrap();

        assert_eq!(transition.kind, ReplacementTransitionKind::Unchanged);
        assert!(transition.after.deleted);
        assert!(transition.history.is_none());
    }

    #[test]
    fn restored_key_preserves_translation_state_and_flags() {
        let transition = plan_transition(ReplacementInput {
            existing: Some(existing(
                EntryState::Checked,
                true,
                EntryFlags {
                    locked: true,
                    hidden: false,
                },
            )),
            uploaded: Some(uploaded("Hello")),
            missing_ordinal: 0,
        })
        .unwrap();

        assert_eq!(
            transition.kind,
            ReplacementTransitionKind::Restore {
                source_changed: false
            }
        );
        assert!(!transition.after.deleted);
        assert_eq!(transition.after.translation, "你好");
        assert_eq!(transition.after.state, EntryState::Checked);
        assert!(transition.after.locked);
        assert_eq!(
            transition.stats_delta,
            EntryStatsDelta {
                visible_total: 1,
                checked: 1,
                ..EntryStatsDelta::default()
            }
        );
    }

    #[test]
    fn restored_hidden_key_updates_hidden_stats_without_changing_visible_stats() {
        let transition = plan_transition(ReplacementInput {
            existing: Some(existing(
                EntryState::Reviewed,
                true,
                EntryFlags {
                    locked: false,
                    hidden: true,
                },
            )),
            uploaded: Some(uploaded("Hello")),
            missing_ordinal: 0,
        })
        .unwrap();

        assert_eq!(
            transition.stats_delta,
            EntryStatsDelta {
                hidden_total: 1,
                hidden_reviewed: 1,
                ..EntryStatsDelta::default()
            }
        );
        assert!(transition.after.hidden);
    }

    #[test]
    fn restored_source_change_is_one_restore_with_source_reset() {
        let plan = ReplacementPlan::from_inputs([ReplacementInput {
            existing: Some(existing(
                EntryState::Reviewed,
                true,
                EntryFlags {
                    locked: true,
                    hidden: false,
                },
            )),
            uploaded: Some(uploaded("Hello again")),
            missing_ordinal: 0,
        }])
        .unwrap();
        let transition = &plan.transitions[0];

        assert_eq!(
            transition.kind,
            ReplacementTransitionKind::Restore {
                source_changed: true
            }
        );
        assert_eq!(transition.after.translation, "你好");
        assert_eq!(transition.after.state, EntryState::Untranslated);
        assert!(transition.after.locked);
        assert_eq!(plan.summary.restored, 1);
        assert_eq!(plan.summary.source_changed, 1);
        assert_eq!(plan.stats_delta.visible_total, 1);
        assert_eq!(plan.stats_delta.untranslated, 1);
    }

    #[test]
    fn new_key_uses_upload_seed_only_once() {
        let transition = plan_transition(ReplacementInput {
            existing: None,
            uploaded: Some(uploaded("Hello")),
            missing_ordinal: 0,
        })
        .unwrap();

        assert_eq!(transition.kind, ReplacementTransitionKind::Insert);
        assert_eq!(transition.after.translation, "上传译文");
        assert_eq!(transition.after.state, EntryState::Reviewed);
        assert!(!transition.after.locked && !transition.after.hidden);
        assert_eq!(
            transition.stats_delta,
            EntryStatsDelta {
                visible_total: 1,
                reviewed: 1,
                ..EntryStatsDelta::default()
            }
        );
        assert_eq!(
            transition.history.as_ref().unwrap().operation,
            ReplacementHistoryOperation::Create
        );
    }

    #[test]
    fn new_key_defaults_missing_seed_values() {
        let transition = plan_transition(ReplacementInput {
            existing: None,
            uploaded: Some(UploadedEntry {
                ordinal: 0,
                key: "empty".to_string(),
                original: original("en", "Empty"),
                translation: None,
                state: None,
            }),
            missing_ordinal: 0,
        })
        .unwrap();

        assert_eq!(transition.after.translation, "");
        assert_eq!(transition.after.state, EntryState::Untranslated);
    }

    #[test]
    fn aggregate_plan_reports_each_rule_and_stats_delta() {
        let plan = ReplacementPlan::from_inputs([
            ReplacementInput {
                existing: None,
                uploaded: Some(uploaded("Hello")),
                missing_ordinal: 0,
            },
            ReplacementInput {
                existing: Some(ExistingEntry {
                    id: 42,
                    key: "removed".to_string(),
                    original: original("en", "Removed"),
                    translation: String::new(),
                    state: EntryState::Untranslated,
                    flags: EntryFlags::default(),
                    deleted: false,
                }),
                uploaded: None,
                missing_ordinal: 8,
            },
        ])
        .unwrap();

        assert_eq!(plan.summary.inserted, 1);
        assert_eq!(plan.summary.tombstoned, 1);
        assert_eq!(plan.stats_delta.visible_total, 0);
        assert_eq!(plan.stats_delta.reviewed, 1);
        assert_eq!(plan.stats_delta.untranslated, -1);
    }

    #[test]
    fn mismatched_join_keys_fail_closed() {
        let error = plan_transition(ReplacementInput {
            existing: Some(existing(
                EntryState::Untranslated,
                false,
                EntryFlags::default(),
            )),
            uploaded: Some(UploadedEntry {
                key: "other".to_string(),
                ..uploaded("Hello")
            }),
            missing_ordinal: 0,
        })
        .unwrap_err();

        assert_eq!(
            error,
            ReplacementPlanError::MismatchedKey {
                existing: "greeting".to_string(),
                uploaded: "other".to_string(),
            }
        );
    }
}
