# PRTS 项目工作区大改造 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不回退现有功能的前提下，完成项目工作区、持久化上传与历史、任务、术语、编辑器搜索、平台管理和延迟删除的统一改造。

**Architecture:** 先建立追加式 fail-closed 审计、持久化任务、effective-visible 物化统计、项目元信息和能力下发，并在同一个 foundation release 部署 `0008+0009`、primary search trigger/backfill 与 dormant worker；之后才按 A–F 暴露新路由/UI。所有大文件处理与长耗时操作进入可恢复 worker；读路径只消费物化状态或键集查询；旧上传和 GET 搜索通过适配层保留一个兼容周期。

**Tech Stack:** Rust、tokio、axum、sqlx、PostgreSQL 16（pg_trgm/pgvector/zhparser）、Redis、`language-tags`、Vue 3、Quasar 2、Vite、pnpm、Pinia、vue-i18n、utoipa、Docker、GHCR。

---

## 0. 执行约束与迁移顺序

实现以 [`../specs/2026-07-10-project-workspace-overhaul-design.md`](../specs/2026-07-10-project-workspace-overhaul-design.md) 为唯一总纲。当前数据库最大迁移号为 `0006`，本计划固定使用：

| 迁移 | 责任 |
| --- | --- |
| `0007_audit_jobs.sql` | 追加式审计、持久化任务、DB-authoritative auth sessions/intents/outbox、nullable `jobs.project_id ON DELETE SET NULL`、任务租约与重试 |
| `0008_workspace_meta_stats.sql` | canonical BCP-47 配置/repair 状态与 issue 元数据、主源分阶段状态、头像元数据、唯一 owner 修复、nullable entry/file/folder 删除基础、effective-visible 项目/文件统计回填；不启用 soft delete writer |
| `0009_primary_source_search.sql` | 仅在 language repair ready 边界后修正搜索函数/触发器读取 canonical `primary_source_lang`，并提供受 gate 的 search rows backfill/reconciliation |
| `0010_upload_file_history.sql` | 上传批次/file attempts、文件变更集、删除保留期与 cleanup 索引；补 `deletion_change_set_id -> file_change_sets(id) ON DELETE RESTRICT` 与 nullable target `ON DELETE SET NULL` |
| `0011_tasks.sql` | 任务、多对多文件、基线词条 ID、任务统计 |
| `0012_terminology.sql` | 双语 POS、带 source_lang 的术语、归档、导入预览 |
| `0013_editor_search.sql` | 删除 context、scrub 历史 JSONB context key、结构化 POST search metadata/indexes/functions |
| `0014_admin_delete_cp.sql` | 初始密码提醒、`cp_tenths BIGINT` 精确 CP 基础、项目待删除状态与 nullable 非级联 `deletion_job_id` |

迁移一经其阶段合并、推送或在任一环境应用即不可修改；任何后续纠正只新增更大编号迁移。每个迁移在首次创建任务中完整声明该阶段后续任务所需的表、字段、约束与索引，后续任务不得 retroactively 修改它。

`0008` 与 `0009` 是不可拆分的 foundation release：同一发布物还必须包含共享 BCP-47 canonicalizer、durable legacy repair worker、读取 canonical `primary_source_lang` 的 trigger/function、repair-ready 项目的 search rows backfill/reconciliation 和 lexical backfill worker。该 release readiness 通过前，API/UI feature gate 不接受非首个主源或已有项目主源更新；任何可独立推送阶段都不得让新字段已暴露而搜索仍读取 `source_langs[1]`，也不得在 legacy JSON keys 未规范化时运行 exact-key backfill。文件/文件夹 soft-delete 行为明确延后到 `0010`：foundation 期间所有 `deletion_change_set_id` 必须为 NULL，legacy delete writer 保持当前硬删除语义并事务维护 stats，不暴露 restore/history。

每个端点都必须进入 utoipa/Swagger，返回稳定错误码和 zh-CN/en 消息。新表和新字段的 Rust 模型只放在 `prts-db`；业务规则放在 `prts-core`；axum handler 只做协议、鉴权与事务编排。

### 每阶段固定闭环

每个阶段完成后执行并保存输出：

```powershell
Set-Location -LiteralPath (git rev-parse --show-toplevel)
function Invoke-Checked([scriptblock]$Command) {
  & $Command
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Invoke-Checked { docker compose -f deploy/docker-compose.yml up -d postgres redis }

Set-Location .\backend
Invoke-Checked { cargo fmt --check }
Invoke-Checked { cargo clippy --all-targets }
Invoke-Checked { cargo test }
Invoke-Checked { cargo test -p prts-api --features db-tests }

Set-Location ..\frontend
Invoke-Checked { pnpm install --frozen-lockfile }
Invoke-Checked { pnpm lint }
Invoke-Checked { pnpm test }
Invoke-Checked { pnpm typecheck }
Invoke-Checked { pnpm build }

Set-Location ..
Invoke-Checked { docker compose -f deploy/docker-compose.yml up -d --build }
```

预期：命令退出码均为 0；数据库集成测试无 ignored 之外失败；前端 lint/test/typecheck/build 全部通过；`/health` 与 `/swagger-ui` 可访问。随后运行该阶段 verify、创建 Conventional Commit、合入并推送 `master`、等待 GitHub Actions 成功，确认 backend/frontend/postgres GHCR 镜像完成后再进入下一阶段。

## 阶段 0：基线与文档

### Task 0.1：冻结当前行为基线

**Files:**

- Modify: `backend/crates/prts-api/tests/db_integration.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `frontend/src/lib/saveButton.spec.ts`
- Create: `frontend/src/api/compatibility.spec.ts`

- [ ] **Step 1: 记录当前公开项目、旧上传、GET 搜索、编辑保存和导出的成功路径**

  为每条既有能力补回归测试，断言旧接口在替代接口上线前仍可工作；测试不得把当前错误行为（`source_langs[1]`、实时统计、context）固化为目标规则。

- [ ] **Step 2: 运行基线测试并保存测试数量**

  Run from repository root:

  ```powershell
  Set-Location .\backend
  cargo test
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..\frontend
  pnpm test
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..
  ```

  Expected: 当前分支测试全部通过；记录后端与前端通过数量，作为每阶段“不回退”比较基线。

- [ ] **Step 3: 提交基线测试**

  ```powershell
  git add backend/crates/prts-api/tests frontend/src
  git commit -m "test: capture project workspace baseline"
  ```

### Task 0.2：确认规范总纲与兼容策略

**Files:**

- Modify: `docs/superpowers/specs/2026-07-10-project-workspace-overhaul-design.md`
- Modify: `docs/superpowers/plans/2026-07-10-project-workspace-overhaul.md`
- Modify: `docs/architecture.md`
- Modify: `plan/26-06-28-init_system.md`

- [ ] **Step 1: 核对 A–F 规格均链接总纲且无冲突条款**

  Run: `rg -n -g '2026-07-01-*' "source_langs\[1\]|scope: all|永久保存行为|entry history 本来不保存 context|公开读、不鉴权|实时 COUNT|NUMERIC\(20,1\)|BigDecimal|rust_decimal" docs/superpowers/specs docs/architecture.md plan/26-06-28-init_system.md`

  Expected: 不出现被总纲否决的设计主张；命中只能是明确说明已删除或历史背景的文字。

- [ ] **Step 2: 提交文档基线**

  ```powershell
  git add docs plan
  git commit -m "docs: reconcile project workspace overhaul design"
  ```

## 阶段 1：Foundation release（审计、任务、统计、元信息与 primary search）

### Task 1.1：建立追加式审计与持久化任务

**Files:**

- Create: `backend/migrations/0007_audit_jobs.sql`
- Create: `backend/crates/prts-db/src/audit.rs`
- Create: `backend/crates/prts-db/src/jobs.rs`
- Create: `backend/crates/prts-db/src/auth_sessions.rs`
- Create: `backend/crates/prts-core/src/jobs.rs`
- Create: `backend/crates/prts-api/src/job_worker.rs`
- Create: `backend/crates/prts-api/src/jobs/mod.rs`
- Create: `backend/crates/prts-api/src/routes/jobs.rs`
- Modify: `backend/crates/prts-db/src/lib.rs`
- Modify: `backend/crates/prts-db/src/models.rs`
- Modify: `backend/crates/prts-core/src/lib.rs`
- Modify: `backend/crates/prts-api/src/main.rs`
- Modify: `backend/crates/prts-api/src/state.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写迁移测试并验证缺表失败**

  测试覆盖审计只增不改、worker 抢占、租约过期接管、同一 job 重试、项目暂停过滤、进度更新、`jobs.project_id` 删除后置 NULL、purge job 依靠 payload snapshot 继续运行；另覆盖 auth session pending/active/rotating/revoked/expired、intent/outbox lease/retry，以及 payload/rows 只保存 token hash/opaque handle、不保存 raw token。

  Run from repository root:

  ```powershell
  Set-Location .\backend
  cargo test -p prts-api --features db-tests audit_jobs -- --nocapture
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..
  ```

  Expected: 在迁移加入前失败，错误指向 `audit_log` 或 `jobs` 不存在。

- [ ] **Step 2: 创建审计表与防篡改约束**

  `audit_log` 至少包含：`id`、`actor_id`、`actor_kind`、`action`、`target_type`、文本型 `target_id`、可空 `project_id_snapshot`、`payload JSONB`、`ip INET`、`created_at`。项目和用户删除后保留快照字段；应用账号只获得 INSERT/SELECT，数据库触发器拒绝 UPDATE/DELETE。payload 由 action-specific DTO/allowlist 生成，禁止通用实体序列化。

