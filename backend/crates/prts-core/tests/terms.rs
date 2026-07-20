//! Task 5.1 术语领域规则的测试先行合同。
//!
//! 本文件只描述 `prts-core` 必须提供的 typed rules/plans；数据库 adapter 只执行这些
//! 结果，handler 不得复制 active/archived、语言 gate 或主源切换真值。

use prts_core::permission::nodes::{PLATFORM_POS_MANAGE, PROJECT_TERM_MANAGE};
use prts_core::terms::{
    plan_primary_source_term_action, plan_term_write, term_matches_source, validate_term_pattern,
    PrimarySourceTermAction, TermPatternError, TermRuleError,
};
use prts_core::{PlatformRole, ProjectRole};

#[test]
fn term_write_canonicalizes_any_valid_tag_and_rejects_invalid_input() {
    let archived = plan_term_write(" DE-de-u-co-phonebk ", "en", true, true, false)
        .expect("合法非项目 source-set tag 可作为 archived term 保存");
    assert_eq!(archived.source_lang, "de-DE-u-co-phonebk");
    assert!(archived.archived);

    let active = plan_term_write("EN", "en", false, true, false)
        .expect("当前 primary 的 active term 可保存");
    assert_eq!(active.source_lang, "en");
    assert!(!active.archived);

    assert_eq!(
        plan_term_write("not_a_language", "en", true, true, false),
        Err(TermRuleError::InvalidLanguageTag)
    );
}

#[test]
fn non_primary_active_is_a_stable_error_and_is_never_silently_archived() {
    let error =
        plan_term_write("JA", "en", false, true, false).expect_err("非主源 active 请求必须失败");
    assert_eq!(error, TermRuleError::ActiveSourceMismatch);
    assert_eq!(error.code(), "TERM_ACTIVE_SOURCE_MISMATCH");

    let archived = plan_term_write("JA", "en", true, true, false)
        .expect("相同非主源 tag 显式 archived 时合法");
    assert_eq!(archived.source_lang, "ja");
    assert!(archived.archived);
}

#[test]
fn ordinary_term_mutations_fail_closed_for_resolution_and_pending_deletion() {
    assert_eq!(
        plan_term_write("en", "en", false, false, false),
        Err(TermRuleError::LanguageResolutionRequired)
    );
    assert_eq!(
        plan_term_write("en", "en", false, true, true),
        Err(TermRuleError::ProjectPendingDeletion)
    );
}

#[test]
fn primary_source_switch_archives_old_active_and_activates_new_archived_terms() {
    assert_eq!(
        plan_primary_source_term_action("en", false, "ja").unwrap(),
        PrimarySourceTermAction::Archive
    );
    assert_eq!(
        plan_primary_source_term_action("ja", true, "ja").unwrap(),
        PrimarySourceTermAction::Activate
    );
    assert_eq!(
        plan_primary_source_term_action("ja", false, "ja").unwrap(),
        PrimarySourceTermAction::Keep
    );
}

#[test]
fn legacy_old_primary_and_other_archived_languages_remain_migration_ready() {
    assert_eq!(
        plan_primary_source_term_action("en", true, "ja").unwrap(),
        PrimarySourceTermAction::Keep
    );
    assert_eq!(
        plan_primary_source_term_action("ko", true, "ja").unwrap(),
        PrimarySourceTermAction::Keep
    );
}

#[test]
fn reviewer_can_manage_terms_but_pos_mutation_is_platform_admin_only() {
    for role in [
        ProjectRole::Owner,
        ProjectRole::Manager,
        ProjectRole::Reviewer,
    ] {
        assert!(role.has(PROJECT_TERM_MANAGE));
    }
    assert!(!ProjectRole::Translator.has(PROJECT_TERM_MANAGE));

    assert!(PlatformRole::SuperAdmin.has(PLATFORM_POS_MANAGE));
    assert!(PlatformRole::Admin.has(PLATFORM_POS_MANAGE));
    assert!(!PlatformRole::Maintainer.has(PLATFORM_POS_MANAGE));
    assert!(!ProjectRole::Owner.has(PLATFORM_POS_MANAGE));
    assert!(!ProjectRole::Manager.has(PLATFORM_POS_MANAGE));
}

#[test]
fn placeholder_only_matches_source_with_arbitrary_middle_text() {
    assert!(term_matches_source(
        "placeholder",
        "AAAA [] BBBB",
        "before AAAA XXXXXHUIHIUQA BBBB after"
    )
    .unwrap());
    assert!(term_matches_source("placeholder", "AAAA [] BBBB", "AAAA  BBBB").unwrap());
    assert!(!term_matches_source("placeholder", "AAAA [] BBBB", "AAAA BBBX").unwrap());
    assert_eq!(
        validate_term_pattern("placeholder", "AAAA BBBB"),
        Err(TermPatternError::PlaceholderRequired)
    );
}

#[test]
fn regex_validation_uses_rust_linear_time_syntax() {
    assert!(term_matches_source("regex", r"攻(击|防御).*增加", "攻击威力增加").unwrap());
    assert_eq!(
        validate_term_pattern("regex", "("),
        Err(TermPatternError::InvalidRegex)
    );
    assert_eq!(
        validate_term_pattern("invalid", "x"),
        Err(TermPatternError::InvalidMode)
    );
}
