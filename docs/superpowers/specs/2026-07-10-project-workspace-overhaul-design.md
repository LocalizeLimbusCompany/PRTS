# PRTS 项目工作区大改造（2026-07-10）— 规范总纲

| 项 | 值 |
| --- | --- |
| 状态 | 已批准，A–F 工作流的生命周期、真值表与冲突裁决唯一规范源 |
| 日期 | 2026-07-10 |
| 最新修订 | 2026-07-20：四状态 + questioned 标签、AI 解释、术语模式与可选 OAuth 安装 |
| 范围 | 项目工作区、上传与历史、任务、术语、编辑器与搜索、平台管理与删除 |
| 实施计划 | [`../plans/2026-07-10-project-workspace-overhaul.md`](../plans/2026-07-10-project-workspace-overhaul.md) |

## 1. 优先级与适用方式

本文件记录作者截至 2026-07-20 的直接决定。发生冲突时，按以下顺序解释：

1. 作者最新的直接决定与本文件；
2. 经本轮同步后的 [`plan/26-06-28-init_system.md`](../../../plan/26-06-28-init_system.md) 与 [`docs/architecture.md`](../../architecture.md)；
3. 2026-07-01 的 A–F 工作流规格；
4. 更早的设计、计划与当前代码行为。

本文件是精确生命周期、状态矩阵、可见性谓词与 API 数据形状的规范源。架构、蓝图和 A–F 文件保留职责划分、交互与工作流细节，但不得另行定义相冲突的真值表。迁移号、API 名和文件名是实施契约，不表示当前分支已经具备对应能力；实施时必须以数据库中下一个可用迁移号为准，同时保持本文给出的依赖与发布边界。

## 2. 全局产品与技术约束

### 2.1 前端、主题、字体与国际化

- 交互界面只使用 Vue 3 + Quasar 2；保留浅色与深色主题，图标统一为 MDI。
- 普通控件采用方角或 2–4px 小圆角。头像等天然圆形元素不受此限制。
- 所有中文界面文字使用 `Noto Sans SC` 同类无衬线字体。`JetBrains Mono` 只用于代码、键名和数字，并在字体链中以 CJK 无衬线字体承接中文。界面不使用思源宋体、SimSun 或其它衬线字体。
- 前端文案完整覆盖 zh-CN 与 en；API 客户端发送当前 locale 的 `Accept-Language`，后端以稳定错误码加本地化消息响应。
- 项目介绍和任务介绍保存 Markdown 源文，渲染前必须净化；服务端不接收或持久化前端生成的可信 HTML。

### 2.2 权限与能力下发

- 权限判定仍以权限节点为底座，但 API 必须返回当前主体的 `capabilities`。前端只消费能力，不从 `owner`、`manager` 等角色字符串推断权限。
- `projects.owner_id` 是项目唯一拥有者。平台管理员不能替代拥有者更改主源语言或删除项目。
- 升级迁移把 `owner_id` 之外的 `owner` 成员关系降为 `manager`，并写审计与通知；同时补齐唯一拥有者的成员关系。
- 拥有者只能授予 `manager`、`reviewer`、`translator`；管理只能授予 `reviewer`、`translator`。任何人都不能直接授予 `owner`，本轮不提供拥有者转让。
- 术语、任务、历史与隐藏数据的能力按各自章节执行；所有服务端写端点再次校验能力，不能依赖前端隐藏控件。
- 翻译可设置 `untranslated/translated` 并独立增删 `questioned` 标签；`checked/reviewed` 仍要求 review capability。`questioned` 不是工作流状态。

### 2.3 审计安全与失败语义

