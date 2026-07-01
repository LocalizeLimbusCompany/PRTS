# Spec D — 持久私信（类微信） Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 独立 `/messages` 私信页（会话列表 + 会话线程，持久历史），复用 Spec C 的 per-user 实时底座；私信仅在共享 ≥1 项目的用户间；并把 poke 收紧到 140 字。

**Architecture:** 新增 `messages` 表 + `prts-db/messages` 仓储；`prts-realtime` 加 `UserEvent::DmMessage`（复用 `/ws/user` + `publish_user`）；`prts-api` 加 `messages` REST + `GET /users/{id}`；前端把 Spec C 的 `/ws/user` 连接抽成**共享 user-stream**（分发 Notification/DmMessage），加 `useMessages` + 会话列表/线程两页 + 导航私信入口（独立红点）。

**Tech Stack:** Rust(axum/sqlx/tokio/redis) · PostgreSQL · Vue3+Quasar+Pinia+vue-router+vue-i18n。

**权威 spec:** [`docs/superpowers/specs/2026-07-01-private-messages-design.md`](../specs/2026-07-01-private-messages-design.md)。

---

## 工作流约定
- `master` 切分支 `feat/spec-d-messages`；每任务本地 commit。**build 确认交给 CI**：不跑本地 `cargo build`/`clippy`/`test`/`pnpm build`；后端 commit 前只 `cargo fmt --all`（CI 首关）。push → PR → CI（fmt/clippy `-D warnings`/test/db-tests + 前端 lint/test/build）。CI 绿后合并 master。
- 分支首推会带上 master 本地那个 `chore: gitignore .superpowers/`（f5a4249）。

## 文件结构
| 路径 | 动作 | 职责 |
| --- | --- | --- |
| `backend/migrations/0006_messages.sql` | 建 | messages 表 + 索引 |
| `backend/crates/prts-db/src/messages.rs` | 建 | create/list_conversation/list_threads/mark_read/unread_count |
| `backend/crates/prts-db/src/{lib,models}.rs` | 改 | 注册 mod + `Message`/`ConversationThread` |
| `backend/crates/prts-realtime/src/lib.rs` | 改 | `UserEvent::DmMessage` |
| `backend/crates/prts-api/src/routes/users.rs` | 改 | `GET /users/{id}` |
| `backend/crates/prts-api/src/routes/messages.rs` | 建 | 会话列表/会话/发送/已读/未读数 |
| `backend/crates/prts-api/src/routes/notifications.rs` | 改 | poke `text` 500→140 |
| `backend/crates/prts-api/src/routes/mod.rs` | 改 | 注册路由 + OpenAPI |
| `frontend/src/composables/useUserStream.ts` | 建 | 单一 `/ws/user` 连接，分发事件 |
| `frontend/src/composables/useNotifications.ts` | 改 | 消费 shared stream（不再自开 WS） |
| `frontend/src/composables/useMessages.ts` | 建 | threads + 未读 + 收 DmMessage |
| `frontend/src/views/MessagesView.vue`,`MessageThreadView.vue` | 建 | 会话列表 / 会话线程 |
| `frontend/src/router/index.ts` | 改 | `/messages`、`/messages/:userId` |
| `frontend/src/App.vue` | 改 | 私信入口 + ✉️ 红点 |
| `frontend/src/views/EditorView.vue` | 改 | 头像菜单「发私信」+ poke maxlength 140 |
| `frontend/src/api/{types,index}.ts` | 改 | Message/Thread/User DTO + api |
| `frontend/src/i18n/locales/*` | 改 | 中英文案 |

---

## 阶段 A · 后端

### Task 1: 迁移 0006
建 `backend/migrations/0006_messages.sql`：
```sql
-- 0006_messages.sql — 私信（持久会话记录）。
CREATE TABLE messages (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    sender_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    recipient_id BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    content      TEXT NOT NULL,
    read_at      TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX messages_pair_idx     ON messages (sender_id, recipient_id, id DESC);
CREATE INDEX messages_pair_rev_idx ON messages (recipient_id, sender_id, id DESC);
CREATE INDEX messages_unread_idx   ON messages (recipient_id) WHERE read_at IS NULL;
```
Commit `feat(db): migration 0006 messages table`。

