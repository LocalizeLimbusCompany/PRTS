//! 项目级访问控制：综合项目可见性、成员角色与平台角色，判定可见性与权限节点。

use prts_common::Error;
use prts_core::permission::nodes;
use prts_core::{PlatformRole, ProjectRole};
use sqlx::PgConnection;

use crate::auth::CurrentUser;
use crate::db_err;
use crate::error::ApiError;
use crate::state::AppState;

/// 某用户（或游客）对某项目的访问上下文。
pub struct ProjectAccess {
    pub project: prts_db::models::Project,
    pub user_id: Option<i64>,
    project_role: Option<ProjectRole>,
    platform_role: Option<PlatformRole>,
}

impl ProjectAccess {
    /// 是否是项目拥有者或显式项目成员；平台跨项目管理权限不会被视作项目成员。
    pub fn is_project_member(&self) -> bool {
        self.user_id == Some(self.project.owner_id) || self.project_role.is_some()
    }

    /// 有效项目角色：平台 admin/super_admin 对任意项目等同拥有者；否则取成员角色。
    pub fn effective_role(&self) -> Option<ProjectRole> {
        if let Some(pr) = self.platform_role {
            if pr.has(nodes::PLATFORM_PROJECT_MANAGE_ALL) {
                return Some(ProjectRole::Owner);
            }
        }
        self.project_role
    }

    /// 是否可见：公开项目人人可见；私有项目仅成员或平台管理可见。
    pub fn can_view(&self) -> bool {
        if self.project.deletion_scheduled_at.is_some() {
            return false;
        }
        self.project.visibility == "public" || self.effective_role().is_some()
    }

    /// 是否拥有某项目权限节点。
    pub fn has_node(&self, node: &str) -> bool {
        if self.project.deletion_scheduled_at.is_some() {
            return false;
        }
        self.effective_role().map(|r| r.has(node)).unwrap_or(false)
    }

    /// 当前主体的显式 capability；owner-only 能力只比较 `projects.owner_id`。
    pub fn capabilities(
        &self,
        primary_source_release_ready: bool,
    ) -> prts_core::capabilities::ProjectCapabilities {
        if self.project.deletion_scheduled_at.is_some() {
            return prts_core::capabilities::ProjectCapabilities::for_subject(
                false, None, false, false, false,
            );
        }
        prts_core::capabilities::ProjectCapabilities::for_subject(
            self.can_view(),
            self.effective_role(),
            self.user_id == Some(self.project.owner_id),
            self.is_project_member(),
            primary_source_release_ready,
        )
    }

    /// 语言歧义项目禁止普通搜索、上传和语言写入。
    pub fn require_language_ready(&self) -> Result<(), ApiError> {
        if self.project.language_repair_state == "ready" {
            Ok(())
        } else {
            Err(Error::ProjectLanguageResolutionRequired.into())
        }
    }

    /// 要求可见，否则 404（不泄露私有项目存在性）。
    pub fn require_view(&self) -> Result<(), ApiError> {
        if self.can_view() {
            Ok(())
        } else {
            Err(Error::NotFound.into())
        }
    }

    /// 要求某项目权限节点，否则 401（未登录）/ 403（无权）。
    pub fn require_node(&self, node: &str) -> Result<(), ApiError> {
        if self.project.deletion_scheduled_at.is_some() {
            return if self.user_id == Some(self.project.owner_id) {
                Err(Error::ProjectPendingDeletion.into())
            } else {
                Err(Error::NotFound.into())
            };
        }
        if self.has_node(node) {
            Ok(())
        } else if self.user_id.is_none() {
            Err(Error::Unauthorized.into())
        } else {
            Err(Error::Forbidden.into())
        }
    }
}

/// 加载项目访问上下文。项目不存在 → 404。
pub async fn load(
    state: &AppState,
    user: Option<&CurrentUser>,
    project_id: i64,
) -> Result<ProjectAccess, ApiError> {
    let project = prts_db::projects::find_by_id(&state.db, project_id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    if project.deletion_scheduled_at.is_some()
        && user.is_none_or(|actor| actor.id != project.owner_id)
    {
        return Err(Error::NotFound.into());
    }

    let (user_id, platform_role, project_role) = match user {
        Some(u) => {
            let role = prts_db::memberships::find_role(&state.db, project_id, u.id)
                .await
                .map_err(db_err)?
                .as_deref()
                .and_then(ProjectRole::parse);
            (Some(u.id), u.platform_role, role)
        }
        None => (None, None, None),
    };

    Ok(ProjectAccess {
        project,
        user_id,
        project_role,
        platform_role,
    })
}

/// 在已锁定项目行的调用方事务内重新加载授权快照。
///
/// 项目 mutation 可先在事务外做快速拒绝，再以本函数取得锁后的最新成员/平台角色，
/// 防止并发撤权后继续使用陈旧的 [`ProjectAccess`]。
pub async fn load_locked_tx(
    conn: &mut PgConnection,
    user: &CurrentUser,
    project: prts_db::models::Project,
) -> Result<ProjectAccess, ApiError> {
    let current_user = prts_db::users::find_by_id_for_update_tx(conn, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;
    let project_role = prts_db::memberships::find_role_tx(conn, project.id, user.id)
        .await
        .map_err(db_err)?
        .as_deref()
        .and_then(ProjectRole::parse);
    let platform_role = current_user
        .platform_role
        .as_deref()
        .and_then(PlatformRole::parse);

    Ok(ProjectAccess {
        project,
        user_id: Some(user.id),
        project_role,
        platform_role,
    })
}