- 登录、登出、认证失败、成功认证/令牌签发以及所有业务 mutation 写入追加式 `audit_log`。普通读取不审计；敏感管理员导出、清除和其它明确标注的敏感读取例外。
- 每种 `action` 使用服务端定义的 payload allowlist。审计只保存必要的目标 ID、计数、允许公开的路径、结果/错误码、actor、IP、时间和已脱敏元数据；禁止保存密码或 hash、OAuth code/verifier/token、refresh/access/API key、challenge 答案、原始上传文件体、完整源文/译文以及其它秘密或内容正文。
- 成功 mutation 的业务写与审计必须在同一 PostgreSQL 事务提交。route 只编排事务与领域调用；所有 repository writer 提供接受 `&mut PgConnection` 的 `_tx` 版本（或等价 transaction-scoped executor），不得由 route 以裸 SQL 绕过审计事务。
- `0007` 建立 DB-authoritative auth session 状态机和 durable intent/outbox。表只保存 refresh hash、opaque session/family handle、状态 `pending|active|rotating|revoked|expired`、expiry、前后继与 intent lease/retry；任何 DB/job/audit payload 都不保存 raw token。refresh/rotation/revoke 每次先查并锁 DB authoritative state，Redis 只作 cache/pending material；DB 非 active 时，即使 Redis 残留也不可认证。
- issuance/rotation 的 active/revoked 状态转换与 allowlisted audit 同一 DB 事务提交，提交后才返回 raw token；Redis populate/invalidate 失败由 outbox 重放，不回滚 DB authority。logout/revoke/改密撤销在 DB commit 后立即失效，Redis DEL 失败不恢复有效性。crash worker 清理未完成 intent、重放 cache 操作；stale Redis token 始终受 DB state 否决。
- 失败认证尝试同步写脱敏事件；若该审计也无法持久化，返回通用 503，而不是返回一个未审计的认证成功或失败结论。不得以易丢失的旁路日志、异步队列或“稍后补写”替代。
- 默认安装不编译 OAuth；只有 `zoot-oauth` 构建注册 ZOOT 路由并允许 `password+oauth/oauth-only`。用户/项目 AI API Key 是 API key 禁止写审计的例外持久化场景：只允许以 `PRTS__AI__MASTER_KEY` 加密后的密文入专用设置表，明文、主密钥与 provider 响应正文均不得进入审计或前端响应。

### 2.4 持久化任务通则

- `jobs` 具备 `queued|running|paused|succeeded|failed|cancelled` 状态、阶段、进度、租约、错误、重试和恢复。同一逻辑阶段重试复用同一 job id；上传文件的每次传输/处理尝试另以 attempt 记录保留，见 §5.1。
- `jobs.project_id` 是可空外键并使用 `ON DELETE SET NULL`。job payload 在创建时复制永久清除后仍需使用的不可变 `project_id`、slug、媒体/temp keys 和删除截止时间，不能依赖项目行存活。
- 项目进入待删除状态后，除该项目的清除任务外，其余任务暂停；取消删除后仅恢复可恢复任务。
- 新上传链路和新编辑器控件可用前，不移除旧入口。旧 JSON 上传 API 与旧 `GET /projects/{id}/search` 在替代接口上线后保留一个兼容周期，并在 OpenAPI 与响应头中标为 deprecated。

### 2.5 BCP-47 规范化与旧数据修复

- 后端统一使用小型 `language-tags` Rust 依赖解析与校验 BCP-47，并由共享 canonicalizer 输出稳定大小写：language 小写、script Titlecase、region 大写，variant/extension/private-use 按解析器的规范序列化归一。普通数据库/API 写路径只保存 canonical form；legacy unresolved 原值只能隔离在 repair 状态/issue 流程中，不能进入普通读取与搜索。
- 每个入口都先规范化再校验：项目 create/update 的 `source_langs`、`primary_source_lang`、`target_lang`，上传 `original` JSON keys，术语 import/CRUD 的 `source_lang`，搜索 `source:<bcp47>` field selector，以及用户语言偏好。无效 tag 或 canonicalization 后重复一律拒绝；不得以原字符串大小写绕过唯一性。
- `0008` 增加 project language repair 状态与诊断关系，并排队一次性 durable、按 users/projects/entries/terms stage 与 keyset cursor 分批的 legacy repair job。它规范化项目语言数组/target/primary、entry `original` keys、现有 term `source_lang` 和用户语言偏好；所有既有项目完成 repair 或被明确 gated 前不得开放其主源 UI/API。
- legacy `original` keys 规范化后若合并且值完全相同，可安全折叠为一个 canonical key；值冲突、任一项目所属 tag 无效、项目语言集合无法满足 primary/target 约束或 term tag 无法修复时，项目标记 `needs_language_resolution`，保存逐实体 issue metadata，不静默丢弃内容。无效 user preference 进入 user-level issue 并从 active order 隔离，不错误标记无关项目。
- `needs_language_resolution` 项目禁用 search、普通语言设置、上传/术语等受歧义影响的语言写入，并返回稳定错误码。唯一 owner 使用 `GET /projects/{id}/language-resolution` 与 `POST /projects/{id}/language-resolution/resolve` 显式映射 tag、选择冲突值并确认 source/primary/target；平台管理员只有 metadata-only `GET /admin/language-resolutions` 与 retry endpoint，不得读取私有正文或替 owner 选择主源。resolve/retry 全部写 allowlisted audit，并在解决后排 canonical search reconciliation。
- `0009` 的 primary-source trigger、exact JSON lookup 与 search backfill 只能处理 `language_repair_state=ready` 的项目；repair 未完成或需要人工 resolution 时不得运行会把缺失 exact key 写成空索引的 backfill。foundation readiness 要求 repair 队列清空且没有 unresolved project，或明确保持这些项目 gated。