### Task 2: prts-db messages 仓储
- `models.rs` 加：
```rust
#[derive(Debug, Clone, FromRow)]
pub struct Message {
    pub id: i64, pub sender_id: i64, pub recipient_id: i64,
    pub content: String, pub read_at: Option<DateTime<Utc>>, pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, FromRow)]
pub struct ConversationThread {
    pub other_user_id: i64, pub username: String, pub avatar_url: Option<String>,
    pub last_content: String, pub last_sender_id: i64, pub last_created_at: DateTime<Utc>,
    pub unread: i64,
}
```
- `messages.rs`（参照 `notifications.rs` 风格）：
```rust
use sqlx::PgPool;
use crate::models::{Message, ConversationThread};

pub async fn create(pool:&PgPool, sender_id:i64, recipient_id:i64, content:&str) -> Result<Message, sqlx::Error> {
    sqlx::query_as::<_,Message>(
        "INSERT INTO messages (sender_id,recipient_id,content) VALUES ($1,$2,$3)
         RETURNING id,sender_id,recipient_id,content,read_at,created_at")
        .bind(sender_id).bind(recipient_id).bind(content).fetch_one(pool).await
}

/// 一对用户间的会话（双向），键集：before_id 之前（更旧）。
pub async fn list_conversation(pool:&PgPool, me:i64, other:i64, before_id:Option<i64>, limit:i64) -> Result<Vec<Message>, sqlx::Error> {
    let limit = limit.clamp(1,100);
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id,sender_id,recipient_id,content,read_at,created_at FROM messages
         WHERE ((sender_id=");
    qb.push_bind(me).push(" AND recipient_id=").push_bind(other)
      .push(") OR (sender_id=").push_bind(other).push(" AND recipient_id=").push_bind(me).push("))");
    if let Some(b)=before_id { qb.push(" AND id<").push_bind(b); }
    qb.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    qb.build_query_as().fetch_all(pool).await
}

/// 会话列表：每个对话方的最后一条 + 我未读数（含对方 username/avatar）。
pub async fn list_threads(pool:&PgPool, me:i64) -> Result<Vec<ConversationThread>, sqlx::Error> {
    sqlx::query_as::<_,ConversationThread>(
        "WITH convo AS (
           SELECT CASE WHEN sender_id=$1 THEN recipient_id ELSE sender_id END AS other,
                  id, content, sender_id, created_at
           FROM messages WHERE sender_id=$1 OR recipient_id=$1),
         last AS (SELECT DISTINCT ON (other) other,id,content,sender_id,created_at
                  FROM convo ORDER BY other, id DESC)
         SELECT l.other AS other_user_id, u.username, u.avatar_url,
                l.content AS last_content, l.sender_id AS last_sender_id, l.created_at AS last_created_at,
                (SELECT COUNT(*) FROM messages m WHERE m.recipient_id=$1 AND m.sender_id=l.other AND m.read_at IS NULL) AS unread
         FROM last l JOIN users u ON u.id=l.other
         ORDER BY l.id DESC")
        .bind(me).fetch_all(pool).await
}

pub async fn mark_read(pool:&PgPool, me:i64, other:i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE messages SET read_at=now() WHERE recipient_id=$1 AND sender_id=$2 AND read_at IS NULL")
        .bind(me).bind(other).execute(pool).await?; Ok(())
}
pub async fn unread_count(pool:&PgPool, me:i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE recipient_id=$1 AND read_at IS NULL")
        .bind(me).fetch_one(pool).await
}
```
- `lib.rs` 加 `pub mod messages;`。
- db-test（追加到 `tests/db_integration.rs`）：建 A、B → A→B 两条、B→A 一条 → `list_conversation(A,B)` 返回 3 条按 id 降序 → `unread_count(B)`==2 → `mark_read(B,A)` → ==0 → `list_threads(A)` 含 B（username/unread）。
- Commit `feat(db): messages repository (conversations + threads)`。

### Task 3: realtime DmMessage 事件
`prts-realtime/src/lib.rs` 的 `UserEvent` 加变体（复用 Spec C 的 `publish_user`/`join_user`/`prts:rt:user`）：
```rust
    /// 收到一条新私信。
    DmMessage { id: i64, from_user_id: i64, content: String, created_at: String },
```
Commit `feat(realtime): UserEvent::DmMessage variant`。

