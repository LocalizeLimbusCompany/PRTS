//! 数据库行模型（仅 `FromRow`，不直接对外序列化；API 层另建 DTO 以隐藏敏感字段）。

use chrono::{DateTime, Utc};
use sqlx::FromRow;

/// 用户行。
#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
    /// Argon2 PHC 哈希；纯 OAuth 账号为空。**切勿对外序列化。**
    pub password_hash: Option<String>,
    pub avatar_url: Option<String>,
    pub description: String,
    pub translation_langs: Vec<String>,
    pub cp: f64,
    pub platform_role: Option<String>,
    pub email_verified: bool,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 外部账号行（关联账号）。
#[derive(Debug, Clone, FromRow)]
pub struct ExternalAccount {
    pub id: i64,
    pub user_id: i64,
    pub provider: String,
    pub external_id: String,
    pub raw: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// API Key 行（不含明文）。
#[derive(Debug, Clone, FromRow)]
pub struct ApiKeyRecord {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub key_hash: String,
    pub prefix: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// 平台设置行。
#[derive(Debug, Clone, FromRow)]
pub struct Setting {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<i64>,
}

/// 项目行。
#[derive(Debug, Clone, FromRow)]
pub struct Project {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub visibility: String,
    pub source_langs: Vec<String>,
    pub target_lang: String,
    pub owner_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 项目成员行。
#[derive(Debug, Clone, FromRow)]
pub struct Membership {
    pub project_id: i64,
    pub user_id: i64,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

/// 成员信息（含用户名/头像，用于成员列表展示）。
#[derive(Debug, Clone, FromRow)]
pub struct MemberInfo {
    pub user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

/// 文件夹行。
#[derive(Debug, Clone, FromRow)]
pub struct Folder {
    pub id: i64,
    pub project_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub created_at: DateTime<Utc>,
}

/// 文件行。
#[derive(Debug, Clone, FromRow)]
pub struct File {
    pub id: i64,
    pub project_id: i64,
    pub folder_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub entry_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 词条行。
#[derive(Debug, Clone, FromRow)]
pub struct Entry {
    pub id: i64,
    pub file_id: i64,
    pub project_id: i64,
    pub key: String,
    pub original: serde_json::Value,
    pub context: String,
    pub translation: String,
    pub state: String,
    pub locked: bool,
    pub hidden: bool,
    pub version: i64,
    pub updated_by: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 词条历史行。
#[derive(Debug, Clone, FromRow)]
pub struct EntryVersion {
    pub id: i64,
    pub entry_id: i64,
    pub version: i64,
    pub kind: String,
    pub translation: Option<String>,
    pub state: Option<String>,
    pub original: Option<serde_json::Value>,
    pub editor_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}
