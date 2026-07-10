# 项目页重构 · 工作流 A（骨架 + 信息 + 文件）— 校准版设计 Spec

| 项 | 值 |
| --- | --- |
| 历史阶段 | 2026-07-01 大改造工作流 A |
| 校准日期 | 2026-07-10 |
| 规范总纲 | [`2026-07-10-project-workspace-overhaul-design.md`](./2026-07-10-project-workspace-overhaul-design.md) |

> 本文件保留 2026-07-01 的工作流拆分与交互背景。精确主源状态矩阵、发布门、有效可见性和统计真值只在规范总纲 §3–§4 定义；本文不构成第二份生命周期规范。

## 1. 范围

工作流 A 建立项目工作区外壳、只读信息页、文件浏览、下载和项目设置，并提供后续 B–F 使用的路由、文件浏览器、能力 DTO、物化统计、主源语言与媒体接口。

固定分区顺序为 `信息·文件·任务·术语·排行榜·下载·管理`。任务与术语在 C/D 完成后再开放，开放时必须是可用成品；排行榜是本轮唯一明确保留的功能占位。编辑器保持独立全屏路由。

旧项目页的上传与其它已存在能力在替代入口就绪前继续可用，不能因拆 Shell 而回退。

## 2. 已确认决策

1. `/projects/:id` 默认进入 `info`；信息页只读，编辑集中在 `manage`。
2. 编辑器从文件或任务文件进入 `/projects/:id/editor?file=<id>`，不嵌套在 Shell。
3. slug 可编辑、保持唯一，不进入项目 URL；下载文件名使用 slug。
4. 项目简介保存 Markdown 源文并通过共享净化组件展示。
5. 交互 UI 只用 Vue 3 + Quasar，保留浅/深主题，图标统一 MDI，普通控件为方角或 2–4px 小圆角。
6. 中文 UI 只使用 Noto Sans SC 同类无衬线字体；JetBrains Mono 仅用于代码、键和数字，并以 CJK sans 承接中文；不使用宋体或其它衬线 UI。
7. 前端完整 zh-CN/en，API 请求携带当前 locale 的 `Accept-Language`。
8. 公共项目游客可浏览文件并进入只读编辑器；游客不能保存、改状态或发起协作动作。私有项目限获授权主体。

## 3. 主源语言

- ready 项目的 `source_langs`、`primary_source_lang` 与 `target_lang` 先经共享 `language-tags` canonicalizer：language 小写、script Titlecase、region 大写，variant/extension/private-use 按 parser 规范序列化；无效 tag 与规范化后重复拒绝。primary 是非空 canonical BCP-47 且必须属于最终 `source_langs`。单源创建自动选择唯一值；多源创建必须显式提供。legacy unresolved 行只允许在 repair state 条件约束下暂存，不能进入普通管理/search。
- 不存在 `null → source_langs[0]` 的永久回退规则。旧项目只在迁移时以当前首个源语言回填一次。
- `0008_workspace_meta_stats.sql` 与 `0009_primary_source_search.sql` 必须随同一 foundation release 部署；legacy project arrays/target/primary、entry original keys、term source_lang 与用户语言偏好先由 durable batched repair canonicalize。冲突/无效数据进入 `needs_language_resolution`，search 与普通语言 edits 保持 gated；owner 用专用 UI/API 解决，platform admin 只有无正文诊断/retry。trigger/function、repair-ready backfill/reconciliation 与 lexical worker readiness 完成前，不开放非首主源或更新路由。任何独立发布都不得让 API 使用新字段而 search 仍读取 `source_langs[1]`。
- 已有项目只有 `projects.owner_id` 能更改主源；平台管理员不能代替。相同值直接成功，不触发 7 天冷却或新 job。
- 真正变化从请求被接受时开始 7 天冷却；下一次变化还要求没有 active/unresolved failed lexical 或 embedding job。失败只重试原阶段 job。
- 移除当前主源时必须在同一保存提交替代主源。已有词条的项目不得修改目标语言。
- 接受变化后立即切换主源并暂停 search/TM。独立 lexical job 完成后恢复 FTS/trgm，再创建/运行 embedding job；provider 禁用/未配置标 degraded/skipped，不阻塞词法。配置了 provider 的失败有界退避，耗尽后手动重试同一 embedding job。
- 项目/job API 按规范总纲 §4.3 返回两个阶段各自的状态、job id、进度、重试和错误/降级原因。D 落地后，同一事务还会归档旧主源术语并激活新主源术语。

