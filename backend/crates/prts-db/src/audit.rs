//! 追加式安全审计仓储。
//!
//! writer 只接受 action-specific DTO，并要求调用方传入事务连接，确保业务 mutation
//! 与审计同一次 PostgreSQL 提交。通用 JSON/实体序列化不会暴露为公开写接口。

use serde::Serialize;
use sqlx::PgConnection;

use crate::models::AuditLog;

/// 审计主体种类。
#[derive(Debug, Clone, Copy)]
pub enum AuditActorKind {
    /// 已登录用户。
    User,
    /// API Key 调用。
    ApiKey,
    /// 后台 worker/系统进程。
    System,
    /// 未认证请求。
    Anonymous,
}

impl AuditActorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ApiKey => "api_key",
            Self::System => "system",
            Self::Anonymous => "anonymous",
        }
    }
}

/// 审计主体与请求来源。
#[derive(Debug, Clone, Copy)]
pub struct AuditActor<'a> {
    /// 删除用户后仍保留的 actor id snapshot。
    pub id: Option<i64>,
    /// 主体种类。
    pub kind: AuditActorKind,
    /// 可选客户端 IP 文本；PostgreSQL 负责 INET 校验。
    pub ip: Option<&'a str>,
}

/// `job.retried` 的字段 allowlist。
#[derive(Debug, Serialize)]
struct JobRetriedPayload<'a> {
    kind: &'a str,
    previous_attempts: i32,
    new_attempts: i32,
}

/// `job.created` 的字段 allowlist。
#[derive(Debug, Serialize)]
struct JobCreatedPayload<'a> {
    kind: &'a str,
    stage: &'a str,
}

/// `auth.session_state_changed` 的字段 allowlist。
#[derive(Debug, Serialize)]
struct AuthSessionStatePayload<'a> {
    session_handle: &'a str,
    previous_state: &'a str,
    new_state: &'a str,
}

/// 追加任务创建审计。调用方必须与任务 INSERT 共用同一事务连接。
pub async fn append_job_created_tx(
    conn: &mut PgConnection,
    actor: AuditActor<'_>,
    job_id: i64,
    project_id_snapshot: Option<i64>,
    kind: &str,
    stage: &str,
) -> Result<AuditLog, sqlx::Error> {
    let payload = serde_json::to_value(JobCreatedPayload { kind, stage })
        .expect("allowlisted audit DTO must serialize");
    append_tx(
        conn,
        actor,
        "job.created",
        "job",
        &job_id.to_string(),
        project_id_snapshot,
        payload,
    )
    .await
}

/// 追加手动重试审计。payload 只包含 job kind 与计数，不包含 job 通用 payload。
pub async fn append_job_retried_tx(
    conn: &mut PgConnection,
    actor: AuditActor<'_>,
    job_id: i64,
    project_id_snapshot: Option<i64>,
    kind: &str,
    previous_attempts: i32,
    new_attempts: i32,
) -> Result<AuditLog, sqlx::Error> {
    let payload = serde_json::to_value(JobRetriedPayload {
        kind,
        previous_attempts,
        new_attempts,
    })
    .expect("allowlisted audit DTO must serialize");
    append_tx(
        conn,
        actor,
        "job.retried",
        "job",
        &job_id.to_string(),
        project_id_snapshot,
        payload,
    )
    .await
}

/// 追加认证会话状态变化审计；不接受 refresh hash 或 raw token。
pub async fn append_auth_session_state_tx(
    conn: &mut PgConnection,
    actor: AuditActor<'_>,
    session_id: i64,
    session_handle: &str,
    previous_state: &str,
    new_state: &str,
) -> Result<AuditLog, sqlx::Error> {
    let payload = serde_json::to_value(AuthSessionStatePayload {
        session_handle,
        previous_state,
        new_state,
    })
    .expect("allowlisted audit DTO must serialize");
    append_tx(
        conn,
        actor,
        "auth.session_state_changed",
        "auth_session",
        &session_id.to_string(),
        None,
        payload,
    )
    .await
}

/// 唯一底层 INSERT；保持私有以阻止调用方绕过 action-specific allowlist DTO。
async fn append_tx(
    conn: &mut PgConnection,
    actor: AuditActor<'_>,
    action: &str,
    target_type: &str,
    target_id: &str,
    project_id_snapshot: Option<i64>,
    payload: serde_json::Value,
) -> Result<AuditLog, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO audit_log (
             actor_id, actor_kind, action, target_type, target_id,
             project_id_snapshot, payload, ip
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::INET)
         RETURNING id, actor_id, actor_kind, action, target_type, target_id,
                   project_id_snapshot, payload, host(ip)::TEXT AS ip, created_at",
    )
    .bind(actor.id)
    .bind(actor.kind.as_str())
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(project_id_snapshot)
    .bind(payload)
    .bind(actor.ip)
    .fetch_one(conn)
    .await
}
