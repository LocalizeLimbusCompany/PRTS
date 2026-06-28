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