- [ ] **Step 3: 创建任务表与租约模型**

  `jobs` 使用稳定 id，字段包含 `kind`、nullable `project_id REFERENCES projects(id) ON DELETE SET NULL`、`state`、`stage`、`payload`、`result`、`progress_current`、`progress_total`、`attempts`、`max_attempts`、`run_after`、`lease_until`、`worker_id`、`last_error_code`、`last_error_message`、时间戳。状态固定为 `queued|running|paused|succeeded|failed|cancelled`；重试重置同一行到 `queued` 并递增 attempts。purge payload 保存 immutable project id/slug/media/temp keys/deadline。

  同一 `0007` 创建 DB-authoritative `auth_sessions` 与 `auth_session_intents`/outbox：只保存 refresh hash、opaque session/family handle、user、state、expiry、predecessor/successor、intent kind/state/lease/retry，不保存 raw access/refresh。refresh 验证、rotation、logout/revoke 必须先锁/读取 DB authoritative state；Redis 只能缓存 active lookup 或保存不可认证 pending material。

- [ ] **Step 4: 实现 `FOR UPDATE SKIP LOCKED` worker**

  worker 领取到期任务、周期续租、进程崩溃后由其它实例接管。`project_id` 对应项目待删除时，仅 `project_purge` 可运行；其它任务变为 `paused`。

- [ ] **Step 5: 暴露只读任务进度与受控重试 API**

  实现 `GET /jobs/{id}` 和按所属资源过滤的进度查询；手动重试只允许任务拥有的业务权限主体调用，并复用原 job id。

- [ ] **Step 6: 运行测试并提交**

  Run from repository root:

  ```powershell
  Set-Location .\backend
  cargo test -p prts-api --features db-tests audit_jobs -- --nocapture
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..
  ```

  Expected: 审计防改、并发领取、租约恢复和同 job 重试全部通过。

  Commit: `feat: add durable audit and job foundation`

### Task 1.2：把现有认证与变更写入审计

**Files:**

- Modify: `backend/crates/prts-api/src/routes/auth.rs`
- Modify: `backend/crates/prts-api/src/routes/users.rs`
- Modify: `backend/crates/prts-api/src/routes/admin.rs`
- Modify: `backend/crates/prts-api/src/routes/admin_settings.rs`
- Modify: `backend/crates/prts-api/src/routes/projects.rs`
- Modify: `backend/crates/prts-api/src/routes/files.rs`
- Modify: `backend/crates/prts-api/src/routes/entries.rs`
- Modify: `backend/crates/prts-api/src/routes/notifications.rs`
- Modify: `backend/crates/prts-api/src/routes/messages.rs`
- Modify: `backend/crates/prts-api/src/main.rs`
- Modify: `backend/crates/prts-api/src/auth/session.rs`
- Modify: `backend/crates/prts-api/src/auth/extract.rs`
- Modify: `backend/crates/prts-db/src/audit.rs`
- Modify: `backend/crates/prts-db/src/auth_sessions.rs`
- Modify: `backend/crates/prts-db/src/jobs.rs`
- Modify: `backend/crates/prts-db/src/users.rs`
- Modify: `backend/crates/prts-db/src/projects.rs`
- Modify: `backend/crates/prts-db/src/files.rs`
- Modify: `backend/crates/prts-db/src/entries.rs`
- Modify: `backend/crates/prts-db/src/memberships.rs`
- Modify: `backend/crates/prts-db/src/settings.rs`
- Modify: `backend/crates/prts-db/src/search_settings.rs`
- Modify: `backend/crates/prts-db/src/api_keys.rs`
- Modify: `backend/crates/prts-db/src/notifications.rs`
- Modify: `backend/crates/prts-db/src/messages.rs`
- Modify: `backend/crates/prts-auth/src/token.rs`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 为 mutation/auth 写 fail-closed 与脱敏测试**

  为当前全部 writer 建立清单并逐项覆盖：`users`、`projects`、`files`、`entries`、`memberships`、`settings/search_settings`、`api_keys`、`notifications`、`messages`，以及启动期 bootstrap role mutation、password/OAuth 用户落库、API-key touch/revoke、session refresh rotation/revoke 等认证持久化。消息发送、mark-read、poke、设置、角色、旧上传、编辑、flags、删除等成功 mutation 均断言业务与 audit 同事务；注入 audit insert failure 时业务不提交并返回本地化 `AUDIT_UNAVAILABLE` 503/内部错误。成功登录/OAuth/refresh/token issuance 在 audit 失败时不得激活或返回 token。失败认证同步写脱敏事件；其 audit 失败时返回通用 503，而不是原认证结论。

  对每个 action 断言 allowlist payload 不含 password/hash、OAuth code/verifier/token、refresh/access/API key、challenge answer、raw file body、完整 original/translation/source text 或任意 secret/content。普通 reads 不产生日志，敏感 admin export/purge 明确产生日志。

- [ ] **Step 2: 为全部 repository writer 建立 transaction-aware 接口**

  `prts-db` 的上述模块把每个被审计 writer 改为接受 `&mut PgConnection`/transaction-scoped executor 的签名，或提供明确的 `*_tx` 配对函数；只读函数可继续接受 `PgPool`。handler 开 `PgTransaction`，用 `&mut *tx` 调用 repository/domain writer，再写 allowlisted audit 并提交。route 只做协议、鉴权与事务编排，不得用裸 `sqlx::query*` 写业务表来绕过 repository/audit 契约。

- [ ] **Step 3: 实现 DB-authoritative session 状态机与 crash recovery**

  access/refresh token 在返回前必须满足 DB session state 与 audit 已同事务提交。issuance 在事务中写 active session hash + issued audit + Redis-sync outbox，提交后才返回 raw token；Redis 写失败不影响 DB authority，由 outbox 重放。rotation 在一个事务中锁 active predecessor、插入 active successor hash、把 predecessor 置 revoked、写 rotation audit/outbox，提交后才返回 successor；事务失败不返回 token。logout/revoke/改密全量撤销在事务中先把 DB state 置 revoked 并写 audit，提交即立即失效；Redis DEL 失败绝不恢复有效性，只由 durable cleanup 重试。

  每次 refresh/authenticate 都先验证 hash 对应 DB session=`active` 且未过期，再把 Redis 当 cache hint；DB 为 pending/rotating/revoked/expired 时，即使 Redis 残留也拒绝。intent/outbox worker 用租约恢复 crash：清理未完成 pending/rotating 状态、重放 cache invalidate/populate；payload 不含 raw token。测试在 issuance/rotation/revoke 的每个 commit/Redis 边界杀 worker，证明 stale Redis token 不可认证、rotation 不双活、logout DB commit 后立即失效且 cleanup 可恢复。

- [ ] **Step 4: 运行全量后端测试并提交**

  Run from repository root:

  ```powershell
  Set-Location .\backend
  cargo test --all-targets
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..
  ```

  Expected: 既有 API 行为不变，新增审计断言通过。

  Commit: `feat: audit authentication and mutations`

### Task 1.3：建立不可拆分的 workspace/primary-search foundation

**Files:**

- Create: `backend/migrations/0008_workspace_meta_stats.sql`
- Create: `backend/migrations/0009_primary_source_search.sql`
- Create: `backend/crates/prts-db/src/stats.rs`
- Create: `backend/crates/prts-core/src/language.rs`
- Create: `backend/crates/prts-core/src/capabilities.rs`
- Create: `backend/crates/prts-api/src/jobs/repair_languages.rs`
- Create: `backend/crates/prts-api/src/jobs/reindex_project.rs`
- Create: `backend/crates/prts-api/src/dto/capabilities.rs`
- Modify: `backend/Cargo.toml`
- Modify: `backend/crates/prts-core/Cargo.toml`
- Modify: `backend/crates/prts-db/src/lib.rs`
- Modify: `backend/crates/prts-db/src/models.rs`
- Modify: `backend/crates/prts-db/src/projects.rs`
- Modify: `backend/crates/prts-db/src/files.rs`
- Modify: `backend/crates/prts-db/src/entries.rs`
- Modify: `backend/crates/prts-db/src/users.rs`
- Modify: `backend/crates/prts-db/src/search.rs`
- Modify: `backend/crates/prts-core/src/permission.rs`
- Modify: `backend/crates/prts-core/src/lib.rs`
- Modify: `backend/crates/prts-api/src/dto.rs`
- Modify: `backend/crates/prts-api/src/auth/project.rs`
- Modify: `backend/crates/prts-api/src/routes/projects.rs`
- Modify: `backend/crates/prts-api/src/routes/users.rs`
- Modify: `backend/crates/prts-api/src/routes/files.rs`
- Modify: `backend/crates/prts-api/src/routes/entries.rs`
- Modify: `backend/crates/prts-api/src/job_worker.rs`
- Modify: `backend/crates/prts-api/src/embed_worker.rs`
- Modify: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写主源、owner 修复、统计口径和 capabilities 失败测试**

  foundation 覆盖 canonical language/repair、owner、effective visibility、stats 与 capabilities；legacy file/folder hard delete 后 stats 正确且 deletion_change_set_id 始终 NULL。upload/term/search ingress 测试留给后续任务；soft-delete/restore 测试只在 Task 3.1/3.3，foundation 不伪造历史。

- [ ] **Step 2: 建立共享 BCP-47 canonicalizer 与 repair schema**

  workspace manifest 增加 `language-tags`，`prts-core::language` 统一解析并输出 language 小写、script Titlecase、region 大写、variant/extension/private-use 按 parser canonical serialization 的 tag。所有 ingress 只能调用这一 helper；无效 tag 和规范化后重复均返回稳定错误码。foundation 当场接入 project create/update 与 user language preferences；后续 upload/terms/search 任务必须复用同一 helper，不能自写正则或大小写逻辑。

  `0008` 增加 language repair state/job 与 issues；issue 保存 immutable entity type/id snapshot，若有 live project/entry ref 则 nullable `ON DELETE SET NULL`，不复制正文/secret。durable job 以 users/projects/entries/terms stage+cursor 分批规范化；等值 canonical keys才折叠，冲突/invalid 保留原内容并将项目置 needs_language_resolution；user preference issue 独立隔离。

