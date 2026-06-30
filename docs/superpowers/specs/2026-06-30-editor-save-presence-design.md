# Spec B · 编辑器保存逻辑 + 实时在场增强 — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | Spec B（P4 brainstorm 拆出的「编辑器 + 实时增强」） |
| 基线 | `master` @ `4e56da6`（P4 已并入） |
| 日期 | 2026-06-30 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | [`CLAUDE.md`](../../../CLAUDE.md)、P4 spec [`2026-06-29-p4-hybrid-search-design.md`](./2026-06-29-p4-hybrid-search-design.md) §15 |

> 已与作者确认：智能按钮**保留**状态下拉框共存；强制保存**仅越过在场拦截**、乐观锁仍生效；`questioned` 未改时推进到 `translated`。

---

## 1. 范围

三项编辑器/实时 UX 增强，**纯前端**（`frontend/`），**无后端改动**：

1. **上下文感知保存按钮** —— 改了译文/状态 → 保存；未改 → 按工作流推进（检查/审核/解决疑问）；终态/无权限 → 灰。
2. **列表编辑者头像** —— 左侧词条列表中，正被他人编辑的词条右侧显示编辑者头像（替换现琥珀色图标）。
3. **强制保存** —— 他人正编辑该词条时，owner/manager 见红色「强制保存」，其他人见灰色禁用。

**不在本 spec**：多人同编时的头像堆叠（v1 只显示最近一位）；Spec C（点头像私信 + 通知子系统）。

**为何无需后端改动**：`update_entry` 已按 `node_for_state(target)` 鉴权（`translated/questioned`→EDIT，`checked/reviewed`→REVIEW）并带 `version` 乐观锁 + 409 冲突路径；`editing` 实时事件已带 `user_id`；`MemberDto` 已带 `avatar_url`/`username`/`role`。本 spec 只在前端消费这些既有能力。

---

## 2. 上下文感知保存按钮

### 2.1 决策（纯函数 `computeSaveButton`）
抽出纯函数便于单测：

```ts
type SaveButton = {
  label: string          // "保存" | "检查" | "审核" | "已翻译" | "强制保存"
  color: string          // 'primary' | 'negative'(红) | undefined(灰)
  disabled: boolean
  mode: 'save' | 'advance' | 'force' | 'none'
  nextState?: string     // advance/force 时的目标状态；save 时用 draftState
}

function computeSaveButton(ctx: {
  isMember: boolean
  locked: boolean
  canEditLocked: boolean   // owner/manager
  isManager: boolean       // owner/manager（= canEditLocked）
  canReview: boolean       // owner/manager/reviewer
  canEdit: boolean         // 任何成员（含 translator）
  state: string            // 已保存状态
  dirty: boolean           // draft !== saved.translation || draftState !== saved.state
  hasContentToSave: boolean// dirty || advanceAvailable
  othersEditing: boolean   // 他人正编辑此词条（在场）
}): SaveButton
```

`dirty = draft !== saved.translation || draftState !== saved.state`（译文或下拉状态任一变化）。

### 2.2 优先级（首个匹配的行胜出）

| # | 条件 | 按钮 | 动作 |
| --- | --- | --- | --- |
| 1 | `!isMember || (locked && !canEditLocked)` | 灰「保存」禁用 | none |
| 2 | `othersEditing && isManager` | 🔴红「强制保存」（`hasContentToSave` 时启用） | force：`save{ translation: draft, state: (dirty?draftState:nextState), version }` |
| 3 | `othersEditing && !isManager` | 灰「保存」禁用（tooltip"他人正在编辑"） | none |
| 4 | `dirty` | 主色「保存」启用 | save：`{ translation: draft, state: draftState, version }` |
| 5 | `!dirty && advanceAvailable && 有 nextState 权限` | 「检查/审核/已翻译」启用 | advance：`{ translation 不变, state: nextState, version }` |
| 6 | 其余（终态 / 无权限 / 未翻译且未改） | 灰「保存」禁用 | none |

第 2 行「强制保存」语义 = **仅越过在场拦截**；`save()` 仍带原 `version`，若对方已存新版本则照常 409 冲突刷新——不会覆盖他人已保存的工作。

### 2.3 推进映射（`nextState` + 标签 + 所需权限）

