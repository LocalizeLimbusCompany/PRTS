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

/// 已提供凭证的认证方式 allowlist；不得把凭证本身或任意客户端字符串写入审计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailureMethod {
    /// 无法解析出具体 bearer 类型的 Authorization 凭证。
    Authorization,
    /// PRTS API Key。
    ApiKey,
    /// 自签 access JWT。
    Jwt,
    /// 不透明 refresh token。
    Refresh,
}

impl AuthFailureMethod {
    /// 稳定审计 wire value。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authorization => "authorization",
            Self::ApiKey => "api_key",
            Self::Jwt => "jwt",
            Self::Refresh => "refresh",
        }
    }
}

/// 认证失败原因 allowlist；每个值只描述判定类别，不包含凭证或用户内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailureReason {
    InvalidCredential,
    InvalidTokenType,
    TokenExpired,
    MissingSession,
    SessionInactive,
    UserMismatch,
    UserNotFound,
    AccountInactive,
    InvalidRefresh,
}

impl AuthFailureReason {
    /// 稳定审计 reason code。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCredential => "invalid_credential",
            Self::InvalidTokenType => "invalid_token_type",
            Self::TokenExpired => "token_expired",
            Self::MissingSession => "missing_session",
            Self::SessionInactive => "session_inactive",
            Self::UserMismatch => "user_mismatch",
            Self::UserNotFound => "user_not_found",
            Self::AccountInactive => "account_inactive",
            Self::InvalidRefresh => "invalid_refresh",
        }
    }
}

/// 业务审计事件的封闭 allowlist。
///
/// 每个 variant 精确声明该 action 可以进入 payload 的字段；调用方无法传入通用 JSON、
/// 完整实体或任意 action，从类型边界阻止密码、token、正文与其它秘密被序列化。
#[derive(Debug)]
pub enum AuditEvent<'a> {
    AuthRegistered {
        user_id: i64,
        method: &'a str,
        status: &'a str,
    },
    AuthLoginSucceeded {
        user_id: i64,
        method: &'a str,
    },
    AuthLoginFailed {
        user_id: i64,
        method: &'a str,
        reason_code: &'a str,
    },
    AuthFailed {
        user_id: i64,
        method: AuthFailureMethod,
        reason: AuthFailureReason,
    },
    AuthOAuthSucceeded {
        user_id: i64,
        provider: &'a str,
        new_user: bool,
    },
    AuthOAuthFailed {
        target_id: &'a str,
        provider: &'a str,
        reason_code: &'a str,
    },
    AuthTokenIssued {
        session_id: i64,
        session_handle: &'a str,
        method: &'a str,
        expires_at: chrono::DateTime<chrono::Utc>,
    },
    AuthRefreshRotated {
        session_id: i64,
        session_handle: &'a str,
        predecessor_handle: &'a str,
        expires_at: chrono::DateTime<chrono::Utc>,
    },
    AuthLoggedOut {
        session_id: i64,
        session_handle: &'a str,
        revoked_sessions: i64,
    },
    AuthBootstrapRoleGranted {
        user_id: i64,
        role: &'a str,
    },
    UserProfileUpdated {
        user_id: i64,
        changed_fields: &'a [&'a str],
        translation_lang_count: usize,
    },
    ApiKeyCreated {
        key_id: i64,
        name: &'a str,
        prefix: &'a str,
    },
    ApiKeyUsed {
        key_id: i64,
        prefix: &'a str,
    },
    ApiKeyRevoked {
        key_id: i64,
        prefix: &'a str,
    },
    SettingsUpdated {
        keys: &'a [String],
    },
    SearchSettingsUpdated {
        changed_fields: &'a [&'a str],
    },
    UploadSettingsUpdated {
        changed_fields: &'a [&'a str],
    },
    UserPlatformRoleChanged {
        user_id: i64,
        previous_role: Option<&'a str>,
        new_role: Option<&'a str>,
    },
    ProjectCreated {
        project_id: i64,
        slug: &'a str,
        visibility: &'a str,
        source_langs: &'a [String],
        target_lang: &'a str,
    },
    ProjectUpdated {
        project_id: i64,
        changed_fields: &'a [&'a str],
        visibility: &'a str,
    },
    ProjectAvatarUpdated {
        project_id: i64,
        content_type: &'a str,
        encoded_bytes: usize,
        replaced: bool,
    },
    ProjectAvatarDeleted {
        project_id: i64,
    },
    ProjectPrimarySourceChanged {
        project_id: i64,
        previous_primary_source: &'a str,
        new_primary_source: &'a str,
        source_language_count: usize,
        lexical_job_id: i64,
    },
    ProjectLanguageResolutionCompleted {
        project_id: i64,
        issue_count: usize,
        source_language_count: usize,
        primary_source_language: &'a str,
        target_language: &'a str,
    },
    ProjectLanguageRepairRetried {
        project_id: i64,
        job_id: i64,
        previous_state: &'a str,
    },
    ProjectDeleted {
        project_id: i64,
        slug: &'a str,
    },
    MembershipUpserted {
        project_id: i64,
        member_id: i64,
        previous_role: Option<&'a str>,
        new_role: &'a str,
    },
    MembershipRemoved {
        project_id: i64,
        member_id: i64,
        previous_role: &'a str,
    },
    EntriesUploaded {
        project_id: i64,
        file_id: i64,
        path: &'a str,
        created: usize,
        updated: usize,
        unchanged: usize,
    },
    FileDeleted {
        project_id: i64,
        file_id: i64,
        path: &'a str,
        entry_count: i32,
    },
    FolderDeleted {
        project_id: i64,
        folder_id: i64,
        path: &'a str,
        file_count: i64,
        entry_count: i64,
    },
    EntryUpdated {
        project_id: i64,
        entry_id: i64,
        previous_version: i64,
        new_version: i64,
        previous_state: &'a str,
        new_state: &'a str,
    },
    EntryFlagsUpdated {
        project_id: i64,
        entry_id: i64,
        locked: bool,
        hidden: bool,
    },
    ProjectExported {
        project_id: i64,
        file_count: usize,
        entry_count: usize,
        include_hidden: bool,
    },
    NotificationMarkedRead {
        user_id: i64,
        notification_ids: &'a [i64],
        count: u64,
        all: bool,
    },
    PokeSent {
        project_id: i64,
        recipient_id: i64,
        notification_id: i64,
        text_length: usize,
    },
    MessageSent {
        message_id: i64,
        recipient_id: i64,
        content_length: usize,
    },
    MessageMarkedRead {
        other_user_id: i64,
        count: u64,
    },
}

