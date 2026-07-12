//! 主源语言变更的纯领域规则。

/// 主源语言真实变化的稳定拒绝原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimarySourceChangeError {
    ReleaseNotReady,
    NotOwner,
    LanguageResolutionRequired,
    CooldownActive,
    RebuildBlocked,
}

impl PrimarySourceChangeError {
    /// 返回 API 可安全暴露的稳定错误标识。
    pub const fn code(self) -> &'static str {
        match self {
            Self::ReleaseNotReady => "primary_source_release_not_ready",
            Self::NotOwner => "primary_source_owner_required",
            Self::LanguageResolutionRequired => "project_language_resolution_required",
            Self::CooldownActive => "primary_source_cooldown_active",
            Self::RebuildBlocked => "primary_source_rebuild_blocked",
        }
    }
}

/// 判断 job 状态是否阻止下一次主源变化。
pub fn rebuild_state_blocks_change(state: &str) -> bool {
    matches!(state, "queued" | "running" | "paused" | "failed")
}

/// 校验一次真实主源变化；相同值由调用方在进入此函数前直接返回。
pub fn validate_primary_source_change(
    release_ready: bool,
    is_owner: bool,
    language_ready: bool,
    cooldown_active: bool,
    lexical_job_state: Option<&str>,
    embedding_job_state: Option<&str>,
) -> Result<(), PrimarySourceChangeError> {
    if !release_ready {
        return Err(PrimarySourceChangeError::ReleaseNotReady);
    }
    if !is_owner {
        return Err(PrimarySourceChangeError::NotOwner);
    }
    if !language_ready {
        return Err(PrimarySourceChangeError::LanguageResolutionRequired);
    }
    if cooldown_active {
        return Err(PrimarySourceChangeError::CooldownActive);
    }
    if lexical_job_state.is_some_and(rebuild_state_blocks_change)
        || embedding_job_state.is_some_and(rebuild_state_blocks_change)
    {
        return Err(PrimarySourceChangeError::RebuildBlocked);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_requires_release_owner_ready_project_and_expired_cooldown() {
        assert_eq!(
            validate_primary_source_change(false, true, true, false, None, None),
            Err(PrimarySourceChangeError::ReleaseNotReady)
        );
        assert_eq!(
            validate_primary_source_change(true, false, true, false, None, None),
            Err(PrimarySourceChangeError::NotOwner)
        );
        assert_eq!(
            validate_primary_source_change(true, true, false, false, None, None),
            Err(PrimarySourceChangeError::LanguageResolutionRequired)
        );
        assert_eq!(
            validate_primary_source_change(true, true, true, true, None, None),
            Err(PrimarySourceChangeError::CooldownActive)
        );
    }

    #[test]
    fn active_or_unresolved_failed_stage_blocks_a_new_change() {
        for state in ["queued", "running", "paused", "failed"] {
            assert_eq!(
                validate_primary_source_change(true, true, true, false, Some(state), None),
                Err(PrimarySourceChangeError::RebuildBlocked)
            );
            assert_eq!(
                validate_primary_source_change(true, true, true, false, None, Some(state)),
                Err(PrimarySourceChangeError::RebuildBlocked)
            );
        }
    }

    #[test]
    fn completed_skipped_and_degraded_stages_do_not_block() {
        for state in ["succeeded", "skipped", "degraded"] {
            assert_eq!(
                validate_primary_source_change(true, true, true, false, Some(state), Some(state)),
                Ok(())
            );
        }
    }
}