- [ ] **Step 3: 增加项目、搜索与删除基础字段**

  `0008` 先 nullable 增加 `primary_source_lang`，由 repair worker 依据规范化后的旧 `source_langs` 选择现有首项；空数组或歧义进入 issue/resolution 流程，不在迁移中静默猜测。约束写成条件式 CHECK：`language_repair_state='ready'` 时 primary 必须非空且属于 canonical source set；unresolved 行可暂时保留 raw/nullable 数据，直至 owner resolution，不能提前 VALIDATE 全局 NOT NULL。另增 `primary_source_changed_at`、搜索状态/job、avatar 字段；entries/files/folders 可预建 nullable `deleted_at/deleted_by/deletion_change_set_id`，但 `0008` 不创建伪 change set、不 backfill restoration payload、不切换 delete writer。运行时不保留 `[1]` fallback。

- [ ] **Step 4: 修复 owner 数据并建立约束**

  以 `projects.owner_id` 为准补 owner membership；其它 owner membership 改 manager；写通知与审计。增加唯一 owner 部分索引与约束触发器，阻止 owner membership 指向非 `owner_id`。

- [ ] **Step 5: 创建 effective-visible `project_stats` 与 `file_stats` 并接管全部旧 writer**

  `0008` 以规范总纲 §3 的 effective predicate backfill visible total/五状态，并建立集合增量。显式改造 `prts-db/entries.rs` 的 legacy upload/edit/flags 与 `prts-db/files.rs` 的现有硬删除 writer，使业务变化与 stats 同事务。foundation 的 `delete_file/delete_folder` 仍物理删除并正确扣减 project/file stats；所有 `deletion_change_set_id` 保持 NULL，不存在 restore。`0010` 才原子替换为 soft-delete/restore writer。提供按同一谓词的 reconciliation/rebuild，正常读不使用。

- [ ] **Step 6: 在 canonical repair 边界后启用 `0009` search 派生**

  `0009` 以 `CREATE OR REPLACE`/重建 trigger 修正已应用 `0004_search.sql`，使 `source_text/source_tsv` 只从 canonical `primary_source_lang` exact JSON key 派生。trigger 对 `language_repair_state != ready` 的项目拒绝/跳过派生并保持 search gated；backfill/reconciliation job 只领取 ready 项目，且必须在该项目 repair commit 之后运行，不能把未规范化 key 的 exact lookup 静默写为空。部署可领取 `primary_source_lexical_reindex` 的 worker 与 embedding-stage worker，但暂不暴露变更 route。不得修改 `0004/0008`。

- [ ] **Step 7: API 返回显式 capabilities 与 language resolution 状态，但保持主源 route gate 关闭**

  返回既定 capability 集；`edit_locked_entry/force_save_presence` 给 owner/manager，owner-only 项只由 owner_id 产生。`needs_language_resolution` 时 search、普通语言 edits、upload/terms 等受影响入口返回 `PROJECT_LANGUAGE_RESOLUTION_REQUIRED`。

  owner API 固定为 `GET /projects/{id}/language-resolution`（issue、entity ref、raw/canonical tag；经项目可见性授权后才可带冲突值）与 `POST /projects/{id}/language-resolution/resolve`（逐 issue 的 canonical tag/mapping/selected value、最终 source/primary/target）。平台 admin 只有 `GET /admin/language-resolutions` 的 metadata/count 列表和 `POST /admin/language-resolutions/{project_id}/retry`，不能读取私有正文或替 owner 选择 primary。resolve 在单事务应用选择、清 issue、置 `repairing`、排 canonical repair + lexical reconcile 并写 `project.language_resolution_completed` audit；worker 校验成功后才置 `ready`。retry 写 `project.language_repair_retried`。直到 foundation readiness 验证通过且阶段 A route release 开启前，`change_primary_source=false`，create/update DTO 不接受非首主源或更新。

- [ ] **Step 8: 删除正常统计读取中的实时聚合**

  `GET /projects/{id}`、tree 与文件列表改读 stats 表；`refresh_entry_count` 改为 stats 修复入口或移除。文件夹聚合留给前端。

- [ ] **Step 9: 验证 foundation readiness、language repair、统计与 20 万行 backfill**

  Run from repository root:

  ```powershell
  Set-Location .\backend
  cargo test -p prts-api --features db-tests workspace_foundation -- --nocapture
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..
  ```

  Expected: `0008/0009` 同环境均已应用；自动可修复项目全部 canonical，冲突项目保持 gated 且 owner resolution 可闭环；所有 trigger/function 不再读取 `source_langs[1]`；未 repair 项目不运行 exact-key backfill；ready 项目的 search rows reconciliation 一致；lexical worker readiness 通过；随机 legacy/current writer 的 state/visibility 变化后物化值等于离线校验；正常详情无 entries 全表 aggregate。只有这些断言通过后，此 foundation release 才可推送/部署。

  Commit: `feat: add workspace primary search foundation`

### Task 1.4：建立共享前端基础与运行配置

**Files:**

- Create: `frontend/src/components/MarkdownView.vue`
- Create: `frontend/src/components/MarkdownEditor.vue`
- Create: `frontend/src/composables/useJobProgress.ts`
- Create: `frontend/src/lib/capabilities.ts`
- Modify: `frontend/package.json`
- Modify: `frontend/pnpm-lock.yaml`
- Modify: `frontend/src/api/http.ts`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/index.ts`
- Create: `backend/crates/prts-db/src/upload_settings.rs`
- Modify: `backend/crates/prts-db/src/lib.rs`
- Modify: `backend/crates/prts-api/src/routes/admin_settings.rs`
- Modify: `backend/crates/prts-api/src/routes/meta.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Create: `frontend/src/api/uploads.ts`
- Modify: `frontend/src/i18n/index.ts`
- Modify: `frontend/src/i18n/locales/zh-CN.json`
- Modify: `frontend/src/i18n/locales/en.json`
- Modify: `frontend/src/main.ts`
- Modify: `frontend/src/styles/theme.scss`
- Modify: `frontend/src/quasar-variables.sass`
- Modify: `backend/crates/prts-common/src/config.rs`
- Modify: `backend/config/default.toml`
- Modify: `.env.example`
- Modify: `deploy/docker-compose.yml`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 添加 Markdown 安全渲染测试**

  输入脚本、事件属性、危险 URL 和合法 Markdown，断言输出保留文字格式并移除可执行内容。

- [ ] **Step 2: 安装并封装 `markdown-it` 与 `dompurify`**

  所有项目/任务 Markdown 只通过共享组件渲染；调用方不能使用 `v-html` 绕过净化组件。

- [ ] **Step 3: 统一字体、圆角与 MDI**

  删除 Noto Serif SC 依赖和 import；`--font-sans` 以 Noto Sans SC 开头；`--font-mono` 在 JetBrains Mono 后加入 Noto Sans SC CJK fallback；Quasar `$typography-font-family` 与 sans 链一致；交互控件圆角限制 2–4px。切换到 `mdi-v7` 图标集并保留浅/深主题。

- [ ] **Step 4: 完成 locale 与能力基础**

  locale 初始化顺序为 localStorage → 浏览器语言 → zh-CN；Axios 每次请求写当前 `Accept-Language`。共享 capability helper 只读取 API 字段，不接受角色字符串。

- [ ] **Step 5: 加入 media、上传运行时设置和文件保留期配置**

  媒体目录默认 `./data/media`，Docker 增加 media 与 upload temp 持久卷，密钥仍只经环境变量。上传的 `max_files_per_batch=500`、`max_bytes_per_file=100MB`、`max_bytes_per_batch=2GB`、`client_concurrency=3` 四项全部存入平台 `settings`；另设 `upload_batch_expiry_hours=24` 运行时设置供 durable cleanup。由 `prts-db::upload_settings::UploadConfig` 统一默认值、校验和持久化，不进入 `default.toml`、环境变量或前端常量。

  `GET/PUT /admin/settings/upload` 使用 `UploadConfigDto` 管理四项；`GET /meta/upload-config` 只读返回相同 DTO 给普通上传客户端。`frontend/src/api/types.ts` 声明 DTO，`frontend/src/api/uploads.ts` 获取当前配置，后续 `useUploadBatch` 使用返回的 `client_concurrency`，不得定义固定前端常量。所有端点进入 utoipa/Swagger，边界值有 API/DB 测试。

- [ ] **Step 6: 运行设置 API 与前端测试并提交**

  Run from repository root:

  ```powershell
  Set-Location .\backend
  cargo test -p prts-api --features db-tests upload_settings -- --nocapture
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..
  ```

  Run from repository root:

  ```powershell
  Set-Location .\frontend
  pnpm lint
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  pnpm test
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  pnpm typecheck
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  pnpm build
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Set-Location ..
  ```

  Expected: 四项默认/更新/只读 DTO 测试，以及 Markdown 安全、字体/图标编译与现有界面测试通过。

  Commit: `feat: add workspace shared foundations`

## 阶段 2：A 项目工作区 route/UI exposure

### Task 2.1：实现项目 Shell 与只读信息页

**Files:**

