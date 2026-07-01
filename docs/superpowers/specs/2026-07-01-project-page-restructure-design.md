# 项目页重构 · 工作流 A（骨架 + 信息 + 文件）— 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | 大改造 6 工作流之 **A**（地基）；后续 B 上传/文件管理 · C 任务 · D 术语 · E 编辑器 · F 平台杂项 |
| 基线 | `master` @ `6b19657` |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | CLAUDE.md、`plan/26-06-28-init_system.md`、`docs/architecture.md` |

> 已与作者确认（含可视化 mockup 逐张过）：**骨架方案 A**（左侧栏含身份，默认落地「信息」）；分区顺序 `信息·文件·任务·术语·排行榜·下载·管理`；**编辑器不占分区**，从文件页点文件进入；**信息页只读**，一切编辑集中「管理→项目设置」；**主源语言**限源语言之一（新字段 `primary_source_lang`）；**头像上传到磁盘存储**（`MediaStore` 可插拔，本地磁盘 + Docker 卷）；**文件浏览器方案 1**（面包屑单栏），状态筛选按「是否仍含某状态词条」，排序 名称/进度/词条数/最近更新；**slug 可编辑**（不在 URL 内，仅展示 + zip 文件名，校验唯一即可）。样式：**少用圆角**、词条状态**用全称**（未翻译/已翻译/有疑问/已检查/已审核）、计数用「万」。

---

## 1. 范围

**做**：把当前单页 `ProjectDetailView` 拆成「侧栏骨架 + 分区」的项目工作区外壳，落地其中 4 个分区（信息 / 文件 / 下载 / 管理→项目设置）+ 2 个占位（任务 / 排行榜，术语分区亦占位）。新增 2 个项目字段（主源语言、头像）与其所需的媒体存储、每文件进度数据。另附带全局顶栏**界面语言切换器**（与 A 的 i18n/外壳同批做）。

**不做（属后续工作流，本阶段仅占位或不动）**：任务功能实现（C）、术语功能实现（D）、管理→成员管理 / 文件管理（B/E）、排行榜真实榜单（P6）、编辑器改造与搜索重构（E）、删除词条 context（E）、平台用户管理与删项目数学门（F）、字体回退修复（F）。**编辑器 `/editor` 本阶段不改**，仅调整「从文件页进入」的入口与传参。

## 2. 决策要点（已确认，逐条落地）

1. 外壳 = 左侧栏（顶部小头像＋项目名＋语言对）+ 右侧分区内容区；默认 `信息`。
2. 分区固定 7 项，顺序 `信息·文件·任务·术语·排行榜·下载·管理`；`任务/术语/排行榜` 渲染占位（「该功能在后续阶段实现」）。
3. 信息页**只读**；无「编辑信息」按钮。
4. 编辑器从**文件页点文件**进入（`/projects/:id/editor?file=<fileId>`）；侧栏不放翻译直达。
5. `主源语言 primary_source_lang`：限 `source_langs` 之一；`null` 时隐含取 `source_langs[0]`（与现搜索一致）。
6. 头像：上传**磁盘存储**，`MediaStore` trait（本地磁盘实现，配置化 media 根目录 + Docker 卷），日后可换对象存储；未设时前端按项目名首字生成默认块。
7. 文件浏览器**方案 1**：面包屑 + 当前目录单栏列表；搜索文件名/目录名（前端过滤，命中即拍平显示完整路径）；排序 名称/进度/词条数/最近更新；状态筛选「含某状态词条」= 全部 / 含未翻译 / 含有疑问 / 已译完 / 已审完；每文件与每文件夹显示进度；点文件进编辑器。维护操作（建夹/上传/移动/删除/历史）**不在此**，留「管理」。
8. slug 可编辑，校验唯一。
9. 样式令牌：小圆角；状态标签全称；数字用「万」；进度定义沿用 `进度 =（总数 − 未翻译）/ 总数`，分段条按 5 状态。

## 3. 前端设计

**路由（hash，数字 id 不变）**：`ProjectShell` 承载嵌套子路由——
- `/projects/:id(\d+)` → **302 到 `…/info`**（兼容旧书签；旧 `ProjectDetailView` 退役后此路径仍可达）
- 子路由：`info` `files` `tasks` `glossary` `leaderboard` `download` `manage`（`<router-view>` 挂在 Shell 内容区）
- `/projects/:id(\d+)/editor` 保持独立**全屏**路由（不套 Shell），由文件页跳入并带 `?file=`。

