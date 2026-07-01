# Spec C — 即时通知 + 编辑器戳一下 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一个通用通知底座（`notifications` 表 + per-user 实时 WS 推送 + 右上角铃铛），并让翻译编辑器点在场头像可给对方发即时提示（`poke`，第一种通知类型）。

**Architecture:** `prts-realtime` Hub 新增按 `user_id` 的房间空间（独立 Redis 频道 `prts:rt:user`），与现有 project 房间并行；新 WS `/ws/user` 让前端在 App 根部常连接收通知；`POST /projects/{id}/poke` 建 `notification` 并 `publish_user` 实时推。前端 `useNotifications` + `<NotificationBell>` + 编辑器头像点击。

**Tech Stack:** Rust(axum/sqlx/tokio/redis) · PostgreSQL · Vue3+Quasar+Pinia · vue-i18n。

**权威 spec:** [`docs/superpowers/specs/2026-07-01-notifications-poke-design.md`](../specs/2026-07-01-notifications-poke-design.md)。

---

## 工作流约定
- 在 `master` 切分支 `feat/spec-c-notifications`；每任务本地 commit；开 PR 触发 CI；后端 fmt/clippy(-D warnings，CI 版 clippy 比本地新，commit 前先 `cargo fmt --all`)/test + db-tests(zhparser 镜像) 为门。DB 相关只在 CI 验证。
- 前端 `pnpm lint && pnpm build`（+ `pnpm test` vitest）。Bash 编译加 `dangerouslyDisableSandbox: true`。

## 文件结构
| 路径 | 动作 | 职责 |
| --- | --- | --- |
| `backend/migrations/0005_notifications.sql` | 建 | notifications 表 + 索引 |
| `backend/crates/prts-db/src/notifications.rs` | 建 | create/list/unread_count/mark_read |
| `backend/crates/prts-db/src/lib.rs` | 改 | `pub mod notifications;` |
| `backend/crates/prts-realtime/src/lib.rs` | 改 | per-user 房间 + `UserEvent` + `join_user`/`publish_user` + 第二 Redis 频道 |
| `backend/crates/prts-api/src/routes/ws.rs` | 改 | 新增 `/ws/user` |
| `backend/crates/prts-api/src/routes/notifications.rs` | 建 | GET 列表/未读数、POST 已读、POST poke |
| `backend/crates/prts-api/src/routes/mod.rs` | 改 | 注册路由 + OpenAPI |
| `backend/crates/prts-api/tests/db_integration.rs` | 改 | notifications + poke db-test |
| `frontend/src/api/types.ts`,`index.ts` | 改 | NotificationDto + api |
| `frontend/src/composables/useNotifications.ts` | 建 | 连 /ws/user、未读、toast |
| `frontend/src/components/NotificationBell.vue` | 建 | 铃铛 + 未读 + 列表 |
| `frontend/src/App.vue` | 改 | 挂 `<NotificationBell>` + 根部启动 useNotifications |
| `frontend/src/views/EditorView.vue` | 改 | 在场头像点击 → 发 poke |
| `frontend/src/i18n/locales/*` | 改 | 中英文案 |

---

## 阶段 A · 后端

### Task 1: 迁移 0005 — notifications 表
**Files:** 建 `backend/migrations/0005_notifications.sql`

- [ ] **Step 1: 写迁移**（参照 `0003`/`0004` 风格）：

```sql
-- 0005_notifications.sql — 通用通知（poke 为第一种 type）。
CREATE TABLE notifications (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE, -- 收件人
    type       TEXT NOT NULL,
    payload    JSONB NOT NULL DEFAULT '{}',
    read_at    TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- 键集列表 + 未读过滤
CREATE INDEX notifications_user_idx ON notifications (user_id, id DESC);
CREATE INDEX notifications_unread_idx ON notifications (user_id) WHERE read_at IS NULL;
```

- [ ] **Step 2:** 由 CI db-tests 验证（无本地 DB）。
- [ ] **Step 3: Commit** `feat(db): migration 0005 notifications table`。

### Task 2: prts-db notifications 仓储
**Files:** 建 `backend/crates/prts-db/src/notifications.rs`；改 `lib.rs`；改 `tests/db_integration.rs`

- [ ] **Step 1: 写仓储**（参照 `settings.rs`/`search.rs` 的 sqlx 风格）：

