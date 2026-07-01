//! `prts-realtime` —— WebSocket 实时协作 hub。
//!
//! 设计（见 plan §13、docs/architecture.md §3.4）：以**项目**为房间，广播在线状态、
//! 「他人正在编辑」与词条变更；通过 Redis pub/sub 在多实例间同步。
//!
//! 除项目房间外，另有按 **用户** 的通知房间（独立 Redis 频道 [`USER_CHANNEL`]）：
//! 用于把针对某个收件人的即时通知（如「戳一下」）推送到其所有在线连接，
//! 与项目房间并行、互不影响。
//!
//! 本地投递也走 Redis 往返（发布实例自身亦订阅同一频道），从而单/多实例行为一致、无重复投递。

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};

/// 房间标识：项目 id。
pub type RoomId = i64;

/// 房间广播事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoomEvent {
    /// 某用户上线（附当前在线用户快照）。
    Join { user_id: i64, online: Vec<i64> },
    /// 某用户离线。
    Leave { user_id: i64 },
    /// 某用户正在编辑某词条。
    Editing { user_id: i64, entry_id: i64 },
    /// 某词条已更新（携带新版本号，供客户端实时刷新 / 乐观锁对齐）。
    EntryUpdated {
        entry_id: i64,
        version: i64,
        by: i64,
    },
}

/// 用户通知事件（用户频道专用，区别于 project 房间的 [`RoomEvent`]）。
///
/// - [`UserEvent::Notification`]：一条新通知落库后推送给收件人（`kind` 即通知类型，如 `poke`）。
/// - [`UserEvent::DmMessage`]：一条新私信落库后推送给收件人，其在线连接即时追加（见 Spec D）。
///
/// `#[serde(tag = "type")]` 使线协议为 `{"type":"Notification",…}` / `{"type":"DmMessage",…}`，
/// 前端 shared user-stream 据 `type` 分发。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UserEvent {
    /// 收件人有一条新通知。
    Notification {
        id: i64,
        kind: String,
        payload: serde_json::Value,
    },
    /// 收件人收到一条新私信。
    DmMessage {
        /// 消息 id。
        id: i64,
        /// 发送者用户 id。
        from_user_id: i64,
        /// 消息正文。
        content: String,
        /// 创建时间（RFC3339 字符串，避免 realtime 层引入 chrono 依赖）。
        created_at: String,
    },
}

struct Room {
    tx: broadcast::Sender<String>,
    /// user_id → 连接数（同一用户可能多标签页）。
    presence: HashMap<i64, usize>,
}

struct Inner {
    /// 项目房间：project id → 房间（含在线状态）。
    rooms: RwLock<HashMap<RoomId, Room>>,
    /// 用户通知房间：user id → 广播通道（无需在线状态，仅服务器→客户端投递）。
    user_rooms: RwLock<HashMap<i64, broadcast::Sender<String>>>,
    publish: ConnectionManager,
}

/// 实时协作 hub（可廉价 clone）。
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Inner>,
}

/// 项目房间 Redis 频道。
const CHANNEL: &str = "prts:rt";
/// 用户通知房间 Redis 频道（与 [`CHANNEL`] 并行，由同一订阅循环按频道名分流）。
const USER_CHANNEL: &str = "prts:rt:user";

