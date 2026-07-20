# PRTS 架构文档

> **状态：项目工作区改造阶段 1–7.3 已实现，阶段 8 正在做最终验证与发布准备。** 当前实施状态以已应用迁移、已发布 feature gate 和 [`2026-07-10 实施计划`](./superpowers/plans/2026-07-10-project-workspace-overhaul.md) 为准。精确生命周期、状态矩阵、可见性谓词与 API truth table 以 [`2026-07-10 规范总纲`](./superpowers/specs/2026-07-10-project-workspace-overhaul-design.md) 为唯一规范源；本文描述当前改造后的组件及组合方式。
>
> 本文与权威蓝图 [`plan/26-06-28-init_system.md`](../plan/26-06-28-init_system.md) 配套。迁移/阶段未完成前，不应以本文的现在时描述推断某项能力已经上线。

## 1. 分层、依赖与 composition root

| Crate | 目标职责 | 不应包含 |
| --- | --- | --- |
| `prts-api` | axum 路由、中间件、WS 入口、worker 编排、utoipa、错误→HTTP/i18n；应用 composition root | 领域规则、手写 SQL |
| `prts-core` | 项目/文件/词条/状态机/权限/能力/任务/术语/历史/删除挑战的框架无关逻辑与 ports | axum、sqlx、HTTP、SQL 方言、`prts-db` |
| `prts-auth` | `AuthProvider`、password/oauth2/zoot、JWT、API-Key 与认证 ports | 项目业务权限 |
| `prts-search` | FTS+trgm+vector/RRF 编排、`EmbeddingProvider` 与搜索 ports | 路由、业务持久化 |
| `prts-realtime` | WS 房间、在线状态、Redis pub/sub adapter | 业务数据库写入 |
| `prts-db` | sqlx 连接池、迁移、查询、行模型，以及 core/search/auth persistence ports 的 adapter | HTTP 与 UI 规则 |
| `prts-common` | 错误、配置、i18n、框架无关通用类型/工具 | 具体业务 |

依赖/组合规则明确如下：

- `prts-api` 是 composition root，显式组合 `prts-core`、`prts-auth`、`prts-search`、`prts-realtime`、`prts-db` 与 `prts-common`。
- `prts-core` 只依赖框架无关类型、traits/ports 与 `prts-common`；它永不依赖 `prts-db`、axum 或 sqlx。
- 数据库 adapters 位于 core 外部，由 `prts-db` 实现 core/search/auth 声明的 ports；axum handlers 依赖 port 接口并由 `prts-api` 注入具体 adapter。
- `prts-auth`、`prts-search` 与 `prts-realtime` 不通过 `prts-core` 反向获得 Web/DB 细节；跨 crate 共享数据使用稳定 DTO/domain types。

## 2. 请求、审计与持久化任务

### 2.1 REST mutation

```text
请求
 → CORS/限流
 → 可选或必需鉴权（JWT/API-Key → Identity）
 → 项目可见性 + 权限节点校验
 → prts-core 领域校验
 → prts-db 参数化事务
 → 业务变更 + action-specific redacted audit 同事务提交
 → 返回 capabilities + {code, message(Accept-Language), details}
```

公开项目只读端点可使用匿名 Identity；私有项目、mutation 与协作动作要求相应 capability。所有 `prts-db` writer 提供接受 `&mut PgConnection` 的 `_tx`/transaction-scoped 入口；route 只开事务、调用 domain/repository、写 audit 并提交，不写裸 SQL。成功 mutation 的 audit 失败时同事务回滚并返回本地化 `AUDIT_UNAVAILABLE` 503/内部错误。

成功认证/令牌签发使用 `0007` 的 DB-authoritative `auth_sessions` + intent/outbox：只存 token hash/opaque handle，state 与 audit 同事务；refresh/rotation/revoke 先查 DB active state，Redis 仅缓存。issuance/rotation DB commit 后才返回 raw token；logout/revoke DB commit 即失效，Redis DEL 失败由 durable outbox 重试且 stale cache 不能绕过 DB。失败认证 audit 写失败时返回通用 503。

