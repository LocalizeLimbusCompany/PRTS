//! 项目任务的基线、进度与文件期望集合规则。
//!
//! 数据库适配器只能执行这里生成的 typed plan；handler 不得自行重判快照或进度语义。

use std::collections::BTreeSet;

use crate::EntryState;

/// 一个候选词条在任务规则所需的最小快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskEntrySnapshot {
    pub state: EntryState,
    pub effectively_visible: bool,
}

/// 单个基线词条对当前任务统计的贡献。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskProgressContribution {
    pub denominator: i64,
    pub completed: i64,
}

/// 单个 baseline entry 的 before→after 物化统计增量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskProgressDelta {
    pub denominator: i64,
    pub completed: i64,
}

/// 一个任务当前的物化进度。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskProgress {
    pub denominator: i64,
    pub completed: i64,
}

impl TaskProgress {
    /// 零基线任务无需处理；其它任务返回完成比例。
    pub fn completion_ratio(self) -> f64 {
        if self.denominator == 0 {
            1.0
        } else {
            self.completed as f64 / self.denominator as f64
        }
    }

    /// 零基线任务的显式产品语义。
    pub fn no_work_required(self) -> bool {
        self.denominator == 0
    }
}

/// 完整期望文件集合与当前 active task files 的差异计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFileSetPlan {
    pub retained_file_ids: Vec<i64>,
    pub added_file_ids: Vec<i64>,
    pub removed_file_ids: Vec<i64>,
}

/// 文件集合请求错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPlanError {
    DuplicateFileId,
    InvalidFileId,
}

/// 加入文件时是否把候选词条写入 immutable baseline。
pub fn include_in_baseline(entry: TaskEntrySnapshot) -> bool {
    entry.effectively_visible && entry.state == EntryState::Untranslated
}

/// 已进入 baseline 的 live 词条对当前分母/完成数的贡献。
pub fn progress_contribution(entry: TaskEntrySnapshot) -> TaskProgressContribution {
    if !entry.effectively_visible {
        return TaskProgressContribution::default();
    }
    TaskProgressContribution {
        denominator: 1,
        completed: i64::from(entry.state != EntryState::Untranslated),
    }
}

/// 生成单词条 mutation 的增量 plan；DB adapter 只把该 delta 应用到引用它的任务。
pub fn progress_delta(before: TaskEntrySnapshot, after: TaskEntrySnapshot) -> TaskProgressDelta {
    let before = progress_contribution(before);
    let after = progress_contribution(after);
    TaskProgressDelta {
        denominator: after.denominator - before.denominator,
        completed: after.completed - before.completed,
    }
}

/// 汇总 baseline live rows 的当前进度。
pub fn summarize_progress(entries: impl IntoIterator<Item = TaskEntrySnapshot>) -> TaskProgress {
    entries.into_iter().map(progress_contribution).fold(
        TaskProgress::default(),
        |mut total, contribution| {
            total.denominator += contribution.denominator;
            total.completed += contribution.completed;
            total
        },
    )
}

/// 将前端提交的完整期望文件集合转为确定性的新增/保留/移除计划。
pub fn plan_file_set(
    current_file_ids: &[i64],
    desired_file_ids: &[i64],
) -> Result<TaskFileSetPlan, TaskPlanError> {
    let current = unique_positive_ids(current_file_ids)?;
    let desired = unique_positive_ids(desired_file_ids)?;
    Ok(TaskFileSetPlan {
        retained_file_ids: current.intersection(&desired).copied().collect(),
        added_file_ids: desired.difference(&current).copied().collect(),
        removed_file_ids: current.difference(&desired).copied().collect(),
    })
}

fn unique_positive_ids(ids: &[i64]) -> Result<BTreeSet<i64>, TaskPlanError> {
    let mut unique = BTreeSet::new();
    for id in ids {
        if *id <= 0 {
            return Err(TaskPlanError::InvalidFileId);
        }
        if !unique.insert(*id) {
            return Err(TaskPlanError::DuplicateFileId);
        }
    }
    Ok(unique)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(state: EntryState, effectively_visible: bool) -> TaskEntrySnapshot {
        TaskEntrySnapshot {
            state,
            effectively_visible,
        }
    }

    #[test]
    fn baseline_only_includes_effectively_visible_untranslated_entries() {
        assert!(include_in_baseline(entry(EntryState::Untranslated, true)));
        assert!(!include_in_baseline(entry(EntryState::Translated, true)));
        assert!(!include_in_baseline(entry(EntryState::Untranslated, false)));
    }

    #[test]
    fn hidden_tombstoned_or_deleted_ancestors_leave_progress() {
        assert_eq!(
            progress_contribution(entry(EntryState::Untranslated, false)),
            TaskProgressContribution::default()
        );
        assert_eq!(
            progress_contribution(entry(EntryState::Translated, false)),
            TaskProgressContribution::default()
        );
    }

    #[test]
    fn translation_completion_and_reversion_change_completed_only() {
        assert_eq!(
            progress_contribution(entry(EntryState::Translated, true)),
            TaskProgressContribution {
                denominator: 1,
                completed: 1,
            }
        );
        assert_eq!(
            progress_contribution(entry(EntryState::Untranslated, true)),
            TaskProgressContribution {
                denominator: 1,
                completed: 0,
            }
        );
        assert_eq!(
            progress_delta(
                entry(EntryState::Untranslated, true),
                entry(EntryState::Translated, true),
            ),
            TaskProgressDelta {
                denominator: 0,
                completed: 1,
            }
        );
        assert_eq!(
            progress_delta(
                entry(EntryState::Translated, true),
                entry(EntryState::Untranslated, true),
            ),
            TaskProgressDelta {
                denominator: 0,
                completed: -1,
            }
        );
    }

    #[test]
    fn visibility_transition_changes_denominator_and_current_completion() {
        assert_eq!(
            progress_delta(
                entry(EntryState::Translated, true),
                entry(EntryState::Translated, false),
            ),
            TaskProgressDelta {
                denominator: -1,
                completed: -1,
            }
        );
        assert_eq!(
            progress_delta(
                entry(EntryState::Untranslated, false),
                entry(EntryState::Untranslated, true),
            ),
            TaskProgressDelta {
                denominator: 1,
                completed: 0,
            }
        );
    }

    #[test]
    fn zero_baseline_is_complete_and_requires_no_work() {
        let progress = summarize_progress([]);
        assert_eq!(progress, TaskProgress::default());
        assert_eq!(progress.completion_ratio(), 1.0);
        assert!(progress.no_work_required());
    }

    #[test]
    fn file_set_plan_is_complete_deterministic_and_rejects_forgery_noise() {
        assert_eq!(
            plan_file_set(&[3, 1], &[2, 3]).unwrap(),
            TaskFileSetPlan {
                retained_file_ids: vec![3],
                added_file_ids: vec![2],
                removed_file_ids: vec![1],
            }
        );
        assert_eq!(
            plan_file_set(&[], &[4, 4]),
            Err(TaskPlanError::DuplicateFileId)
        );
        assert_eq!(plan_file_set(&[], &[0]), Err(TaskPlanError::InvalidFileId));
    }
}
