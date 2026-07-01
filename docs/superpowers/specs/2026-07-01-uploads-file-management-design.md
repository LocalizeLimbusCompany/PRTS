# 上传改造 + 文件管理 · 工作流 B — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | 大改造 6 工作流之 **B**；前置 **A**（管理分区外壳 + 文件列表组件） |
| 基线 | `master` @ `12101b5`（A spec 提交后） |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | A spec（`2026-07-01-project-page-restructure-design.md`）、CLAUDE.md、蓝图 §14 上传下载 / §5.2 性能 |

> 已与作者确认（含可视化 mockup）：**上传文件 = PRTS 词条数组现格式**（`[{key, original:{lang:text}, translation?, state?}]`，无需格式转换，只把「粘贴」换「传文件」）；支持**多文件 + 整文件夹上传**（保留相对子路径）；文件落**当前所在目录**，路径 = 目录 + 文件名，**重名 = 重传**（沿用现「保留旧译文 + 变动置未翻译 + 记 diff」）；文件管理 = **建夹 / 上传 / 移动 / 重命名 / 删除**（移动用目录选择器）；**文件历史推迟到 P5**（依赖 `audit_log`，蓝图 §8，现仅到 P4）；性能用 **UNNEST** 优化 `bulk_upsert`。

---

## 1. 范围

**做**：把上传从「文本框粘贴 JSON」改为「传文件」（多文件 + 整文件夹）；在 **管理 → 文件管理** 子标签实现文件维护（建夹 / 上传 / 移动 / 重命名 / 删除）；后端补建夹、移动/重命名端点，并用 UNNEST 优化批量入库。

**不做（后续/推迟）**：**文件历史 / 上传批次记录 / 全操作审计 / 按时间清除**（均属 P5 `audit_log`，本阶段仅在 UI 留位、不实现）；上传其它文件格式（PO/CSV/properties——蓝图 §20 未决，暂仅 JSON 词条数组）；拖拽移动（用目录选择器替代）。删除词条 `context` 属 E，不在此。

## 2. 决策要点

1. 上传体不变（词条数组现格式）；前端**去掉 JSON 文本框**，改文件选择/拖拽。
2. 多文件：前端逐文件（顺序或有限并发）POST 现有 `/upload`，逐条显示进度。
3. 整文件夹：`webkitdirectory` 取目录，按 `webkitRelativePath` 保留相对路径，逐文件带相对路径上传（`ensure_file_at_path` 自动建嵌套夹）。
4. 归属：落当前目录，`path = 当前目录 + 文件名`；重名走现有 upsert（保留旧译文、变动置未翻译）。
5. 文件管理操作门 = `project.file.upload`（owner/manager）；删除端点由 `project.manage` **统一改为** `project.file.upload`（二者同为 owner/manager，行为不变）。
6. 移动/重命名 = 改 `folder_id`/`parent_id` + `name`，**重算 path**；移动文件夹**级联重写**所有后代 `folders`/`files` 的 path 前缀；防环（不可移入自身子孙）；`(project_id, path)` 冲突 → 409。
7. 文件历史 UI 位（行 ⋯ 菜单）**本阶段不放**，待 P5。

## 3. 数据模型

**无新迁移**。复用现有 `folders(id, project_id, parent_id?, name, path, UNIQUE(project_id,path))` 与 `files(id, project_id, folder_id?, name, path, entry_count, UNIQUE(project_id,path))`。移动/重命名仅改 `parent_id/folder_id/name/path`，`entries` 引用 `file_id` 不受影响。

## 4. 后端（`prts-api` + `prts-db`，全部进 Swagger）

**上传性能**：重写 `prts_db::entries::bulk_upsert` 用 **UNNEST**——将一批 `key/original/...` 拆成数组参数，一条 `INSERT ... SELECT * FROM unnest(...) ON CONFLICT (file_id,key) DO UPDATE`（分 500–1000 一块，事务内），显著减少往返；保持返回 `created/updated/unchanged` 语义（用 `xmax=0` 或 RETURNING 判定新旧）与「原文变→置未翻译 + 记 `entry_versions`」逻辑。`/upload` 端点签名不变。

