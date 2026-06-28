//! `prts-realtime` —— 实时协作（WebSocket + Redis pub/sub）。
//!
//! 设计（见 plan §13、docs/architecture.md §3.4）：编辑器以文件为房间，广播在线状态、
//! 「他人正在编辑」与词条变更；多实例间经 Redis pub/sub 同步。保存仍走乐观锁版本校验。
//!
//! P0 仅定义房间标识与广播事件骨架；WS 会话管理与 pub/sub 见 P3。

use serde::{Deserialize, Serialize};

/// 房间标识：按文件聚合在线协作者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoomId(pub i64);

/// 房间内广播的事件（草案）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoomEvent {
    /// 某用户进入房间。
    Join { user_id: i64 },
    /// 某用户离开房间。
    Leave { user_id: i64 },
    /// 某用户正在编辑某词条。
    Editing { user_id: i64, entry_id: i64 },
    /// 某词条已更新（携带新版本号，供客户端乐观锁对齐）。
    EntryUpdated { entry_id: i64, version: i64 },
}
