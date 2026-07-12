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
    /// 贡献分。
    pub cp: f64,
    /// 平台角色（super_admin|admin|maintainer），普通用户为 null。
    pub platform_role: Option<String>,
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
            cp: u.cp,
            platform_role: u.platform_role.clone(),
            created_at: u.created_at.to_rfc3339(),
        }
    }
}