## 3. 有效可见性、删除继承与统计真值

### 3.1 规范谓词

对 `entries`、所属 `files` 和文件所在 folder ancestor-or-self 集合，定义：

```text
effective_active(entries, files) =
  entries.deleted_at IS NULL
  AND files.deleted_at IS NULL
  AND NOT EXISTS(folder ancestor-or-self WHERE folder.deleted_at IS NOT NULL)

effective_visible(entries, files, include_hidden) =
  effective_active(entries, files)
  AND (entries.hidden = false OR include_hidden = true)
```

- 普通列表、统计、搜索、导出、任务快照和任务进度使用 `include_hidden=false`。
- `include_hidden=true` 只是 owner/manager 显式授权的隐藏覆盖；它绝不包含 entry tombstone、已删除文件或已删除文件夹树。
- search、export、task 与 stats 必须复用相同谓词/数据库 helper；不得各自实现简化版本。

### 3.2 文件夹删除与恢复

- 生命周期边界固定：`0008` 只预建 nullable deletion columns 和支持 effective-visible 的读取/统计；foundation 期间 legacy file/folder delete writer 继续当前硬删除语义并同事务维护物化统计，所有 `deletion_change_set_id IS NULL`，不暴露 soft delete、restore 或 history。以下 soft-delete/restore 规则从 `0010` 创建完整 change-set schema并原子切换 writer 后才生效；不得为 foundation 硬删除伪造 restoration payload/backfill。
- 为避免热读路径递归判断，文件夹删除事务把 folder subtree 及其 descendant files 一并软删除。folders/files 均记录本次操作的 `deletion_change_set_id`；事务只标记此前 active 的行。
- 恢复某次删除只清除 `deletion_change_set_id` 等于该操作 ID 的 folder/file 行。删除前已被其它操作删除的后代保持删除，不能被祖先恢复误带回来。
- 同路径重传产生的 `entries.deleted_at` 是独立 entry tombstone。文件/文件夹删除与恢复不得写入、清除或改造这些 tombstone。

### 3.3 物化统计

- `0008_workspace_meta_stats.sql` 先建立旧五状态统计基础；`0017_editor_ai_terms_mobile.sql` 把 `questioned` 迁为正交标签。最终项目与文件物化四个互斥工作流状态（untranslated/translated/checked/reviewed）及可见总数，`questioned_count` 是可与任意状态重叠的独立计数；写事务增量维护，正常读取不实时 `COUNT(*)`/全项目 `GROUP BY`。
- `file_stats` 保留文件内 active、非 hidden entry 的物化计数。删除文件/文件夹时，从 `project_stats` 与受影响 task exposure 中减去 descendant files 的物化统计，不修改 entry tombstone；恢复时只为实际恢复的 files 加回。
- reconciliation/verify 直接按规范 `effective_visible` 谓词重算并与物化值比较。文件夹 UI 聚合后代 active files；空文件夹最近时间使用自身创建时间。
- 可见词条总数为零时，项目、文件和文件夹进度显示“—”。任务零基线规则见 §6。

## 4. 工作流 A：项目工作区与项目元信息

### 4.1 外壳与页面

- 项目工作区默认进入“信息”，固定分区顺序为：信息、文件、任务、术语、排行榜、下载、管理。
- 编辑器是独立全屏路由，不嵌入项目外壳；从文件或任务中的文件入口进入。
- 信息页只读；项目名称、slug、简介、可见性、语言和头像等修改集中在“管理”。
- 排行榜本轮仅显示明确的功能占位；不渲染虚假的 0 CP 榜单。任务与术语在各自阶段完成后才开放。
- 公开项目允许游客打开 active file 并进入只读编辑器。游客不能保存、改状态、锁定、隐藏、加入 presence、poke 或私信；私有项目仍按项目可见性鉴权。

### 4.2 主源语言发布边界

