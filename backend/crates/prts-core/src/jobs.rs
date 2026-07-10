//! 持久化任务的领域状态规则。
//!
//! 本模块不依赖 Web 或数据库；仓储和 worker 复用这些状态/权限规则，避免在 handler
//! 或 SQL 中各自定义第二套任务生命周期。

use serde::{Deserialize, Serialize};

use crate::permission::nodes;

/// 持久化任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// 等待 worker。
    Queued,
    /// 已被 worker 租用。
    Running,
    /// 因所属资源状态暂停。
    Paused,
    /// 成功结束。
    Succeeded,
    /// 重试耗尽或不可恢复失败。
    Failed,
    /// 用户或系统取消。
    Cancelled,
}

impl JobState {
    /// 解析数据库状态字符串。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// 数据库存储值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// 当前状态是否允许受控手动重试。
    pub const fn manual_retry_allowed(self) -> bool {
        matches!(self, Self::Failed)
    }

    /// 是否已终止，不应再被 worker 领取。
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// 项目待删除期间是否仍允许执行此任务。
pub fn may_run_while_project_pending_deletion(kind: &str) -> bool {
    kind == "project_purge"
}

/// 某个 allowlisted job kind 的手动重试策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobRetryPolicy {
    /// 必须具备的现有项目权限节点。
    pub permission_node: &'static str,
    /// 是否还必须精确匹配 `projects.owner_id`，平台管理员不能替代。
    pub owner_only: bool,
}

/// 某个 allowlisted job kind 的读取策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobViewPolicy {
    /// 必须具备的现有项目权限节点。
    pub permission_node: &'static str,
    /// 是否仅唯一 owner 可见，平台管理员不能替代。
    pub owner_only: bool,
}

/// 当前 release 对外可识别的 job kind；新增 kind 必须同时定义 view/retry policy。
pub const KNOWN_JOB_KINDS: &[&str] = &[
    "project_purge",
    "primary_source_lexical_reindex",
    "primary_source_embedding_backfill",
    "upload_process",
    "upload_cleanup",
    "file_purge",
    "file_retention_cleanup",
];

/// 返回 allowlisted job kind 的读取策略；未知 kind 不得通过 ID 或列表泄露。
pub fn job_view_policy(kind: &str) -> Option<JobViewPolicy> {
    match kind {
        "project_purge"
        | "primary_source_lexical_reindex"
        | "primary_source_embedding_backfill" => Some(JobViewPolicy {
            permission_node: nodes::PROJECT_DELETE,
            owner_only: true,
        }),
        "upload_process" | "upload_cleanup" => Some(JobViewPolicy {
            permission_node: nodes::PROJECT_FILE_UPLOAD,
            owner_only: false,
        }),
        "file_purge" | "file_retention_cleanup" => Some(JobViewPolicy {
            permission_node: nodes::PROJECT_MANAGE,
            owner_only: false,
        }),
        _ => None,
    }
}

/// 返回 allowlisted job kind 的手动重试策略；未知 kind fail closed。
pub fn manual_retry_policy(kind: &str) -> Option<JobRetryPolicy> {
    match kind {
        "project_purge"
        | "primary_source_lexical_reindex"
        | "primary_source_embedding_backfill" => Some(JobRetryPolicy {
            permission_node: nodes::PROJECT_DELETE,
            owner_only: true,
        }),
        "upload_process" | "upload_cleanup" => Some(JobRetryPolicy {
            permission_node: nodes::PROJECT_FILE_UPLOAD,
            owner_only: false,
        }),
        "file_purge" | "file_retention_cleanup" => Some(JobRetryPolicy {
            permission_node: nodes::PROJECT_MANAGE,
            owner_only: false,
        }),
        _ => None,
    }
}

/// 自动重试退避秒数，按 attempt 指数增长并封顶五分钟。
pub fn retry_backoff_seconds(attempts: i32) -> i64 {
    let exponent = attempts.clamp(0, 8) as u32;
    (1_i64 << exponent).min(300)
}

/// 验证任务进度，不允许负值或超过总量。
pub fn progress_is_valid(current: i64, total: Option<i64>) -> bool {
    current >= 0
        && match total {
            Some(total) => total >= 0 && current <= total,
            None => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_states_roundtrip_and_only_failed_is_manually_retryable() {
        for state in [
            JobState::Queued,
            JobState::Running,
            JobState::Paused,
            JobState::Succeeded,
            JobState::Failed,
            JobState::Cancelled,
        ] {
            assert_eq!(JobState::parse(state.as_str()), Some(state));
        }
        assert!(JobState::Failed.manual_retry_allowed());
        assert!(!JobState::Running.manual_retry_allowed());
    }

    #[test]
    fn only_project_purge_runs_during_pending_deletion() {
        assert!(may_run_while_project_pending_deletion("project_purge"));
        assert!(!may_run_while_project_pending_deletion("upload_process"));
        assert_eq!(
            manual_retry_policy("project_purge"),
            Some(JobRetryPolicy {
                permission_node: nodes::PROJECT_DELETE,
                owner_only: true,
            })
        );
        assert_eq!(
            manual_retry_policy("upload_process"),
            Some(JobRetryPolicy {
                permission_node: nodes::PROJECT_FILE_UPLOAD,
                owner_only: false,
            })
        );
        assert_eq!(manual_retry_policy("unknown"), None);
    }

    #[test]
    fn progress_and_backoff_are_bounded() {
        assert!(progress_is_valid(6, Some(10)));
        assert!(!progress_is_valid(11, Some(10)));
        assert!(!progress_is_valid(-1, None));
        assert_eq!(retry_backoff_seconds(0), 1);
        assert_eq!(retry_backoff_seconds(20), 256);
    }

    #[test]
    fn view_policy_is_allowlisted_and_unknown_kinds_fail_closed() {
        assert!(KNOWN_JOB_KINDS
            .iter()
            .all(|kind| job_view_policy(kind).is_some()));
        assert_eq!(
            job_view_policy("project_purge"),
            Some(JobViewPolicy {
                permission_node: nodes::PROJECT_DELETE,
                owner_only: true,
            })
        );
        assert_eq!(
            job_view_policy("upload_process"),
            Some(JobViewPolicy {
                permission_node: nodes::PROJECT_FILE_UPLOAD,
                owner_only: false,
            })
        );
        assert_eq!(job_view_policy("unknown"), None);
    }
}