```rust
//! 通知仓储：收件人维度的通知增查改（键集分页）。
use sqlx::PgPool;
use crate::models::Notification; // 见 Step 2

pub async fn create(
    pool: &PgPool, user_id: i64, kind: &str, payload: &serde_json::Value,
) -> Result<Notification, sqlx::Error> {
    sqlx::query_as::<_, Notification>(
        "INSERT INTO notifications (user_id, type, payload) VALUES ($1,$2,$3)
         RETURNING id, user_id, type, payload, read_at, created_at")
        .bind(user_id).bind(kind).bind(payload).fetch_one(pool).await
}

/// 键集分页：按 id 降序，after=游标（返回更旧的）。
pub async fn list(pool: &PgPool, user_id: i64, before_id: Option<i64>, limit: i64)
    -> Result<Vec<Notification>, sqlx::Error> {
    let limit = limit.clamp(1, 100);
    match before_id {
        Some(b) => sqlx::query_as::<_, Notification>(
            "SELECT id,user_id,type,payload,read_at,created_at FROM notifications
             WHERE user_id=$1 AND id<$2 ORDER BY id DESC LIMIT $3")
            .bind(user_id).bind(b).bind(limit).fetch_all(pool).await,
        None => sqlx::query_as::<_, Notification>(
            "SELECT id,user_id,type,payload,read_at,created_at FROM notifications
             WHERE user_id=$1 ORDER BY id DESC LIMIT $2")
            .bind(user_id).bind(limit).fetch_all(pool).await,
    }
}

pub async fn unread_count(pool: &PgPool, user_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id=$1 AND read_at IS NULL")
        .bind(user_id).fetch_one(pool).await
}

/// ids 为空 = 标记全部已读。
pub async fn mark_read(pool: &PgPool, user_id: i64, ids: &[i64]) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        sqlx::query("UPDATE notifications SET read_at=now() WHERE user_id=$1 AND read_at IS NULL")
            .bind(user_id).execute(pool).await?;
    } else {
        sqlx::query("UPDATE notifications SET read_at=now() WHERE user_id=$1 AND id=ANY($2) AND read_at IS NULL")
            .bind(user_id).bind(ids.to_vec()).execute(pool).await?;
    }
    Ok(())
}
```

- [ ] **Step 2:** 在 `prts-db/src/models.rs` 加：
```rust
#[derive(Debug, Clone, FromRow)]
pub struct Notification {
    pub id: i64,
    pub user_id: i64,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub payload: serde_json::Value,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```
（`type` 是 SQL 保留列名→用 `#[sqlx(rename="type")]` 映射到 `kind`。）在 `lib.rs` 加 `pub mod notifications;`。

- [ ] **Step 3: db-test**（追加到 `db_integration.rs`，沿用 `pool()` harness）：建用户→`create` 两条→`unread_count`==2→`mark_read` 一条→==1→`list` 返回按 id 降序。清理。
- [ ] **Step 4:** `cargo build -p prts-db` + `cargo test -p prts-api --features db-tests --no-run` 编译通过。**Commit** `feat(db): notifications repository`。

### Task 3: prts-realtime per-user 房间
**Files:** 改 `backend/crates/prts-realtime/src/lib.rs`

- [ ] **Step 1: 先读现有实现**（`RoomId`、`Room`、`Hub`、`join`、`publish`、Redis 订阅循环、频道常量 `prts:rt` 与线格式 `{room_id}\x01{payload}`）。**按同样机制**新增 per-user 房间：

```rust
// 用户通知事件（用户频道专用，区别于 project 的 RoomEvent）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UserEvent {
    Notification { id: i64, kind: String, payload: serde_json::Value },
}

// 第二 Redis 频道常量：
const USER_CHANNEL: &str = "prts:rt:user";
```

在 `Hub` 内加与 project 房间并行的用户房间：`user_rooms: Mutex<HashMap<i64, broadcast::Sender<String>>>`（用户房间无需 presence，只要广播通道）。加方法：

```rust
/// 用户连接其通知房间，返回接收器（订阅本用户的通知）。
pub fn join_user(&self, user_id: i64) -> broadcast::Receiver<String> {
    let mut rooms = self.inner.user_rooms.lock().unwrap();
    let tx = rooms.entry(user_id)
        .or_insert_with(|| broadcast::channel(64).0);
    tx.subscribe()
}

/// 向某用户推送事件（跨实例经 Redis USER_CHANNEL）。
pub async fn publish_user(&self, user_id: i64, event: &UserEvent) {
    let payload = match serde_json::to_string(event) { Ok(p) => p, Err(_) => return };
    let wire = format!("{user_id}\u{1}{payload}");
    let mut conn = self.inner.publish.clone();
    let _: Result<(), _> = redis::cmd("PUBLISH").arg(USER_CHANNEL).arg(wire)
        .query_async(&mut conn).await;
}
```