- ready 项目的 `source_langs`、`primary_source_lang` 与 `target_lang` 使用 §2.5 的 canonical BCP-47，且 primary 始终属于去重后的 `source_langs`。单源项目自动选中唯一源语言；多源项目创建时必须显式选择主源语言。legacy unresolved 行只在 `language_repair_state!=ready` 的条件约束下暂存 raw/nullable 值，不能进入普通项目/搜索路径。
- `0008_workspace_meta_stats.sql` 与 `0009_primary_source_search.sql` 必须属于同一个 foundation release。在该 release 部署完成前，任何 API/UI 都不得接受非首个源语言作为主源，也不得暴露已有项目主源更新。
- route/feature exposure 的硬门槛是：两个迁移均已应用；legacy language repair 已完成或 unresolved 项目被显式 gated；读取 canonical `primary_source_lang` 的 search trigger/function 已生效；既有 search rows 已 backfill/reconcile；词法 backfill worker 已部署并通过 readiness。任何可独立推送的阶段都不得在 trigger/search 仍读取 `source_langs[1]` 或 repair 未建立边界时暴露 `primary_source_lang` 能力。
- 已有项目只有 `projects.owner_id` 指向的用户可以更改主源。相同值是无副作用成功，不触发冷却或 job。
- 真正变化从请求接受时起计算 7 天冷却。下一次真正变化要求冷却结束，且不存在 active 或 unresolved failed 的 lexical/embedding job；失败阶段只能先重试原 job。`degraded/skipped` 不属于失败阻塞。
- 同一保存中若移除当前主源，必须同时提交替代主源。项目已有任何词条后不得更改目标语言。

### 4.3 主源重建状态矩阵

主源变化使用两个明确 job：`primary_source_lexical_reindex` 与 `primary_source_embedding_backfill`。项目状态为：

| 维度 | 状态 | 语义 |
| --- | --- | --- |
| lexical | `ready` | FTS/trgm 可用 |
| lexical | `rebuilding` | 词法 job queued/running，search/TM 返回重建中 |
| lexical | `failed` | 词法 job 重试耗尽；search/TM 不可用，须手动重试同 job |
| embedding | `pending` | 等待 lexical 成功 |
| embedding | `running` | 已配置 provider 正在 backfill |
| embedding | `ready` | vector 与 embedding TM 可用 |
| embedding | `degraded` | provider 禁用/未配置，job 以 `outcome=skipped` 成功；词法 search 与 trgm TM 可用 |
| embedding | `failed` | 已配置 provider 重试耗尽；词法 search 与 trgm TM 仍可用，须手动重试同 embedding job |

- 接受变化的事务立即写新主源、归档旧主源术语、设置 `lexical=rebuilding`/`embedding=pending`、创建 lexical job 并写审计。该时刻起 search/TM 暂停。
- lexical job 按键集批次重算 `source_text/source_tsv`；成功时原子设 `lexical=ready` 并创建/排队 embedding job，立即恢复 FTS/trgm。
- provider 禁用或未配置时，不调用 provider；embedding job 记录 `succeeded + outcome=skipped + reason`，项目设 `degraded`。已配置 provider 的失败使用同一 embedding job 做有界指数退避；耗尽后项目/job 标 failed，API 提供手动重试同 job，不得重新调用整个主源变化。
- 项目 DTO/`GET /jobs/{id}` 暴露 `primary_source_changed_at`、`cooldown_until`，以及 lexical/embedding 各自的 `state`、`job_id`、`progress_current`、`progress_total`、`attempts`、`max_attempts`、`next_retry_at`、稳定 `error_code`/`degraded_reason` 和 `manual_retry_allowed`。前端按该结构展示，不从一个笼统“重建中”字段猜阶段。

### 4.4 头像、简介与工作区统计

- 头像使用 `MediaStore` 抽象的本地实现，文件放入 Docker 持久卷。项目永久清除后按 §9.3 删除媒体，不把二进制写入数据库。
- 前端用 Quasar 对话框和浏览器原生 canvas 做 1:1 裁剪，目标输出 256×256 WebP。服务端校验真实文件签名和可解码内容，并同时要求正方形、宽高各 `<=1024`、总像素 `<=1,048,576`、编码体积 `<=512KB`。
- 公开项目头像公开读取；私有项目头像遵循项目可见性与认证，前端通过带鉴权的 blob 请求展示。
- 项目简介按净化后的 Markdown 展示；信息页只读取 §3 的物化统计。

## 5. 工作流 B：上传、文件管理与历史