/// 追加一个强类型业务审计事件；业务写与本函数必须复用同一事务连接。
pub async fn append_event_tx(
    conn: &mut PgConnection,
    actor: AuditActor<'_>,
    event: AuditEvent<'_>,
) -> Result<AuditLog, sqlx::Error> {
    let (action, target_type, target_id, project_id_snapshot, payload) = match event {
        AuditEvent::AuthRegistered {
            user_id,
            method,
            status,
        } => (
            "auth.registered",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({"method": method, "status": status}),
        ),
        AuditEvent::AuthLoginSucceeded { user_id, method } => (
            "auth.login_succeeded",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({"method": method}),
        ),
        AuditEvent::AuthLoginFailed {
            user_id,
            method,
            reason_code,
        } => (
            "auth.login_failed",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({"method": method, "reason_code": reason_code}),
        ),
        AuditEvent::AuthFailed {
            user_id,
            method,
            reason,
        } => (
            "auth.failed",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({
                "method": method.as_str(),
                "reason_code": reason.as_str(),
            }),
        ),
        AuditEvent::AuthOAuthSucceeded {
            user_id,
            provider,
            new_user,
        } => (
            "auth.oauth_succeeded",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({"provider": provider, "new_user": new_user}),
        ),
        AuditEvent::AuthOAuthFailed {
            target_id,
            provider,
            reason_code,
        } => (
            "auth.oauth_failed",
            "oauth_identity",
            target_id.to_string(),
            None,
            serde_json::json!({"provider": provider, "reason_code": reason_code}),
        ),
        AuditEvent::AuthTokenIssued {
            session_id,
            session_handle,
            method,
            expires_at,
        } => (
            "auth.token_issued",
            "auth_session",
            session_id.to_string(),
            None,
            serde_json::json!({
                "session_handle": session_handle,
                "method": method,
                "expires_at": expires_at,
            }),
        ),
        AuditEvent::AuthRefreshRotated {
            session_id,
            session_handle,
            predecessor_handle,
            expires_at,
        } => (
            "auth.refresh_rotated",
            "auth_session",
            session_id.to_string(),
            None,
            serde_json::json!({
                "session_handle": session_handle,
                "predecessor_handle": predecessor_handle,
                "expires_at": expires_at,
            }),
        ),
        AuditEvent::AuthLoggedOut {
            session_id,
            session_handle,
            revoked_sessions,
        } => (
            "auth.logged_out",
            "auth_session",
            session_id.to_string(),
            None,
            serde_json::json!({
                "session_handle": session_handle,
                "revoked_sessions": revoked_sessions,
            }),
        ),
        AuditEvent::AuthBootstrapRoleGranted { user_id, role } => (
            "auth.bootstrap_role_granted",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({"role": role}),
        ),
        AuditEvent::UserProfileUpdated {
            user_id,
            changed_fields,
            translation_lang_count,
        } => (
            "user.profile_updated",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({
                "changed_fields": changed_fields,
                "translation_lang_count": translation_lang_count,
            }),
        ),
        AuditEvent::ApiKeyCreated {
            key_id,
            name,
            prefix,
        } => (
            "api_key.created",
            "api_key",
            key_id.to_string(),
            None,
            serde_json::json!({"name": name, "prefix": prefix}),
        ),
        AuditEvent::ApiKeyUsed { key_id, prefix } => (
            "api_key.used",
            "api_key",
            key_id.to_string(),
            None,
            serde_json::json!({"prefix": prefix}),
        ),
        AuditEvent::ApiKeyRevoked { key_id, prefix } => (
            "api_key.revoked",
            "api_key",
            key_id.to_string(),
            None,
            serde_json::json!({"prefix": prefix}),
        ),
        AuditEvent::SettingsUpdated { keys } => (
            "settings.updated",
            "settings",
            "platform".to_string(),
            None,
            serde_json::json!({"keys": keys, "count": keys.len()}),
        ),
        AuditEvent::SearchSettingsUpdated { changed_fields } => (
            "search_settings.updated",
            "settings",
            "search.config".to_string(),
            None,
            serde_json::json!({"changed_fields": changed_fields}),
        ),
        AuditEvent::UploadSettingsUpdated { changed_fields } => (
            "upload_settings.updated",
            "settings",
            "upload.config".to_string(),
            None,
            serde_json::json!({"changed_fields": changed_fields}),
        ),
        AuditEvent::UserPlatformRoleChanged {
            user_id,
            previous_role,
            new_role,
        } => (
            "user.platform_role_changed",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({
                "previous_role": previous_role,
                "new_role": new_role,
            }),
        ),
        AuditEvent::ProjectCreated {
            project_id,
            slug,
            visibility,
            source_langs,
            target_lang,
        } => (
            "project.created",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "slug": slug,
                "visibility": visibility,
                "source_langs": source_langs,
                "target_lang": target_lang,
            }),
        ),
        AuditEvent::ProjectUpdated {
            project_id,
            changed_fields,
            visibility,
        } => (
            "project.updated",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "changed_fields": changed_fields,
                "visibility": visibility,
            }),
        ),
        AuditEvent::ProjectAvatarUpdated {
            project_id,
            content_type,
            encoded_bytes,
            replaced,
        } => (
            "project.avatar_updated",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "content_type": content_type,
                "encoded_bytes": encoded_bytes,
                "replaced": replaced,
            }),
        ),
        AuditEvent::ProjectAvatarDeleted { project_id } => (
            "project.avatar_deleted",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({"had_avatar": true}),
        ),
        AuditEvent::ProjectPrimarySourceChanged {
            project_id,
            previous_primary_source,
            new_primary_source,
            source_language_count,
            lexical_job_id,
        } => (
            "project.primary_source_changed",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "previous_primary_source": previous_primary_source,
                "new_primary_source": new_primary_source,
                "source_language_count": source_language_count,
                "lexical_job_id": lexical_job_id,
            }),
        ),
        AuditEvent::ProjectLanguageResolutionCompleted {
            project_id,
            issue_count,
            source_language_count,
            primary_source_language,
            target_language,
        } => (
            "project.language_resolution_completed",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "issue_count": issue_count,
                "source_language_count": source_language_count,
                "primary_source_language": primary_source_language,
                "target_language": target_language,
            }),
        ),
        AuditEvent::ProjectLanguageRepairRetried {
            project_id,
            job_id,
            previous_state,
        } => (
            "project.language_repair_retried",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({"job_id": job_id, "previous_state": previous_state}),
        ),
        AuditEvent::ProjectDeleted { project_id, slug } => (
            "project.deleted",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({"slug": slug}),
        ),
        AuditEvent::MembershipUpserted {
            project_id,
            member_id,
            previous_role,
            new_role,
        } => (
            "membership.upserted",
            "membership",
            format!("{project_id}:{member_id}"),
            Some(project_id),
            serde_json::json!({
                "member_id": member_id,
                "previous_role": previous_role,
                "new_role": new_role,
            }),
        ),
        AuditEvent::MembershipRemoved {
            project_id,
            member_id,
            previous_role,
        } => (
            "membership.removed",
            "membership",
            format!("{project_id}:{member_id}"),
            Some(project_id),
            serde_json::json!({
                "member_id": member_id,
                "previous_role": previous_role,
            }),
        ),
        AuditEvent::EntriesUploaded {
            project_id,
            file_id,
            path,
            created,
            updated,
            unchanged,
        } => (
            "entries.uploaded",
            "file",
            file_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "file_id": file_id,
                "path": path,
                "created": created,
                "updated": updated,
                "unchanged": unchanged,
            }),
        ),
        AuditEvent::FileDeleted {
            project_id,
            file_id,
            path,
            entry_count,
        } => (
            "file.deleted",
            "file",
            file_id.to_string(),
            Some(project_id),
            serde_json::json!({"path": path, "entry_count": entry_count}),
        ),
        AuditEvent::FolderDeleted {
            project_id,
            folder_id,
            path,
            file_count,
            entry_count,
        } => (
            "folder.deleted",
            "folder",
            folder_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "path": path,
                "file_count": file_count,
                "entry_count": entry_count,
            }),
        ),
        AuditEvent::EntryUpdated {
            project_id,
            entry_id,
            previous_version,
            new_version,
            previous_state,
            new_state,
        } => (
            "entry.updated",
            "entry",
            entry_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "previous_version": previous_version,
                "new_version": new_version,
                "previous_state": previous_state,
                "new_state": new_state,
            }),
        ),
        AuditEvent::EntryFlagsUpdated {
            project_id,
            entry_id,
            locked,
            hidden,
        } => (
            "entry.flags_updated",
            "entry",
            entry_id.to_string(),
            Some(project_id),
            serde_json::json!({"locked": locked, "hidden": hidden}),
        ),
        AuditEvent::ProjectExported {
            project_id,
            file_count,
            entry_count,
            include_hidden,
        } => (
            "project.exported",
            "project",
            project_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "file_count": file_count,
                "entry_count": entry_count,
                "include_hidden": include_hidden,
            }),
        ),
        AuditEvent::NotificationMarkedRead {
            user_id,
            notification_ids,
            count,
            all,
        } => (
            "notification.marked_read",
            "user",
            user_id.to_string(),
            None,
            serde_json::json!({
                "notification_ids": notification_ids,
                "count": count,
                "all": all,
            }),
        ),
        AuditEvent::PokeSent {
            project_id,
            recipient_id,
            notification_id,
            text_length,
        } => (
            "poke.sent",
            "notification",
            notification_id.to_string(),
            Some(project_id),
            serde_json::json!({
                "recipient_id": recipient_id,
                "project_id": project_id,
                "notification_id": notification_id,
                "text_length": text_length,
            }),
        ),
        AuditEvent::MessageSent {
            message_id,
            recipient_id,
            content_length,
        } => (
            "message.sent",
            "message",
            message_id.to_string(),
            None,
            serde_json::json!({
                "recipient_id": recipient_id,
                "message_id": message_id,
                "content_length": content_length,
            }),
        ),
        AuditEvent::MessageMarkedRead {
            other_user_id,
            count,
        } => (
            "message.marked_read",
            "conversation",
            other_user_id.to_string(),
            None,
            serde_json::json!({"other_user_id": other_user_id, "count": count}),
        ),
    };
    append_tx(
        conn,
        actor,
        action,
        target_type,
        &target_id,
        project_id_snapshot,
        payload,
    )
    .await
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