- Create: `frontend/src/views/project/ProjectShell.vue`
- Create: `frontend/src/views/project/ProjectInfoView.vue`
- Create: `frontend/src/views/project/ProjectFilesView.vue`
- Create: `frontend/src/views/project/ProjectDownloadView.vue`
- Create: `frontend/src/views/project/ProjectManageView.vue`
- Create: `frontend/src/views/project/ProjectLeaderboardView.vue`
- Create: `frontend/src/components/project/ProjectFileBrowser.vue`
- Create: `frontend/src/components/project/ProjectProgress.vue`
- Create: `frontend/src/components/project/LegacyProjectControls.vue`
- Modify: `frontend/src/router/index.ts`
- Modify: `frontend/src/views/ProjectDetailView.vue`
- Modify: `frontend/src/api/index.ts`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/i18n/locales/zh-CN.json`
- Modify: `frontend/src/i18n/locales/en.json`

- [ ] **Step 1: 写嵌套路由与能力显隐测试**

  断言 `/projects/:id` 重定向到 info；编辑器不嵌 Shell；信息只读；管理只由 capability 显示；固定分区顺序正确；排行榜显示本轮唯一允许的功能占位。

- [ ] **Step 2: 拆分旧 `ProjectDetailView` 并保留兼容控件**

  Shell 负责一次项目详情请求和导航；信息、文件、下载、管理各自消费共享 DTO。把尚未替换的上传、成员和立即删除控件移入临时 `LegacyProjectControls`。Task 2.1 不展示/提交主源变更或非首主源创建；只有 Task 2.2 readiness 通过后才开放。B/F 替换对应旧控件后再删除临时组件。

- [ ] **Step 3: 文件浏览器只读视图使用物化统计**

  支持面包屑、名称搜索、名称/进度/词条数/最近时间排序和状态筛选。文件夹前端聚合后代文件；空进度显示“—”；空文件夹最近时间取 created_at。

- [ ] **Step 4: 项目介绍改用安全 Markdown**

  信息页只展示净化 Markdown；管理页编辑源文并预览。所有文案补齐 zh-CN/en。

- [ ] **Step 5: 运行组件测试并提交**

  Commit: `feat(frontend): add project workspace shell`

### Task 2.2：暴露主源语言变更与双 job 重建状态

**Files:**

- Create: `backend/crates/prts-core/src/project_language.rs`
- Modify: `backend/crates/prts-api/src/jobs/reindex_project.rs`
- Modify: `backend/crates/prts-db/src/projects.rs`
- Modify: `backend/crates/prts-db/src/search.rs`
- Modify: `backend/crates/prts-api/src/job_worker.rs`
- Modify: `backend/crates/prts-api/src/routes/projects.rs`
- Modify: `backend/crates/prts-api/src/routes/search.rs`
- Modify: `backend/crates/prts-api/src/routes/suggestions.rs`
- Modify: `backend/crates/prts-api/src/embed_worker.rs`
- Create: `frontend/src/components/project/LanguageResolutionDialog.vue`
- Modify: `frontend/src/views/project/ProjectManageView.vue`
- Modify: `frontend/src/composables/useJobProgress.ts`
- Test: `backend/crates/prts-api/tests/db_integration.rs`
- Test: `backend/crates/prts-api/tests/search_perf.rs`

- [ ] **Step 1: 写规则测试**

  覆盖 foundation readiness 未通过时 route 不暴露；所有项目语言 ingress 的 canonical casing、无效/重复拒绝；单源自动、多源必选、主源属于 source_langs、相同值不消费冷却、非 owner 拒绝、7 天冷却、active/unresolved failed lexical/embedding job 阻塞新变化、degraded/skipped 不阻塞、移除当前主源需替换、有词条后目标语言锁定；`needs_language_resolution` 时普通语言 edits/search 拒绝，owner 显式 mapping/value resolution 与 admin metadata-only retry 安全。

- [ ] **Step 2: 以 readiness check 开启 route/feature exposure**

  启动/部署检查 `0008+0009` migration version、primary trigger/function revision、backfill reconciliation marker 与 lexical worker health；任一不满足就保持 `change_primary_source=false`、拒绝非首主源 create/update。不得在本任务改写已部署迁移。

- [ ] **Step 3: 接受请求时原子切换状态**

  同一事务更新主源与 `primary_source_changed_at`，设置 `lexical=rebuilding`、`embedding=pending`，创建 `primary_source_lexical_reindex` job，写 allowlisted audit。相同值直接返回，不更新时间、冷却或 job。

  项目 create/update 的 source/primary/target 先调用共享 `language-tags` canonicalizer，canonical duplicate、无效 tag或 primary 不在 source set 均在事务前拒绝。普通设置端点不得充当 language resolution 旁路。

- [ ] **Step 4: 词法阶段按 entry id 键集批处理**

  每批从 `original ->> primary_source_lang` 更新 `source_text/source_tsv`，清空旧 embedding 并更新 lexical job 进度。成功时原子设 `lexical=ready`、恢复 FTS/trgm，并创建 `primary_source_embedding_backfill` job。失败/耗尽设 lexical/project failed，search/TM 继续返回稳定状态与该 job id，手动重试同 job。

- [ ] **Step 5: Embedding/TM 阶段安全降级**

  Provider 禁用/未配置时不调用 provider：embedding job 以 `succeeded + outcome=skipped + reason` 结束，项目设 degraded，trgm TM 可用。已配置 provider 失败时，同一 embedding job 有界指数退避并暴露 `next_retry_at`；耗尽设 failed，用户手动重试同 job，不重新调用主源变更。任何失败不回滚新主源或 lexical success。

- [ ] **Step 6: 前端展示冷却、阶段、进度与降级原因**

  项目 DTO/job API 返回 `language_repair_state`、resolution issue 摘要、`primary_source_changed_at/cooldown_until`，以及 lexical/embedding 各自 state/job_id/progress/attempts/max_attempts/next_retry_at/error_code/degraded_reason/manual_retry_allowed。管理页只按 capability 与这些字段操作；唯一 owner 的 resolution dialog 显式展示 raw/canonical tag 和冲突值选择后提交，普通语言表单保持禁用。平台 admin 页面只显示 project/issue counts 和 retry，不显示私有原文。lexical ready 后 search 立即恢复，不等待 embedding。

- [ ] **Step 7: 运行 20 万词条重建 verify 并提交**

  Expected: foundation gate 不能旁路；lexical 可断点/同 job 手动重试；lexical ready 立即恢复 FTS/trgm；provider 缺失=degraded/skipped，配置 provider 失败按 bounded backoff 并只重试 embedding stage；新主源请求阻塞条件与冷却正确。

  Commit: `feat: add primary source rebuild workflow`

### Task 2.3：实现项目头像与私有可见性

**Files:**

- Create: `backend/crates/prts-api/src/media.rs`
- Create: `backend/crates/prts-api/src/routes/project_media.rs`
- Create: `frontend/src/components/project/AvatarCropDialog.vue`
- Modify: `backend/crates/prts-api/Cargo.toml`
- Modify: `backend/crates/prts-api/src/state.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Modify: `deploy/docker-compose.yml`
- Modify: `frontend/src/views/project/ProjectInfoView.vue`
- Modify: `frontend/src/views/project/ProjectManageView.vue`
- Modify: `frontend/src/api/index.ts`

- [ ] **Step 1: 写签名、尺寸、大小与权限测试**

  伪造 MIME、非 WebP、非正方形、解码失败、任一维度超过 1024、总像素超过 1,048,576、编码体积超过 512KB、公开项目匿名读、私有项目非成员拒绝均需覆盖。

- [ ] **Step 2: 实现 `MediaStore` 与 `LocalMediaStore`**

  key 固定在 `projects/{project_id}/avatar.webp`；写入采用临时文件 + 原子 rename；替换和删除写审计。

- [ ] **Step 3: 实现头像 API**

  `POST /projects/{id}/avatar`、`DELETE /projects/{id}/avatar`、`GET /projects/{id}/avatar` 全部入 Swagger。读取先判断项目可见性；私有项目必须鉴权。

- [ ] **Step 4: 实现 canvas 1:1 裁剪**

  对话框目标输出 256×256 WebP；服务端仍独立执行正方形、维度、像素总量与编码体积 ceiling。私有头像始终通过鉴权 blob + object URL 展示并在组件卸载时 revoke。

- [ ] **Step 5: 运行生命周期测试并提交**

  Commit: `feat: add secure project avatars`

## 阶段 3：B 上传、文件管理与历史

### Task 3.1：建立流式上传 batch 与临时存储

**Files:**