### Task 4: GET /users/{id} + poke 收紧到 140
- `routes/users.rs` 加（`MaybeUser` 或公开；返回 `UserDto`，不含 email）：
```rust
#[utoipa::path(get, path="/users/{id}", tag="user", responses((status=200, body=UserDto),(status=404)))]
pub async fn get_user(State(state):State<AppState>, Path(id):Path<i64>) -> Result<Json<UserDto>, ApiError> {
    let u = prts_db::users::find_by_id(&state.db, id).await.map_err(db_err)?.ok_or(Error::NotFound)?;
    Ok(Json((&u).into()))
}
```
在 `mod.rs` 的 `routes!(users::me, users::update_me)` 加 `users::get_user`。
- `routes/notifications.rs` 的 poke 校验里，把 `text` 上限 `500` 改成 `140`（找到那处 `.chars().count() <= 500` 改 `140`；错误消息同步）。
- Commit `feat(api): GET /users/{id}; tighten poke to 140 chars`。

### Task 5: messages REST 端点
建 `routes/messages.rs`（`CurrentUser` 鉴权，参照 `notifications.rs`）：
- `MessageDto { id, sender_id, recipient_id, content, read_at, created_at }`、`ThreadDto`（映射 `ConversationThread`）、`SendReq { to_user_id, content }`，均 `utoipa::ToSchema`。
- **共享项目校验 helper**：
```rust
async fn share_project(db:&PgPool, a:i64, b:i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_,bool>(
        "SELECT EXISTS(SELECT 1 FROM memberships m1 JOIN memberships m2 ON m1.project_id=m2.project_id
         WHERE m1.user_id=$1 AND m2.user_id=$2)").bind(a).bind(b).fetch_one(db).await
}
```
- 端点：
  - `GET /messages` → `list_threads` → `Vec<ThreadDto>`。
  - `GET /messages/{user_id}?before&limit` → 校验 `share_project`（否则 403）→ `list_conversation` → `Vec<MessageDto>`。
  - `POST /messages {to_user_id, content}`：`content` trim 非空且 ≤2000；`share_project(me,to)` 否则 403；`messages::create` → `publish_user(to, DmMessage{...})`。返回 200/`{id}`。
  - `POST /messages/{user_id}/read` → `mark_read`。
  - `GET /messages/unread_count` → `{count}`。
- `mod.rs` 注册 + OpenAPI（tag `message`）。
- db-test：A、B 同项目、C 不同项目 → A→B 发送成功 + B 收到；A→C 发送 403；`GET /messages/{B}` 由 A 拿到会话。
- Commit `feat(api): direct message endpoints (share-project gated)`。

---

## 阶段 B · 前端

### Task 6: 共享 `/ws/user` user-stream + api
- 建 `composables/useUserStream.ts`：把 Spec C `useNotifications` 里开 `/ws/user` 的连接/重连逻辑搬来，模块级单例；对外暴露 `onEvent(handler)`（注册回调，收到 wire JSON 后按 `msg.type` 分发）+ `connect()`/`disconnect()`。**不破坏**通知行为。
- 改 `useNotifications.ts`：不再自开 WS，改为 `useUserStream().onEvent(msg => { if(msg.type==='Notification'){...原逻辑} })`；其余（unread/items/toast/refresh/markAllRead）不变。App 根部改为 `useUserStream().connect()`（登录后）。
- `api/types.ts`：`MessageDto`、`ThreadDto { other_user_id, username, avatar_url, last_content, last_sender_id, last_created_at, unread }`、`UserDto`（若无）。
- `api/index.ts`：`messagesApi = { threads(), conversation(userId,before?,limit?), send(to_user_id,content), markRead(userId), unreadCount() }`、`usersApi.getUser(id)`。
- Commit `feat(frontend): shared /ws/user stream + messages/users api`。

### Task 7: useMessages + 导航私信入口
- 建 `composables/useMessages.ts`（模块级单例）：`threads`、`unread`；`useUserStream().onEvent(msg => { if(msg.type==='DmMessage'){ unread++; 更新对应 thread / 若在该会话页则追加; toast可选 } })`；`refresh()`（拉 threads + unreadCount）；App 根部随登录 `refresh()`。
- `App.vue` 用户下拉加「私信」`q-item` `:to="{name:'messages'}"`；旁边（或下拉项上）显示 ✉️ 未读红点（`useMessages().unread`）。
- Commit `feat(frontend): useMessages + 私信 nav entry with unread dot`。