**新端点**
- `POST /projects/{id}/folders` `{parent_id?: i64|null, name}` → 建夹：`path = parent.path + "/" + name`（或根级 `name`）；`(project_id,path)` 查重（409）；门 `project.file.upload`。
- `PATCH /projects/{id}/files/{file_id}` `{folder_id?: i64|null, name?}` → 移动/重命名：重算 `path`；查重（409）；门 `project.file.upload`。
- `PATCH /projects/{id}/folders/{folder_id}` `{parent_id?: i64|null, name?}` → 移动/重命名文件夹：**事务内**改本夹 `parent_id/name/path` + 前缀重写所有后代（`UPDATE folders/files SET path = new || substring(path from len(old)+1) WHERE project_id=$ AND (path=old OR path LIKE old||'/%')`）；**防环**（`parent_id` 不得为自身或后代）；查重（409）；门 `project.file.upload`。
- 复用 `DELETE .../files/{id}`、`DELETE .../folders/{id}`（级联删词条），门统一为 `project.file.upload`。

**`prts-db/files.rs`**：`create_folder`、`move_or_rename_file`、`move_or_rename_folder`（含级联 path 重写 + 防环）；复用 `refresh_entry_count`。

## 5. 前端

**`ProjectManageView` → 文件管理子标签**（owner/manager 可见）：
- 复用 A 的**面包屑文件列表组件**，加：勾选多选；工具栏 `新建文件夹` / `上传文件`；选中后 `移动`/`删除`；行尾 ⋯ 菜单 `移动 / 重命名 / 删除`。
- **上传对话框**：多文件选择 + 拖拽 + `整文件夹上传`（webkitdirectory）；提示「上传到：<当前目录>」；预览列表按现有/新增标「新增 / 将更新」；点上传后逐文件进度（顺序或并发上限 3～4）+ 总进度；完成汇总 created/updated/unchanged。
- **新建文件夹对话框**（名称，落当前目录）；**移动对话框**（目录选择器树，选目标夹）；**删除**二次确认（普通确认即可，非删项目那种数学门）。
- **`api`**：新增 `createFolder / moveFile / renameFile / moveFolder / renameFolder`；复用 `upload / deleteFile / deleteFolder`；多文件上传封装一个带进度回调的编排函数。
- i18n 双语；样式少圆角、状态全称。旧 `ProjectDetailView` 的文本框上传随 A 退役移除。

## 6. 性能

- UNNEST 批量 upsert（减少 20w 词条上传往返）；多文件顺序/有限并发编排。
- 文件夹移动的后代 path 重写：单事务两条前缀 `UPDATE`（folders/files），走 `(project_id,path)` 索引。
- verify：20w 词条单文件上传基准（UNNEST 前后对比）；整文件夹（多文件）上传总耗时。

## 7. 测试

- **单元**：path 重算（移动/重命名/建夹）；文件夹移动防环判定；path 前缀重写正确性；上传冲突计数（created/updated/unchanged）不因 UNNEST 改写而变。
- **db-test**：建夹（重名 409）；移动文件（改 folder_id/path、重名 409）；移动文件夹（级联后代 path、防环拒绝、重名 409）；重命名；删除级联；各操作权限门（非 `file.upload` → 403）；`bulk_upsert` UNNEST 版与旧版结果一致（含原文变置未翻译 + entry_versions）。
- **前端**：CI build/lint。

## 8. 涉及文件

`prts-db/files.rs`（create_folder / move_or_rename_file / move_or_rename_folder+级联+防环）、`prts-db/entries.rs`（bulk_upsert 改 UNNEST）；`prts-api/routes/files.rs`（POST folders、PATCH files、PATCH folders、删除门统一）、`mod.rs`、Swagger；前端（`ProjectManageView` 文件管理子标签、上传/新建夹/移动对话框、多文件上传编排、复用文件列表、`api`、`i18n`）；`docs/architecture.md`（补文件管理）。

## 9. 红线 / 未决

- 移动**防环**必检；`(project_id, path)` 唯一冲突返 409；所有维护操作过 `project.file.upload` 门 + 审计留位（P5 补）。
- 上传严格校验 JSON 结构与非空 key；分块事务，避免长事务与内存峰值。
- **未决（实现时定）**：整文件夹/多文件上传的**单次文件数与单文件大小上限**阈值；多文件上传的并发上限取值。文件历史依赖 P5，本阶段不实现。
