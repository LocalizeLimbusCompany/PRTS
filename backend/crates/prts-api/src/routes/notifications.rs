//! 通知 REST 端点（收件人自助）与「戳一下」发送端点。
//!
//! - `GET  /notifications?before&limit`：键集分页拉取当前用户通知列表。
//! - `GET  /notifications/unread_count`：当前用户未读通知数。
//! - `POST /notifications/read`：标记已读（`ids` 缺省/空 = 全部）。
//! - `POST /projects/{id}/poke`：向项目内某成员发「戳一下」通知并实时推送。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use prts_common::Error;
use prts_db::audit::{AuditActor, AuditActorKind, AuditEvent};
use prts_realtime::UserEvent;

use crate::auth::{project as paccess, CurrentUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

// ============================= DTO =============================

/// 通知对外表示（`kind` 序列化为 `type` 以与前端/线协议保持一致）。
#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationDto {
    pub id: i64,
    pub user_id: i64,
    /// 通知类型（如 `poke`）。序列化字段名为 `type`。
    #[serde(rename = "type")]
    pub kind: String,
    pub payload: serde_json::Value,
    pub read_at: Option<String>,
    pub created_at: String,
}

impl From<prts_db::models::Notification> for NotificationDto {
    fn from(n: prts_db::models::Notification) -> Self {
        Self {
            id: n.id,
            user_id: n.user_id,
            kind: n.kind,
            payload: n.payload,
            read_at: n.read_at.map(|t| t.to_rfc3339()),
            created_at: n.created_at.to_rfc3339(),
        }
    }
}

// ============================= 查询 DTO =============================

/// 通知列表查询参数。
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// 键集游标：返回比此 id 更旧的通知（降序）。
    pub before: Option<i64>,
    /// 单页条数（1–100，默认 50）。
    pub limit: Option<i64>,
}

// ============================= 处理器 =============================

