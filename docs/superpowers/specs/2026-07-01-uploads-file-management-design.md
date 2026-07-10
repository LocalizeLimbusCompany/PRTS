# 上传改造 + 文件管理 · 工作流 B — 校准版设计 Spec

| 项 | 值 |
| --- | --- |
| 历史阶段 | 2026-07-01 大改造工作流 B |
| 校准日期 | 2026-07-10 |
| 规范总纲 | [`2026-07-10-project-workspace-overhaul-design.md`](./2026-07-10-project-workspace-overhaul-design.md) |

> 2026-07-01 版本确定了多文件/文件夹上传、相对路径和文件维护方向。本文件保留上传/文件 UI 细节；精确 batch/attempt 状态、删除继承、恢复和历史保留真值以规范总纲 §3、§5 为准。

## 1. 范围

工作流 B 完成原始 JSON 文件/文件夹上传、持久化 batch 进度、同路径完整替换、建夹/移动/重命名/软删除/恢复、成员可读历史、owner/manager 回滚，以及删除保留期清理。

最终 UI 不提供粘贴 JSON 文本框。旧 `/projects/{id}/upload` 在新链路稳定后仍保留一个兼容周期并标 deprecated；旧控件只在替代 UI 可用后移除。

生命周期边界：`0008` 只预建 nullable deletion columns，foundation 的 legacy delete 仍硬删除、维护 stats、deletion_change_set_id 全 NULL且无 restore/history。`0010` 先断言全 NULL，再创建完整 history/FK 并与 soft-delete writer 同 release 原子切换；不得伪造 backfill payload。

## 2. 上传会话

### 2.1 运行时限制与路径

- 每 batch 500 文件、每文件 100MB、每 batch 2GB、客户端/浏览器并发 3 四项均为平台数据库运行时设置及默认值。`GET/PUT /admin/settings/upload` 读写四项，普通上传客户端通过 `GET /meta/upload-config` 的只读 `UploadConfigDto` 获取当前值；并发不能是前端固定常量。
- 浏览器不读取或解析内容，只上传原始 `File` 流。文件夹上传保留相对路径，并以用户当前文件夹为根。
- 服务端规范化路径，拒绝绝对路径、`..` 越界、空段、保留段和同项目 path 冲突。
- V1 不支持 Range/offset 续传。断流或失败后在同一 logical batch file 下创建新 attempt，并从 byte zero 重新上传；旧 attempt/error 保留。
- 原始文件写入 upload temp 持久卷；成功处理后立即清理，取消/失败/过期由 durable cleanup 幂等清理。未完成或 abandoned batch 默认 24 小时过期（可运行时配置）；原始文件不进入项目历史或导出。

### 2.2 batch API

1. `POST /projects/{id}/upload-batches` 声明目标目录、相对路径和字节数，校验三项限制。
2. `PUT /projects/{id}/upload-batches/{batch_id}/files/{file_id}/attempts/{attempt_id}` 从 byte zero 流式接收单文件，不把全文件装入内存，也不接受 Range/offset。
3. `POST /projects/{id}/upload-batches/{batch_id}/complete` 校验全部已声明文件并为每文件排持久化 job。
4. `GET /projects/{id}/upload-batches/{batch_id}` 返回 batch 与逐文件阶段/进度/结果。
5. `POST .../files/{file_id}/retry` 复用 logical processing job，但创建新的 per-file attempt 并返回 byte-zero PUT 目标。
6. `POST /projects/{id}/upload-batches/{batch_id}/cancel` 进入 `cancelling`，取消 queued/temp items；已进入单文件事务的处理允许原子完成或回滚，全部 active attempts 终止后进入 `cancelled`。

batch 状态使用 `draft|uploading|queued|processing|cancelling|cancelled|partially_succeeded|succeeded|failed|expired`；attempt 记录上传/排队/处理/成功/失败/取消/过期及错误历史。每文件独立原子，batch 允许部分成功；取消不撤销已成功文件。

### 2.3 解析与重复 key

- 后台从 temp 文件流式解析 PRTS JSON 数组，按批写事务临时表，再以集合 SQL 应用替换。
- 临时表保存数组 ordinal 并对 key 唯一。重复 key 拒绝该文件，错误返回 key、首次位置、重复位置，以及 parser 可提供的行列。
- 每个 `original` JSON key 通过共享 `language-tags` canonicalizer；无效 BCP-47、规范化后重复 key（无论值是否相同）或不属于项目 canonical source set 都拒绝该文件并返回位置。语法、结构、state 或空 key 错误同样只失败该文件。所有错误使用稳定 code 和本地化 message。

## 3. 同路径完整替换

replacement 真值由 `prts-core::upload_replacement` 产生 typed transition plan；DB adapter 执行 plan，API worker 只编排。translation preserve、source-change reset、tombstone/restore 与 stats/history delta 不得重复实现于 SQL/handler。

- 同一路径定位同一个平台 file；上传语义是完整 replacement，不是出现 key 的增量 patch。
- 上传缺失的旧 key 标记 `deleted_at`，可从历史恢复；再次出现时恢复同一 entry 身份与历史。
- 已存在 entry 的平台 translation、locked、hidden 与历史保留。
- original 变化时保留 translation，但 state 重置 `untranslated`；original 未变时保留当前 state。
- 上传体的 translation/state 只 seed 平台从未存在过的新 key。软删除后恢复的旧 key 不被上传值覆盖。
- 单文件成功事务同时写 entry/file delta、物化统计、文件版本和 audit；任何一步失败整文件回滚。

## 4. 文件与文件夹管理

