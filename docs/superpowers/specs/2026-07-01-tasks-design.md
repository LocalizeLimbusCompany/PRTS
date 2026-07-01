# 任务（Tasks）· 工作流 C — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | 大改造 6 工作流之 **C**；前置 **A**（任务分区外壳 + 文件列表组件 + 每文件状态计数） |
| 基线 | `master` @ `d0177bc`（B spec 提交后） |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | A spec、B spec、CLAUDE.md |

> 已与作者确认（含可视化 mockup）：任务 = 一组项目文件 + 标题 + **Markdown 介绍** + 进度；进度用**快照基线**——加入文件时快照该文件「未翻译数」为分母，`进度 = Σ max(0, 基线未翻译 − 当前未翻译) / Σ 基线未翻译`；文件**可属多任务**（多对多）；**独立「管理任务」界面**统一设 标题/介绍/增删文件（勾选项目文件加入、勾文件夹=其下整体、加入即快照）；任务详情的文件用**文件管理器样式**展示（下钻、点文件进编辑器）；管理权限**复用 `project.manage`**（owner/manager），其余成员只读；创建流程 = 新建 → 进管理界面 → 保存。

---

## 1. 范围

**做**：任务分区（列表 / 详情 / 管理界面）；`tasks` + `task_files` 数据模型与端点；快照基线进度计算；Markdown 介绍（渲染 + 编辑预览）；任务文件的文件管理器式展示与勾选式增删。

**不做（后续/别处）**：「在此任务翻译」的**搜索/过滤**实现（属 **E**，本阶段仅留入口 + 传 `task_id`）；任务与 CP/排行榜关联（无）；任务级新权限节点（复用 `project.manage`）；任务的历史/审计（P5）。

## 2. 决策要点

1. 进度 = 快照基线：`task_files.baseline_untranslated` 加入时快照；`完成 = Σ_file max(0, baseline − 当前未翻译)`；`进度 = 完成 / Σ baseline`（`Σbaseline=0` → 视为 100%）。当前未翻译 = 该文件 `state='untranslated'` 实时计数（复用 A 的每文件状态计数口径）。
2. 文件↔任务**多对多**（`task_files` 联结表，主键 `(task_id, file_id)`）。
3. 「管理任务」界面统一保存：标题 + Markdown 介绍 + 文件成员集合（勾选）。保存以**期望成员集合**协调：新增的快照基线、取消的移除、既有的**保留原基线**。
4. 权限：查看=可查看项目者；建/改/删/增移文件 = `project.manage`。
5. Markdown 介绍：存**源文**（TEXT），前端 `markdown-it` 渲染 + `DOMPurify` 净化。

## 3. 数据模型 · 迁移 `0008_tasks.sql`

```
tasks(
  id BIGINT IDENTITY PK,
  project_id BIGINT NOT NULL REFERENCES projects ON DELETE CASCADE,
  title TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',      -- Markdown 源文
  created_by BIGINT REFERENCES users ON DELETE SET NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
)
task_files(
  task_id BIGINT NOT NULL REFERENCES tasks ON DELETE CASCADE,
  file_id BIGINT NOT NULL REFERENCES files ON DELETE CASCADE,
  baseline_untranslated INTEGER NOT NULL,    -- 加入时快照
  added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (task_id, file_id)
)
索引：tasks(project_id)；task_files(file_id)（供「文件删除级联」与反查）。
```
文件删除 → `task_files` 级联移除（该文件退出所有任务）。

## 4. 后端（`prts-api` + `prts-db`，进 Swagger）

**`prts-db/tasks.rs`**：`create`、`get`、`list_with_progress`、`update_meta`、`set_files`（协调成员：新增项 `baseline = 当前未翻译计数`，移除缺席项，保留既有基线）、`delete`、`list_files_with_progress`（任务详情用：每文件 baseline + 当前未翻译 + 完成度）。

**进度查询**（列表一条 SQL 汇总）：
```sql
SELECT t.id, COALESCE(SUM(tf.baseline_untranslated),0) AS base,
       COALESCE(SUM(GREATEST(0, tf.baseline_untranslated - COALESCE(cur.untrans,0))),0) AS done
FROM tasks t
LEFT JOIN task_files tf ON tf.task_id = t.id
LEFT JOIN (SELECT file_id, COUNT(*) FILTER (WHERE state='untranslated') AS untrans
           FROM entries WHERE project_id=$1 GROUP BY file_id) cur ON cur.file_id = tf.file_id
WHERE t.project_id=$1 GROUP BY t.id;
```

