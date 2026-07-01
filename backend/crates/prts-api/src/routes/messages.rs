//! 私信 REST 端点（收发双方自助）。
//!
//! - `GET  /messages`：会话列表（每个对话方的最后一条 + 我方未读数）。
//! - `GET  /messages/unread_count`：我的未读私信总数（顶栏 ✉️ 红点）。
//! - `GET  /messages/{user_id}?before&limit`：与某用户的会话消息（键集分页）。
//! - `POST /messages {to_user_id, content}`：发送一条私信并实时推送。
//! - `POST /messages/{user_id}/read`：将与某用户的会话标记为已读。
//!
//! **门限**：私信双方须**共享 ≥1 项目**（`memberships` 交集），否则 403（防陌生人骚扰，红线 §8）；
//! `content` trim 后非空且 ≤2000 字；不允许给自己发私信。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

use prts_common::Error;
use prts_realtime::UserEvent;

use crate::auth::CurrentUser;
use crate::db_err;
use crate::error::ApiError;
use crate::state::AppState;

// ============================= DTO =============================

/// 私信对外表示。
#[derive(Debug, Serialize, ToSchema)]
pub struct MessageDto {
    pub id: i64,
    pub sender_id: i64,
    pub recipient_id: i64,
    pub content: String,
    pub read_at: Option<String>,
    pub created_at: String,
}

impl From<prts_db::models::Message> for MessageDto {
    fn from(m: prts_db::models::Message) -> Self {
        Self {
            id: m.id,
            sender_id: m.sender_id,
            recipient_id: m.recipient_id,
            content: m.content,
            read_at: m.read_at.map(|t| t.to_rfc3339()),
            created_at: m.created_at.to_rfc3339(),
        }
    }
}

/// 会话摘要对外表示（会话列表项）。
#[derive(Debug, Serialize, ToSchema)]
pub struct ThreadDto {
    pub other_user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub last_content: String,
    pub last_sender_id: i64,
    pub last_created_at: String,
    pub unread: i64,
}

impl From<prts_db::models::ConversationThread> for ThreadDto {
    fn from(t: prts_db::models::ConversationThread) -> Self {
        Self {
            other_user_id: t.other_user_id,
            username: t.username,
            avatar_url: t.avatar_url,
            last_content: t.last_content,
            last_sender_id: t.last_sender_id,
            last_created_at: t.last_created_at.to_rfc3339(),
            unread: t.unread,
        }
    }
}

/// 发送私信请求体。
#[derive(Debug, Deserialize, ToSchema)]
pub struct SendReq {
    /// 收件人用户 id（须与我共享 ≥1 项目）。
    pub to_user_id: i64,
    /// 消息正文（trim 后非空，≤2000 字）。
    pub content: String,
}

/// 发送成功响应（新消息 id）。
#[derive(Debug, Serialize, ToSchema)]
pub struct SentDto {
    pub id: i64,
}

/// 未读私信总数响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct UnreadCountDto {
    pub count: i64,
}

/// 会话查询参数（键集分页）。
#[derive(Debug, Deserialize)]
pub struct ConversationQuery {
    /// 键集游标：返回比此 id 更旧的消息。
    pub before: Option<i64>,
    /// 单页条数（1–100，默认 50）。
    pub limit: Option<i64>,
}

// ============================= 门限 =============================

/// 两个用户是否共享 ≥1 个项目（`memberships` 自连接交集）。
///
/// 私信门限：仅共享项目的用户之间可互发，防陌生人骚扰（红线 §8）。
async fn share_project(db: &PgPool, a: i64, b: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
             SELECT 1 FROM memberships m1
             JOIN memberships m2 ON m1.project_id = m2.project_id
             WHERE m1.user_id = $1 AND m2.user_id = $2)",
    )
    .bind(a)
    .bind(b)
    .fetch_one(db)
    .await
}

// ============================= 处理器 =============================

