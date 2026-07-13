//! 权限节点 RBAC（平台 + 项目）。见 plan §7。
//!
//! 角色 = 一组**权限节点**。平台角色全局生效（存于 `users.platform_role`）；
//! 项目角色随项目成员关系（P2 的 memberships）生效。判定统一为「该角色是否含某节点」。

use serde::{Deserialize, Serialize};

use crate::entry::EntryState;

/// 全部权限节点的稳定字符串标识。
pub mod nodes {
    // —— 平台级 ——
    /// 任免管理员（仅总管理员）。
    pub const PLATFORM_ADMIN_GRANT: &str = "platform.admin.grant";
    /// 创建项目。
    pub const PLATFORM_PROJECT_CREATE: &str = "platform.project.create";
    /// 管理所有项目。
    pub const PLATFORM_PROJECT_MANAGE_ALL: &str = "platform.project.manage_all";
    /// 删除任意项目。
    pub const PLATFORM_PROJECT_DELETE_ANY: &str = "platform.project.delete_any";
    /// 平台设置。
    pub const PLATFORM_SETTINGS: &str = "platform.settings";
    /// 清除操作历史。
    pub const PLATFORM_HISTORY_PURGE: &str = "platform.history.purge";

    // —— 项目级 ——
    /// 管理项目（设置 / 语言 / 文件结构等）。
    pub const PROJECT_MANAGE: &str = "project.manage";
    /// 删除项目。
    pub const PROJECT_DELETE: &str = "project.delete";
    /// 管理成员与角色。
    pub const PROJECT_MEMBER_MANAGE: &str = "project.member.manage";
    /// 上传文件。
    pub const PROJECT_FILE_UPLOAD: &str = "project.file.upload";
    /// 下载导出。
    pub const PROJECT_DOWNLOAD: &str = "project.download";
    /// 查看文件历史（全部项目成员）。
    pub const PROJECT_HISTORY_VIEW: &str = "project.history.view";
    /// 回滚/恢复文件历史（仅拥有者与管理）。
    pub const PROJECT_HISTORY_ROLLBACK: &str = "project.history.rollback";
    /// 编辑词条（翻译）。
    pub const PROJECT_ENTRY_EDIT: &str = "project.entry.edit";
    /// 校对 / 审核词条。
    pub const PROJECT_ENTRY_REVIEW: &str = "project.entry.review";
    /// 锁定 / 解锁词条。
    pub const PROJECT_ENTRY_LOCK: &str = "project.entry.lock";
    /// 隐藏 / 取消隐藏词条。
    pub const PROJECT_ENTRY_HIDE: &str = "project.entry.hide";
}

/// 平台级角色（全局）。层级：总管理员 > 管理员 > 维护者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformRole {
    /// 总管理员。
    SuperAdmin,
    /// 管理员。
    Admin,
    /// 维护者。
    Maintainer,
}

/// 项目级角色。层级：拥有者 > 管理 > 校对 > 翻译。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    /// 拥有者。
    Owner,
    /// 管理。
    Manager,
    /// 校对。
    Reviewer,
    /// 翻译。
    Translator,
}

impl PlatformRole {
    /// 解析数据库中的字符串标识。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "super_admin" => Some(Self::SuperAdmin),
            "admin" => Some(Self::Admin),
            "maintainer" => Some(Self::Maintainer),
            _ => None,
        }
    }

    /// 稳定字符串标识。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SuperAdmin => "super_admin",
            Self::Admin => "admin",
            Self::Maintainer => "maintainer",
        }
    }

    /// 该角色拥有的权限节点。
    pub fn nodes(self) -> &'static [&'static str] {
        use nodes::*;
        match self {
            Self::SuperAdmin => &[
                PLATFORM_ADMIN_GRANT,
                PLATFORM_PROJECT_CREATE,
                PLATFORM_PROJECT_MANAGE_ALL,
                PLATFORM_PROJECT_DELETE_ANY,
                PLATFORM_SETTINGS,
                PLATFORM_HISTORY_PURGE,
            ],
            // 管理员：除「任免管理员」外的全部平台能力。
            Self::Admin => &[
                PLATFORM_PROJECT_CREATE,
                PLATFORM_PROJECT_MANAGE_ALL,
                PLATFORM_PROJECT_DELETE_ANY,
                PLATFORM_SETTINGS,
                PLATFORM_HISTORY_PURGE,
            ],
            // 维护者：仅可创建项目。
            Self::Maintainer => &[PLATFORM_PROJECT_CREATE],
        }
    }

    /// 是否拥有某权限节点。
    pub fn has(self, node: &str) -> bool {
        self.nodes().contains(&node)
    }
}