**端点**
- `GET /projects/{id}/tasks` — 列表（title、description、base、done、file_count）。可查看项目者。
- `POST /projects/{id}/tasks` `{title?, description?}` — 建空任务，返回 id（`project.manage`）。
- `GET /projects/{id}/tasks/{task_id}` — 详情（meta + 进度 + 文件列表，每文件 baseline/当前未译/完成度）。
- `PUT /projects/{id}/tasks/{task_id}` `{title, description, file_ids:[i64]}` — 统一保存（改 meta + 协调成员）。`project.manage`。
- `DELETE /projects/{id}/tasks/{task_id}` — 删除（`task_files` 级联）。`project.manage`。
- 「在此任务翻译」不新增端点：前端带 `task_id` 进编辑器，过滤逻辑属 E（E 的搜索支持「在当前任务」）。

## 5. 前端

**路由**：`/projects/:id/tasks`（列表）、`/projects/:id/tasks/:taskId`（详情浏览）、`/projects/:id/tasks/:taskId/manage`（管理，owner/manager）。新建 → `POST` → 跳 `…/manage`。

- **`TaskListView`**：任务卡（标题、介绍摘要、单色进度条 = done/base、文件数）+ `新建任务`（manager）。
- **`TaskDetailView`**（浏览）：标题 + **渲染 Markdown 介绍** + 进度条（`done/base`，`base=0`→100%）+ 文件**文件管理器样式**（复用 A 面包屑列表，数据=任务文件子集；每文件显示 基线/当前未译/完成度；点文件进编辑器）+ `管理任务`（manager）/`在此任务翻译` 入口。
- **`TaskManageView`**（manager）：标题输入；**Markdown 介绍**编辑器（源 + 实时预览）；**文件勾选**——复用 A 面包屑文件浏览器加勾选框，勾=加入（保存时对新增项快照基线）、勾文件夹=其下文件整体加入、取消=移出；`保存`（PUT）/`删除任务`。
- **`api` tasksApi**：list/create/get/update(含 file_ids)/remove。
- **依赖**：`markdown-it` + `dompurify`（轻量、标准）用于介绍渲染；封装 `<MarkdownView>` / `<MarkdownEditor>` 组件（术语 D 若需也可复用）。
- i18n 双语；样式少圆角、状态全称；进度条单色（完成比例，非 5 状态）。

## 6. 性能

- 列表进度一条聚合 SQL（按 `entries(project_id)` + `file_id` 分组）；任务数量级小，可接受，无需物化。
- 详情每文件进度同口径；文件浏览器前端过滤（任务文件子集，规模小）。
- `set_files` 协调在单事务内完成（新增快照 + 删除）。

## 7. 测试

- **单元**：进度公式（含 `base=0`→100%、`current>baseline` 钳位 0）；成员协调（新增快照/移除/既有保留基线）。
- **db-test**：建任务、PUT 改 meta + file_ids（新增项 baseline = 当时未翻译数、移除生效、既有基线不变）、列表进度 SQL 正确、删除级联、权限门（非 manage → 403）、文件删除→task_files 级联。
- **前端**：CI build/lint；Markdown 渲染净化（XSS 输入被清理）。

## 8. 涉及文件

迁移 `0008_tasks.sql`；`prts-db/tasks.rs`(+`lib`)；`prts-api/routes/tasks.rs`（5 端点）、`mod.rs`、Swagger；前端（`TaskListView`/`TaskDetailView`/`TaskManageView`、`MarkdownView`/`MarkdownEditor`、复用文件浏览器（勾选变体）、`api` tasksApi、`router`、`i18n`、依赖 `markdown-it`/`dompurify`）；`docs/architecture.md`（补任务）。

## 9. 红线 / 未决

- Markdown 介绍**必须净化**（DOMPurify）防 XSS；渲染只在前端，存源文。
- 进度查询避免 N+1（列表一条聚合 SQL）；不实时全表 COUNT 热路径。
- 权限：写操作过 `project.manage`；查看随项目可见性。
- **未决（实现时）**：`base=0`（加入时文件已全译）UI 呈现（100% 还是「—」，mockup 用「—」于单文件、任务总体用 100%）；「在此任务翻译」的过滤在 E 落地。
