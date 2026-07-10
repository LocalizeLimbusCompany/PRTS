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

/// 通知行（收件人维度；`poke` 为第一种 type）。
#[derive(Debug, Clone, FromRow)]
pub struct Notification {
    pub id: i64,
    /// 收件人用户 id。
    pub user_id: i64,
    /// 通知类型（如 `poke`）。`type` 是 SQL 保留列名，映射到字段 `kind`。
    #[sqlx(rename = "type")]
    pub kind: String,
    /// 类型相关的结构化载荷（如 poke 的 from_user_id / from_username / text）。
    pub payload: serde_json::Value,
    /// 已读时间；`None` 表示未读。
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
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

/// 私信行（一条消息；会话 = 一对用户之间的全部消息，无独立会话表）。
#[derive(Debug, Clone, FromRow)]
pub struct Message {
    pub id: i64,
    /// 发送者用户 id。
    pub sender_id: i64,
    /// 收件人用户 id。
    pub recipient_id: i64,
    /// 消息正文（应用层限制 ≤ 2000 字）。
    pub content: String,
    /// 收件人读取时间；`None` 表示未读。
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// 会话摘要行（会话列表用）：对话方资料 + 该会话最后一条消息 + 我方未读数。
#[derive(Debug, Clone, FromRow)]
pub struct ConversationThread {
    /// 对话方（对方）用户 id。
    pub other_user_id: i64,
    /// 对话方用户名。
    pub username: String,
    /// 对话方头像。
    pub avatar_url: Option<String>,
    /// 该会话最后一条消息正文。
    pub last_content: String,
    /// 最后一条消息的发送者 id（前端据此判断是否显示「你: 」前缀）。
    pub last_sender_id: i64,
    /// 最后一条消息时间。
    pub last_created_at: DateTime<Utc>,
    /// 我方在该会话中的未读消息数。
    pub unread: i64,
}

/// 追加式审计行。IP 在查询时规范化为文本，避免向上层暴露数据库专用网络类型。
#[derive(Debug, Clone, FromRow)]
pub struct AuditLog {
    pub id: i64,
    pub actor_id: Option<i64>,
    pub actor_kind: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub project_id_snapshot: Option<i64>,
    pub payload: serde_json::Value,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 持久化任务行。payload/result 只供受控 worker 使用，不直接序列化到 API。
#[derive(Debug, Clone, FromRow)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub project_id: Option<i64>,
    pub state: String,
    pub pause_reason: Option<String>,
    pub stage: String,
    pub payload: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub progress_current: i64,
    pub progress_total: Option<i64>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub run_after: DateTime<Utc>,
    pub lease_until: Option<DateTime<Utc>>,
    pub worker_id: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}