- Create: `backend/migrations/0010_upload_file_history.sql`
- Create: `backend/crates/prts-db/src/uploads.rs`
- Create: `backend/crates/prts-api/src/routes/uploads.rs`
- Create: `backend/crates/prts-api/src/jobs/process_upload.rs`
- Create: `backend/crates/prts-api/src/jobs/cleanup_uploads.rs`
- Modify: `frontend/src/api/uploads.ts`
- Create: `frontend/src/composables/useUploadBatch.ts`
- Modify: `backend/crates/prts-db/src/lib.rs`
- Modify: `backend/crates/prts-db/src/models.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Modify: `backend/crates/prts-api/src/job_worker.rs`
- Modify: `backend/crates/prts-common/src/config.rs`
- Modify: `backend/crates/prts-db/src/upload_settings.rs`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写限制、byte-zero retry、取消、过期和部分成功测试**

  首先写 `0010` 整阶段 schema contract RED：完整断言 upload/history 表、状态 CHECK、retention/cleanup 索引、`file_change_sets.id UUID`、nullable target SET NULL、deletion_change_set_id RESTRICT、`0008` 遗留 deletion ids 全 NULL前置条件，以及迁移后 writer cutover marker；在迁移存在前必须因缺 schema 失败。后续 Task 3.3 只补行为 RED，不得回改已冻结 `0010`。

  覆盖四项设置/DTO、501 files、100MB+1、2GB+1、路径越界；V1 拒绝 Range/offset resume；断流/失败重试创建新 attempt 并从 byte zero；旧 attempt/error 保留；cancel 在 queued/processing race 下原子；abandoned batch 默认 24h expired；durable cleanup；一个坏文件不回滚其它文件；processing job id 复用。

- [ ] **Step 2: 一次性创建本阶段完整上传、历史与生命周期 schema**

  `0010` 首句断言 deletion ids 全 NULL；随后创建完整 upload/history schema。upload batch-file/attempt 对业务 target_file_id 使用 nullable `ON DELETE SET NULL`，逻辑 attempt/history 不随业务 file cascade；deletion FK RESTRICT，change-set target SET NULL。与同一 release 原子切换 soft-delete writer。

- [ ] **Step 3: 实现声明、传输、提交、状态、重试与取消 API**

  - `POST /projects/{id}/upload-batches`：从当前 `UploadConfig` 校验 max files/per-file/batch 三项，不使用编译期常量；
  - `PUT /projects/{id}/upload-batches/{batch_id}/files/{file_id}/attempts/{attempt_id}`：从 byte zero 流式写临时卷，不接受 Range/offset；
  - `POST /projects/{id}/upload-batches/{batch_id}/complete`：校验完整性并排队；
  - `GET /projects/{id}/upload-batches/{batch_id}`：batch/file/attempt 进度与历史；
  - `POST .../files/{file_id}/retry`：复用 logical job，创建并返回新 attempt；
  - `POST /projects/{id}/upload-batches/{batch_id}/cancel`：进入 cancelling，取消 queued jobs/temp attempts；已开始数据库事务允许完成/回滚，最后 cancelled，已成功文件不撤销。

  incomplete/abandoned batch 默认 24h 由 durable cleanup job 标 expired 并清 temp；成功 raw temp 处理后立即删，失败/取消/过期 temp 幂等清理。

- [ ] **Step 4: 保留旧 `/projects/{id}/upload`**

  旧 handler 标为 deprecated，但保持既有客户端可用；新 UI 全量切换并稳定一个兼容周期后才允许删除。

- [ ] **Step 5: 运行 API 测试并提交**

  Commit: `feat: add durable streaming upload batches`

### Task 3.2：实现文件原子完整替换

**Files:**

- Create: `backend/crates/prts-core/src/upload_replacement.rs`
- Create: `backend/crates/prts-core/src/ports/file_repository.rs`
- Modify: `backend/crates/prts-core/src/lib.rs`
- Modify: `backend/crates/prts-db/src/entries.rs`
- Modify: `backend/crates/prts-db/src/files.rs`
- Modify: `backend/crates/prts-db/src/uploads.rs`
- Modify: `backend/crates/prts-api/src/jobs/process_upload.rs`
- Modify: `backend/crates/prts-api/src/routes/entries.rs`
- Test: `backend/crates/prts-api/tests/db_integration.rs`
- Create: `backend/crates/prts-api/tests/upload_perf.rs`

- [ ] **Step 1: 在 prts-core 写 typed transition plan 纯规则测试**

  `prts-core::upload_replacement` 输入现状+上传记录，输出 typed `ReplacementPlan`（insert/restore/source_changed/tombstone/unchanged 与 stats/history deltas）。纯测试覆盖译文保留、源文变化重置 untranslated、缺失 tombstone、旧 key 恢复、hidden/locked 保留、seed 规则；不得把真值写在 SQL/worker if 分支。

- [ ] **Step 2: 流式解析进事务临时表**

  使用 serde JSON stream 逐项读取，按批写 PostgreSQL 临时表；临时表以 key 唯一并保存 ordinal。每个 `original` object key 先走共享 BCP-47 canonicalizer；无效 tag、canonicalization 后重复（即使原始大小写不同）或不属于项目 canonical source set 都返回位置化错误。重复 entry key 返回首个与重复数组位置，语法错误返回 parser 行列；任一错误回滚该文件事务。

- [ ] **Step 3: 用集合 SQL 应用完整替换**

  API worker 只解析/鉴权/开事务并调用 core service；prts-db adapter 按 `ReplacementPlan` 执行参数化集合 SQL、stats/history/audit，不自行重判领域规则。成功后清 temp；失败由 cleanup 处理。

- [ ] **Step 4: 运行 100MB 与 20 万词条 verify**

  Expected: backend RSS 不随文件大小线性增长到整文件量级；文件事务失败时线上文件状态完全不变；成功时统计与 delta 一致。

  Commit: `feat: implement atomic file replacement uploads`

### Task 3.3：实现文件操作、变更集、回滚、恢复与保留期

**Files:**

- Create: `backend/crates/prts-core/src/file_history.rs`
- Modify: `backend/crates/prts-core/src/ports/file_repository.rs`
- Modify: `backend/crates/prts-core/src/lib.rs`
- Create: `backend/crates/prts-db/src/file_history.rs`
- Create: `backend/crates/prts-api/src/routes/file_history.rs`
- Create: `backend/crates/prts-api/src/jobs/purge_deleted_files.rs`
- Modify: `backend/crates/prts-db/src/files.rs`
- Modify: `backend/crates/prts-api/src/routes/files.rs`
- Modify: `backend/crates/prts-api/src/job_worker.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 用 core typed plan 定义历史/回滚/恢复行为**

  `prts-core::file_history` 纯测试生成 typed move/delete/restore/rollback plans，包含 path/tree、operation ownership、before/after allowlist 与 stats deltas；restore/rollback 真值不得塞进 handler/SQL。prts-db 只执行 plan，API 只编排。Task 3.3 的 RED 只测行为，不修改 Task 3.1 已冻结 schema。

- [ ] **Step 2: 实现建夹、移动、重命名、软删除、恢复**

  文件夹移动检查环与后代 path 冲突。删除创建 operation/change-set id，并在事务中只给 active folder subtree/descendant files 写相同 `deletion_change_set_id` 与 `deleted_at/purge_after`；从 project/task exposure 扣 materialized file stats，不改 entry tombstone。restore 只清相同 operation id 并加回实际恢复 files；默认 purge_after=+30d。

- [ ] **Step 3: 实现历史列表与回滚**

  项目成员可 `GET` 历史；owner/manager 可选版本回滚。服务端把目标版本物化为期望状态，再生成 current→target 新 delta；回滚本身写新版本、审计、0 CP，不改旧记录。

- [ ] **Step 4: 实现到期清除任务**

  worker 键集扫描到期软删除树，确认 operation 未恢复后锁定 tree/change sets。固定顺序：cancel/detach 指向业务 file 的 upload attempts/jobs（target FK SET NULL，attempt history按 batch retention保留）；将 task live FKs 与 language-issue live refs置 NULL并重算 task/project stats；删除 entry-derived search/vector rows（明确 `ON DELETE CASCADE`）和 entry_versions；叶到根删除 entries/files/folders，file_stats 随 file CASCADE；显式删除相关 file_change_items/file_change_sets。不得依赖未声明默认 RESTRICT/模糊 cascade；audit 与 immutable snapshot IDs 保留。

  30 天内 restore 锁定相同 tree/change set，只清匹配 operation 的 `deleted_at/deleted_by/purge_after/deletion_change_set_id`；此前删除后代和 entry tombstone 不变。restore 成功后 change set 继续保留为普通历史，直到对象未来实际 purge/历史策略清除。

- [ ] **Step 5: 运行路径/回滚/保留期测试并提交**

  Commit: `feat: add reversible file history and retention`

### Task 3.4：替换上传与文件管理 UI

**Files:**

- Create: `frontend/src/components/project/UploadBatchDialog.vue`
- Create: `frontend/src/components/project/FileMoveDialog.vue`
- Create: `frontend/src/components/project/FileHistoryDialog.vue`
- Modify: `frontend/src/views/project/ProjectFilesView.vue`
- Modify: `frontend/src/views/project/ProjectManageView.vue`
- Modify: `frontend/src/components/project/ProjectFileBrowser.vue`
- Modify: `frontend/src/components/project/LegacyProjectControls.vue`
- Modify: `frontend/src/views/ProjectDetailView.vue`
- Modify: `frontend/src/i18n/locales/zh-CN.json`
- Modify: `frontend/src/i18n/locales/en.json`

- [ ] **Step 1: 浏览器只上传原始 File**

  不调用 `File.text()`、`JSON.parse()` 或把内容放进 Pinia。文件和文件夹选择保留 relative path，并使用 `UploadConfigDto.client_concurrency`，不硬编码 3。

- [ ] **Step 2: 完成批次进度与逐文件重试**

  显示 batch/file/attempt 的 uploading/queued/processing/succeeded/failed/cancelling/cancelled/expired；失败展示错误并从 byte zero 新建 attempt。提供 batch cancel；cancelling 中说明已开始的文件事务可能原子完成。

- [ ] **Step 3: 完成文件维护与历史交互**

  新建、移动、重命名、删除、恢复、历史、回滚均按 capability 显示；删除展示 30 天恢复说明。

- [ ] **Step 4: 新 UI 稳定后隐藏旧粘贴控件**

  从 `LegacyProjectControls` 删除旧上传部分；旧 API 继续兼容一个周期。成员与项目删除旧控件保留到 F 的替代能力完成。

- [ ] **Step 5: 运行前端测试并提交**

  Commit: `feat(frontend): add upload and file history workflows`

## 阶段 4：C 任务

### Task 4.1：实现任务基线 ID 与进度

**Files:**