## 4. 头像、文件浏览与统计

### 4.1 头像

- 使用 `MediaStore` + 本地 Docker volume；默认 key 为 `projects/{id}/avatar.webp`。
- 前端 Quasar 对话框配合原生 canvas 做 1:1 裁剪，目标输出 256×256 WebP。
- 服务端校验真实签名与可解码内容，要求正方形、宽高各 `<=1024`、总像素 `<=1,048,576`、编码体积 `<=512KB`，不信任 MIME/扩展名。
- 公开项目头像公开读取；私有项目头像遵循项目可见性与认证，前端以鉴权 blob 展示。
- 写入、替换和移除都写追加式审计；项目清除时清理媒体。

### 4.2 文件浏览器

- 面包屑 + 当前目录单栏；文件夹在前；支持名称搜索、名称/进度/词条数/最近时间排序和状态筛选。
- 文件夹统计由前端聚合后代文件物化统计。文件夹最近时间为后代文件 `updated_at` 最大值；空文件夹使用 `created_at`。
- 点文件进入编辑器；维护操作由 B 在管理区加入。

### 4.3 统计口径

- `project_stats` 与 `file_stats` 按规范总纲 §3 的 `effective_visible` 物化可见总数和五状态计数。详情和 tree 正常读路径禁止实时扫描 entries 做 `COUNT(*)`/`GROUP BY`。
- 普通统计同时排除 hidden、entry tombstone、deleted file 和 deleted ancestor folder。`include_hidden` 不穿透删除；file/folder restore 也不清除 reupload tombstone。
- 总数为零显示“—”。进度仍定义为 `(visible_total - visible_untranslated) / visible_total`，状态分段使用五种完整状态名。

## 5. API 与前端边界

### 后端

- 项目 DTO 增加 `primary_source_lang`、搜索重建状态、头像元数据、物化统计和 `capabilities`。
- `POST/DELETE/GET /projects/{id}/avatar` 全部进入 Swagger。
- 项目更新在事务内校验 slug、canonical 语言、owner-only 主源变更、目标语言限制并写审计；`needs_language_resolution` 只允许 owner resolution endpoint，不允许普通 update 旁路。
- `GET /projects/{id}/tree` 返回文件物化统计和时间，不计算文件夹聚合。
- 所有能力由服务端计算；`change_primary_source` 只给 owner_id，`include_hidden` 只给 owner/manager。

### 前端

- `ProjectShell.vue` 承载嵌套路由和项目上下文；窄屏使用 Quasar drawer/menu。
- `ProjectInfoView.vue` 展示头像、语言、净化 Markdown、物化进度和统计。
- `ProjectFilesView.vue` 使用共享 `ProjectFileBrowser`；`ProjectDownloadView.vue` 保持导出。
- `ProjectManageView.vue` 编辑项目设置并展示 language repair/resolution、主源冷却/重建进度；owner resolution dialog 显式选择 canonical mapping/冲突值，控件只依据 capabilities 显隐或禁用。
- locale 持久化到 `prts.locale`，初始化顺序为 localStorage → 浏览器 → zh-CN。

## 6. 验收

- foundation 发布门、主源创建/更新、7 天冷却、lexical/embedding 分阶段重试、目标语言锁定均有单元与数据库测试。
- 头像覆盖伪 MIME、错误尺寸、大小、公开/私有读取和替换删除生命周期。
- 随机状态/hidden/deleted 变化后，物化统计等于离线校验值；项目详情 SQL 不以 entries 聚合为正常读取。
- 路由、浅/深主题、MDI、字体、zh-CN/en、Markdown 净化、游客只读与 capability 显隐有前端测试。
- 阶段结束执行测试、verify、Conventional Commit、推 master、等待 CI 与 GHCR。