**扩展 Redis 订阅循环**：现在只 `SUBSCRIBE prts:rt`；改为同时订阅 `USER_CHANNEL`，并按收到消息的**频道名**分流：`prts:rt` → 现有 project 房间广播；`prts:rt:user` → 解析 `{user_id}\x01{payload}`，向对应 `user_rooms[user_id]` 本地广播。（`redis` 的 PubSub `on_message` 可读 `msg.get_channel_name()`。）保持与现有 project 分发一致的写法。

- [ ] **Step 2:** `cargo build -p prts-realtime`；加单测/集成：`publish_user(A)` 后 A 的接收器收到、B 的收不到（可纯本地 broadcast 层测，不依赖 Redis）。
- [ ] **Step 3: Commit** `feat(realtime): per-user notification rooms + publish_user`。

### Task 4: WS `/ws/user`
**Files:** 改 `backend/crates/prts-api/src/routes/ws.rs`、`routes/mod.rs`

- [ ] **Step 1:** 参照现有 `/ws/projects/{id}` 的 `handle_socket`（token→user_id 认证、`join` → 转发循环）写 `GET /ws/user?token=`：认证得 `user_id` → `state.realtime.join_user(user_id)` → 把收到的 wire payload 直接转发给客户端（用户房间只有服务器→客户端方向，无需处理客户端入站消息，或忽略之）。断开即结束。
- [ ] **Step 2:** 在 `mod.rs` 的 `.route("/ws/projects/{id}", ...)` 附近加 `.route("/ws/user", get(ws::user_ws_handler))`（同样不进 OpenAPI）。
- [ ] **Step 3:** `cargo build -p prts-api`。**Commit** `feat(api): /ws/user notification stream`。

### Task 5: 通知 REST 端点
**Files:** 建 `backend/crates/prts-api/src/routes/notifications.rs`；改 `mod.rs`；db-test

- [ ] **Step 1:** 写三个处理器（`CurrentUser` 鉴权，沿用 `admin_settings.rs` 的风格）：`GET /notifications?before&limit`（→ `NotificationDto[]`）、`GET /notifications/unread_count`（→ `{count}`）、`POST /notifications/read`（body `{ids?: number[]}`，空/缺省=全部）。`NotificationDto`（utoipa::ToSchema）从 `prts_db::models::Notification` 映射（`kind`→`type` 字段名对前端友好：序列化为 `type`）。进 OpenAPI。
- [ ] **Step 2:** `mod.rs` 注册 `.routes(routes!(notifications::list, notifications::unread_count, notifications::mark_read))`。
- [ ] **Step 3:** db-test：为用户建 2 条通知→`GET /notifications` 逻辑（可直接测仓储已在 Task 2；此处测 DTO/鉴权可选）。`cargo build` + `--no-run`。**Commit** `feat(api): notifications list/unread/read endpoints`。

### Task 6: poke 端点
**Files:** 改/建 `backend/crates/prts-api/src/routes/notifications.rs`（加 poke）；改 `mod.rs`；db-test

- [ ] **Step 1:** `POST /projects/{id}/poke`（`CurrentUser`）body `{to_user_id: i64, text: String}`：
  - 校验发送者对项目有访问（`paccess::load(&state, Some(&user), id).await?.require_view()?`）且**是成员**；校验 `to_user_id` **也是该项目成员**（`prts_db::memberships::find_role(&state.db, id, to_user_id)` 非空），否则 403/400。
  - `text` trim 非空且 ≤500 字，否则 400。
  - `payload = {from_user_id: user.id, from_username: user.username, project_id: id, text}`；`let n = prts_db::notifications::create(&state.db, to_user_id, "poke", &payload).await?;`
  - `state.realtime.publish_user(to_user_id, &UserEvent::Notification{ id:n.id, kind:n.kind.clone(), payload: n.payload.clone() }).await;`
  - 返回 200（空或 `{id}`）。进 OpenAPI。
- [ ] **Step 2:** `mod.rs` 注册路由。
- [ ] **Step 3:** db-test：项目+两成员→A poke B→B 有 1 条 `poke` 通知；非成员 C poke → 403。`cargo build` + `--no-run`。**Commit** `feat(api): POST /projects/{id}/poke → notification + realtime push`。

---

## 阶段 B · 前端

### Task 7: api 类型 + 客户端
**Files:** 改 `frontend/src/api/types.ts`、`index.ts`

- [ ] **Step 1:** types：
```ts
export interface NotificationDto { id: number; type: string; payload: Record<string, unknown>; read_at: string | null; created_at: string }
```
- [ ] **Step 2:** index：`notificationsApi = { list(before?,limit?), unreadCount(), markRead(ids?) }`；`pokeApi = { send(projectId, to_user_id, text) → POST /projects/${projectId}/poke }`。沿用现有 http 风格。
- [ ] **Step 3:** `pnpm lint`。**Commit** `feat(frontend): notifications + poke api`。