### 5.1 上传批次、尝试与清理

- 最终体验选择一个或多个本地原始 JSON 文件，并从项目当前 active 文件夹中选择目标目录（可选项目根目录）；不提供本地目录选择器或粘贴文本框。浏览器不解析内容，声明路径由项目目标目录与文件名组成并流式上传。
- 每批 500 文件、每文件 100MB、每批 2GB、浏览器并发 3 是数据库运行时设置及默认值；服务端在声明、接收和提交阶段校验，前端从配置 DTO 读取。
- V1 不提供 HTTP Range、offset 或分块续传协议。失败/断流重试必须从 byte zero 开始，在同一 logical batch file 下创建新的 `upload_file_attempt`；旧 attempt 的阶段、字节数、错误码和时间保留到 batch/文件生命周期清理。处理 job id 复用，attempt id 递增。
- batch 状态为 `draft|uploading|queued|processing|cancelling|cancelled|partially_succeeded|succeeded|failed|expired`；file/attempt 至少区分 `uploading|queued|processing|succeeded|failed|cancelled|expired`。`POST /projects/{id}/upload-batches/{batch_id}/cancel` 把 batch 置 `cancelling`，取消 queued jobs 与未处理/temp attempts。已进入单文件数据库事务的 worker 可原子完成或回滚，不在半事务中断；全部 active attempts 终止后 batch 置 `cancelled`，已成功文件保持成功。
- 未完成或 abandoned batch 默认 24 小时过期（可由运行时设置调整），由 durable cleanup job 标 `expired` 并清理 temp。成功处理的 raw temp 立即删除；取消、失败/断流和过期 attempt 的 temp 也由幂等 durable cleanup 清除。
- 文件夹路径规范化后拒绝越界、空段、保留段和冲突。单文件解析、重复键校验和替换提交原子；某文件失败不回滚同批其它文件。
- 每个上传对象的 `original` keys 在解析阶段使用 §2.5 canonicalizer；无效 tag、canonicalization 后重复 key（无论值是否相同）或不属于项目最终 `source_langs` 的 key 都拒绝该文件，不能把歧义带入线上数据。

### 5.2 同路径重传

- replacement/restore/tombstone/state reset 与 history rollback 的领域真值由 prts-core typed transition plans 产生；DB adapter只执行，API/worker只编排，SQL不得另定义第二套规则。
- 同一路径代表对该平台文件的完整 replacement，而不是 patch。
- 上传中缺失的旧 key 形成独立 entry tombstone；重新出现时恢复同一平台记录。
- 已存在 key 的平台译文、locked、hidden 与历史保留。源文变化时保留译文但把状态重置为 `untranslated`；源文未变时保留当前状态。
- 上传 `translation/state` 只 seed 从未存在的新 key；恢复旧 key 不覆盖平台译文和状态。
- 每个成功文件生成一个 change set，记录新增、源文变化、entry 恢复和缺失 tombstone，并按 §3 增量更新统计。

### 5.3 文件操作、历史与保留期

- owner/manager 可创建、移动、重命名、删除与恢复文件/文件夹；删除继承和恢复严格使用 §3.2 的 `deletion_change_set_id`。
- 文件历史对项目成员可见；只有 owner/manager 可回滚或恢复。回滚生成 current→target 新 change set，0 CP，不改写旧历史。
- 从文件历史功能首次发布起，entry change-item 的 `before/after` JSON 只允许 `key, original, translation, state, locked, hidden, deleted_at`；禁止用通用实体序列化捕获 `context`。file/folder 结构项使用各自的明确 allowlist，也不得保存正文外的任意字段。
- `files.deletion_change_set_id` 与 `folders.deletion_change_set_id` 均引用 `file_change_sets(id) ON DELETE RESTRICT`（或默认 NO ACTION），防止仍有可恢复业务行时删除 restoration payload；`file_change_sets.file_id/folder_id` 可空并分别 `ON DELETE SET NULL`，允许先物理清除业务树。
- 30 天内 restore 锁定 change set/tree，只清除匹配 operation 的 `deleted_at/deleted_by/purge_after/deletion_change_set_id`；删除前已删除的后代和 entry tombstone 不变。restore 后该 change set 继续作为正常历史保留，直到对象未来真正 purge 或命中另行定义的历史保留策略。
- 到期 purge 固定顺序为：锁定 deletion change set、整棵待清除树及所有以待清除实体为 target 的 restoration-bearing change sets；按叶到根物理删除 descendant entries、files、folders，使 change-set target FK 变为 NULL 且所有 `deletion_change_set_id` 引用消失；显式删除这些 sets 的 `file_change_items`；最后删除对应 `file_change_sets`。change set/delta restoration payload 至此消失，rollback/restore 永久不可能；allowlisted audit metadata 按 retention/项目策略保留且没有恢复正文。
- 跨域 FK 必须显式：task live file/entry refs 与 language-issue live refs `ON DELETE SET NULL` 并保留 immutable snapshot ID；entry-derived search/vector/entry_versions 与 file_stats 使用声明的 `ON DELETE CASCADE`；upload attempts/jobs 对业务 file 使用 nullable target `ON DELETE SET NULL`，保留 attempt 历史到 batch retention。file purge 先 detach/null + 重算 stats，再删派生行和业务树，最后删 history payload；不得依赖默认 RESTRICT。
- 原始上传文件不进入历史或导出。