impl Hub {
    /// 创建 hub 并启动 Redis 订阅中继任务。
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let publish = ConnectionManager::new(client.clone()).await?;
        let inner = Arc::new(Inner {
            rooms: RwLock::new(HashMap::new()),
            user_rooms: RwLock::new(HashMap::new()),
            publish,
        });
        tokio::spawn(subscribe_loop(client, Arc::clone(&inner)));
        Ok(Self { inner })
    }

    /// 加入房间：返回事件接收端与当前在线用户快照；用户由离线转上线时广播 Join。
    pub async fn join(
        &self,
        room: RoomId,
        user_id: i64,
    ) -> (broadcast::Receiver<String>, Vec<i64>) {
        let (rx, online, newly) = {
            let mut rooms = self.inner.rooms.write().await;
            let r = rooms.entry(room).or_insert_with(|| Room {
                tx: broadcast::channel(256).0,
                presence: HashMap::new(),
            });
            let rx = r.tx.subscribe();
            let c = r.presence.entry(user_id).or_insert(0);
            *c += 1;
            let newly = *c == 1;
            let online: Vec<i64> = r.presence.keys().copied().collect();
            (rx, online, newly)
        };
        if newly {
            self.publish(
                room,
                &RoomEvent::Join {
                    user_id,
                    online: online.clone(),
                },
            )
            .await;
        }
        (rx, online)
    }

    /// 离开房间：用户全部连接断开后广播 Leave。
    pub async fn leave(&self, room: RoomId, user_id: i64) {
        let gone = {
            let mut rooms = self.inner.rooms.write().await;
            match rooms.get_mut(&room) {
                Some(r) => {
                    let gone = match r.presence.get_mut(&user_id) {
                        Some(c) => {
                            *c -= 1;
                            if *c == 0 {
                                r.presence.remove(&user_id);
                                true
                            } else {
                                false
                            }
                        }
                        None => false,
                    };
                    if r.presence.is_empty() {
                        rooms.remove(&room);
                    }
                    gone
                }
                None => false,
            }
        };
        if gone {
            self.publish(room, &RoomEvent::Leave { user_id }).await;
        }
    }

    /// 发布事件到房间（经 Redis 广播；所有实例含自身经订阅中继投递给本地连接）。
    pub async fn publish(&self, room: RoomId, event: &RoomEvent) {
        let payload = match serde_json::to_string(event) {
            Ok(p) => p,
            Err(_) => return,
        };
        let wire = format!("{room}\u{1}{payload}");
        let mut conn = self.inner.publish.clone();
        let _: Result<(), _> = redis::cmd("PUBLISH")
            .arg(CHANNEL)
            .arg(wire)
            .query_async(&mut conn)
            .await;
    }

    /// 用户连接其通知房间，返回接收端（订阅本用户的通知）。
    ///
    /// 用户房间无在线状态，只需一个广播通道：多个标签页各自 `subscribe`，
    /// 房间在首个连接到达时惰性创建、并常驻（无连接计数，进程内长期保留一个空发送端，开销极小）。
    pub async fn join_user(&self, user_id: i64) -> broadcast::Receiver<String> {
        let mut rooms = self.inner.user_rooms.write().await;
        let tx = rooms
            .entry(user_id)
            .or_insert_with(|| broadcast::channel(64).0);
        tx.subscribe()
    }

    /// 向某用户推送事件（跨实例经 Redis [`USER_CHANNEL`]，与 [`publish`](Self::publish) 同机制）。
    pub async fn publish_user(&self, user_id: i64, event: &UserEvent) {
        let payload = match serde_json::to_string(event) {
            Ok(p) => p,
            Err(_) => return,
        };
        let wire = format!("{user_id}\u{1}{payload}");
        let mut conn = self.inner.publish.clone();
        let _: Result<(), _> = redis::cmd("PUBLISH")
            .arg(USER_CHANNEL)
            .arg(wire)
            .query_async(&mut conn)
            .await;
    }
}