- Create: `backend/migrations/0011_tasks.sql`
- Create: `backend/crates/prts-db/src/tasks.rs`
- Create: `backend/crates/prts-core/src/tasks.rs`
- Create: `backend/crates/prts-api/src/routes/tasks.rs`
- Modify: `backend/crates/prts-db/src/lib.rs`
- Modify: `backend/crates/prts-db/src/models.rs`
- Modify: `backend/crates/prts-core/src/permission.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Modify: `backend/crates/prts-api/src/dto/capabilities.rs`
- Modify: `backend/crates/prts-api/src/jobs/purge_deleted_files.rs`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写基线语义测试**

  覆盖加入时只快照 `effective_visible(..., false)+untranslated`；hidden、entry tombstone、file/folder deletion 离开分母；各自合法恢复返回且 folder restore 不清 tombstone；翻译完成/回退、移除重加、新 entry 排除和零基线。

- [ ] **Step 2: 创建四表模型**

  `tasks.project_id REFERENCES projects ON DELETE CASCADE`；`task_files` 保存 `file_id_snapshot BIGINT NOT NULL` 与 nullable `live_file_id REFERENCES files(id) ON DELETE SET NULL`，active uniqueness 只约束非 NULL live ref；`task_baseline_entries` 保存 `entry_id_snapshot BIGINT NOT NULL` 与 nullable `live_entry_id REFERENCES entries(id) ON DELETE SET NULL`，并以 `task_file_id REFERENCES task_files ON DELETE CASCADE`。`task_stats.task_id REFERENCES tasks ON DELETE CASCADE`。显式删除 task_file 才删除其快照；永久 file/entry purge 仅置 live FK 为 NULL，snapshot IDs 保留解释性。同任务更新 file-purge worker：先 SET NULL live refs并重算 task_stats，再删除业务行。

- [ ] **Step 3: 快照与任务统计同事务更新**

  添加 active file 时以规范 `effective_visible(..., false) AND state='untranslated'` 同时写 snapshot/live IDs。entry state/hidden/tombstone 以及 file/folder deletion exposure 变化使用集合更新维护 task_stats；file/folder soft delete/restore 复用 materialized file exposure。永久 purge 先让 live FKs SET NULL并重算 stats，NULL live rows退出分母但保留 snapshot。

- [ ] **Step 4: 实现 API 与权限**

  `GET/POST /projects/{id}/tasks`、`GET/PUT/DELETE /projects/{id}/tasks/{task_id}`；owner/manager 写，其它项目可见者读。Markdown 保存源文，前端净化展示。

- [ ] **Step 5: 当前任务范围返回当前可见词条集合**

  提供给 E search 的 db 查询只按当前 active task_files 限定，不联接 baseline entries；route 仍须验证 task 属于 URL project 且 task/project 对 caller 可见。

- [ ] **Step 6: 运行测试并提交**

  Commit: `feat: add snapshot-based project tasks`

### Task 4.2：实现任务 UI

**Files:**

- Create: `frontend/src/views/project/tasks/TaskListView.vue`
- Create: `frontend/src/views/project/tasks/TaskDetailView.vue`
- Create: `frontend/src/views/project/tasks/TaskManageView.vue`
- Create: `frontend/src/api/tasks.ts`
- Modify: `frontend/src/router/index.ts`
- Modify: `frontend/src/views/project/ProjectShell.vue`
- Modify: `frontend/src/components/project/ProjectFileBrowser.vue`
- Modify: `frontend/src/i18n/locales/zh-CN.json`
- Modify: `frontend/src/i18n/locales/en.json`

- [ ] **Step 1: 实现列表、详情与管理路由**

  详情显示净化 Markdown、有效完成/分母、文件集合和“在此任务翻译”；零基线显示 100% 与“无需处理”。

- [ ] **Step 2: 文件勾选保存期望集合**

  文件夹勾选展开为当时的后代文件 ID；后端对新增项快照、既有项保留、移除项删除。

- [ ] **Step 3: capability 驱动操作并提交**

  Commit: `feat(frontend): add project task workspace`

## 阶段 5：D 术语

### Task 5.1：实现双语 POS 与 source-aware terms

**Files:**

- Create: `backend/migrations/0012_terminology.sql`
- Create: `backend/crates/prts-db/src/terms.rs`
- Create: `backend/crates/prts-db/src/pos.rs`
- Create: `backend/crates/prts-core/src/terms.rs`
- Create: `backend/crates/prts-api/src/routes/terms.rs`
- Create: `backend/crates/prts-api/src/routes/pos.rs`
- Modify: `backend/crates/prts-db/src/lib.rs`
- Modify: `backend/crates/prts-db/src/models.rs`
- Modify: `backend/crates/prts-core/src/permission.rs`
- Modify: `backend/crates/prts-api/src/routes/projects.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Modify: `backend/crates/prts-api/src/dto/capabilities.rs`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写 NULL-safe 唯一、归档与语言测试**

  两个相同 `(project,source_lang,source_text,NULL)` 必须冲突；不同 POS 可并存。任意合法 canonical BCP-47 source_lang 可存，不要求属于项目 source set；但 `archived=false` 只允许 `source_lang == primary_source_lang`，非主源 active 请求返回稳定校验错误且不得静默归档。主源切换归档旧 active、激活新主源已有归档术语；legacy old-primary 保持 archived/migration-ready。CRUD/import 统一 canonical casing，无效 tag 与 canonical duplicate 拒绝。

- [ ] **Step 2: 创建 POS 与术语表**

  `pos_presets` 保存 `name_zh_cn`、`name_en`、sort_order，至少一个名称非空；API 按 Accept-Language 回退。`terms` 只保存共享 `language-tags` canonicalizer 输出的 source_lang/source_text/translation/notes/pos_id/archived_at，并用 `UNIQUE NULLS NOT DISTINCT` 建唯一约束。若部署环境已存在兼容 term 表，`0012` 先调用 foundation repair 逻辑；冲突项目进入 `needs_language_resolution`，D 路由保持 gated。

- [ ] **Step 3: 把归档接入主源切换事务**

  切换时令 `source_lang != new_primary` 的 active terms 归档，`source_lang = new_primary` 的术语激活；与项目主源更新、job 创建和审计同事务。

- [ ] **Step 4: 实现 CRUD、键集分页与匹配 API**

  owner/manager/reviewer 写，其它项目可见者读。所有 source_lang 入参先规范化并在规范化后做唯一校验；合法 canonical tag 可作为 archived term 保存，无需属于项目 source set。若请求 active，则事务内校验 source_lang 精确等于当前 primary，否则返回稳定错误。`needs_language_resolution` 项目禁止普通 term mutation。匹配只返回当前主源 active terms；列表可按 current/archived/mixed 过滤。

- [ ] **Step 5: 平台 POS 管理只授予平台管理员**

  maintainer 与项目 owner 均不能修改 POS；所有 POS mutation 写审计。

- [ ] **Step 6: 运行测试并提交**

  Commit: `feat: add source-aware project terminology`

### Task 5.2：实现 CSV/JSON 预览确认导入与混合导出

**Files:**

- Create: `backend/crates/prts-api/src/term_import.rs`
- Create: `frontend/src/components/terms/TermImportDialog.vue`
- Create: `frontend/src/api/terms.ts`
- Modify: `backend/crates/prts-api/src/routes/terms.rs`
- Modify: `backend/crates/prts-api/src/routes/pos.rs`
- Modify: `backend/crates/prts-db/src/terms.rs`
- Modify: `frontend/src/views/project/ProjectShell.vue`
- Create: `frontend/src/views/project/ProjectTermsView.vue`
- Modify: `frontend/src/views/AdminView.vue`
- Modify: `frontend/src/i18n/locales/zh-CN.json`
- Modify: `frontend/src/i18n/locales/en.json`

- [ ] **Step 1: 定义稳定格式**

  术语 CSV 字段为 `source_lang,source_text,translation,pos,notes,archived`；JSON 使用同名字段。POS CSV/JSON 同时包含 `name_zh_cn,name_en,sort_order`。混合导出始终带 source_lang 与 archived。

- [ ] **Step 2: 实现 preview token**

  `POST .../imports/preview` 只解析校验并返回行预览、created/updated、未知 POS 警告和一次性 token；source_lang 在 digest/唯一性计算前 canonicalize，无效或规范化后重复行明确报错。合法非项目 source-set tag 可导入，但 `archived=false` 且不等于当前 primary 的行必须报稳定错误，不能静默改 archived。token 使用 CSPRNG，至少 128-bit entropy，TTL 固定 15 分钟，并绑定 `actor_id + project_id + import_kind(term|pos) + canonical content digest`。

  `POST .../imports/{token}/confirm` 使用 Redis Lua/等价原子 GET-and-consume，校验 actor/project/kind/digest 后一次性消费，并在开启数据库事务后重新检查当前 permission、当前 primary 与每行 active/archived 约束，再按 NULL-safe key upsert 与写 audit。token 不匹配、过期、已使用、权限已撤销、非主源 active 行或项目进入 language-resolution/pending-deletion 状态均拒绝且不写业务表；并发 confirm 只能一个成功。未知 POS 写 NULL，不阻断其它合法行。

- [ ] **Step 3: 实现术语/POS UI 与权限**

  项目术语支持 current/archived/mixed；POS 名按 locale 回退；导入必须先显示预览再允许确认。

- [ ] **Step 4: 往返、过期、重放与权限撤销测试并提交**

  Commit: `feat: add terminology import preview and export`

## 阶段 6：E 编辑器、搜索与 context 清理

### Task 6.1：删除 context 并保持旧上传兼容

**Files:**

- Create: `backend/migrations/0013_editor_search.sql`
- Modify: `backend/crates/prts-db/src/models.rs`
- Modify: `backend/crates/prts-db/src/entries.rs`
- Modify: `backend/crates/prts-api/src/routes/entries.rs`
- Modify: `backend/crates/prts-api/src/dto.rs`
- Modify: `backend/crates/prts-api/tests/db_integration.rs`
- Modify: `backend/crates/prts-api/tests/search_perf.rs`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/index.ts`
- Modify: `frontend/src/views/EditorView.vue`
- Modify: `frontend/src/views/ProjectDetailView.vue`

- [ ] **Step 1: 写 `0013` 完整迁移契约测试**

  API DTO 不含 context；数据库列不存在；新旧上传带 context 时忽略；`ProjectDetailView.vue` 旧上传文案/fixture 不再提 context；导出与 history JSONB 无 context；POST search schema存在。

- [ ] **Step 2: 在 `0013` 完成 drop、history scrub 与结构化 search schema**

  不改写 `0003`。`0013` scrub history 后 DROP；entry-derived search/vector metadata 若为独立表必须 `entry_id ON DELETE CASCADE`、project metadata明确 CASCADE，不能默认 RESTRICT。同步删除 models/DTO/Swagger/types/EditorView 与 `ProjectDetailView.vue` context 引用。

- [ ] **Step 3: 运行仓库级搜索**

  Run: `rg -n "\bcontext\b" backend/crates frontend/src backend/migrations -g '!0003_projects.sql'`

  Expected: 运行时代码/前端无词条 context；`ProjectDetailView.vue` 上传文案/fixture 也无 context；命中只允许已应用 `0003` 历史与 `0013` 的 DROP/scrub；file-history serializer 只出现明确 deny/allowlist 测试。

- [ ] **Step 4: 提交**

  Commit: `refactor: remove entry context field`

### Task 6.2：实现结构化 POST 搜索与 GET 兼容适配

**Files:**

- Create: `backend/crates/prts-core/src/search_query.rs`
- Modify: `backend/crates/prts-db/src/search.rs`
- Modify: `backend/crates/prts-search/src/orchestrator.rs`
- Modify: `backend/crates/prts-search/src/lib.rs`
- Modify: `backend/crates/prts-api/src/routes/search.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Modify: `backend/crates/prts-api/tests/db_integration.rs`
- Modify: `backend/crates/prts-api/tests/search_perf.rs`

