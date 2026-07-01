# 平台 / 管理杂项 · 工作流 F — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | 大改造 6 工作流之 **F**（末）；前置 **A**（管理分区外壳/成员管理位）、CP 展示依赖 P6 计分 |
| 基线 | `master` @ `e7d6d45`（E spec 提交后） |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | A–E spec、CLAUDE.md、蓝图 §7 权限 / §10 CP |

> 已与作者确认（含 mockup）：**平台用户管理**——列表（用户名/UID/平台角色/**平台 CP**/加入时间/项目数），点表头排序 + 角色筛选 + 用户名搜索 + 键集分页；**直接添加用户 = 用户名 + 初始密码**（无 SMTP）；改平台角色**「低于自身可授」**（不可授同级/更高）。**项目成员管理**（管理→成员管理）——按用户名搜索添加 + 改项目角色 + 移除，新增**「项目 CP」列**；越权约束：管理不可设「拥有者/管理」，仅拥有者可；**拥有者转让不做**（本阶段）。**CP 模型**：项目 CP 与平台 CP（平台 = 各项目 CP 之和），平台 CP 显示于用户管理 + 个人主页，项目 CP 显示于成员管理 + 排行榜；**计分本身属 P6**，F 仅加存储列 + 展示。**删项目**——二次确认 + **后端出题校验**（题型平台后台切「高数 / 简单算术」，高数用整数答案模板）。**字体回退修复**。

---

## 1. 范围

**做**：① 平台用户管理（列表/排序/筛选/搜索/键集分页 + 直接添加 + 改角色不越权）；② 项目成员管理（按用户名增删改，含项目 CP 列）；③ 删项目二次确认 + 后端数学题门槛（含平台题型开关）；④ 为 CP 展示加最小存储（`memberships.cp`）并派生平台 CP；⑤ 字体回退修复。

**不做（后续/别处）**：**CP 计分逻辑**（Levenshtein 累加，属 P6，F 仅显示 0 值列直到 P6 填充）；**拥有者转让**；SMTP/邮件邀请（无 SMTP，P7）；审计（P5）。

## 2. 决策要点

1. 平台角色秩：`super_admin(3) > admin(2) > maintainer(1) > user(0)`；**仅可授秩 < 自身**的角色，且**仅可修改现秩 < 自身**的用户。取代原「`platform.admin.grant` 仅超管」的粗粒度。
2. 项目角色秩：`owner(3) > manager(2) > reviewer(1) > translator(0)`；`project.member.manage`（owner/manager）者**仅可设秩 < 自身**的角色、**仅可改秩 < 自身**的成员；拥有者不可被改（转让本阶段不做）。
3. 添加用户：管理员填 用户名 + 初始密码（+ 可选平台角色，默认 user，受秩约束）；Argon2id 存储。
4. CP：加 `memberships.cp`（默认 0，P6 填）；**平台 CP = 该用户各 `memberships.cp` 之和**（用户管理列、个人主页）；**项目 CP = `memberships.cp`**（成员管理列、排行榜）。
5. 删项目门槛：`POST …/delete-challenge`（仅 `project.delete`=拥有者）后端出题存 Redis(TTL)，`DELETE …` 带 `{challenge_id, answer}` 后端校验通过才删；题型平台设置 `project_delete_challenge_mode = advanced|simple`（默认 advanced）。真正保护靠权限，题目仅防误删。
6. 用户检索端点复用于「成员管理按用户名添加」与用户管理搜索。

## 3. 数据模型 · 迁移 `0011_member_cp.sql`

```
ALTER TABLE memberships ADD COLUMN cp BIGINT NOT NULL DEFAULT 0;   -- 项目 CP，P6 填充
-- 平台设置新键（若 settings 为 KV 动态表则无需迁移，插入默认）：
--   project_delete_challenge_mode = 'advanced'
索引：memberships(user_id)（供平台 CP 求和 / 用户项目数）。
```
`users.cp` 单列在 P6 收敛为派生（`SUM(memberships.cp)`）；F 起用户管理/主页的平台 CP 走求和，`users.cp` 不再直接展示（P6 决定是否移除）。

## 4. 后端（`prts-api` + `prts-db` + `prts-core`，进 Swagger）

**权限模型**（`prts-core/permission.rs`）：以「角色秩」实现「低于自身可授/可改」；平台与项目各一套秩比较 helper（取代平台仅超管授权的写法）。

**用户管理（平台）**
- `GET /admin/users?q=&role=&sort=&after=&limit=` — 列表：用户名/UID/平台角色/**平台 CP(=SUM memberships.cp)**/加入时间/项目数；键集分页 + 排序（username/cp/joined/projects）+ 角色筛选 + 搜索。平台管理员。
- `POST /admin/users` `{username, password, role?}` — 建号（Argon2id；`role` 受秩约束）。
- `PUT /admin/users/{uid}/role` `{role}` — 改角色（秩校验：目标角色 < 自身秩 且 被改用户现秩 < 自身秩，否则 403）。取代/收敛现 `POST /admin/users/{id}/role`。
- `GET /users/search?q=` — 用户名检索（成员添加 + 用户管理共用；返回精简公开资料）。