impl ProjectRole {
    /// 解析数据库中的字符串标识。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Self::Owner),
            "manager" => Some(Self::Manager),
            "reviewer" => Some(Self::Reviewer),
            "translator" => Some(Self::Translator),
            _ => None,
        }
    }

    /// 稳定字符串标识。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Manager => "manager",
            Self::Reviewer => "reviewer",
            Self::Translator => "translator",
        }
    }

    /// 该角色拥有的权限节点。
    pub fn nodes(self) -> &'static [&'static str] {
        use nodes::*;
        match self {
            // 拥有者：项目内全部。
            Self::Owner => &[
                PROJECT_MANAGE,
                PROJECT_DELETE,
                PROJECT_MEMBER_MANAGE,
                PROJECT_FILE_UPLOAD,
                PROJECT_DOWNLOAD,
                PROJECT_HISTORY_VIEW,
                PROJECT_HISTORY_ROLLBACK,
                PROJECT_ENTRY_EDIT,
                PROJECT_ENTRY_REVIEW,
                PROJECT_ENTRY_LOCK,
                PROJECT_ENTRY_HIDE,
            ],
            // 管理：同拥有者但不可删除项目。
            Self::Manager => &[
                PROJECT_MANAGE,
                PROJECT_MEMBER_MANAGE,
                PROJECT_FILE_UPLOAD,
                PROJECT_DOWNLOAD,
                PROJECT_HISTORY_VIEW,
                PROJECT_HISTORY_ROLLBACK,
                PROJECT_ENTRY_EDIT,
                PROJECT_ENTRY_REVIEW,
                PROJECT_ENTRY_LOCK,
                PROJECT_ENTRY_HIDE,
            ],
            // 校对：可编辑与校对 / 审核，并可下载。
            Self::Reviewer => &[
                PROJECT_ENTRY_EDIT,
                PROJECT_ENTRY_REVIEW,
                PROJECT_DOWNLOAD,
                PROJECT_HISTORY_VIEW,
            ],
            // 翻译：仅可编辑词条与下载。
            Self::Translator => &[PROJECT_ENTRY_EDIT, PROJECT_DOWNLOAD, PROJECT_HISTORY_VIEW],
        }
    }

    /// 是否拥有某权限节点。
    pub fn has(self, node: &str) -> bool {
        self.nodes().contains(&node)
    }

    /// 是否可修改「已锁定」词条（仅拥有者与管理）。见 plan §6。
    pub fn can_edit_locked(self) -> bool {
        matches!(self, ProjectRole::Owner | ProjectRole::Manager)
    }
}

/// 把词条目标状态映射到所需的权限节点：
/// 未翻译/已翻译/有疑问 需 `project.entry.edit`；已检查/已审核 需 `project.entry.review`。
pub fn node_for_state(state: EntryState) -> &'static str {
    match state {
        EntryState::Untranslated | EntryState::Translated | EntryState::Questioned => {
            nodes::PROJECT_ENTRY_EDIT
        }
        EntryState::Checked | EntryState::Reviewed => nodes::PROJECT_ENTRY_REVIEW,
    }
}

#[cfg(test)]
mod tests {
    use super::nodes::*;
    use super::*;

    #[test]
    fn platform_role_parse_roundtrip() {
        for r in [
            PlatformRole::SuperAdmin,
            PlatformRole::Admin,
            PlatformRole::Maintainer,
        ] {
            assert_eq!(PlatformRole::parse(r.as_str()), Some(r));
        }
        assert_eq!(PlatformRole::parse("nope"), None);
    }

    #[test]
    fn only_super_admin_can_grant_admins() {
        assert!(PlatformRole::SuperAdmin.has(PLATFORM_ADMIN_GRANT));
        assert!(!PlatformRole::Admin.has(PLATFORM_ADMIN_GRANT));
        assert!(!PlatformRole::Maintainer.has(PLATFORM_ADMIN_GRANT));
    }

    #[test]
    fn maintainer_can_only_create_projects() {
        assert!(PlatformRole::Maintainer.has(PLATFORM_PROJECT_CREATE));
        assert!(!PlatformRole::Maintainer.has(PLATFORM_PROJECT_MANAGE_ALL));
        assert!(!PlatformRole::Maintainer.has(PLATFORM_PROJECT_DELETE_ANY));
    }

    #[test]
    fn owner_can_delete_but_manager_cannot() {
        assert!(ProjectRole::Owner.has(PROJECT_DELETE));
        assert!(!ProjectRole::Manager.has(PROJECT_DELETE));
        // 其余管理能力两者相同
        assert!(ProjectRole::Manager.has(PROJECT_MANAGE));
        assert!(ProjectRole::Manager.has(PROJECT_MEMBER_MANAGE));
    }

    #[test]
    fn translator_edits_but_cannot_review() {
        assert!(ProjectRole::Translator.has(PROJECT_ENTRY_EDIT));
        assert!(!ProjectRole::Translator.has(PROJECT_ENTRY_REVIEW));
        assert!(ProjectRole::Reviewer.has(PROJECT_ENTRY_REVIEW));
        assert!(ProjectRole::Reviewer.has(PROJECT_ENTRY_EDIT));
    }

    #[test]
    fn translator_and_reviewer_cannot_manage_members() {
        assert!(!ProjectRole::Translator.has(PROJECT_MEMBER_MANAGE));
        assert!(!ProjectRole::Reviewer.has(PROJECT_MEMBER_MANAGE));
        assert!(ProjectRole::Translator.has(PROJECT_HISTORY_VIEW));
        assert!(ProjectRole::Reviewer.has(PROJECT_HISTORY_VIEW));
        assert!(!ProjectRole::Translator.has(PROJECT_HISTORY_ROLLBACK));
        assert!(!ProjectRole::Reviewer.has(PROJECT_HISTORY_ROLLBACK));
    }

    #[test]
    fn node_for_state_maps_edit_vs_review() {
        assert_eq!(node_for_state(EntryState::Untranslated), PROJECT_ENTRY_EDIT);
        assert_eq!(node_for_state(EntryState::Translated), PROJECT_ENTRY_EDIT);
        assert_eq!(node_for_state(EntryState::Questioned), PROJECT_ENTRY_EDIT);
        assert_eq!(node_for_state(EntryState::Checked), PROJECT_ENTRY_REVIEW);
        assert_eq!(node_for_state(EntryState::Reviewed), PROJECT_ENTRY_REVIEW);
    }

    #[test]
    fn only_owner_and_manager_edit_locked() {
        assert!(ProjectRole::Owner.can_edit_locked());
        assert!(ProjectRole::Manager.can_edit_locked());
        assert!(!ProjectRole::Reviewer.can_edit_locked());
        assert!(!ProjectRole::Translator.can_edit_locked());
    }
}