**组件**
- `ProjectShell.vue`：拉取 project 详情（复用 `GET /projects/{id}`）；侧栏身份 + 导航；响应式——窄屏侧栏折叠为顶部下拉/抽屉。分区高亮由当前子路由决定。
- `ProjectInfoView.vue`（只读）：头像（`GET …/avatar` 失败→默认块）+ 名称 + 公开/私有；语言行（源语言 chips，主源标 `主源` → 目标）；简介；总进度（分段条 + 全称图例 + 「万」计数 + 「进度 X%」头条）；统计卡（文件/词条/成员；术语数待 D，本阶段不显示）。
- `ProjectFilesView.vue`（方案 1）：顶部搜索框 + 排序下拉 + 状态筛选下拉；面包屑；当前目录行列表（文件夹在前，含聚合进度；文件含进度条 + 词条数）；点文件夹下钻/面包屑返回；**搜索态**拍平全树匹配项显示完整路径。全部前端过滤/排序，数据来自扩展后的 tree 接口。空态/加载态。
- `ProjectDownloadView.vue`：迁移现有导出（`GET …/export` → `{slug}.zip`），保留下载按钮 + 说明。
- `ProjectLeaderboardView.vue` / `ProjectTasksView.vue` / `ProjectGlossaryView.vue`：占位组件（统一「后续阶段实现」空态）。
- `ProjectManageView.vue`：子标签 `项目设置`（本阶段）·`成员管理`·`文件管理`（后二者占位）。**项目设置表单**：头像（上传/移除 + 提示 ≤512KB、PNG/JPG/WebP、方形建议、客户端预压缩）、名称、slug（可编辑，失焦查重）、简介、可见性（公开/私有）、源语言（多 chip 增删，BCP-47）、主源语言（下拉，选项 = 当前源语言）、目标语言（单，BCP-47）、保存/重置。仅 `project.manage` 可见可用。
- 现 `ProjectDetailView.vue` 拆解退役：文件树逻辑迁 `ProjectFilesView`，进度/统计迁 `ProjectInfoView`，导出迁 `ProjectDownloadView`，删除/上传/成员相关入口迁「管理」（占位挂钩，B/E 实现）。

**附带 · 界面语言切换器（全局顶栏）**：`App.vue` 顶栏右侧加 🌐 下拉（变体 2，可扩展多语言），切换即时设 `i18n.global.locale` + 存 `localStorage`（键 `prts.locale`；初始化读取，缺省跟随浏览器→回落 zh-CN）；API 客户端按当前 locale 带 `Accept-Language`，后端本地化消息同步。

**i18n**：新增分区/字段/状态筛选/排序文案，zh-CN + en 双语。**样式**：抽 `--radius`（小）与状态色令牌到 `theme.scss`，状态标签常量统一全称。

## 4. 后端设计

**迁移 `0007_project_meta.sql`**
```
ALTER TABLE projects
  ADD COLUMN primary_source_lang TEXT,               -- 须 ∈ source_langs；null=隐含 source_langs[0]
  ADD COLUMN avatar_path         TEXT,               -- MediaStore 相对键，含扩展名；null=未设
  ADD COLUMN avatar_updated_at   TIMESTAMPTZ;         -- 作 ETag / 缓存击穿
```
（`primary_source_lang` 的「∈ source_langs」在应用层校验：数组成员随 source_langs 变动，跨列 CHECK 维护成本高。）

**每文件进度**：扩展 `GET /projects/{id}/tree`，文件节点新增 `state_counts: {state→count}`（一次 `SELECT file_id, state, COUNT(*) FROM entries WHERE project_id=$1 GROUP BY file_id, state`，与项目级 state_counts 口径一致——计全部词条）。文件夹聚合在前端做。索引：现有 `entries(file_id)` 足够；必要时加 `entries(project_id, file_id, state)`。20w 量级单次分组扫描可接受，无需实时 COUNT 热路径。

**媒体存储**：`MediaStore` trait（`put/get/delete(key)`）+ `LocalDiskStore`（根目录来自 `prts-common` 配置 `PRTS__MEDIA__DIR`，默认 `./data/media`）。放 `prts-api::media` 模块（暂不新起 crate；日后接对象存储再提升）。avatar key = `projects/{id}/avatar.<ext>`。

