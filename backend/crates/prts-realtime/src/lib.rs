//! `prts-realtime` —— WebSocket 实时协作 hub。
//!
//! 设计（见 plan §13、docs/architecture.md §3.4）：以**项目**为房间，广播在线状态、
//! 「他人正在编辑」与词条变更；通过 Redis pub/sub 在多实例间同步。
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

struct Room {
    tx: broadcast::Sender<String>,
    /// user_id → 连接数（同一用户可能多标签页）。
    presence: HashMap<i64, usize>,
}

struct Inner {
    rooms: RwLock<HashMap<RoomId, Room>>,
    publish: ConnectionManager,
}

/// 实时协作 hub（可廉价 clone）。
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Inner>,
}

const CHANNEL: &str = "prts:rt";

impl Hub {
    /// 创建 hub 并启动 Redis 订阅中继任务。
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let publish = ConnectionManager::new(client.clone()).await?;
        let inner = Arc::new(Inner {
            rooms: RwLock::new(HashMap::new()),
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

async fn run_subscribe(
    client: &redis::Client,
    inner: &Arc<Inner>,
) -> Result<(), redis::RedisError> {
    let mut pubsub = client.get_async_pubsub().await?;
    pubsub.subscribe(CHANNEL).await?;
    let mut stream = pubsub.on_message();
    while let Some(msg) = stream.next().await {
        let wire: String = msg.get_payload()?;
        if let Some((room_str, payload)) = wire.split_once('\u{1}') {
            if let Ok(room) = room_str.parse::<RoomId>() {
                let rooms = inner.rooms.read().await;
                if let Some(r) = rooms.get(&room) {
                    let _ = r.tx.send(payload.to_string());
                }
            }
        }
    }
    Ok(())
}