**成员管理（项目）**
- `GET /projects/{id}/members` — 扩展返回 `cp`（项目 CP）；支持按 CP 排序。
- `POST /projects/{id}/members` `{user_id|username, role}` — 按用户名/ID 加（`project.member.manage` + 秩约束）。
- `PUT /projects/{id}/members/{user_id}` `{role}` — 改角色（秩约束：目标与被改现秩均 < 自身；拥有者不可被改）。
- `DELETE /projects/{id}/members/{user_id}` — 移除（现有）。

**删项目门槛**
- `POST /projects/{id}/delete-challenge` — 仅 `project.delete`（拥有者）；按 `project_delete_challenge_mode` 生成题目（advanced：导数在某点/定积分/极限，**整数答案**模板；simple：`a op b`），Redis 存 `challenge:{id} → answer`（TTL ~5min，绑定 user+project），返回 `{challenge_id, question}`。
- `DELETE /projects/{id}` — 请求体 `{challenge_id, answer}`；校验 challenge 归属 + 答案（整数精确比对）通过 → 删除（级联）；错误 → 400（前端可重取题）。
- 平台设置 `project_delete_challenge_mode` 在 `GET/PUT /admin/settings` 暴露。

## 5. 前端

- **`AdminView` 用户管理**：表格（列如上，表头排序、角色筛选、搜索、键集分页）；`添加用户`对话框（用户名 + 初始密码 + 角色[受秩约束下拉]）；行内`改角色`下拉（只列可授项）。
- **`ProjectManageView` 成员管理子标签**：成员表（+项目 CP 列，可排序）；`按用户名添加`（`/users/search` 自动补全）+ 角色下拉（受秩约束）；`改角色`/`移除`；拥有者行锁定。
- **删项目对话框**：红色不可逆警告 + 拉取 `delete-challenge` 显示题目 + 答案输入 → 提交带 `{challenge_id, answer}` 的 `DELETE`；答错重取题。平台后台设置加「删除项目题型」开关。
- **个人主页**：CP 显示为**平台 CP**（=SUM）；（排行榜 A 占位，P6 用项目 CP 实装）。
- **字体修复**（见 §6）。
- MDI 图标（E 已定）；i18n 双语；样式少圆角、状态全称、无 emoji。

## 6. 字体回退修复（排查 + 修）

现象：许多字仍回退到 SimSun/宋体，尽管已配 `Noto Sans SC / Noto Serif SC`（`@fontsource`）。排查方向：① `@fontsource` 是否真被打包/加载（Network 查 woff2 是否 200；`main.ts` import 是否被 tree-shake）；② `--font-display` 链 `'Noto Serif SC','Songti SC','SimSun',serif` —— 思源宋子集缺字时回退 SimSun；③ Quasar 默认 `$typography-font-family` 覆盖 body 字体；④ 权重/unicode-range 子集未覆盖所需字形。修：确保思源黑/宋按需权重与全 CJK 子集加载；`--font-*` 链移除 SimSun 或换更好回退；必要时 body 统一 `Noto Sans SC` 优先；设置 Quasar 字体变量与全局 `font-family` 一致。验收：常见界面文本不再出现宋体。

## 7. 性能

- 用户列表键集分页（`(cp,id)` 或 `(joined,id)` 视排序）；平台 CP 用 `SUM(memberships.cp)`（走 `memberships(user_id)` 索引）——用户量大时可考虑物化，暂直算。
- 用户检索 `q` 走 `users.username` 前缀/trgm。
- 删题校验命中 Redis，O(1)。

## 8. 测试

- **单元**：秩比较授权（平台/项目：可授<自身、不可改≥自身、拥有者不可改）；删题模板生成 + 整数答案校验；平台 CP 求和。
- **db-test**：建用户（重名 409、密码哈希）、改角色越权 403、用户检索；成员按用户名加 + 改角色越权 + 移除 + 项目 CP 返回；`delete-challenge`→`DELETE` 正确答案删除/错误答案 400/无权 403/challenge 过期。
- **前端**：CI build/lint；字体回退目视/快照核验。

## 9. 涉及文件

迁移 `0011_member_cp.sql`；`prts-core/permission.rs`（角色秩 + 授权 helper）；`prts-db`（users 建号/检索/列表、memberships cp/角色改、平台 CP 求和）；`prts-api`（`routes/admin.rs` 用户管理、`routes/users.rs` search、`routes/projects.rs` 成员增改+CP、delete-challenge + DELETE 校验、settings 题型键、Swagger）；`prts-search`/Redis（challenge 存取）；前端（`AdminView` 用户管理、`ProjectManageView` 成员管理、删项目对话框、`ProfileView` 平台 CP、`api`、`i18n`、`theme.scss`/`main.ts` 字体修复、`quasar-variables.sass`）；`docs/architecture.md`。

## 10. 红线 / 未决

- 授权严格按秩，**不可越权**（可授<自身、不可改≥自身）；建号密码 Argon2id、仅服务端。
- 删项目：权限（仅拥有者）为真边界，数学题仅防误删；challenge 绑定 user+project + TTL，防重放。
- 键集分页不深翻；检索参数化。
- **未决（实现时）**：`memberships.cp` 与 P6 最终 CP 存储是否需迁移调整（F 先占位）；`users.cp` 何时移除（P6）；高数题模板集合与难度；平台 CP 求和是否需缓存/物化（用户量决定）；拥有者转让（暂不做，日后单列）。