Audit payload 按 action 使用 allowlist，只存 ID、允许的路径、计数、结果/错误码、actor/IP/time 与脱敏 metadata；不存密码/hash、OAuth/refresh/access/API secrets、challenge answer、raw upload body 或完整源文/译文。普通 read 不审计，敏感管理员导出/清除除外。完整规则见总纲 §2.3。

### 2.2 持久化任务

通用状态为：

```text
queued → running → succeeded
             ├→ failed → retry same job
             ├→ paused → queued
             └→ cancelled
```

worker 使用 `FOR UPDATE SKIP LOCKED`、租约续期和崩溃接管，并持久化 stage/progress/error/retry。`jobs.project_id` 可空且 `ON DELETE SET NULL`；需要在项目删除后继续工作的 job 把不可变 project snapshot、media/temp keys 与 deadline 放入 payload。

主源切换明确拆为 lexical reindex 与 embedding backfill 两个 job；provider 禁用/未配置只让 embedding 进入 degraded/skipped，不阻塞 lexical search。上传 file retry 复用 logical processing job，但每次 byte-zero 传输/处理使用新的 attempt 行。项目待删除时，除 `project_purge` 外的任务暂停。

## 3. 关键数据流

### 3.1 ZOOT 登录（OAuth2 + PKCE）

OAuth 是可选安装项。默认 Cargo/Compose 构建不启用 `zoot-oauth`，因此不注册 OAuth provider、路由或登录能力；叠加 `deploy/docker-compose.oauth.yml` 或使用 `prts-backend:oauth-latest` 才启用下列流程。`oauth-only` 仅对含 OAuth 的构建有效。

```text
前端选择 ZOOT
 → prts-auth 生成 state+PKCE，state 存 Redis
 → 浏览器授权回调 code
 → 校验 state → token → userinfo
 → 映射 profile/work/external
 → 同 PostgreSQL 事务 upsert user/external_account + auth session active state + redacted audit/outbox
 → DB commit 后返回 access/refresh
 → Redis populate/invalidate 由 durable outbox 收敛；stale cache 始终受 DB session state 否决
```

ZOOT 字段见 [`external/oauth_integration.md`](./external/oauth_integration.md)。ZOOT 是通用 `OAuth2Provider` 的配置实例与映射器；code/verifier/token 永不进入 audit payload。

### 3.2 项目工作区与主源语言

项目工作区目标分区为信息/文件/任务/术语/排行榜/下载/管理，编辑器是独立全屏路由。项目/任务 Markdown 保存源文、净化展示。

语言入口共享 `language-tags` canonicalizer：language 小写、script Titlecase、region 大写，variant/extension/private-use 按 parser 规范序列化。项目 source/primary/target、上传 original keys、term source_lang、search source selector 与用户语言偏好都先规范化；invalid/规范化后重复拒绝。

`0008_workspace_meta_stats.sql` 与 `0009_primary_source_search.sql` 必须在同一 foundation release 部署。durable repair 先按键集批次规范化 legacy project/entry/term/user language data；冲突或 invalid 数据把 project 标 `needs_language_resolution`，禁用 search 与普通语言写，只有 owner resolution UI/API 可选择 mapping/value，platform admin 只有无正文诊断/retry。repair-ready search trigger/function、既有行 backfill/reconciliation 与 lexical worker readiness 完成之前，不开放非首主源创建或已有项目主源更新。部署边界与状态矩阵以总纲 §2.5、§4.2–§4.3 为准。

主源变化的目标数据流：

```text
校验 owner/cooldown/languages/无 active 或 unresolved failed jobs
 → 同事务写新 primary source + terms archive/activate
 → lexical=rebuilding, embedding=pending
 → 创建 lexical job + redacted audit
 → lexical 按 keyset backfill source_text/source_tsv
 → lexical=ready，恢复 FTS/trgm，创建 embedding job
 → embedding ready | degraded(skipped) | failed(retry same stage)
```

相同值无副作用、不消费冷却。配置了 provider 的 embedding 失败做有界指数退避，耗尽后手动重试同一 embedding job，不重新提交主源变更。

### 3.3 有效可见性、删除继承与统计

普通读、统计、search、export 与 task 统一使用总纲 §3 的 `effective_visible`：entry 未 tombstone、file active、无 deleted ancestor folder，且普通流要求 entry 非 hidden。`include_hidden` 只移除 hidden 条件，永不穿透任何删除层。

