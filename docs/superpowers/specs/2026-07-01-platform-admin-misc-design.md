# 平台 / 管理杂项 · 工作流 F — 校准版设计 Spec

| 项 | 值 |
| --- | --- |
| 历史阶段 | 2026-07-01 大改造工作流 F |
| 校准日期 | 2026-07-10 |
| 规范总纲 | [`2026-07-10-project-workspace-overhaul-design.md`](./2026-07-10-project-workspace-overhaul-design.md) |

> 2026-07-01 版本确定了用户管理、成员秩、数学门和字体排查方向。本文件保留 F 工作流与 UI 细节；审计 fail-closed/redaction、job FK 和 purge 顺序只以规范总纲 §2.3–§2.4、§9.3 为准。

## 1. 范围

工作流 F 完成：

1. 管理员用户列表、搜索/过滤/排序/键集分页和直接建号；
2. 初始密码提醒与自助修改密码；
3. 项目成员严格授权与唯一 owner 规则；
4. 未来兼容的 exact-tenths CP 整数存储，不实现评分或真实榜单；
5. Redis 数学 challenge、24 小时待删除、倒计时、取消和持久化清除；
6. 字体、MDI、圆角和 zh-CN/en 的最终收口。

审计、持久化 job、物化统计和 capabilities 已在地基阶段建立；F 新增 mutation 必须直接接入，不能再留待其它阶段。

## 2. 平台用户管理

### 2.1 列表与建号

- `GET /admin/users?q=&role=&sort=&after=&limit=` 使用稳定 cursor，支持用户名搜索、平台角色过滤及 username/joined/projects 等稳定排序。
- `POST /admin/users` 接收 username、initial_password 和受秩约束的可选 platform role；密码只以 Argon2id hash 保存。
- 管理表本轮不增加全为 0 的 CP 列。

### 2.2 平台秩

平台秩为 `super_admin > admin > maintainer > user`：

- actor 只能管理当前秩严格低于自己的人；
- 只能授予严格低于自己的角色；
- 不能修改自己、同级或更高用户；
- maintainer 不具备用户管理能力；
- 服务端执行规则，前端只按 capabilities 显示可用选项。

### 2.3 初始密码与自助改密

- 管理员直接建的密码账号设置持久化 `password_change_required=true`。
- 登录后 App 和个人设置持续提醒，但不阻止正常使用。
- `PUT /me/password` 验证旧密码、写新 Argon2id hash、清除提醒，并撤销旧 refresh token。
- API 永不返回初始密码或 password hash；audit payload 也不得保存密码/hash、token 或 challenge answer。建号、改密和失败尝试同步写脱敏审计；审计失败时不提交成功状态/令牌，失败认证也返回通用 503 而非未审计结果。

## 3. 项目成员与唯一 owner

- `projects.owner_id` 是唯一拥有者。升级时补 owner membership，把其它 owner membership 降为 manager，并产生 audit + notification。
- 拥有者可添加/改为 manager、reviewer、translator；管理可添加/改为 reviewer、translator。
- 任何 API 都拒绝把 role 设置为 owner；本轮不提供 owner transfer。
- manager 不能修改/移除 manager；任何人不能通过成员 API 修改/移除 `owner_id`。
- 平台管理员不能绕过 owner-only 主源变化或项目删除；其它跨项目能力仍按服务端 capabilities 明确返回。
- 成员 UI 只消费 `manage_members` 与每行可执行 capability，不比较角色字符串。

## 4. CP 存储边界

- `users.cp_tenths BIGINT NOT NULL DEFAULT 0` 与 `memberships.cp_tenths BIGINT NOT NULL DEFAULT 0` 是唯一真源；一单位代表 0.1 CP，准确保存未来 reviewer 的 0.3 权重结果。
- Rust 直接使用 `i64`。不得引入十进制 crate/sqlx decimal feature，也不得保留浮点或十进制 CP 真源。
- 本轮不在翻译、校对、回滚路径计分，不生成真实排行榜，不增加 0 CP 管理列。排行榜继续显示明确的功能占位。
- 未来排序直接使用 `cp_tenths`，UI 展示时再四舍五入为整数；回滚/恢复固定 0 CP 且不追扣旧分。

## 5. 项目删除

### 5.1 权限与 challenge

- 只有 `projects.owner_id` 可领取和提交 challenge；平台管理员、额外 owner membership 和 manager 均不可。
- `ProjectDeleteDialog` 使用三阶段 UX：第一次完整展示删除后果、24 小时等待期和待删除期间只读状态，owner 必须显式继续；第二次要求输入完整项目 slug 并精确匹配；两关通过后才请求 `POST /projects/{id}/delete-challenge`。
- 两次确认只是前端交互门槛。challenge 仍由服务端执行 owner-only、绑定 user+project、短 TTL、一次性消费与最终答案校验，前端状态不能代替后端授权。
- Redis challenge 绑定 user+project、短 TTL、一次性消费，防重放。
- 平台设置在 `advanced`（预定义标准整数高等微积分题）和 `simple`（简单整数算术）之间切换。两种模式答案都是整数，服务端使用模板计算，不 eval 用户输入。

### 5.2 待删除 24 小时