### Task 8: 路由 + 会话列表页
- `router/index.ts` 加 `{ path:'/messages', name:'messages', component:()=>import('@/views/MessagesView.vue'), meta:{requiresAuth:true} }` 和 `{ path:'/messages/:userId(\\d+)', name:'message-thread', component:()=>import('@/views/MessageThreadView.vue'), props:r=>({userId:Number(r.params.userId)}), meta:{requiresAuth:true} }`（对齐现有 guard 写法）。
- 建 `MessagesView.vue`：`useMessages().threads` 列表——头像 + 用户名 + 最后一条 + 相对时间 + 未读徽标；点进 `/messages/:userId`；空态。挂载时 `refresh()`。
- Commit `feat(frontend): messages route + conversation list view`。

### Task 9: 会话线程页
建 `MessageThreadView.vue`（props `userId`）：
- 挂载：`usersApi.getUser(userId)`（头名）+ `messagesApi.conversation(userId)`（消息，倒序→正序渲染）+ `messagesApi.markRead(userId)` + `useMessages().refresh()`。
- 消息气泡（`sender_id===me` 右，否则左）+ 时间；底部发送框 `maxlength=2000` + 发送键 → `messagesApi.send(userId, text)`（发完清空、本地追加）。
- 实时：`useMessages` 收到该 `from_user_id===userId` 的 `DmMessage` 时追加到本页（用共享 `threads`/一个事件总线，或组件内 `useUserStream().onEvent`）。
- Commit `feat(frontend): message thread view (send + realtime + mark read)`。

### Task 10: 编辑器/成员列表发起 + poke 限字
- `EditorView.vue`：Spec C 在场头像的菜单里，除「戳一下」再加「发私信」→ `router.push({name:'message-thread', params:{userId: member.user_id}})`。poke compose 的输入加 `maxlength=140` + 字数计数（超限禁发）。
- 项目成员列表（成员管理处）每个成员加「私信」按钮 → 同上跳转。
- Commit `feat(frontend): DM entry from editor avatar + members; poke maxlength 140`。

### Task 11: i18n
`zh-CN.json`+`en.json` 加：`messages.title/empty/send/placeholder/unread`、`dm.entry`（发私信）、poke 计数等。Commit `feat(frontend): i18n for private messages`。

---

## 阶段 C · 文档 / 收尾

### Task 12: 文档 + 收尾
- `docs/architecture.md` §3.5 补「私信」一段（复用 per-user 底座、共享项目限制、DmMessage）。
- 后端 `cargo fmt --all`；推分支 + PR → CI（含 db-tests）；绿后合并 master（触发 GHCR）。Commit docs + 收尾。

---

## Self-review（写计划时已做）
- **Spec 覆盖**：§2 迁移→T1；§3 仓储→T2、DmMessage→T3、/users/{id}+poke140→T4、messages REST+共享校验→T5；§4 前端→T6–T11（含单一 stream 重构 T6、私信入口 T7、两页 T8/T9、编辑器/成员入口+poke限字 T10）；§5 实时→T3/T5/T7/T9；§6 测试→T2/T5 db-test；§7 文件→全覆盖；§8 红线→T5(共享校验/鉴权)、T4(users 不含 email)、T2(键集)。
- **占位扫描**：无 TBD；「参照 xxx 风格 / 对齐现有 guard」是明确查找指令。
- **类型一致**：`Message`/`ConversationThread`(db)→`MessageDto`/`ThreadDto`(api)→前端同名；`UserEvent::DmMessage{id,from_user_id,content,created_at}` T3 定义、T5 构造、T7/T9 消费一致；`messages::{create,list_conversation,list_threads,mark_read,unread_count}` 跨 T2/T5 一致；`useUserStream().onEvent` T6 定义、T7/T9 用。
- **实现者需按现有代码解析的查找点**（非占位）：`notifications.rs` 风格、`users::find_by_id`/`UserDto::from`、`mod.rs` 注册、router guard、`useNotifications` 当前连接逻辑、poke 现有 `<=500` 处、成员管理 UI 位置——各任务已注明去哪对照。
