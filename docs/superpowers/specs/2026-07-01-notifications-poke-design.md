# Spec C · 即时通知 + 编辑器「戳一下」 — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | Spec C（P4 brainstorm 拆出的通知子系统 · 第一部分） |
| 基线 | `master` @ `748a80c` |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | [`CLAUDE.md`](../../../CLAUDE.md)、蓝图 §20（通知体系 v1 未决，本 spec 落地其第一步） |

> 已与作者确认：**实时 per-user WS 推送**；**通用 `notifications` 表**（`poke` 为第一种 type）；本期只做「通知底座 + 编辑器戳一下」，**持久私信页拆 Spec D** 复用同一底座。

---

## 1. 范围
- **通知底座**：`notifications` 表 + per-user 实时推送 + 右上角铃铛（未读数 + 列表 + 标记已读）。
- **编辑器「戳一下」**：翻译编辑器中点击**在场头像**（Spec B 已有）→ 写一段提示 → 对方**即时**收到通知。典型用例：协调占用某文件的译者换个文件。

**不在本 spec**：持久私信页（Spec D，复用底座）；系统事件通知 / @提及（未来新增 `type`）。

## 2. 数据模型 · 迁移 `0005_notifications.sql`
```
notifications(
  id BIGINT 主键, user_id BIGINT 收件人 → users(id) ON DELETE CASCADE,
  type TEXT, payload JSONB, read_at TIMESTAMPTZ NULL, created_at TIMESTAMPTZ)
索引 (user_id, id DESC)；未读 = read_at IS NULL。
```
`poke` 的 `payload`：`{ from_user_id, from_username, project_id, text }`。

## 3. 后端
- **`prts-db/notifications.rs`**：`create` / `list`(键集分页) / `unread_count` / `mark_read`(按 id 数组或全部)。
- **`prts-realtime` Hub 扩展 per-user 房间**：现 `RoomId=project_id`；新增按 `user_id` 的房间空间 + **独立 Redis 频道 `prts:rt:user`**（避免与 project 房间 id 相撞）。加 `join_user(uid) -> 接收器` 与 `publish_user(uid, &UserEvent)`；`UserEvent::Notification { id, type, payload, created_at }`。跨实例经该频道广播。
- **`prts-api`**：
  - WS `GET /ws/user?token=` → 认证（token→user_id）→ `join_user` → 推通知事件。
  - `POST /projects/{id}/poke`（`{to_user_id, text}`）：校验**发送者与收件人均为该项目成员**（只在同项目内戳）→ 建 `notification(type=poke, payload=...)` + `publish_user`。`text` 限长（如 ≤500）。
  - `GET /notifications`（键集）、`GET /notifications/unread_count`、`POST /notifications/read`（`{ids?}` 或全部）。全部进 utoipa/Swagger。

## 4. 前端
- **`useNotifications()`**：App 根部登录后连 `/ws/user`，维护未读数 + 最近通知；收到即 `$q.notify` toast + 铃铛红点；断线重连；登出断开。
- **`<NotificationBell>`**：`App.vue` 顶栏（主题切换按钮前）；铃铛 + `q-badge` 未读数；点开 `q-menu` 列表 + 「全部已读」；空态友好。
- **编辑器**：Spec B 的在场头像加点击 → 小输入框（`q-menu`/`q-dialog`）写提示 → `POST /projects/{id}/poke`；成功 toast。
- i18n 中英、亮/深主题、移动端适配。

## 5. 实时流 / 降级
发起方 POST poke → 建通知(DB) + `publish_user` → 在线接收方 WS 即时 toast + badge；**离线**则下次连接/加载经 `unread_count`/`list` 看到（通知**持久于 DB 至已读**）。WS 不可用 → 铃铛在加载时拉取 unread 兜底。

## 6. 测试
- **db-test**：notifications CRUD + unread_count + mark_read；poke 建通知 + 成员鉴权（非成员 → 403）。
- **`prts-realtime`**：per-user `join_user`/`publish_user` 单元/集成（发给 A 的事件不达 B）。
- **前端**：`pnpm lint`/`build`（+ 可选 composable 纯逻辑单测）。

## 7. 涉及文件
- **新/改**：迁移 `0005`；`prts-db/notifications.rs`(+`lib.rs`)；`prts-realtime/lib.rs`（per-user 房间 + `UserEvent`）；`prts-api`（`ws.rs` 增 `/ws/user`、`routes/notifications.rs`、poke 端点（并入 `routes/projects.rs` 或新 `routes/poke.rs`）、`mod.rs` 注册、`state.rs` 若需）；前端 `composables/useNotifications.ts`、`components/NotificationBell.vue`、`App.vue`、`views/EditorView.vue`（头像点击）、`api/*`、`i18n/*`；Swagger、`docs/architecture.md`。
- **最有分量/风险**：`prts-realtime` 的 per-user 房间与 WS 通道。

## 8. 红线核对
- ✅ poke 需项目成员；通知**仅推给收件人本人**（WS 用户房间按 `token→user_id` 鉴权，不能订阅他人）。
- ✅ 键集分页（通知列表），不深翻。
- ✅ payload 无敏感数据；无密钥/搜索改动。
- ⏭ poke 是否记 `audit_log` = P5 审计范畴，本期略（可后补）。