## 6. 工作流 C：任务

- 任务包含标题、净化 Markdown 介绍和多对多 active file 关系。owner/manager 管理，其它项目可见者只读。
- 文件加入任务时，快照该文件当时 `effective_visible(..., false) AND state=untranslated` 的词条 ID；不能只保存数量。
- 有效分母是 snapshot IDs 中当前仍满足 `effective_visible(..., false)` 的词条；完成数是其中 state 非 `untranslated` 的数量。entry tombstone、文件/文件夹删除或 hidden 使其离开，符合规则的恢复/取消隐藏使其返回。
- 文件移除再加入建立新快照；加入后的新词条不进入旧快照。有效基线为零显示 100% 和“无需处理”。
- `current_task` 搜索覆盖 task 当前 active files 的当前 effective-visible entries，不限 snapshot；scope 的任务与可见性验证按 §8.2。
- `task_files` 保存 immutable file_id snapshot + nullable live file FK `ON DELETE SET NULL`；`task_baseline_entries` 保存 immutable entry_id snapshot + nullable live entry FK `ON DELETE SET NULL`，task_file 删除才 CASCADE 其 baseline。永久 file/entry purge 后 live ref 为 NULL、退出分母，但历史基线仍可解释。

## 7. 工作流 D：术语

- 项目术语保存真实 `source_lang`、`source_text`、目标译文、备注、POS、归档状态和 `match_mode=exact|placeholder|regex`，不能假定源语言是英语。
- 任意合法 canonical BCP-47 source_lang 的术语都可存，不要求属于项目 source_langs；但 active set 严格为当前 primary 的未归档术语。非主源语言 term 只能 archived，请求 active 必须稳定失败而非静默改写。主源变化时旧主源术语归档，新主源已有归档术语恢复 active；legacy old-primary 保持 archived/migration-ready，可人工迁移。
- 混合导出显式输出 `source_lang`、`match_mode` 与 `archived`。POS 保存 zh-CN/en 名称并回退；迁移提供常用双语预设，只有平台管理员管理 POS。
- 导入先 preview，再按 `(project_id, source_lang, source_text, pos_id, match_mode)` 和 `NULLS NOT DISTINCT` upsert。`source_lang` 先执行 §2.5 规范化；未知 POS 置 NULL 并警告；legacy 文件缺 `match_mode` 时稳定按 `exact`。
- preview token 至少 128-bit entropy、TTL 15 分钟，绑定 `actor_id + project_id + import_kind + canonical content digest`，在 confirm 时原子一次性消费。confirm 必须重新检查当前权限；actor/project/kind/digest 不匹配、过期、已使用或权限已撤销均拒绝且不写业务表。
- owner/manager/reviewer 管理术语；编辑器只匹配 active set，建议点击只改本地 draft。`placeholder` 的每个 `[]` 仅在原文侧代表任意文本；`regex` 同样只匹配原文。两者都不做译文捕获替换，并必须提供合法性校验与样例测试。

## 8. 工作流 E：编辑器、结构化搜索与 context 清理

### 8.1 编辑器动作