`0008` 仅预建 nullable deletion columns/effective-visible stats；foundation legacy delete 继续硬删除并同事务维护 stats，所有 deletion_change_set_id 为 NULL，不提供 restore/history。`0010` 断言该列全 NULL、创建 change-set schema/FK 后，才原子切换 soft-delete writer：folder delete 为 active subtree/descendant files 写同一 operation，restore 只清匹配 ID 且不触碰 entry tombstone。

### 3.4 头像与可见性

项目头像使用 `MediaStore` 的本地 Docker volume adapter。前端 canvas 1:1 裁剪，目标输出 256×256 WebP；服务端校验签名和可解码内容，并要求正方形、宽高各 `<=1024`、总像素 `<=1,048,576`、编码体积 `<=512KB`。公开项目头像公开读；私有项目头像鉴权；项目 DB purge 提交后按 purge job payload 幂等清理媒体。

### 3.5 上传、完整替换与文件历史

最终 UI 选择本地原始 JSON 文件，并从项目现有文件夹中选择目标目录（可选项目根目录）；不打开本地目录选择器、不粘贴 JSON，浏览器不解析内容。上传限制来自 `UploadConfigDto`，不内置浏览器并发常量。

```text
create logical batch/files
 → 为每个 file 创建 byte-zero attempt
 → PUT 原始流到 upload temp volume（V1 无 Range/offset resume）
 → complete batch
 → durable job 逐文件流式解析到事务临时表
 → 重复 key/结构校验
 → 集合 SQL 原子 replacement
 → effective stats + allowlisted change set + redacted audit
 → 成功立即清理 raw temp；其它 temp 由 durable cleanup 清理
```

retry 在同一 logical batch file 下创建新 attempt，并保留旧 attempt/error；cancel 先进入 `cancelling`，取消 queued/temp items，已开始的单文件事务允许原子完成或回滚，最后进入 `cancelled`。未完成/abandoned batch 默认 24h 过期。精确状态见总纲 §5.1。

兼容期内 `POST /projects/{id}/upload` 仍适配同一 replacement 规则，并在 OpenAPI 标为 deprecated；新前端只调用 upload-batches。兼容调用量在生产入口按 method/path 统计，达到约定退役条件前不删除旧 handler 或回归测试。

同路径重传是完整 replacement：缺失 key 形成独立 entry tombstone；旧译文/locked/hidden/history 保留；源文变化重置未翻译；上传 translation/state 只 seed 全新 key。file/folder restore 不清除 entry tombstone。

这些 transition 由 `prts-core` typed plans 决定；prts-db 执行计划，API/worker 仅编排。SQL 不重新定义 translation preserve、reset、tombstone、restore/rollback 真值。

上传 `original` JSON keys 在解析阶段 canonicalize；invalid、规范化后重复或不属于项目 source set 会拒绝该文件。文件历史只保存 allowlisted change set，不存 raw upload 或 context。`files/folders.deletion_change_set_id -> file_change_sets(id)` 使用 RESTRICT/NO ACTION，`file_change_sets.file_id/folder_id` 可空且 ON DELETE SET NULL。30 天内 restore 后 change set 保留为普通历史；到期 purge 按 entries/files/folders 叶到根→显式 delete items→delete change set 顺序移除 restoration payload，之后 rollback/restore 不可能，只剩无恢复正文的 audit metadata。

### 3.6 混合搜索（P4 扩展）

搜索列与索引使用 `source_text`、`source_tsv`、`translation_tsv`、trgm GIN 与 vector HNSW；foundation release 后所有触发器/函数只读取 `primary_source_lang`。

`POST /projects/{id}/search` 的 `scope` 是拒绝未知字段、以 `type` 为 discriminator 的 tagged union：`all`、`path {path:string}`、`file {file_id:i64}`、`current_file {file_id:i64}`、`current_task {task_id:i64}`。file/task 沿用数据库 BIGINT，无 UUID migration；`{type:"all",file_id:...}` 必须拒绝。route 验证引用资源属于 URL project 且 caller 可见，并排除 deleted file/folder/task。旧 GET 只把 i64 `file_id` 映射为 `file`，没有时映射为 `all`，绝不猜 current context。完整 DTO/examples/tests 见总纲 §8.2。

