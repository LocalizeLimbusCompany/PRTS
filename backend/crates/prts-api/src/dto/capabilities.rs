//! OpenAPI-facing capability DTO。

use serde::Serialize;
use utoipa::ToSchema;

use prts_core::permission::nodes;

/// 当前用户的显式平台能力；前端不得从平台角色名称推断权限。
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct PlatformCapabilitiesDto {
    pub access_admin: bool,
    pub grant_platform_roles: bool,
    pub manage_users: bool,
    pub create_project: bool,
    pub manage_pos: bool,
}

impl PlatformCapabilitiesDto {
    pub fn from_role(role: Option<&str>) -> Self {
        let role = role.and_then(prts_core::PlatformRole::parse);
        let has = |node| role.is_some_and(|role| role.has(node));
        Self {
            access_admin: has(nodes::PLATFORM_SETTINGS),
            grant_platform_roles: has(nodes::PLATFORM_ADMIN_GRANT),
            manage_users: has(nodes::PLATFORM_USER_MANAGE),
            create_project: has(nodes::PLATFORM_PROJECT_CREATE),
            manage_pos: has(nodes::PLATFORM_POS_MANAGE),
        }
    }
}

/// 当前主体在项目中的显式能力。
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct ProjectCapabilitiesDto {
    pub view_project: bool,
    pub manage_project: bool,
    pub manage_members: bool,
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

impl From<prts_core::capabilities::ProjectCapabilities> for ProjectCapabilitiesDto {
    fn from(value: prts_core::capabilities::ProjectCapabilities) -> Self {
        Self {
            view_project: value.view_project,
            manage_project: value.manage_project,
            manage_members: value.manage_members,
            upload_files: value.upload_files,
            view_file_history: value.view_file_history,
            rollback_file_history: value.rollback_file_history,
            manage_tasks: value.manage_tasks,
            manage_terms: value.manage_terms,
            download: value.download,
            edit_entry: value.edit_entry,
            review_entry: value.review_entry,
            lock_entry: value.lock_entry,
            hide_entry: value.hide_entry,
            edit_locked_entry: value.edit_locked_entry,
            force_save_presence: value.force_save_presence,
            collaborate: value.collaborate,
            resolve_languages: value.resolve_languages,
            change_primary_source: value.change_primary_source,
            delete_project: value.delete_project,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_capabilities_are_explicit_permission_node_projections() {
        let admin = PlatformCapabilitiesDto::from_role(Some("admin"));
        assert!(admin.access_admin);
        assert!(admin.manage_pos);
        assert!(admin.manage_users);
        assert!(!admin.grant_platform_roles);

        let maintainer = PlatformCapabilitiesDto::from_role(Some("maintainer"));
        assert!(maintainer.create_project);
        assert!(!maintainer.access_admin);
        assert!(!maintainer.manage_pos);
        assert!(!maintainer.manage_users);

        let ordinary = PlatformCapabilitiesDto::from_role(None);
        assert!(!ordinary.create_project);
        assert!(!ordinary.manage_pos);
        assert!(!ordinary.manage_users);
    }
}