/// 列出当前用户的通知（键集分页，按 id 降序）。
///
/// 通过 `before` 游标翻页，`limit` 默认 50，最大 100。
/// 需要已认证（`Authorization: Bearer <token>`）。
#[utoipa::path(
    get,
    path = "/notifications",
    tag = "notification",
    params(
        ("before" = Option<i64>, Query, description = "键集游标：返回比此 id 更旧的通知"),
        ("limit" = Option<i64>, Query, description = "单页条数（1–100，默认 50）"),
    ),
    responses(
        (status = 200, description = "通知列表", body = [NotificationDto]),
        (status = 401, description = "未认证"),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<NotificationDto>>, ApiError> {
    let limit = q.limit.unwrap_or(50);
    let items = prts_db::notifications::list(&state.db, user.id, q.before, limit)
        .await
        .map_err(db_err)?;
    Ok(Json(items.into_iter().map(NotificationDto::from).collect()))
}

/// 当前用户的未读通知数。
///
/// 需要已认证（`Authorization: Bearer <token>`）。
#[utoipa::path(
    get,
    path = "/notifications/unread_count",
    tag = "notification",
    responses(
        (status = 200, description = "未读通知数", body = UnreadCountDto),
        (status = 401, description = "未认证"),
    )
)]
pub async fn unread_count(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<UnreadCountDto>, ApiError> {
    let count = prts_db::notifications::unread_count(&state.db, user.id)
        .await
        .map_err(db_err)?;
    Ok(Json(UnreadCountDto { count }))
}

/// 未读通知数响应体。
#[derive(Debug, Serialize, ToSchema)]
pub struct UnreadCountDto {
    pub count: i64,
}

/// 标记已读请求体（`ids` 缺省或空 = 全部标记已读）。
#[derive(Debug, Deserialize, ToSchema)]
pub struct MarkReadReq {
    #[serde(default)]
    pub ids: Option<Vec<i64>>,
}

/// 标记通知已读。
///
/// `ids` 缺省或为空数组 = 将当前用户全部未读通知标记为已读。
/// 需要已认证（`Authorization: Bearer <token>`）。
#[utoipa::path(
    post,
    path = "/notifications/read",
    tag = "notification",
    request_body = MarkReadReq,
    responses(
        (status = 200, description = "标记成功"),
        (status = 401, description = "未认证"),
        (status = 503, description = "审计服务不可用，通知状态未更新", body = ErrorResponse),
    )
)]
pub async fn mark_read(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<MarkReadReq>,
) -> Result<StatusCode, ApiError> {
    let ids = body.ids.unwrap_or_default();
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let count = prts_db::notifications::mark_read_tx(&mut tx, user.id, &ids)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::NotificationMarkedRead {
            user_id: user.id,
            notification_ids: &ids,
            count,
            all: ids.is_empty(),
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;
    Ok(StatusCode::OK)
}

// ============================= poke =============================

/// 「戳一下」请求体。
#[derive(Debug, Deserialize, ToSchema)]
pub struct PokeReq {
    /// 收件人用户 id（必须是该项目成员）。
    pub to_user_id: i64,
    /// 提示文本（trim 后非空，最多 140 字）。
    pub text: String,
}

/// 给项目内某成员发「戳一下」通知（实时推送 + 持久化）。
///
/// 发送者须能访问该项目（`require_view`）且是项目成员；收件人也须是项目成员，否则 400。
/// `text` trim 后非空且 ≤140 字，否则 400。成功后向收件人实时推送 `UserEvent::Notification`。
#[utoipa::path(
    post,
    path = "/projects/{id}/poke",
    tag = "notification",
    request_body = PokeReq,
    responses(
        (status = 200, description = "poke 已发送"),
        (status = 400, description = "请求非法（目标非成员或文本不合规）"),
        (status = 401, description = "未认证"),
        (status = 403, description = "无权访问该项目"),
        (status = 404, description = "项目不存在"),
        (status = 503, description = "审计服务不可用，poke 未发送", body = ErrorResponse),
    )
)]
pub async fn poke(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(body): Json<PokeReq>,
) -> Result<StatusCode, ApiError> {
    // 发送者必须对该项目有查看权限（`require_view` 在私有项目中要求成员资格）。
    let access = paccess::load(&state, Some(&user), id).await?;
    access.require_view()?;

    // 验证文本。
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return Err(Error::bad_request("text 不能为空").into());
    }
    if text.chars().count() > 140 {
        return Err(Error::bad_request("text 不能超过 140 字").into());
    }

    // 查询发送者用户名（CurrentUser 不携带 username，需回查 DB）。
    let sender = prts_db::users::find_by_id(&state.db, user.id)
        .await
        .map_err(db_err)?
        .ok_or(Error::Unauthorized)?;

    // 构造 payload 并落库。
    let payload = json!({
        "from_user_id": user.id,
        "from_username": sender.username,
        "project_id": id,
        "text": text,
    });
    let text_length = text.chars().count();
    let mut tx = state.db.begin().await.map_err(db_err)?;
    let project = prts_db::projects::find_by_id_for_update_tx(&mut tx, id)
        .await
        .map_err(db_err)?
        .ok_or(Error::NotFound)?;
    paccess::load_locked_tx(&mut tx, &user, project)
        .await?
        .require_view()?;
    if prts_db::memberships::find_role_tx(&mut tx, id, user.id)
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err(Error::bad_request("发送者不是该项目成员").into());
    }
    if prts_db::memberships::find_role_tx(&mut tx, id, body.to_user_id)
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err(Error::bad_request("收件人不是该项目成员").into());
    }
    let n = prts_db::notifications::create_tx(&mut tx, body.to_user_id, "poke", &payload)
        .await
        .map_err(db_err)?;
    prts_db::audit::append_event_tx(
        &mut tx,
        AuditActor {
            id: Some(user.id),
            kind: AuditActorKind::User,
            ip: None,
        },
        AuditEvent::PokeSent {
            project_id: id,
            recipient_id: body.to_user_id,
            notification_id: n.id,
            text_length,
        },
    )
    .await
    .map_err(|_| Error::AuditUnavailable)?;
    tx.commit().await.map_err(db_err)?;

    // 实时推送给收件人（跨实例经 Redis）。
    state
        .realtime
        .publish_user(
            body.to_user_id,
            &UserEvent::Notification {
                id: n.id,
                kind: n.kind.clone(),
                payload: n.payload.clone(),
            },
        )
        .await;

    Ok(StatusCode::OK)
}