path resolver 精确区分 file 与 folder；folder 仅包含 segment-boundary descendants，禁止 naive prefix。默认稳定 keyset 为 `(rrf_score DESC, entry_id ASC)`；opaque v1 cursor 绑定 URL project_id、query/filter/scope fingerprint 与最后 score/id，错误、跨项目或跨查询 cursor 400。limit 默认 50、最大 100，响应返回 `next_after`。

```text
query + AND conditions + tagged scope + states + include_hidden + vector(false)
 → project/resource visibility + lexical state
 → FTS | trgm | optional vector
 → RRF
 → effective-visible/scope/condition 一致过滤
 → keyset/bounded result
```

lexical ready 后立即恢复 FTS/trgm；embedding degraded/failed 只关闭 vector 并以 trgm 提供 TM。GET 与 POST 复用同一 service/SQL。

兼容期内旧 `GET /projects/{id}/search` 只映射 `all/file`，响应 `Deprecation`、`Sunset` 与 successor `Link`；OpenAPI 同步标为 deprecated。新前端快捷/高级搜索只调用结构化 POST，生产入口按 method/path 统计旧 GET 调用量。

### 3.7 实时编辑与游客只读

```text
登录成员进入 active file → WS file room → presence/editing
 → 保存携带 expected version
 → 命中：translation/state/version + entry_version + stats + redacted audit
 → 冲突：409 最新版本，前端合并
```

locked 修改和“强制保存”由 capability 控制；强制保存只越过 presence 提示，仍校验 version。右下只有状态下拉 + 一个智能按钮，真值表以总纲 §8.1 为准。公开游客只用只读 REST，不建立可写 presence 或 mutation/协作动作。

工作流只有 `untranslated/translated/checked/reviewed` 四个互斥状态；`questioned` 是可叠加到任意状态的标签，并可在同一保存事务中附带疑问原因评论。项目/文件进度条只分四个工作流状态，有疑问数量单独展示。原文与译文区域有响应式最大高度和内部滚动；移动端使用单列/抽屉布局。用户级保存差异预览跨设备保存、默认关闭，仅在替换非空旧译文时出现。历史对纯状态/标签变更显示明确的旧值→新值，并隐藏内部版本号。

### 3.8 AI 原文解释

```text
用户显式点击解释当前 primary source
 → 校验登录、项目可见性/实际 membership 与 entry 绑定
 → 按 auto/personal/project 解析 provider（显式来源不回退）
 → 解密 scoped API key，校验 HTTPS endpoint 并固定公网解析地址
 → OpenAI-compatible chat completion
 → 校验结构化响应，去重 tokens，返回整体含义/语境义/POS/语法
```

个人 AI 设置归当前用户，项目 AI 设置只由项目唯一 owner 管理；平台管理员不能冒充 owner。项目 AI 只供实际项目成员使用。API Key 以 XChaCha20-Poly1305 和环境变量 `PRTS__AI__MASTER_KEY` 加密，明文不回传。出站请求拒绝私网/保留地址、HTTP、重定向和 DNS rebinding；缓存键包含 personal user/project owner scope、endpoint、model 与 prompt，避免跨租户复用。读取词条或切换词条不会自动调用第三方。

### 3.9 任务与术语

任务加入 active file 时快照 effective-visible+untranslated entry IDs。有效分母动态使用 effective-visible；文件/文件夹删除使 exposure 离开，符合 deletion change-set 的 restore 后返回。`current_task` 搜索使用 task 当前 active files，而不是 snapshot 子集。

术语保存任意合法 canonical source_lang/source_text/translation/notes/POS/archived/match_mode，不要求 source_lang 属于项目 source set；active 术语必须精确匹配当前 primary，非主源 active 请求稳定拒绝，legacy old-primary 保持 archived/migration-ready。`exact` 按字面包含匹配；`placeholder` 把 source pattern 中每个 `[]` 解释为任意文本，因此 `AAAA [] BBBB` 可命中包含 `AAAA … BBBB` 的原文；`regex` 使用受校验的正则。三种模式都只匹配原文，不做译文捕获替换；辅助端点提供合法性校验与样例测试。候选读取有 5000 项硬上限，避免小请求 limit 截断候选。CSV/JSON 先 preview 后 NULL-safe upsert，旧文件缺 `match_mode` 时按 `exact`；preview token 至少 128-bit entropy、15 分钟 TTL，绑定 actor/project/import kind/content digest，confirm 原子一次性消费并重验权限。编辑器术语卡展示词性、备注和匹配模式；内置双语 POS 预设仍可由平台管理员维护。