- 正确答案调用现 DELETE 语义但返回 202；安排事务固定为：先创建/排队 durable `project_purge` job（payload 复制不可变 project id/slug/media/temp keys/deadline）→再把项目更新为 pending，并写 `deletion_scheduled_at=now()+24h`、`deletion_requested_by`、`deletion_job_id`→写 allowlisted audit→提交。job 成功持久化前不得先更新项目，也不立即级联删除。
- 待删除项目从普通公开/成员列表消失；除唯一 owner 外按不可见处理。
- 唯一 owner 只能查看只读倒计时和取消入口；所有其它 mutation 返回稳定 `PROJECT_PENDING_DELETION`。
- 该项目除 `project_purge` 外的 jobs 变 paused；取消后恢复项目可见性和可恢复 jobs，并写 audit。
- `GET /projects/{id}/deletion` 返回倒计时；`POST /projects/{id}/deletion/cancel` 仅 owner_id。

### 5.3 到期清除

- job 到期显式清除项目数据库业务内容、媒体、上传临时文件、任务、术语和文件历史。文件树必须先按叶到根删除 entries/files/folders，再删 `file_change_items`/`file_change_sets`，兼容 `deletion_change_set_id ON DELETE RESTRICT`；不得依赖 project cascade 猜测顺序。
- `audit_log` 不以项目 FK 级联，保留 project id/name/slug 快照、actor、challenge 成功和 purge 元数据。
- `jobs.project_id` 可空且 `ON DELETE SET NULL`；`projects.deletion_job_id` 可空且非级联。purge payload 保存不可变 project id/slug/media/temp keys/deadline。
- 到期先锁 job/project、写 audit metadata、detach/cancel 其它 jobs，在同一 DB 事务中执行上述文件历史顺序和其它关系清理后删除 project；提交后才清理外部 media/temp，最后标同一 job succeeded。外部失败只重试同一 job 的 cleanup stage，绝不复活项目。

## 6. 字体与界面收口

- 删除 `@fontsource/noto-serif-sc` 依赖和所有 serif/SimSun/Songti UI font chain。
- 普通文本、标题和按钮统一 `Noto Sans SC` 同类 sans；Quasar `$typography-font-family` 同步。
- `JetBrains Mono` 只用于 code/key/number class，并把 Noto Sans SC 放在其后承接 CJK，避免中文落入系统宋体。
- 全站 MDI、浅/深主题、方角或 2–4px 圆角；头像圆形不受此限制。
- 管理、删除、密码、成员和设置文案完整覆盖 zh-CN/en，后端错误按 Accept-Language 返回。

## 7. 数据与 API

计划迁移 `0014_admin_delete_cp.sql`：

Task 7.1 先写完整 schema contract RED，覆盖 CP、password reminder、pending deletion/job FK 与索引；同任务把后端 model/DTO/users routes 和前端 api types/Profile/Admin/auth store 的旧 cp:f64 全部转换/移除并 GREEN。Task 7.2 不承担编译遗留，Task 7.3 不回改冻结迁移。

- `users.password_change_required BOOLEAN NOT NULL DEFAULT false`；
- 旧 `users.cp DOUBLE PRECISION` 经受控 `round(cp*10)` 与一致性断言迁移为 `users.cp_tenths BIGINT NOT NULL DEFAULT 0`，随后删除旧列；
- `memberships.cp_tenths BIGINT NOT NULL DEFAULT 0`；
- `projects.deletion_scheduled_at`、`deletion_requested_by`、`deletion_job_id`；
- `deletion_job_id` 的 nullable 非级联关系，以及项目删除 challenge 模式等本阶段运行时设置；
- 待删除扫描、用户列表 cursor 和未来 exact-tenths CP 排序索引。

`jobs` 及其 nullable `project_id REFERENCES projects(id) ON DELETE SET NULL` 由不可变 foundation 迁移 `0007_audit_jobs.sql` 一次性建立；`0014` 不得回改或重复接管该约束。

所有管理员、成员、密码、challenge、schedule、cancel、purge 端点进入 Swagger 并返回 capabilities/稳定错误码。

## 8. 验收

- 平台秩覆盖低于/同级/高于/自己；项目矩阵覆盖 owner/manager/reviewer/translator 与 owner 输入拒绝。
- 升级数据测试证明只有 owner_id 保留 owner，额外 owner 被降级并收到通知/审计。
- 建号、非阻断提醒、旧密码验证、改密、refresh 撤销与 Argon2id 有集成测试。
- CP schema 以 tenths integer 精确往返 0.1/0.3，不存在 decimal/f64 依赖；保存和回滚路径保持 0，不出现虚假榜单/列。
- 前端测试覆盖第一次后果/24h/只读确认、完整 slug 精确匹配（大小写与字符均精确）、未完成任一确认不得请求 challenge；challenge API 覆盖两题型、绑定、过期、重放、错误答案、owner-only 与正确答案 202。待删除覆盖列表隐藏、只读、jobs pause/cancel、nullable FK、DB-first purge、外部清理失败同 job 重试且项目不复活。
- 字体构建产物不包含 Noto Serif SC，computed style 不落入 SimSun/serif；MDI、主题、圆角、i18n 通过前端验证。
- 阶段结束执行测试、verify、Conventional Commit、推 master、等待 CI 与 GHCR。
