//! API 下发的项目能力集合，前端不得从角色名推断操作权限。

use serde::Serialize;

use crate::permission::nodes;
use crate::ProjectRole;

/// 当前主体在单个项目中的显式能力。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ProjectCapabilities {
    pub view_project: bool,
    pub manage_project: bool,
    pub manage_members: bool,
    pub upload_files: bool,
    pub download: bool,
    pub edit_entry: bool,
    pub review_entry: bool,
    pub edit_locked_entry: bool,
    pub force_save_presence: bool,
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
        Self {
            view_project: can_view,
            manage_project: has(nodes::PROJECT_MANAGE),
            manage_members: has(nodes::PROJECT_MEMBER_MANAGE),
            upload_files: has(nodes::PROJECT_FILE_UPLOAD),
            download: has(nodes::PROJECT_DOWNLOAD),
            edit_entry: has(nodes::PROJECT_ENTRY_EDIT),
            review_entry: has(nodes::PROJECT_ENTRY_REVIEW),
            edit_locked_entry: elevated_editor,
            force_save_presence: elevated_editor,
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
        assert!(!manager.resolve_languages);
        assert!(!manager.change_primary_source);
        assert!(!manager.delete_project);

        let owner = ProjectCapabilities::for_subject(true, Some(ProjectRole::Owner), true, false);
        assert!(owner.resolve_languages);
        assert!(owner.delete_project);
        assert!(!owner.change_primary_source);
    }
}