### 3.10 通知与私信

通知使用 `notifications(user_id,type,payload,read_at,created_at)` 与 `/ws/user`；列表键集分页。poke 仅项目成员间，文本上限 140。

私信使用 `messages(sender_id,recipient_id,content,read_at,created_at)`；双方须共享至少一个项目，内容 `<=2000`，仅收发双方可见。通知与私信复用用户 WS，并按 event type 分发。

### 3.11 24 小时项目删除

删除对话框先展示后果/24h/只读并要求显式继续，再要求完整 slug 精确匹配，之后才向服务端请求数学 challenge。challenge 仍由后端 owner-only 校验，绑定 user+project、短 TTL、一次性消费；正确答案返回 202 并安排 purge。`projects.deletion_job_id` 为可空非级联关系。

purge 到期后先在数据库事务中锁 job/project、写 audit metadata、detach/cancel 其它 jobs，再对每棵文件树执行 entries/files/folders 叶到根业务行清理，随后显式删除全部 project-scoped file_change_items/change_sets、tasks/terms/memberships/其它关系与 project，并持久化 `external_cleanup_pending` stage；不得依赖 project cascade 穿过 RESTRICT FK。purge job 以 `project_id=NULL` 存活。提交后按 immutable payload 删除 media/temp，成功后标 job succeeded。外部清理失败只重试同一 job 的 external-cleanup stage，绝不恢复 DB project。精确顺序见总纲 §9.3。

### 3.12 CP 与排行榜

在线词条保存先锁定项目、授权与词条版本，再按目标状态选择权重：`checked/reviewed` 为校对/审核 `0.3`，其它状态为翻译/编辑 `1.0`。`prts-core` 按 Unicode 标量值计算 `Levenshtein(previous_translation, new_translation)`，输出 exact tenths；正分事件、`users.cp_tenths`、当前 `memberships.cp_tenths`、entry version 与 allowlisted audit 在同一事务提交。距离为零不写事件；上传、文件历史回滚/恢复与 worker 固定 0 CP。

`contribution_events` 是周期榜的只追加事实账本，`(entry_id, entry_version)` 唯一防止重复发分。项目榜读取当前成员累计值；移除成员后不展示，重新加入时由事件账本恢复该项目累计值。平台总榜读取用户累计值，月榜与周榜按 UTC `[start,end)` 聚合；周一 00:00 UTC 为周起点。同分均以 user id 升序稳定排序，接口返回明确周期边界。

## 4. 数据模型摘要

```text
user 1─* external_account
user 1─* api_key
user *─* project (membership: owner|manager|reviewer|translator)
project.owner_id = 唯一拥有者

project 1─* folder 1─* file 1─* entry 1─* entry_version
folder/file -- deleted_at + deletion_change_set_id
project 1─1 project_stats
file    1─1 file_stats

project 1─* task *─* file
task_file 1─* task_baseline_entry

project 1─* term *─0..1 pos_preset
user    1─0..1 user_ai_settings
project 1─0..1 project_ai_settings

upload_batch 1─* upload_batch_file 1─* upload_file_attempt
upload_batch_file → reusable processing job
file_change_set 1─* file_change_item

job        -- nullable project_id, durable state/stage/progress/retry/lease/payload snapshot
audit_log  -- append-only redacted allowlisted event
setting    -- runtime non-secret configuration
```

`entry` 不含 context；`state` 只含四个互斥工作流状态，`questioned/locked/hidden` 是正交布尔字段；entry history payload 只允许总纲 §5.3 列出的字段。`projects.owner_id` 是 owner 真源。CP 使用 `users.cp_tenths/memberships.cp_tenths BIGINT`（一单位 0.1 CP）保存累计真源；只追加 `contribution_events` 保存在线词条保存的 actor/project/entry-version/kind/distance/exact tenths，支撑平台 UTC 月榜/周榜且不引入 decimal 依赖。上传、回滚、恢复和系统任务不产生 CP。