- 从数据库、上传 DTO、API、前端类型、界面、历史描述和权威文档中删除词条 `context`；兼容期上传体若仍带字段则忽略。
- `0013_editor_search.sql` 必须完成三件事：删除 entries.context；创建/更新 POST 搜索所需 metadata、indexes 和 functions；从既有 `file_change_items.before/after` JSONB entry payload 中 scrub `context` key。迁移前已有历史也不得残留该 key。
- `0017_editor_ai_terms_mobile.sql` 把工作流固定为 `untranslated/translated/checked/reviewed`，新增独立 `questioned` 标签并迁移旧 questioned 行；状态统计与进度条只按四状态互斥分段，有疑问数量单列。原文/译文限高内部滚动，移动端采用可用的单列/抽屉布局。
- 用户级保存差异预览跨设备保存、默认关闭；仅在替换非空旧译文时弹出。纯状态/标签历史显示明确旧值→新值且隐藏内部 `vN`。
- AI 只在用户显式点击时分析当前 primary source；个人与项目 owner 可保存 OpenAI-compatible Base URL/API Key/Model，用户选择 `auto/personal/project`。显式 personal/project 缺失配置不得静默回退；项目 AI 只供实际项目成员使用。响应包含整体含义、去重 tokens、逐词语境义、POS 和语法。
- 右下恰好保留状态下拉 + 一个智能按钮。脏且未翻译→翻译；其它脏状态→保存且状态不变；clean translated/checked 且有 review capability→检查/审核；presence conflict 且有 force capability→强制保存，但仍校验版本；其它禁用。
- 公开游客只读，不建立可写 presence，不显示 mutation/协作动作。

### 8.2 搜索 scope DTO 与验证

`SearchScope` 是以 `type` 为 discriminator 的封闭 tagged union：

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

对应 JSON 请求片段示例（数组中的每个元素分别代表一个请求）：

```json
[
  { "scope": { "type": "all" } },
  { "scope": { "type": "path", "path": "chapter/01" } },
  { "scope": { "type": "file", "file_id": 41 } },
  { "scope": { "type": "current_file", "file_id": 41 } },
  { "scope": { "type": "current_task", "task_id": 73 } }
]
```

- 项目 search route 先用共享 file-path canonicalizer 规范化 path，再验证 path/file/task 属于 URL 中的项目且对 caller 可见；`current_task` 还必须验证 task 本身与 task project 可见。已删除 file/folder、deleted ancestor 下的 path/file、已删除/不可见 task 均返回稳定的 not-found/forbidden，不进入搜索。
- path resolution 必须按 segment boundary：canonical path 精确命中 active file 时只搜索该 file；命中 active folder 时搜索该 folder 的 active descendant files，条件为 exact folder path 或 `folder_path + '/'` subtree，禁止 naive string prefix（如 `dir2` 命中 `dir`）。歧义、跨项目、deleted ancestor 返回稳定 400/404/403。
- `file` 与 `current_file` 都必须带数据库 `BIGINT`/Rust `i64` 的 file_id；后者只是明确表达编辑器当前上下文，服务端绝不从 session/query 猜测。`current_task` 同理必须带 `BIGINT`/`i64` task_id。path 仍为字符串，不进行 UUID ID 迁移。
- tagged union 拒绝未知字段；例如 `{ "type": "all", "file_id": 41 }`、`path` variant 多带 task_id、缺 payload 或未知 type 都返回 400，不能被 serde 静默忽略。
- 旧 GET 适配器只有两种映射：存在 `file_id` 时映射为 `{ "type": "file", "file_id": 41 }`，否则 `{ "type": "all" }`；它绝不制造 `current_file` 或 `current_task`。

### 8.3 搜索语义

- 快捷搜索在列表上方；IME composing 不触发。Enter 发送 `all`，Shift+Enter 发送带当前 file_id 的 `current_file`。
- POST conditions 仅 AND；字段为 `source:<bcp47>|source_any|translation|key`，操作符为 `contains|not_contains|starts_with|ends_with|equals`，不支持 regex。`source:<bcp47>` 在构造数据库 selector 前按 §2.5 规范化；无效 tag 或与项目 canonical source set 不匹配时拒绝。
- 支持多状态、显式 `include_hidden` 和默认 false 的 `vector`。所有召回/fetch 使用 §3 谓词；include_hidden 越权返回 403。
- lexical rebuilding/failed 返回稳定状态与 lexical job 引用；lexical ready 时即恢复 FTS/trgm，不等待 embedding。GET 兼容适配使用同一 service/SQL。
- 类型、OpenAPI 和测试必须覆盖 union 每个 variant 的成功形状、缺少 payload/未知 type 的 400、跨项目或不可见 path/file/task、deleted ancestor 排除，以及 GET 的 file/all 两种映射。
- 唯一默认排序为 `(rrf_score DESC, entry_id ASC)`；任何新增 sort 必须定义稳定 tie-break。limit 默认 50、范围 1..=100。响应为 `{items,next_after}`。
- `after` 是 opaque versioned cursor，至少绑定 `v=1`、URL `project_id`、query+conditions+states+scope+include_hidden+vector 的 canonical fingerprint、最后 `rrf_score` 与 `entry_id`；服务端签名/验证后按 keyset 继续。格式错误、版本未知、签名错误或跨 project/query/filter/scope 重用一律 400，不能退回首页或猜测。