| 当前 state | nextState | 标签 | 所需节点 |
| --- | --- | --- | --- |
| `translated` | `checked` | 检查 | `PROJECT_ENTRY_REVIEW`（`canReview`） |
| `questioned` | `translated` | 已翻译 | `PROJECT_ENTRY_EDIT`（`canEdit`，**译者可消解自己的疑问**） |
| `checked` | `reviewed` | 审核 | `PROJECT_ENTRY_REVIEW`（`canReview`） |
| `reviewed` / `untranslated` | —（无） | — | — |

`advanceAvailable = nextState 存在 && 用户有该 nextState 对应节点`。故译者在未改的 `translated`/`checked` 上 → 无推进权 → 第 6 行灰保存（符合作者「已审核→灰」的延伸）；译者在未改的 `questioned` 上 → 「已翻译」可点。

### 2.4 接线（`EditorView.vue`）
- 计算 `dirty`（新增；现无脏检测）。
- 现有手动「状态下拉框」**保留**：用户显式改下拉 → `dirty=true` → 走第 4 行普通保存（可设「有疑问」/回退）。未动下拉与译文 → 第 5 行智能推进。二者不冲突。
- 用 `computeSaveButton(...)` 的返回驱动单个保存按钮的 `label`/`color`/`:disable`/`@click`。`@click` 按 `mode`：`save`/`advance`/`force` 都调用现有 `save()`，差别仅在传入的 `state`（`draftState` 或 `nextState`）；推进成功后 `draftState` 同步为 `nextState`。`none` 时按钮禁用。
- 沿用现有 409 冲突处理（刷新为最新）。

---

## 3. 列表编辑者头像

- 行模板（现 ~第 375 行的琥珀 `q-icon name="edit"`）替换为头像：
  - `uid = editingMap[entry.id]`；`uid && uid !== 自己` 时，`m = members.find(x => x.user_id === uid)`。
  - 有 `m.avatar_url` → `<q-avatar size="18px"><img :src="m.avatar_url"></q-avatar>`；否则首字母占位（`m.username[0]`）。
  - `<q-tooltip>` 显示 `用户名 · 角色 · 正在编辑`。
- 无 `m`（不在 members，如平台管理员临时介入）→ 退回通用「有人正在编辑」图标。
- **零后端改动**：`editing` 事件已带 `user_id`、6s 超时清除已存在；`members` 已在进入项目时加载。
- v1 每词条只显示**最近一位**编辑者（`editingMap` 现为 `{entry_id: user_id}`，后写覆盖）。多人同编堆叠为后续增强（需把 map 改成 `{entry_id: Set<user_id>}` + 各自超时）——本期不做。

---

## 4. 强制保存（前端门控）

即 §2.2 第 2/3 行，无独立逻辑：
- `othersEditing && isManager` → 红「强制保存」（可保存内容时启用）。
- `othersEditing && !isManager` → 灰禁用 + tooltip。
- `save()` 不变（带 `version`；409 照常）。**无后端改动、无 version 绕过。**

---

## 5. 结构与测试

- 新文件 `frontend/src/lib/saveButton.ts`（或就近）：纯函数 `computeSaveButton` + `advanceOf(state) -> {nextState,label,node} | null`。
- **单测**（vitest，纯逻辑）覆盖 §2.2 优先级表与 §2.3 推进映射的代表用例：非成员/锁定、他人编辑×(管理/非管理)、dirty、各状态未改×(译者/校对) 的推进/灰、reviewed 终态。
- `EditorView.vue` 仅消费该函数 + 头像模板改动；`pnpm lint`/`pnpm build` 验证；编辑器既有功能（搜索/浏览、保存、实时、历史、建议）不回归。

---

## 6. 涉及文件
- 改：`frontend/src/views/EditorView.vue`（dirty 计算、保存按钮驱动、列表头像）、i18n（zh-CN+en：检查/审核/已翻译/强制保存/正在编辑 等）。
- 新：`frontend/src/lib/saveButton.ts` + 其单测。
- **后端**：无。

## 7. 红线核对
- ✅ 状态推进真正的权限校验在后端 `update_entry`（`node_for_state`）；前端按钮仅 UX。
- ✅ `version` 乐观锁不变；强制保存不绕过 version（无数据丢失风险）。
- ✅ `locked` 仍仅 owner/manager 可改（`panelReadOnly` 不变）。
- ✅ 无密钥/审计/搜索相关改动。
