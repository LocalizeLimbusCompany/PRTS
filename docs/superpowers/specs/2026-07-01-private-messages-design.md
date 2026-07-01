# Spec D · 持久私信（类微信） — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | Spec D（通知子系统第二部分：持久私信） |
| 基线 | `master` @ `f5a4249` |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | Spec C spec（通知底座 `2026-07-01-notifications-poke-design.md`）、CLAUDE.md |

> 已与作者确认（含可视化预览）：独立 `/messages` 页；flat `messages` 表；复用 `/ws/user` 加 `DmMessage`；发起限共享 ≥1 项目的人；私信未读独立红点（顶栏 ✉️，与铃铛分开）。并纳入两处 **Spec C poke 小改**：限 **140** 字 + **不做面板内气泡**（toast 已显示内容）。

---

## 1. 范围
- **持久私信**：独立 `/messages` 页（会话列表 + 会话线程），复用 Spec C 通知底座（per-user WS）。
- 发起限共享 ≥1 项目的人；私信未读独立于铃铛。
- **顺带的 poke 小改**：`text` 上限 500→**140**（后端 + 前端）；poke 不做面板内气泡（仅 toast + 铃铛，toast 已含内容）。

**不做**：群聊、附件、消息撤回、已读回执 UI（`read_at` 先存）、全局用户搜索。

## 2. 数据模型 · 迁移 `0006_messages.sql`
```
messages(id, sender_id → users, recipient_id → users, content TEXT, read_at TIMESTAMPTZ NULL, created_at)
索引：会话对 (least(sender,recipient), greatest, id DESC) 或 (sender_id, recipient_id, id DESC) + (recipient_id, id DESC)；收件人未读 (recipient_id) WHERE read_at IS NULL。
```
会话 = 一对用户间的消息，无单独 conversations 表。

## 3. 后端
- **`prts-db/messages.rs`**：`create`、`list_conversation`（键集）、`list_threads`（每个对话方最后一条 + 未读数）、`mark_read`（某会话）、`unread_count`（总）。
- **`prts-realtime`**：`UserEvent` 加 `DmMessage { id, from_user_id, content, created_at }`，复用 `publish_user`。
- **`prts-api`**：
  - `GET /users/{id}`（公开资料，不含 email）。
  - `GET /messages`（会话列表）、`GET /messages/{user_id}`（会话，键集）、`POST /messages {to_user_id, content}`、`POST /messages/{user_id}/read`、`GET /messages/unread_count`。
  - **poke 小改**：`POST /projects/{id}/poke` 的 `text` 上限 500→140。
  - **权限**：私信双方须**共享 ≥1 项目**（`memberships` 交集查询）否则 403；`content` ≤ 2000。
  - 共享校验：`EXISTS (memberships m1 JOIN memberships m2 ON m1.project_id=m2.project_id WHERE m1.user_id=$me AND m2.user_id=$other)`。

## 4. 前端
- **单一 `/ws/user` 共享连接**：把 Spec C 连接抽为 shared user-stream，按事件 `type` 分发——`Notification`→通知、`DmMessage`→私信；`useMessages` 不另开 WS。
- 路由：`/messages`（`MessagesView` 会话列表）、`/messages/:userId`（`MessageThreadView` 会话）。
- `App.vue` 用户下拉加「私信」入口 + ✉️ **红点**（私信未读，独立于铃铛）。
- `useMessages()`：`threads` + 总未读；收 `DmMessage` → 追加/刷新 + 红点。
- 编辑器在场头像菜单加「发私信」→ 跳 `/messages/:userId`；项目成员列表加「私信」按钮。
- **poke 前端小改**：compose `maxlength=140` + 字数计数（超限禁发）。
- `MessagesView`（对话方头像/名 + 最后一条 + 未读）、`MessageThreadView`（消息气泡 + 发送框 + 实时追加 + 进入即标记已读）。i18n、亮/深、移动端。

## 5. 实时 / 降级
发 `POST /messages` → 建 message + `publish_user(DmMessage)` → 对方在线即时追加 + 红点；离线下次加载经 `unread_count`/`threads` 看到（持久 DB）。

## 6. 测试
- **db-test**：messages CRUD + `list_threads` + `mark_read` + `unread_count` + 共享项目校验（非共享 → 403）。
- 前端 CI（build 交给 CI）。

## 7. 涉及文件
迁移 `0006`；`prts-db/messages.rs`(+`lib`)；`prts-realtime`（`DmMessage`）；`prts-api`（`users::get_user`、`routes/messages.rs`、poke 改 140、`mod.rs`、Swagger）；前端（shared user-stream 重构、`useMessages`、`MessagesView`、`MessageThreadView`、router、`App.vue`、编辑器/成员列表入口、poke `maxlength`、api、i18n）；`docs/architecture.md`。

## 8. 红线
- 私信双方须共享项目（防骚扰）；消息仅收发双方可见（端点按 user 鉴权）。
- `GET /users/{id}` 不下发 email；键集分页，不深翻。