- [ ] **Step 1: 定义并测试请求类型**

  ```rust
  #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
  enum SearchScope {
      All,
      Path { path: String },
      File { file_id: i64 },
      CurrentFile { file_id: i64 },
      CurrentTask { task_id: i64 },
  }

  struct StructuredSearchRequest {
      query: Option<String>,
      conditions: Vec<SearchCondition>,
      scope: SearchScope,
      states: Vec<EntryState>,
      #[serde(default)]
      include_hidden: bool,
      #[serde(default)]
      vector: bool,
      after: Option<String>,
      limit: u16,
  }
  ```

  JSON scope 精确为 `{ "type": "all" }`、`{ "type": "path", "path": "..." }`、`{ "type": "file", "file_id": 41 }`、`{ "type": "current_file", "file_id": 41 }`、`{ "type": "current_task", "task_id": 73 }`。files/tasks 沿用 BIGINT/i64。limit 默认 50、范围 1..=100；响应 DTO 固定 `{ items, next_after }`。测试 variant、缺 payload、unknown field/type、错误 ID、limit 0/101。

- [ ] **Step 2: 在项目 route 验证 scope resource**

  path 先 canonicalize 并按 segment boundary resolve：精确命中 active file 时只取该 file；命中 folder 时取 active descendants，SQL 使用 exact path 或 `folder_path || '/%'` 并转义 LIKE，禁止 naive prefix。path/file/task 必须属于 URL project且可见；歧义、跨项目、deleted ancestor稳定拒绝。

- [ ] **Step 3: 参数化构造过滤并接入 P4**

  query 继续走 FTS/trgm/vector/RRF；conditions/scope/states 与规范 effective-visible filter 在召回/fetch 一致应用。`source:<bcp47>` field selector 在构造 JSON lookup 前使用共享 `language-tags` canonicalizer，无效 tag 或不属于项目 canonical source set 时拒绝；`needs_language_resolution` 项目 search 保持 gated。include_hidden 只覆盖 hidden，绝不包含 tombstone/deleted file/folder。`vector=false` 默认不调用 provider。

  默认排序固定 `(rrf_score DESC, entry_id ASC)`。opaque cursor v1 的签名/fingerprint 必须包含 URL project_id、canonical query/conditions/states/scope/include_hidden/vector 与最后 score+id；keyset按 score降序/id升序继续。tamper、未知版本、跨 project/query/filter/scope cursor 返回 400；不得静默重置。测试用两个项目构造相同 all scope/过滤，证明 cursor 不能跨项目重用。

- [ ] **Step 4: 强制 hidden 与重建状态**

  默认使用 `effective_visible(..., false)`；include_hidden 越权返回 403，授权后也只覆盖 hidden。lexical rebuilding/failed 返回稳定状态和 lexical job；lexical ready + embedding degraded/failed 仍返回词法结果。

- [ ] **Step 5: GET 只做 file/all 适配**

  旧 GET 有 `file_id` 时按现有 i64 解析并映射 `{ "type": "file", "file_id": 41 }`，否则 `{ "type": "all" }`；绝不制造 current_file/current_task。响应加入 `Deprecation: true` 与 Sunset，OpenAPI 标 deprecated；不得维护第二套 SQL。

- [ ] **Step 6: 运行操作符、path truth table、cursor/limit、scope 资源、兼容适配与性能测试并提交**

  Commit: `feat: add structured project search`

### Task 6.3：重构编辑器智能动作、术语建议与游客只读模式

**Files:**

- Create: `frontend/src/components/editor/SearchBar.vue`
- Create: `frontend/src/components/editor/AdvancedFilterDialog.vue`
- Create: `frontend/src/components/editor/TermSuggestions.vue`
- Modify: `frontend/src/components/SearchFilters.vue`
- Modify: `frontend/src/views/EditorView.vue`
- Modify: `frontend/src/lib/saveButton.ts`
- Modify: `frontend/src/lib/saveButton.spec.ts`
- Modify: `frontend/src/router/index.ts`
- Modify: `frontend/src/composables/useRealtime.ts`
- Modify: `frontend/src/api/index.ts`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/i18n/locales/zh-CN.json`
- Modify: `frontend/src/i18n/locales/en.json`
- Modify: `backend/crates/prts-api/src/routes/entries.rs`
- Modify: `backend/crates/prts-api/src/routes/ws.rs`

- [ ] **Step 1: 先写智能按钮真值表测试**

  dirty+untranslated=translate；dirty+其它=save 且 state 不变；clean+translated+`review_entry`=check；clean+checked+`review_entry`=review；presence conflict+`force_save_presence`=force；其它 disabled。owner/manager 获得 force capability，但前后端不比较角色名；force 请求仍携带 expected version，版本冲突返回 409。

- [ ] **Step 2: 底部只保留状态下拉与一个按钮**

  删除左下 combobox 和其它保存按钮。状态项按 capabilities 置灰，服务端仍校验状态、locked 和 version。

- [ ] **Step 3: 实现快捷搜索和高级筛选**

  SearchBar 位于列表上方；composition 期间 Enter 不触发；Enter 发送 `{ "type": "all" }`，Shift+Enter 发送 `{ "type": "current_file", "file_id": 41 }`（实际当前文件的 i64）。高级对话框为 path/file/current_file/current_task 收集必需 payload，不以 UI 隐式上下文替代；支持字段/五操作符/多状态/include_hidden/vector=false。

- [ ] **Step 4: 实现 active term 高亮与插入**

  只请求当前主源 active terms。点击建议时有 selection 就替换 selection，否则插入 cursor；只更新本地 draft，不调用保存或状态 API。

- [ ] **Step 5: 实现公开游客只读编辑器**

  公开项目 editor 路由取消强制登录；匿名只调用可读 REST，不建立可写 presence/WS，不显示保存、状态、poke、私信、锁定或隐藏动作。私有项目匿名仍拒绝。

- [ ] **Step 6: 自己头像无协作菜单**

  列表显示本人 editing avatar；点击本人不显示 poke/DM，其它成员仍按 capability 使用既有功能。

- [ ] **Step 7: 运行前端与 API 测试并提交**

  Commit: `feat(frontend): overhaul editor search workflow`

## 阶段 7：F 管理、权限、CP 与项目删除

### Task 7.1：实现管理员用户列表、建号与密码提醒

**Files:**

- Create: `backend/migrations/0014_admin_delete_cp.sql`
- Modify: `backend/crates/prts-db/src/users.rs`
- Modify: `backend/crates/prts-db/src/models.rs`
- Modify: `backend/crates/prts-api/src/dto.rs`
- Modify: `backend/crates/prts-api/src/routes/admin.rs`
- Modify: `backend/crates/prts-api/src/routes/users.rs`
- Modify: `backend/crates/prts-auth/src/password.rs`
- Modify: `backend/crates/prts-core/src/permission.rs`
- Modify: `frontend/src/views/AdminView.vue`
- Modify: `frontend/src/views/ProfileView.vue`
- Modify: `frontend/src/stores/auth.ts`
- Modify: `frontend/src/App.vue`
- Modify: `frontend/src/api/index.ts`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/i18n/locales/zh-CN.json`
- Modify: `frontend/src/i18n/locales/en.json`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写平台秩与 cursor 测试**

  首先写 `0014` 完整 schema contract RED：users.cp_tenths/memberships.cp_tenths、旧 cp 列消失、password_change_required、pending deletion columns、deletion_job_id nullable SET NULL、无 project/job cascade、索引/约束全部断言；迁移前必须 RED。后续 Task 7.3 只补行为 RED，不回改已冻结 `0014`。

  super_admin 只能管理 admin 以下；admin 只能管理 maintainer/user；不能管理同级、更高或自己。各 sort 的 cursor 包含排序值+id，翻页无重复/遗漏。

- [ ] **Step 2: 一次性创建本阶段完整管理、CP 与待删除 schema**

  `0014` 增 password_change_required；受控转换旧 users.cp 为 `users.cp_tenths BIGINT`，新增 `memberships.cp_tenths BIGINT DEFAULT 0`；新增 deletion_scheduled_at/requested_by 与 nullable `deletion_job_id REFERENCES jobs ON DELETE SET NULL`，禁止双向 cascade。同步删除/转换所有 cp:f64 引用：prts-db model、prts-api DTO/users routes、frontend api types/Profile/Admin/auth store 全部改为 cp_tenths i64或不暴露列；迁移、后端与前端必须在 Task 7.1 GREEN，不能把编译修复留给 7.2。

- [ ] **Step 3: 实现用户管理 API**

  `GET /admin/users` 支持 q/role/sort/after/limit；`POST /admin/users` 接 username/initial_password/role；角色变更执行严格秩规则。响应不增加全为 0 的 CP 列。

- [ ] **Step 4: 实现 UI 与非阻断提醒**

  App 展示持久提醒，Profile 提供修改密码；用户可继续正常使用其它功能。

- [ ] **Step 5: 运行测试并提交**

  Commit: `feat: add admin user and password workflows`

### Task 7.2：收紧项目成员授权并消费已完成的 CP schema

**Files:**

- Modify: `backend/crates/prts-db/src/memberships.rs`
- Modify: `backend/crates/prts-api/src/routes/projects.rs`
- Modify: `backend/crates/prts-core/src/permission.rs`
- Modify: `frontend/src/views/project/ProjectManageView.vue`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写项目授权矩阵测试**

  owner 可授 manager/reviewer/translator；manager 可授 reviewer/translator；任何 owner 输入被拒；manager 不能改/移除 manager；owner_id 不可被改/移除；平台管理员也不能绕过 owner transfer 禁令。

- [ ] **Step 2: 验证 Task 7.1 已完成的 exact-tenths 类型**

  本任务只消费已 GREEN 的 cp_tenths model/DTO，不修改迁移或修补遗留 f64；扫描发现旧 cp:f64 即失败并回到 Task 7.1 修正。

- [ ] **Step 3: 不实现评分与 CP 表格**

  本阶段不在编辑保存路径累加 CP，不上线真实排行榜，不在管理员或成员表添加全 0 CP 列。未来排序 SQL 直接按 `cp_tenths`；UI 展示时再把 exact tenths 四舍五入为整数。

