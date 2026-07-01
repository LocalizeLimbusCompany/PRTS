# 编辑器（工作台 + 搜索重构 + 删 context）· 工作流 E — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | 大改造 6 工作流之 **E**（最大）；前置 **A**（每文件状态计数）、**C**（任务→文件，供「当前任务」范围）、**D**（术语，供编辑器匹配，见 §1 不做） |
| 基线 | `master` @ `e6c213e`（D spec 提交后） |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | A–D spec、P3 实时编辑器、P4 混合搜索 |

> 已与作者确认（含逐张 mockup）：**工作台**——撤左下状态 combobox，右下「状态下拉 + 智能按钮」并排；按钮随「改动+状态+权限」变：未翻译·改动→`翻译`(靛)、已翻译·改动→`保存`(蓝)、已翻译·未改→`检查`(青)、已检查·未改→`审核`(绿)、他人编辑·管理→`强制保存`(红)、终态/无权→禁用；状态下拉设任意**有权限**状态、无权灰掉；列表里**自己**编辑的词条显示**自己头像**。**搜索**——移到列表上方；`Enter` 全项目 / `Shift+Enter` 当前文件；筛选图标下拉=单选本文件状态 + 高级筛选入口；高级弹窗=**AND** 多条件（字段：原文·各源语言 / 原文·全部源语言 / 译文 / 键名；操作符：包含·不包含·开头是·结尾是·等于·正则）+ 范围（全项目 / 指定文件·目录 / 当前文件 / 当前任务）+ 多选状态 + 向量开关（默认关，开启才出「语义查询」）。**删除词条 `context`**（数据丢失，作者已确认「删了吧」）。**全站图标统一 MDI**。

---

## 1. 范围

**做**：① 工作台底栏智能按钮 + 状态下拉 + 自编头像；② 搜索重构（位置 + 快捷键 + 筛选下拉 + 高级筛选弹窗 + 结构化搜索端点）；③ 删除 `entries.context`；④ 全站图标改 MDI。

**不做（后续/别处）**：编辑器内**术语匹配高亮/建议**（D 数据已具，匹配 UI 可作为 E 后续小增或单列，本 spec 不含）；OR 条件组合（仅 AND）；保存乐观锁本身（P3 已有，不改）；审计（P5）。

## 2. 决策要点

1. **按钮语义**（精化现 `lib/saveButton.ts`）：`未翻译+dirty→翻译(置已翻译)`；`有译文+dirty→保存(不改状态)`；`未 dirty` 时按状态与权限给推进动作 `检查(→已检查)/审核(→已审核)`；他人编辑且本人管理→`强制保存`；否则禁用。标签/色/图标随 mode。
2. **状态下拉**紧邻按钮：列全部工作流状态，按角色权限灰掉不可设项（翻译只可 未翻译/已翻译/有疑问；校对/管理可全部）；选中=保存并置该状态。
3. **自编头像**：列表行的「正在编辑」头像**含本人**（现仅显示他人）；复用实时 editingMap。
4. **搜索位置/快捷键**：列表上方；`Enter`=全项目、`Shift+Enter`=当前文件。
5. **筛选下拉**：单选本文件状态 + 「高级筛选…」。
6. **高级筛选**：AND 多条件（字段/操作符/值）+ 范围 + 多选状态 + 向量开关（默认关）。范围选「当前文件/当前任务」时禁用文件·目录选择。
7. **删 context**：迁移 drop 列 + 前后端清理 + 上传忽略该字段。
8. **MDI**：Quasar 图标集改 `mdi-v7`，替换现有 Material 图标名。

## 3. 数据模型 · 迁移 `0010_drop_context.sql`

```
ALTER TABLE entries DROP COLUMN context;   -- 数据丢失（作者已确认）
```
`entry_versions` 无 context 列，不受影响。搜索**无新表**；复用 P4 的 `source_text/source_tsv/translation_tsv/embedding` 及 trgm/GIN/HNSW 索引。

## 4. 后端

**结构化搜索** `POST /projects/{id}/search`（取代/扩展现 GET /search）：
```
body: {
  q?: string,                       // 关键词：FTS+trgm；vector=true 时兼作语义查询
  conditions?: [{ field, op, value }],   // AND；field=source:<lang> | source_any | translation | key
  scope?: { type: all|path|file|task, file_id?, folder_id?, task_id? },
  states?: string[], vector?: bool = false,
  sort?: relevance|key|updated, after?, limit?
}
```
- **条件→WHERE**（参数化）：`source:<lang>` = `original->>'<lang>'`；`source_any` = 对项目 `source_langs` 各值 OR（或对 `source_text` 匹配）；`translation`、`key` 直列。操作符：`包含`→`ILIKE '%v%'`、`不包含`→`NOT ILIKE`、`开头是`→`ILIKE 'v%'`、`结尾是`→`ILIKE '%v'`、`等于`→`=`、`正则`→`~`。
- **范围**：`file`→`file_id=`；`path`→`files.path LIKE '<folder>/%'`（连表或先查 file_ids）；`task`→ 取该任务 `task_files.file_id ∈`；`all`→ 项目内。
- **状态**：`state = ANY(states)`。
- **排序/召回**：有 `q` 或 `vector` → 走 `prts-search` 混合编排（`vector=false` 时只 FTS+trgm；`true` 且平台已配 Embedding 时加向量路，RRF），在其上叠加 conditions/scope/states 过滤；无 `q` → 纯过滤，键集分页（`key,id`）。
- **正则安全**：`~` 交给 PG；设 `statement_timeout` 兜底防病态正则；`value` 参数化。
- 保留/弃用旧 `GET /search`：前端快捷框也走 `POST`（`{q, scope}`）；GET 可保留兼容或移除（实现时定）。