### Task 8: useNotifications 组合式
**Files:** 建 `frontend/src/composables/useNotifications.ts`

- [ ] **Step 1:** 参照 `useRealtime.ts` 的 WS 连接方式，连 `/ws/user?token=`（App 根部登录后启动）。维护 `unread`(ref number) + `items`(ref NotificationDto[])。入站 `UserEvent::Notification` → `unread++`、unshift 到 items、`$q.notify` toast（显示 `payload.from_username: payload.text`）。提供 `markAllRead()`（调 api + 清零）、`refresh()`（初次/重连拉 `unreadCount`+`list`）、`disconnect()`。断线重连（沿用 useRealtime 的重连策略）。
- [ ] **Step 2:** `pnpm lint && pnpm build`。**Commit** `feat(frontend): useNotifications composable (/ws/user + toast)`。

### Task 9: NotificationBell + App.vue
**Files:** 建 `frontend/src/components/NotificationBell.vue`；改 `frontend/src/App.vue`

- [ ] **Step 1:** `NotificationBell.vue`：`q-btn` 铃铛 + `q-badge`(unread>0 时显示数)；点开 `q-menu` 列出最近通知（from_username + text + 相对时间）+「全部已读」按钮；空态文案。props/inject 取 `useNotifications` 的状态（在 App.vue 提供，或组件内自持）。
- [ ] **Step 2:** `App.vue`：顶栏 `<q-space />` 后、主题切换 `q-btn` 前插入 `<NotificationBell />`；在 `<script setup>` 根部：登录态下启动 `useNotifications()`（`auth.user` 存在时连、登出时断）。
- [ ] **Step 3:** i18n（见 Task 11 合并）。`pnpm lint && pnpm build`。**Commit** `feat(frontend): notification bell in top bar`。

### Task 10: 编辑器头像 → 发 poke
**Files:** 改 `frontend/src/views/EditorView.vue`

- [ ] **Step 1:** Spec B 的在场头像（`editorOf(item.id)` 渲染处）加点击：打开一个小输入（`q-menu` 内 `q-input` + 发送键，或 `q-dialog`），确认 → `pokeApi.send(props.id, editor.user_id, text)` → 成功 `$q.notify('已发送提示')`；空文本禁用。不影响 Spec B 的头像展示与既有功能。
- [ ] **Step 2:** i18n。`pnpm lint && pnpm build`。**Commit** `feat(editor): click presence avatar to send a poke`。

### Task 11: i18n
**Files:** 改 `frontend/src/i18n/locales/zh-CN.json`、`en.json`

- [ ] **Step 1:** 加键（两 locale）：`notifications.title/empty/markAllRead/bell`、`poke.compose/send/placeholder/sent`、toast 文案。
- [ ] **Step 2:** `pnpm lint && pnpm build`。**Commit** `feat(frontend): i18n for notifications + poke`。

---

## 阶段 C · 文档 / 收尾

### Task 12: 文档 + 全量门
- [ ] **Step 1:** `docs/architecture.md` 加「通知子系统」小节（per-user 房间 + poke 流）；Swagger 复核 3+1 端点。
- [ ] **Step 2:** 后端 `cargo fmt --all && cargo clippy --workspace --all-targets`（本地）；前端 `pnpm lint && pnpm test && pnpm build`。
- [ ] **Step 3:** 推分支 + PR → CI（含 db-tests）；绿后合并 master（触发 GHCR）。**Commit** docs + 收尾。

---

## Self-review（写计划时已做）
- **Spec 覆盖**：§2 迁移→T1；§3 仓储→T2、Hub→T3、/ws/user→T4、REST→T5、poke→T6；§4 前端→T7–T11；§5 实时流→T3/T4/T6/T8；§6 测试→T2/T3/T5/T6 db-test + T8 前端；§7 文件→全覆盖；§8 红线→T6(成员鉴权)、T4(用户房间按 token 鉴权)、T2(键集)。
- **占位扫描**：无 TBD；「读现有实现后镜像」类为明确查找指令（Hub 内部、ws handler、http 风格），非占位。
- **类型一致**：`Notification`(db)→`NotificationDto`(api)→`NotificationDto`(前端)；`UserEvent::Notification{id,kind,payload}` 在 T3 定义、T6 构造、T8 消费一致；`notifications::{create,list,unread_count,mark_read}` 跨 T2/T5/T6 一致；`publish_user`/`join_user` T3 定义、T4/T6 使用。
- **需实现者按现有代码解析的查找点**（非占位）：realtime Hub 内部结构与 Redis 订阅循环、`ws.rs` 的 handle_socket、`memberships::find_role`、http 客户端与 auth store 用法——各任务已注明去哪个文件对照。