- [ ] **Step 4: capability 驱动成员 UI 并提交**

  Commit: `feat: enforce owner membership invariants`

### Task 7.3：实现 Redis 数学 challenge 与 24 小时延迟删除

**Files:**

- Create: `backend/crates/prts-core/src/delete_challenge.rs`
- Create: `backend/crates/prts-api/src/jobs/purge_project.rs`
- Modify: `backend/crates/prts-db/src/projects.rs`
- Modify: `backend/crates/prts-db/src/jobs.rs`
- Modify: `backend/crates/prts-api/src/routes/projects.rs`
- Modify: `backend/crates/prts-api/src/routes/admin.rs`
- Modify: `backend/crates/prts-api/src/auth/project.rs`
- Modify: `backend/crates/prts-api/src/job_worker.rs`
- Modify: `backend/crates/prts-api/src/media.rs`
- Create: `frontend/src/components/project/ProjectDeleteDialog.vue`
- Modify: `frontend/src/views/project/ProjectManageView.vue`
- Delete: `frontend/src/components/project/LegacyProjectControls.vue`
- Modify: `frontend/src/views/project/ProjectShell.vue`
- Modify: `frontend/src/views/ProjectsView.vue`
- Modify: `frontend/src/views/AdminView.vue`
- Modify: `frontend/src/api/index.ts`
- Test: `backend/crates/prts-api/tests/db_integration.rs`

- [ ] **Step 1: 写 challenge、待删除和取消测试**

  前端覆盖三阶段门槛：第一次显示删除后果/24h/待删除只读并要求显式继续；第二次完整 slug 精确匹配；任一未通过都不得请求 challenge。API 覆盖 owner-only、challenge 绑定 user+project、TTL、一次性消费、过期/重放/错误、正确答案 202；并覆盖 24h 前不清除、列表隐藏、mutation 拒绝、jobs pause/cancel、nullable FK。到期 DB-first purge 后 job 以 project_id=NULL 存活；external cleanup 失败重试同 job 且 project 不复活。

- [ ] **Step 2: 接入迁移已建立的待删除字段**

  使用 `0014` 已建立的 `deletion_scheduled_at`、`deletion_requested_by`、`deletion_job_id`。普通项目查询统一过滤 scheduled；项目 access guard 对唯一 owner 返回只读倒计时视图，对其它主体按不可见处理；本任务不修改迁移。

- [ ] **Step 3: 实现安全题库**

  advanced 使用预定义整数微分/定积分/极限模板，simple 使用有界整数四则运算；生成器直接计算整数答案，不 eval 字符串。Redis TTL challenge 一次性消费。

- [ ] **Step 4: 正确答案只安排清除**

  `DELETE /projects/{id}` 的安排事务固定为：创建/排队 project_purge job（payload 复制 immutable project id/slug/media/temp keys/deadline）→更新 scheduled_at/deletion_job_id→写 allowlisted audit→提交并返回 202。`GET .../deletion` 与 `POST .../cancel` 只允许 owner_id。

- [ ] **Step 5: 按 DB-first 固定顺序清除并保留 purge job**

  到期 worker：锁定 purge job/project→写 audit metadata→detach/cancel其它 jobs与 upload attempts→逐树先 NULL task live refs/language issue refs、清 stats/search/vector/entry_versions，再叶到根删 entries/files/folders→显式删除 project-scoped file_change_items/change_sets→按已声明 FK 顺序删除 task baselines/task_files/tasks、terms/POS links、stats、upload metadata、language issues、memberships→删除 project并写 `external_cleanup_pending` 后提交。只允许明确列出的 CASCADE/SET NULL，不能依赖默认 RESTRICT或模糊 project cascade；jobs.project_id SET NULL 保留 purge job。提交后幂等清 media/temp。

- [ ] **Step 6: 完成三阶段删除确认、倒计时与取消 UI**

  `ProjectDeleteDialog` 第一屏完整展示不可逆后果、24 小时等待期和 pending 期间只读语义，显式“继续”后进入第二屏；第二屏要求输入完整项目 slug，逐字符精确匹配后才启用“获取验证题”；第三屏才请求并展示服务端数学 challenge，正确答案提交到后端并以 202 进入 pending。两次前端确认不写可信状态，后端仍重新校验 owner、challenge 绑定/TTL/一次性和答案。待删除 owner 进入项目只能查看倒计时和取消；其它操作置灰并由后端再次拒绝。成员与立即删除替代完成后删除 `LegacyProjectControls`。

- [ ] **Step 7: 运行时间控制测试并提交**

  Commit: `feat: add delayed owner-only project deletion`

## 阶段 8：最终验证、兼容交接与发布

### Task 8.1：全量契约与文档核对

**Files:**

- Modify: `docs/architecture.md`
- Modify: `plan/26-06-28-init_system.md`
- Modify: `README.md`
- Modify: `README.en.md`
- Modify: `backend/crates/prts-api/src/openapi.rs`
- Create: `scripts/verify-project-workspace.ps1`

- [ ] **Step 1: 生成并检查 OpenAPI**

  确认每个新端点有鉴权、请求、响应、错误码、中文描述；旧 upload 与 GET search 标 deprecated；SearchScope 是 `deny_unknown_fields` 的 tagged union，file/task ID 为 i64；无 context schema/history key；所有 BCP-47 ingress 使用共享 canonicalizer。

- [ ] **Step 2: 运行文档关键词审计**

  Run: `rg -n -g '!2026-07-10-project-workspace-overhaul.md' "scope: all|永久保存行为|entry history 本来不保存 context|断流.*续传|source_langs\[1\].*运行时|audit.*旁路|先删媒体.*项目|NUMERIC\(20,1\)|BigDecimal|rust_decimal|file_id:\s*Uu[id]|task_id:\s*Uu[id]" docs plan README.md README.en.md`

  Expected: 不存在与总纲冲突的现行规则；历史说明必须明确已被 2026-07-10 总纲覆盖。

- [ ] **Step 3: 运行相对链接与路径检查**

  verify 脚本解析 Markdown 相对链接并对本地目标执行 `Test-Path`；同时校验计划中列出的 Create/Modify 路径在相应阶段已存在。

- [ ] **Step 4: 提交文档与 verify**

  Commit: `docs: finalize project workspace overhaul`

### Task 8.2：规模、故障恢复与安全验证

**Files:**

- Modify: `backend/crates/prts-api/tests/search_perf.rs`
- Modify: `backend/crates/prts-api/tests/upload_perf.rs`
- Modify: `scripts/verify-project-workspace.ps1`

- [ ] **Step 1: 20 万词条场景**

  验证 project/file stats 不扫 entries；effective-visible reconciliation 覆盖 deleted ancestor/restore op/tombstone；lexical 重建可断点恢复；tagged search 五 scope 满足延迟预算；task progress 不实时 COUNT。

- [ ] **Step 2: 上传与历史场景**

  验证 500 文件/2GB、100MB 流式、无 Range/offset、byte-zero retry/attempt history、cancel race、24h expiry/cleanup、部分成功、replacement、回滚再回滚、30 天 purge 后 restoration payload 消失。

- [ ] **Step 3: 故障注入**

  在 language repair、解析、lexical、embedding、file purge、project DB purge/external cleanup 中途终止 worker；同一 job/stage 恢复。配置 provider 失败只重试 embedding；项目 DB 删除后 external retry 不复活项目。

- [ ] **Step 4: 权限与数据泄露检查**

  匿名公开只读、头像 1024/像素/512KB ceiling、include_hidden 不穿透删除、scope i64/unknown-field/跨项目/不可见资源、BCP-47 canonical ingress 与 resolution gate、preview token 绑定/TTL/一次性/权限重验、owner-only 主源/删除、管理员/成员秩、audit redaction/fail-closed、待删除项目、temp 文件逐项验证。

### Task 8.3：兼容周期与生产发布

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify: `deploy/docker-compose.yml`
- Modify: `deploy/nginx/default.conf`

- [ ] **Step 1: 保留并监控兼容端点**

  统计旧 upload 与 GET search 调用；兼容周期内保持测试。新前端不得再调用旧 upload，快捷/高级搜索不得再调用 GET。

- [ ] **Step 2: 运行固定闭环全部命令**

  Expected: fmt/clippy/test/db-tests、lint/test/typecheck/build、Docker health、Swagger、verify、规模测试均通过。

- [ ] **Step 3: 推送 master 并等待 CI/GHCR**

  ```powershell
  git push origin master
  gh run watch --exit-status
  ```

  Expected: GitHub Actions 成功；GHCR 的 backend、frontend、postgres 镜像均有本次 commit 对应标签。

- [ ] **Step 4: 部署后冒烟**

  创建公开与私有项目，上传文件夹，重传并恢复，创建任务和术语，切换主源并观察降级恢复，以游客打开只读编辑器，安排并取消一次项目删除。所有步骤产生审计，任务进度可查询。

## 2026-07-20 编辑器协作补充

后续编辑器改造以右侧“术语 / 历史 / 评论”上下文区域替代旧术语建议组件，并补充可靠在线状态、键集分页、术语版本及评论能力。本节的文件动作覆盖前文对同一路径的旧动作。

**Files:**

- Delete: `frontend/src/components/editor/TermSuggestions.vue`
- Create: `frontend/src/components/editor/EntryCommentsTab.vue`
- Create: `frontend/src/components/editor/EntryHistoryTab.vue`
- Create: `frontend/src/components/editor/EntryTermsTab.vue`
- Create: `frontend/src/components/editor/SourceTermText.vue`
- Create: `frontend/src/lib/editorDiff.ts`
- Create: `frontend/src/lib/editorDiff.spec.ts`
- Create: `backend/crates/prts-api/src/routes/entry_comments.rs`
- Create: `backend/crates/prts-db/src/comments.rs`
- Create: `backend/migrations/0016_editor_collaboration.sql`
- Modify: `frontend/src/views/EditorView.vue`
- Modify: `scripts/verify-project-workspace.ps1`