**删 context**：移除 `prts-db` 实体、`UploadEntry`、`EntryDto`、`bulk_upsert` 与 upload 对 context 的读写；上传体若含 `context` 字段则**忽略**（不报错）。Swagger 更新。

**工作台**：无新端点；复用 `PUT /entries/{id}`（改译文/状态，权限已校验）。

## 5. 前端

**工作台（`EditorView` + `lib/saveButton.ts`）**
- 重构 `computeSaveButton`：新增 `translate` mode（未翻译+dirty），mode→{label, color, mdiIcon}：翻译=`primary/indigo·mdi-translate`、保存=`blue·mdi-content-save`、检查=`cyan·mdi-check-circle-outline`、审核=`green·mdi-shield-check`、强制=`negative·mdi-flash`、禁用=灰·`mdi-lock`。
- 右下：`q-btn`（主动作）+ 紧邻 `q-btn-dropdown`/`q-select`（状态，按权限 `disable` 项）；**移除**左下状态 combobox。
- 列表行「正在编辑」头像逻辑加入本人（`editingMap` 含自己 → 显示自己头像）。

**搜索（`EditorView` + 新 `SearchBar` / `AdvancedFilterDialog`，替换现 `SearchFilters`）**
- 列表上方 `SearchBar`：输入框 + 筛选图标；`Enter`=全项目、`Shift+Enter`=当前文件（IME 合成中不触发）；结果态显示范围 chip + 清除。
- 筛选下拉：单选本文件状态（全部/未翻译/…）+「高级筛选…」。
- `AdvancedFilterDialog`：AND 条件行（字段 select[原文·各源语言 / 原文·全部源语言 / 译文 / 键名] + 操作符 select + 值 + 删除 + 添加条件）；范围（全项目 / 指定文件·目录[选择器] / 当前文件 / 当前任务，后两者禁用选择器）；多选状态 chips；向量开关（默认关，开启显「语义查询」输入）；重置/搜索。调 `POST /search`。
- **MDI**：`quasar.config` 用 `iconSet: 'mdi-v7'` + 装 `@quasar/extras`；把现有 `name="person/mail/shield/logout/…"` 换成 `mdi-*`。
- 删 context：移除编辑器原文块的「注释」展示；`api`/类型去 context。
- i18n 双语（按钮标签、操作符、字段、范围、向量）；样式少圆角、状态全称、**无 emoji**。

## 6. 性能

- 条件过滤走参数化 SQL；`contains` 命中 P4 的 `source_text/translation/key` trgm GIN；`source:<lang>`（`original->>lang`）与 `等于/开头/结尾/正则` 可能走顺扫——大项目下建议限定范围（文件/目录/任务）后再跑，UI 提示；必要时后续加按源语言的表达式索引（未决）。
- 有排序（q/vector）→ 复用 P4 融合上限 + offset 窗口；纯过滤→键集分页。
- 正则设 `statement_timeout` 兜底。

## 7. 测试

- **单元**：`computeSaveButton` 各分支（翻译/保存/检查/审核/强制/禁用 × 角色/dirty/他人编辑）；条件→SQL 片段构造（各字段/操作符，参数化，正则转义）；范围解析（file/path/task/all）。
- **db-test**：结构化搜索（单/多条件 AND、各操作符、source:lang vs source_any、范围 file/path/task、states 多选、vector 开关降级）；删 context 迁移后上传/CRUD 正常；`PUT` 改状态权限门。
- **前端**：CI build/lint；Enter/Shift+Enter（含 IME 合成）；工作台按钮态快照。

## 8. 涉及文件

迁移 `0010_drop_context.sql`；`prts-db`（entries 去 context、search 结构化查询构造）、`prts-search`（编排接入 conditions/scope/states）、`prts-api`（`routes/search.rs` 改 POST 结构化、`entries.rs`/DTO 去 context、Swagger）、`prts-core`（若状态推进逻辑辅助）；前端（`EditorView`、`lib/saveButton.ts`、`SearchBar`、`AdvancedFilterDialog`（替换 `SearchFilters`）、`api`（search POST/去 context）、`quasar.config`（mdi-v7）+ 全站图标名、`i18n`）；`docs/architecture.md`。

## 9. 红线 / 未决

- 删 context **不可逆**（已确认）；迁移前无需备份（作者定）。
- 搜索全程参数化；正则/大范围加 `statement_timeout` 与 UI 提示，避免热路径病态查询；键集分页不深翻。
- 权限：状态设置/推进过 `entry.edit`/`entry.review` 节点；`locked` 词条仅管理/拥有者。
- **未决（实现时）**：`source:<lang>` 等非 trgm 命中的**表达式索引**是否加（先不加，靠范围收窄）；旧 `GET /search` 保留兼容或移除；编辑器**术语匹配高亮**是否本阶段附带（默认否，留后续）；MDI 切换后逐一核对现有图标名映射。