## 5. 权限与能力

- owner 授 manager/reviewer/translator；manager 授 reviewer/translator；任何 API 不授 owner，本轮不转让 owner。
- 主源变化与项目删除只认 owner_id，平台管理员不能替代。
- 任务 owner/manager 写；术语 owner/manager/reviewer 写；历史成员读、owner/manager 回滚；questioned 由 entry edit capability 修改，hidden overlay owner/manager。
- 平台用户管理严格按秩；API 返回 capabilities，前端不自行推导。

## 6. 性能要点（20 万+ 词条）

- 列表、terms、tasks、admin users 与清理扫描使用 keyset/cursor，禁止大 OFFSET。
- project/file 四个互斥状态与 visible total 物化；questioned 是独立重叠计数；正常详情不实时 COUNT/GROUP BY entries。
- 上传流式解析、每文件原子、batch 部分成功/cancel；不在浏览器或后端整文件内存解析。
- lexical backfill 按 entry id 批次断点；搜索使用 GIN/HNSW 与有界 RRF。
- audit 可按时间分区但保持追加式；job worker 使用租约与 `SKIP LOCKED`。

## 7. 配置、安全与 UI

- DB/Redis/JWT/OAuth/Qwen 与 `PRTS__AI__MASTER_KEY` 等平台密钥只经环境变量，不入库、不下发前端；用户/项目 AI API Key 只保存加密密文与脱敏 hint。
- 运行时非密钥设置包括搜索、上传四项限制、batch 默认 24h 过期、文件默认 30 天保留、删除题型等。
- sqlx 参数化、HTTPS、最小权限、输入/签名/大小校验、稳定错误码和 fail-closed audit。
- 交互 UI 仅 Vue 3 + Quasar，浅/深主题、MDI、方角/2–4px；中文使用 Noto Sans SC 同类 sans；前端 zh-CN/en，后端按 Accept-Language 本地化。

## 8. 部署

```text
nginx ── frontend(Quasar 静态)
      └─ /api,/ws → backend(axum + workers)
backend ─ PostgreSQL 16(pg_trgm+pgvector+zhparser)
        ├ Redis(session/refresh/limit/ws/challenge)
        └ media/upload-temp Docker volumes
```

foundation release 必须把 `0008+0009`、language repair、primary search trigger/backfill 和 worker readiness 作为一个部署门。默认 `deploy/docker-compose.yml` 明确构建无 OAuth 后端并使用 `prts-backend:latest`；需要 ZOOT 时叠加 `deploy/docker-compose.oauth.yml`，构建 `zoot-oauth` 并使用 `prts-backend:oauth-latest`。镜像发布到 GHCR；media 与 upload temp 使用独立持久卷。

## 9. 测试与交付

- `prts-core`：状态机、权限/能力、主源 gate/state、任务 snapshot、术语、数学题纯逻辑。
- API/db：audit fail-closed/redaction、四状态+questioned overlay stats、AI 加密/SSRF/cache scope、术语模式、job 恢复/FK、effective visibility、upload cancel/expiry/attempt、history retention、tagged search scope、purge 顺序。
- 前端：智能按钮、AI 显式解释、译文差异预览、术语详情/模式、移动端布局、IME、capabilities、Markdown 净化、游客只读、上传取消/重试、字体/主题/i18n。
- 静态/自动契约：`scripts/verify-project-workspace.ps1` 检查 Markdown 相对链接、计划最终路径、冲突关键词、冻结迁移、BCP-47 共享入口、OpenAPI、context 清理与新前端兼容交接。
- 手动规模：20 万词条 stats reconciliation/lexical backfill/五 scope/task progress，以及 100MB 流式、500 文件/2GB 合同、replacement/cancel/expiry/purge；只有实际传入开关并保存输出的运行才算实测结果。
- 发布闭环：后端 fmt/clippy/test/db-tests/build，前端 format/lint/test/typecheck/build，verify、规模测试、Docker health 与 Swagger；实际合并 master、GHCR 发布和生产部署前必须经过发布确认。