/// 我的会话列表（每个对话方的最后一条 + 我方未读数，最近有来往者在前）。
///
/// 需要已认证（`Authorization: Bearer <token>`）。
#[utoipa::path(
    get,
    path = "/messages",
    tag = "message",
    responses(
        (status = 200, description = "会话列表", body = [ThreadDto]),
        (status = 401, description = "未认证"),
    )
)]
pub async fn list_threads(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<ThreadDto>>, ApiError> {
    let threads = prts_db::messages::list_threads(&state.db, user.id)
        .await
        .map_err(db_err)?;
    Ok(Json(threads.into_iter().map(ThreadDto::from).collect()))
}

/// 我的未读私信总数（顶栏 ✉️ 红点）。
///
/// 需要已认证。
#[utoipa::path(
    get,
    path = "/messages/unread_count",
    tag = "message",
    responses(
        (status = 200, description = "未读私信数", body = UnreadCountDto),
        (status = 401, description = "未认证"),
    )
)]
pub async fn unread_count(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<UnreadCountDto>, ApiError> {
    let count = prts_db::messages::unread_count(&state.db, user.id)
        .await
        .map_err(db_err)?;
    Ok(Json(UnreadCountDto { count }))
}

/// 与某用户的会话消息（键集分页，按 id 降序）。
///
/// 须与对方共享 ≥1 项目，否则 403；`before` 游标翻页，`limit` 默认 50、最大 100。
#[utoipa::path(
    get,
    path = "/messages/{user_id}",
    tag = "message",
    params(
        ("user_id" = i64, Path, description = "对话方用户 id"),
        ("before" = Option<i64>, Query, description = "键集游标：返回比此 id 更旧的消息"),
        ("limit" = Option<i64>, Query, description = "单页条数（1–100，默认 50）"),
    ),
    responses(
        (status = 200, description = "会话消息", body = [MessageDto]),
        (status = 401, description = "未认证"),
        (status = 403, description = "与对方无共享项目"),
    )
)]
pub async fn conversation(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(other): Path<i64>,
    Query(q): Query<ConversationQuery>,
) -> Result<Json<Vec<MessageDto>>, ApiError> {
    if !share_project(&state.db, user.id, other)
        .await
        .map_err(db_err)?
    {
        return Err(Error::Forbidden.into());
    }
    let limit = q.limit.unwrap_or(50);
    let items = prts_db::messages::list_conversation(&state.db, user.id, other, q.before, limit)
        .await
        .map_err(db_err)?;
    Ok(Json(items.into_iter().map(MessageDto::from).collect()))
}

/// 发送一条私信（须与收件人共享 ≥1 项目），并实时推送给收件人。
///
/// `content` trim 后非空且 ≤2000 字；不允许给自己发私信。成功后经 `publish_user`
/// 向收件人推送 `UserEvent::DmMessage`（在线连接即时追加；离线下次加载经会话/未读数看到）。
#[utoipa::path(
    post,
    path = "/messages",
    tag = "message",
    request_body = SendReq,
    responses(
        (status = 200, description = "已发送", body = SentDto),
        (status = 400, description = "正文不合规或收件人为自己"),
        (status = 401, description = "未认证"),
        (status = 403, description = "与收件人无共享项目"),
    )
)]
pub async fn send(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<SendReq>,
) -> Result<Json<SentDto>, ApiError> {
    // 不允许给自己发私信。
    if body.to_user_id == user.id {
        return Err(Error::bad_request("不能给自己发私信").into());
    }

    // 正文校验（trim 非空、≤2000 字）。
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(Error::bad_request("content 不能为空").into());
    }
    if content.chars().count() > 2000 {
        return Err(Error::bad_request("content 不能超过 2000 字").into());
    }

    // 共享项目门限（防骚扰）。
    if !share_project(&state.db, user.id, body.to_user_id)
        .await
        .map_err(db_err)?
    {
        return Err(Error::Forbidden.into());
    }

    // 落库。
    let m = prts_db::messages::create(&state.db, user.id, body.to_user_id, &content)
        .await
        .map_err(db_err)?;

    // 实时推送给收件人（跨实例经 Redis）。
    state
        .realtime
        .publish_user(
            body.to_user_id,
            &UserEvent::DmMessage {
                id: m.id,
                from_user_id: user.id,
                content: m.content.clone(),
                created_at: m.created_at.to_rfc3339(),
            },
        )
        .await;

    Ok(Json(SentDto { id: m.id }))
}

/// 将与某用户的会话中「对方 → 我」的未读消息全部标记为已读（进入会话页即调用）。
///
/// 需要已认证。
#[utoipa::path(
    post,
    path = "/messages/{user_id}/read",
    tag = "message",
    params(("user_id" = i64, Path, description = "对话方用户 id")),
    responses(
        (status = 200, description = "标记成功"),
        (status = 401, description = "未认证"),
    )
)]
pub async fn mark_read(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(other): Path<i64>,
) -> Result<StatusCode, ApiError> {
    prts_db::messages::mark_read(&state.db, user.id, other)
        .await
        .map_err(db_err)?;
    Ok(StatusCode::OK)
}