## 9. 工作流 F：平台管理、CP 与项目删除

### 9.1 用户与成员管理

- 平台用户列表使用键集分页；管理员可用用户名 + 初始密码建号。持久化改密提醒不阻止使用。
- 平台严格按秩授权；项目严格执行 §2.2 的 owner/manager 范围。前端只消费 capability。

### 9.2 CP

- 本轮只准备未来兼容的精确十分之一单位整数存储：`users.cp_tenths BIGINT NOT NULL DEFAULT 0` 与 `memberships.cp_tenths BIGINT NOT NULL DEFAULT 0`。一单位代表 0.1 CP；不增加任何十进制数据库/Rust 类型或 sqlx decimal feature。
- 本轮不评分、不生成真实排行榜、不增加全 0 CP 列。未来排序直接按 `cp_tenths`；UI 需要展示时再把 exact tenths 四舍五入为整数。回滚与恢复未来仍固定 0 CP，不追扣历史得分。

### 9.3 项目删除与清除顺序

- `ProjectDeleteDialog` 固定三阶段：第一次展示不可逆后果、24 小时等待期与待删除期间只读状态，并要求 owner 显式继续；第二次要求输入完整项目 slug 且精确匹配；两关通过后才请求 server-side 数学 challenge。两次确认只是前端交互门槛，不能替代后端鉴权。
- 只有 `projects.owner_id` 可领取/提交绑定 user+project、短 TTL、一次性消费的 Redis 整数 challenge；后端最终再次校验 owner 与答案。正确答案安排 24 小时后清除并返回 202；待删除项目从普通列表消失、只读、其它 jobs 暂停。
- `projects.deletion_job_id` 是可空、非级联关系，使用 `ON DELETE SET NULL` 或等价约束；purge job 不能因项目删除而删除。
- 安排事务固定为：创建/排队 purge job（payload 写不可变 project id/slug/media keys/deadline）→写项目 pending 字段与 `deletion_job_id`→写审计→提交。
- 到期 worker 固定顺序为：锁 job/project→审计→detach/cancel jobs/uploads→逐树 NULL task/language live refs并清 stats/search/vector/versions→叶到根删业务树→删 project-scoped history items/sets→按声明 FK 删除 task snapshots/tasks、terms/POS links、stats、upload metadata、language issues、memberships→删 project并提交 external cleanup stage。只使用明确 CASCADE/SET NULL；不得依赖默认 RESTRICT或模糊 project cascade。purge job 以 jobs.project_id SET NULL 保留。
- 外部清理失败时数据库项目保持已删除，不得复活；同一 purge job 保持/回到可重试 external-cleanup stage，使用 payload keys 幂等重试。审计只保留标识与清除元数据，不含 restoration payload。

## 10. Foundation release、发布与验收

- 阶段顺序固定为：基线/文档 → foundation release（`0007`，以及同一 release 的 `0008+0009`、stats/primary trigger/backfill worker、能力基础）→ A route/UI exposure → B → C → D → E → F → 全量验证与发布。
- foundation release 的 readiness 必须证明 `0008/0009` 均已应用、BCP-47 repair worker 已完成可自动修复项目且 unresolved 项目保持 gated、search trigger/function 不再读取 `source_langs[1]`、canonical primary exact-key backfill/reconciliation 完成且 worker 可领取 lexical job；之后才可开放非首主源创建或已有项目主源更新。
- 任一阶段不得回退现有登录、项目、上传、编辑器、实时、通知、私信、搜索或导出。替代链路通过兼容适配或 feature gate 渐进启用。
- 每阶段必须完成单元/集成/前端测试、verify、Conventional Commit、推送 master、等待 CI 成功，并构建推送 GHCR 镜像后再进入下一阶段。
- 20 万词条验证至少覆盖：effective-visible stats reconciliation、主源 lexical backfill、单文件 replacement、批量上传/cancel/expiry、结构化 search 五种 scope 与 task progress。