/// Redis 订阅中继：把频道消息投递给本地对应房间的广播通道。断线自动重连。
async fn subscribe_loop(client: redis::Client, inner: Arc<Inner>) {
    loop {
        if let Err(e) = run_subscribe(&client, &inner).await {
            tracing::warn!("realtime redis subscribe error: {e}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// 同时订阅项目频道与用户频道，按**收到消息的频道名**分流：
/// - [`CHANNEL`]（`prts:rt`）→ 解析 `{room_id}\x01{payload}` → 投递到项目房间（行为不变）；
/// - [`USER_CHANNEL`]（`prts:rt:user`）→ 解析 `{user_id}\x01{payload}` → 投递到用户通知房间。
async fn run_subscribe(
    client: &redis::Client,
    inner: &Arc<Inner>,
) -> Result<(), redis::RedisError> {
    let mut pubsub = client.get_async_pubsub().await?;
    pubsub.subscribe(CHANNEL).await?;
    pubsub.subscribe(USER_CHANNEL).await?;
    let mut stream = pubsub.on_message();
    while let Some(msg) = stream.next().await {
        let wire: String = msg.get_payload()?;
        match msg.get_channel_name() {
            USER_CHANNEL => deliver_user(&inner.user_rooms, &wire).await,
            // 默认按项目频道处理（含 CHANNEL）：保持原有 project 房间投递逻辑不变。
            _ => deliver_room(&inner.rooms, &wire).await,
        }
    }
    Ok(())
}

/// 项目频道投递：解析 `{room_id}\x01{payload}`，广播到对应项目房间（与原实现一致）。
async fn deliver_room(rooms: &RwLock<HashMap<RoomId, Room>>, wire: &str) {
    if let Some((room_str, payload)) = wire.split_once('\u{1}') {
        if let Ok(room) = room_str.parse::<RoomId>() {
            let rooms = rooms.read().await;
            if let Some(r) = rooms.get(&room) {
                let _ = r.tx.send(payload.to_string());
            }
        }
    }
}

/// 用户频道投递：解析 `{user_id}\x01{payload}`，广播到对应用户通知房间。
async fn deliver_user(user_rooms: &RwLock<HashMap<i64, broadcast::Sender<String>>>, wire: &str) {
    if let Some((user_str, payload)) = wire.split_once('\u{1}') {
        if let Ok(user_id) = user_str.parse::<i64>() {
            let rooms = user_rooms.read().await;
            if let Some(tx) = rooms.get(&user_id) {
                let _ = tx.send(payload.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助：在给定 user_rooms 中为某用户建房间并返回接收端（等价于 `join_user` 的本地部分）。
    async fn join(
        user_rooms: &RwLock<HashMap<i64, broadcast::Sender<String>>>,
        user_id: i64,
    ) -> broadcast::Receiver<String> {
        let mut rooms = user_rooms.write().await;
        rooms
            .entry(user_id)
            .or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }

    /// 用户频道投递的**路由隔离**：发给用户 A 的 wire（`{A}\x01{payload}`）经 `deliver_user`
    /// 只送到 A 的房间——A 的接收端收到、B 的收不到。
    ///
    /// 该测试直接驱动本地广播层（`join_user` + `deliver_user`），不依赖真实 Redis：
    /// `publish_user` 与 `deliver_user` 使用同一 wire 格式，这里覆盖 Redis 往返落地后的本地分发。
    #[tokio::test]
    async fn publish_user_routes_only_to_target() {
        let user_rooms: RwLock<HashMap<i64, broadcast::Sender<String>>> =
            RwLock::new(HashMap::new());

        // A(=1)、B(=2) 各自加入自己的用户通知房间。
        let mut rx_a = join(&user_rooms, 1).await;
        let mut rx_b = join(&user_rooms, 2).await;

        // 构造发给 A 的事件，按 publish_user 的 wire 格式拼装并本地分发。
        let event = UserEvent::Notification {
            id: 42,
            kind: "poke".to_string(),
            payload: serde_json::json!({ "text": "hi A" }),
        };
        let payload = serde_json::to_string(&event).unwrap();
        let wire = format!("1\u{1}{payload}");
        deliver_user(&user_rooms, &wire).await;

        // A 收到该事件且可解回 UserEvent。
        let got = rx_a.try_recv().expect("A 应收到发给自己的通知");
        let decoded: UserEvent = serde_json::from_str(&got).unwrap();
        // `UserEvent` 现有多个变体，解构需用 let...else（构造的即 Notification，else 不会命中）。
        let UserEvent::Notification { id, kind, .. } = decoded else {
            panic!("应解码为 Notification 变体");
        };
        assert_eq!(id, 42);
        assert_eq!(kind, "poke");

        // B 什么都没收到（路由隔离）。
        assert!(
            rx_b.try_recv().is_err(),
            "B 不应收到发给 A 的通知（用户房间按 user_id 隔离）"
        );
    }

    /// 频道分流不串扰：项目频道 wire（`{room_id}\x01…`）经 `deliver_room` 投递到项目房间，
    /// 不应误入某个碰巧同号的用户房间；反之亦然。
    #[tokio::test]
    async fn project_and_user_channels_do_not_crosstalk() {
        let rooms: RwLock<HashMap<RoomId, Room>> = RwLock::new(HashMap::new());
        let user_rooms: RwLock<HashMap<i64, broadcast::Sender<String>>> =
            RwLock::new(HashMap::new());

        // 项目房间 7 与用户房间 7（同号，验证空间隔离）。
        let mut rx_room = {
            let mut w = rooms.write().await;
            let r = w.entry(7).or_insert_with(|| Room {
                tx: broadcast::channel(16).0,
                presence: HashMap::new(),
            });
            r.tx.subscribe()
        };
        let mut rx_user = join(&user_rooms, 7).await;

        // 投递到项目房间 7。
        deliver_room(&rooms, "7\u{1}{\"type\":\"leave\",\"user_id\":9}").await;
        assert_eq!(
            rx_room.try_recv().unwrap(),
            "{\"type\":\"leave\",\"user_id\":9}",
            "项目房间 7 应收到项目频道消息"
        );
        assert!(
            rx_user.try_recv().is_err(),
            "同号用户房间 7 不应收到项目频道消息"
        );

        // 投递到用户房间 7。
        deliver_user(&user_rooms, "7\u{1}{\"type\":\"Notification\"}").await;
        assert_eq!(
            rx_user.try_recv().unwrap(),
            "{\"type\":\"Notification\"}",
            "用户房间 7 应收到用户频道消息"
        );
        assert!(
            rx_room.try_recv().is_err(),
            "同号项目房间 7 不应收到用户频道消息"
        );
    }
}