**端点（`prts-api`，全部进 utoipa/Swagger）**
- `POST /projects/{id}/avatar`（`project.manage`；multipart 单图；校验 MIME ∈ {png,jpg,webp} 且 ≤512KB；写 MediaStore + 置 `avatar_path/avatar_updated_at`）。
- `DELETE /projects/{id}/avatar`（`project.manage`；删文件 + 清列）。
- `GET /projects/{id}/avatar`（**公开读**：流式返回图 + `Cache-Control` + `ETag=avatar_updated_at`；未设→404，前端回落默认块。取公开是因 `<img>` 不便带 JWT，且头像本身低敏；若作者要私有项目头像也鉴权，则前端改带鉴权 blob 拉取——见 §9）。
- 扩展 `PUT /projects/{id}`：接受 `primary_source_lang`（校验 ∈ 最终 `source_langs`，否则 400）与 `slug`（变更时查重，冲突 409）。
- axum 启用 `multipart` feature（无重依赖）；上传大小限流。
- **跨域/部署**：`<img>` 跨源加载头像无需 CORS，但 nginx 须正确反代该端点并回传 `Content-Type`/缓存头；若日后前端改 `fetch` blob 则需 `Access-Control-Allow-Origin`。P0 现放开 CORS，P7 收紧时白名单勿漏头像端点。

**审计**：`audit_log` 属 P5 尚未落地——本阶段编辑/上传处**预留** audit 调用点（TODO 注释），P5 接入时补。

## 5. 权限

- 分区可见性：信息/文件/下载/排行榜 = 任何可读该项目者（私有项目限成员）；管理 = `project.manage`（拥有者/管理）。
- 头像写、项目设置写 = `project.manage`；头像读随项目可见性。
- 沿用现 `crate::auth::project::load` + 权限节点，不新增节点。

## 6. 性能

- 每文件进度：单次 `GROUP BY file_id,state`，走索引；文件数级别结果（百级），前端聚合文件夹。
- 文件浏览器搜索/排序/筛选**纯前端**（tree 一次性加载，百级文件可控）；不引入新分页。
- 头像小图（≤512KB，客户端预压缩至 ≤256px），流式返回 + 缓存。

## 7. 测试

- **单元**：slug 唯一校验；`primary_source_lang ∈ source_langs` 校验（含 source_langs 收缩使原主源失效的分支）；`LocalDiskStore` put/get/delete；每文件进度聚合函数；进度百分比与分段计算。
- **db-test**：`PUT /projects` 带主源语言/改 slug（唯一冲突 409）；tree 返回 per-file `state_counts`；头像 POST→GET→DELETE 生命周期与权限（非 manage 403、私有项目非成员读 403/404）。
- **前端**：CI build/lint；关键组件渲染与响应式退化（交 CI）。

## 8. 涉及文件

迁移 `0007_project_meta.sql`；`prts-common`（media 配置）、`prts-api`（`media` 模块、`routes/projects.rs` 头像三端点 + PUT 扩展、tree 扩展、`mod.rs`、Swagger）、`prts-db/projects.rs`（新列读写、slug 查重、per-file 进度查询）；`deploy/`（media 卷 + `.env.example` `PRTS__MEDIA__DIR`）；前端（`router` + 旧路由 302、`ProjectShell` + 6 分区视图、`ProjectDetailView` 退役拆解、`App.vue`（语言切换器）、`i18n/index.ts`（locale 持久化/初始化）、`api`（avatar/tree 类型 + `Accept-Language`）、`theme.scss` 圆角/状态令牌）；`docs/architecture.md`（补 §项目工作区）。

## 9. 红线 / 未决

- 密钥/媒体根仅经 env；上传严格校验类型与大小；sqlx 参数化；私有项目资源鉴权。
- 不用大 OFFSET、不加实时 `COUNT(*)` 热路径；进度用一次性分组。
- **已确认（原未决）**：media 根 `./data/media` + Docker 卷；头像**公开读、不鉴权**（注意跨域，见 §4）；`/projects/:id` **302→`…/info`** 兼容旧书签。实现中如遇其它细节不明再问作者（蓝图 §8）。
