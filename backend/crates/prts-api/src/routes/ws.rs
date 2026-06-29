//! 实时协作 WebSocket：`GET /ws/projects/{id}?token=<access_jwt>`。
//!
//! 浏览器 WebSocket 无法设置 Authorization 头，故用查询参数传 access token。
//! 鉴权 + 可见性校验后加入项目房间：转发房间事件给客户端，并接收客户端「正在编辑」通知。

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use prts_realtime::RoomEvent;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
}

/// WebSocket 升级入口。
pub async fn ws_handler(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(user_id) = authenticate(&state, q.token.as_deref()) else {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    };
    if !can_access(&state, project_id, user_id).await {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state, project_id, user_id))
}

fn authenticate(state: &AppState, token: Option<&str>) -> Option<i64> {
    let token = token?;
    let claims = prts_auth::jwt::decode(token, state.jwt_secret()).ok()?;
    if !claims.is_valid_at(chrono::Utc::now().timestamp()) {
        return None;
    }
    Some(claims.sub)
}

/// 公开项目任何登录用户可加入；私有项目需成员或平台管理。
async fn can_access(state: &AppState, project_id: i64, user_id: i64) -> bool {
    match prts_db::projects::find_by_id(&state.db, project_id).await {
        Ok(Some(p)) if p.visibility == "public" => true,
        Ok(Some(_)) => {
            if let Ok(Some(u)) = prts_db::users::find_by_id(&state.db, user_id).await {
                if matches!(
                    u.platform_role.as_deref(),
                    Some("super_admin") | Some("admin")
                ) {
                    return true;
                }
            }
            matches!(
                prts_db::memberships::find_role(&state.db, project_id, user_id).await,
                Ok(Some(_))
            )
        }
        _ => false,
    }
}

async fn handle_socket(socket: WebSocket, state: AppState, project_id: i64, user_id: i64) {
    let (mut sender, mut receiver) = socket.split();
    let (mut rx, online) = state.realtime.join(project_id, user_id).await;

    // 先把当前在线快照发给本连接
    if let Ok(snapshot) = serde_json::to_string(&RoomEvent::Join { user_id, online }) {
        let _ = sender.send(Message::Text(snapshot.into())).await;
    }

    // 房间事件 → 客户端
    let mut send_task = tokio::spawn(async move {
        while let Ok(payload) = rx.recv().await {
            if sender.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
    });

    // 客户端消息（正在编辑）→ 房间
    let hub = state.realtime.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(txt) = msg {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(txt.as_str()) {
                    if v.get("type").and_then(|t| t.as_str()) == Some("editing") {
                        if let Some(entry_id) = v.get("entry_id").and_then(|e| e.as_i64()) {
                            hub.publish(project_id, &RoomEvent::Editing { user_id, entry_id })
                                .await;
                        }
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    state.realtime.leave(project_id, user_id).await;
}