- owner/manager 可建夹、移动、重命名、删除、恢复；前端依据 `manage_files` capability，而不是角色字符串。
- 文件夹移动禁止移入自身或后代，事务内重算全部后代 path，并在写入前检查目标冲突。
- 删除文件夹时，同一事务把 active folder subtree 与 descendant files 标记为同一 `deletion_change_set_id`；恢复只清除此操作 ID 标记的行，不能恢复此前已删除后代。
- 删除使 descendant files 从项目/任务 exposure、普通搜索与导出退出，但不修改 entry tombstone。默认保留 30 天，平台可配置；保留期内只加回实际恢复 files 的物化统计。
- 上传、建夹、移动、重命名、软删除、恢复、purge 均写追加式 audit。

保留期内 restore 锁定 change set/tree，只清匹配 operation 的 `deleted_at/deleted_by/purge_after/deletion_change_set_id`。成功恢复后 change set 继续作为普通历史保留，不能因清除 marker 就删掉历史 payload。

## 5. 行为完整历史

move/delete/restore/rollback 由 `prts-core::file_history` 纯规则生成 typed plan；repository port 隔离 DB，handler 不拥有领域真值。

### 5.1 模型

- `file_change_sets`：`id UUID`（与 deletion operation id 同型）、project、nullable file/folder、递增版本、kind、actor、来源 change set、时间。
- `file_change_items`：entity type/id、稳定顺序、before JSONB、after JSONB。entry payload 从首次发布即严格 allowlist `key/original/translation/state/locked/hidden/deleted_at`，不得捕获 `context`；结构实体使用独立明确 allowlist。
- 上传 replacement 记录新增、恢复、源文变化和缺失删除；结构操作记录 path/tree 前后状态。

行为 delta 在文件/文件夹存活期间以及软删除保留期内保存，不保存原始上传文件。精确关系为 `files/folders.deletion_change_set_id REFERENCES file_change_sets(id) ON DELETE RESTRICT`，而 `file_change_sets.file_id/folder_id` 可空并分别 `ON DELETE SET NULL`。因此仍有可恢复业务行时数据库会阻止先删 restoration payload。

到期永久 purge 锁定 change set/tree 和所有以待清除实体为 target 的 restoration-bearing sets，按叶到根删除 descendant entries、files、folders；target FK 自动置 NULL 且 relationship references 消失后，显式删除这些 sets 的 `file_change_items`，最后删除对应 `file_change_sets`。此后不能 rollback/restore；audit 仅按其 retention/项目策略保留无恢复正文的元数据。项目成员可查看历史；只有 owner/manager 可回滚或恢复。

### 5.2 回滚

- 服务端把选定历史版本物化为目标状态，再生成 current → target 的新 change set。
- 回滚本身成为新可逆版本，不覆盖、删除或重排旧历史。
- 回滚/恢复固定 0 CP，不追回旧 CP；所有写入仍执行 path 冲突、权限、审计和统计维护。

## 6. 数据与实现边界

计划迁移 `0010_upload_file_history.sql` 一次性增加：

实现迁移前先写整阶段 schema contract RED，覆盖全部表、CHECK、FK、retention/purge indexes 与 deletion ids 全 NULL前置；后续行为任务不得回改冻结迁移。

- `upload_batches`、`upload_batch_files`、持久化 `upload_file_attempts`（保存每次 byte-zero retry 的 attempt_no、阶段、字节数、temp key、错误与时间/结果历史）；
- `file_change_sets`、`file_change_items`；
- files/folders 复用 foundation 已建立的 `deleted_at/deleted_by/deletion_change_set_id`，本迁移补 `purge_after`；为 deletion_change_set_id 增加指向 `file_change_sets(id)` 的 RESTRICT/NO ACTION FK，为 `file_change_sets.file_id/folder_id` 增加 nullable `ON DELETE SET NULL` FK；
- 支撑 active path、待清理扫描和历史版本的索引。

worker 使用 `FOR UPDATE SKIP LOCKED` 与租约；重启从同一 job 恢复。文件进入项目待删除状态时，upload 与普通 purge 暂停。

上传配置使用类型化 `UploadConfig`/`UploadConfigDto` 贯穿 settings 存储、管理端 GET/PUT、只读 meta API 与前端上传 composable；服务端与浏览器消费同一份当前配置。

## 7. 前端

- `UploadBatchDialog` 支持多文件、拖拽和 `webkitdirectory`；只持有元数据/File handle，不调用 `File.text()` 或 `JSON.parse()`。
- 显示总进度及每 file attempt 的上传、排队、解析、提交、成功、失败/取消/过期；失败文件可从 byte zero 单独重试，batch 可取消。
- 文件管理工具栏与行菜单提供新建、移动、重命名、删除、恢复、历史和回滚。
- 删除说明明确 30 天恢复期；回滚说明明确会产生新版本且不影响历史 CP。
- 全部文案覆盖 zh-CN/en，使用 Quasar/MDI、小圆角和共享 capability。

## 8. 验收

- 限制边界、路径越界、无 offset resume、byte-zero retry、cancel race、24h expiry/cleanup、部分成功、重复 key 位置均有 API 测试。
- replacement 矩阵覆盖新 key、源文不变/变化、缺失、恢复、上传 seed、hidden/locked 保留。
- 文件夹移动覆盖防环、全后代 path、冲突回滚；删除/恢复覆盖 30 天、restore 后 change-set 保留、RESTRICT 防误删 payload、叶到根 purge 顺序和统计口径。
- 回滚到任意历史版本后再回滚仍可恢复，且每次产生新 change set、0 CP、完整 audit。
- 100MB 文件与 20 万词条 verify 证明不做浏览器解析、不整文件载入后端内存、单文件失败不污染线上状态。
- 阶段结束执行测试、verify、Conventional Commit、推 master、等待 CI 与 GHCR。
