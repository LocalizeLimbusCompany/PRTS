//! API 下发的项目能力集合，前端不得从角色名推断操作权限。

use serde::Serialize;

use crate::permission::{assignable_project_roles_for_scope, nodes};
use crate::ProjectRole;

/// 当前主体在单个项目中的显式能力。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ProjectCapabilities {
    pub view_project: bool,
    pub manage_project: bool,
    pub manage_members: bool,
    pub member_assignable_roles: &'static [&'static str],
    pub upload_files: bool,
    pub view_file_history: bool,
    pub rollback_file_history: bool,
    pub manage_tasks: bool,
    pub manage_terms: bool,
    pub download: bool,
    pub edit_entry: bool,
    pub review_entry: bool,
    pub lock_entry: bool,
    pub hide_entry: bool,
    pub edit_locked_entry: bool,
    pub force_save_presence: bool,
    pub collaborate: bool,
    pub resolve_languages: bool,
    pub change_primary_source: bool,
    pub delete_project: bool,
}

impl ProjectCapabilities {
    /// 根据权限节点、唯一 owner 身份及 release gate 生成能力。
    pub fn for_subject(
        can_view: bool,
        role: Option<ProjectRole>,
        is_owner: bool,
        primary_source_release_ready: bool,
    ) -> Self {
        let has = |node| role.is_some_and(|project_role| project_role.has(node));
        let elevated_editor = role.is_some_and(ProjectRole::can_edit_locked);
        let member_assignable_roles = assignable_project_roles_for_scope(role);
        Self {
            view_project: can_view,
            manage_project: has(nodes::PROJECT_MANAGE),
            manage_members: has(nodes::PROJECT_MEMBER_MANAGE),
            member_assignable_roles,
            upload_files: has(nodes::PROJECT_FILE_UPLOAD),
            view_file_history: has(nodes::PROJECT_HISTORY_VIEW),
            rollback_file_history: has(nodes::PROJECT_HISTORY_ROLLBACK),
            manage_tasks: has(nodes::PROJECT_TASK_MANAGE),
            manage_terms: has(nodes::PROJECT_TERM_MANAGE),
            download: has(nodes::PROJECT_DOWNLOAD),
            edit_entry: has(nodes::PROJECT_ENTRY_EDIT),
            review_entry: has(nodes::PROJECT_ENTRY_REVIEW),
            lock_entry: has(nodes::PROJECT_ENTRY_LOCK),
            hide_entry: has(nodes::PROJECT_ENTRY_HIDE),
            edit_locked_entry: elevated_editor,
            force_save_presence: elevated_editor,
            collaborate: role.is_some(),
            resolve_languages: is_owner,
            change_primary_source: is_owner && primary_source_release_ready,
            delete_project: is_owner,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_only_capabilities_ignore_platform_admin_override() {
        let manager =
            ProjectCapabilities::for_subject(true, Some(ProjectRole::Manager), false, true);
        assert!(manager.edit_locked_entry);
        assert!(manager.force_save_presence);
        assert!(manager.lock_entry);
        assert!(manager.hide_entry);
        assert!(manager.collaborate);
        assert!(manager.view_file_history);
        assert!(manager.rollback_file_history);
        assert!(manager.manage_tasks);
        assert!(manager.manage_terms);
        assert!(!manager.resolve_languages);
        assert!(!manager.change_primary_source);
        assert!(!manager.delete_project);

        let owner = ProjectCapabilities::for_subject(true, Some(ProjectRole::Owner), true, false);
        assert!(owner.force_save_presence);
        assert!(owner.collaborate);
        assert!(owner.resolve_languages);
        assert!(owner.delete_project);
        assert!(!owner.change_primary_source);

        let reviewer =
            ProjectCapabilities::for_subject(true, Some(ProjectRole::Reviewer), false, true);
        assert!(reviewer.manage_terms);
        assert!(!reviewer.manage_tasks);

        let translator =
            ProjectCapabilities::for_subject(true, Some(ProjectRole::Translator), false, true);
        assert!(!translator.manage_terms);
        assert!(translator.collaborate);
        assert!(!translator.lock_entry);

        let guest = ProjectCapabilities::for_subject(true, None, false, true);
        assert!(guest.view_project);
        assert!(!guest.edit_entry);
        assert!(!guest.collaborate);
    }
}
