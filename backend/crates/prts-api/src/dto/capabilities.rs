//! OpenAPI-facing capability DTO。

use serde::Serialize;
use utoipa::ToSchema;

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
    pub download: bool,
    pub edit_entry: bool,
    pub review_entry: bool,
    pub edit_locked_entry: bool,
    pub force_save_presence: bool,
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
            download: value.download,
            edit_entry: value.edit_entry,
            review_entry: value.review_entry,
            edit_locked_entry: value.edit_locked_entry,
            force_save_presence: value.force_save_presence,
            resolve_languages: value.resolve_languages,
            change_primary_source: value.change_primary_source,
            delete_project: value.delete_project,
        }
    }
}
