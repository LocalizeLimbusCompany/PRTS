//! 对外数据传输对象（DTO）。与数据库行模型分离，避免泄露敏感字段（如 password_hash）。

use serde::Serialize;
use utoipa::ToSchema;

use prts_db::models::User;

#[path = "dto/capabilities.rs"]
pub mod capabilities;
#[path = "dto/upload.rs"]
pub mod upload;

/// 用户对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct UserDto {
    pub id: i64,
    pub username: String,
    /// 仅本人可见的邮箱（公开资料接口将省略，P2）。
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub description: String,
    /// 个人翻译语言偏好（BCP-47）。
    pub translation_langs: Vec<String>,
    /// Exact contribution tenths; one stored unit equals 0.1 CP.
    pub cp_tenths: i64,
    /// 平台角色（super_admin|admin|maintainer），普通用户为 null。
    pub platform_role: Option<String>,
    /// 显式平台能力；客户端不得从 platform_role 字符串推断授权。
    pub platform_capabilities: capabilities::PlatformCapabilitiesDto,
    /// 持久、非阻断的密码修改提醒。
    pub password_change_required: bool,
    /// 加入时间（RFC3339）。
    pub created_at: String,
}

impl From<&User> for UserDto {
    fn from(u: &User) -> Self {
        Self {
            id: u.id,
            username: u.username.clone(),
            email: u.email.clone(),
            avatar_url: u.avatar_url.clone(),
            description: u.description.clone(),
            translation_langs: u.translation_langs.clone(),
            cp_tenths: u.cp_tenths,
            platform_role: u.platform_role.clone(),
            platform_capabilities: capabilities::PlatformCapabilitiesDto::from_role(
                u.platform_role.as_deref(),
            ),
            password_change_required: u.password_change_required,
            created_at: u.created_at.to_rfc3339(),
        }
    }
}
