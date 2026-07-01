# 术语（Terminology）· 工作流 D — 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | 大改造 6 工作流之 **D**；前置 **A**（`primary_source_lang` 主源语言字段）；平台后台 POS 管理与 F 同处 |
| 基线 | `master` @ `1cc1a33`（C spec 提交后） |
| 日期 | 2026-07-01 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置 | A/B/C spec、CLAUDE.md |

> 已与作者确认（含可视化 mockup）：**术语项目级**——原文（**主源语言**）→ 翻译（目标语言）+ 备注 + 词性；**词性 = 平台全局预设**（内置一套完整默认集，平台后台增删 + CSV/JSON 导入导出）；**术语与词性都支持 CSV+JSON 导入导出**；术语维护权限 = **owner/manager/校对**（新增节点 `project.term.manage`），其余成员只读；术语列表**键集分页 + 后端搜索**（注意翻页，术语可上千）；词性 UI 用文字描述放平台后台；**编辑器术语匹配/高亮属 E**（本阶段仅提供数据）。

---

## 1. 范围

**做**：项目级术语表（CRUD + 键集分页 + 搜索 + CSV/JSON 导入导出）；平台全局词性预设（默认集 + 平台后台管理 + CSV/JSON 导入导出）；术语分区 UI + 平台后台词性管理 UI。

**不做（后续/别处）**：编辑器内**术语匹配高亮 / 建议**（属 E，D 仅提供术语数据与查询）；词性**双语名**（暂单名 `name`，见 §10）；术语与译文的自动一致性校验/告警。

## 2. 决策要点

1. 词性预设 = **平台全局**（无 `project_id`），迁移内置默认集；平台管理员（`platform.settings`）增删改 + CSV/JSON 导入导出。
2. 术语 = 项目级：`source_text`（主源语言）、`translation`（目标语言）、`notes`、`pos_id?`（引用全局预设）。允许**同形异性**（同原文不同词性并存）。
3. 权限：查看/导出 = 可查看项目者；增/改/删/导入 = 新节点 **`project.term.manage`**（owner/manager/reviewer）。
4. 列表 **键集分页**（`after` 游标 + `limit`）+ 后端搜索（原文/翻译 `ILIKE`/trgm）。
5. 导入导出：术语与词性均支持 **CSV 与 JSON**；术语导入按 `(project_id, source_text, pos_id)` **upsert**（命中更新译文/备注，否则插入），返回 created/updated 计数。

## 3. 数据模型 · 迁移 `0009_terminology.sql`

```
pos_presets(                              -- 平台全局词性预设
  id BIGINT IDENTITY PK,
  name TEXT NOT NULL UNIQUE,
  sort_order INT NOT NULL DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
)
-- 迁移内置默认集（按 sort_order）：名词/动词/形容词/副词/代词/数词/量词/
--   介词/连词/助词/叹词/专有名词/短语/习语/拟声词/其他
terms(
  id BIGINT IDENTITY PK,
  project_id BIGINT NOT NULL REFERENCES projects ON DELETE CASCADE,
  source_text TEXT NOT NULL,              -- 主源语言原文
  translation TEXT NOT NULL DEFAULT '',   -- 目标语言译文
  notes TEXT NOT NULL DEFAULT '',
  pos_id BIGINT REFERENCES pos_presets ON DELETE SET NULL,
  created_by BIGINT REFERENCES users ON DELETE SET NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
)
索引：terms(project_id, source_text, id)（键集 + 排序）；GIN trgm on terms.source_text（搜索 + E 匹配）；可选 GIN trgm on translation。
```

## 4. 后端（`prts-api` + `prts-db`，进 Swagger）

**术语（项目级）**
- `GET /projects/{id}/terms?q=&after=&limit=` — 键集分页 + 搜索（原文/译文）。可查看项目者。
- `POST /projects/{id}/terms` `{source_text, translation, notes, pos_id?}` — 建。`project.term.manage`。
- `PUT /projects/{id}/terms/{term_id}` — 改。`project.term.manage`。
- `DELETE /projects/{id}/terms/{term_id}` — 删。`project.term.manage`。
- `POST /projects/{id}/terms/import?format=csv|json` — multipart 上传，upsert（按 `(source_text,pos_id)`），未知词性名→`pos_id=null` + 计入告警；返回 created/updated/warnings。`project.term.manage`。
- `GET /projects/{id}/terms/export?format=csv|json` — 全量导出。可查看项目者。

**词性预设（平台全局）**
- `GET /pos-presets` — 列表（任意登录者；术语编辑下拉用）。
- `POST/PUT/DELETE /admin/pos-presets[/{pos_id}]` — 平台管理（`platform.settings`）。
- `POST /admin/pos-presets/import?format=csv|json`、`GET /admin/pos-presets/export?format=csv|json` — 平台管理。

**权限**：新增 `project.term.manage` 节点，加入 owner/manager/reviewer 的默认节点集（`prts-core/permission.rs`）。POS 管理走平台 `platform.settings`。

## 5. 前端

- **`ProjectGlossaryView`（术语分区）**：表格（原文/翻译/词性/备注）+ 顶部搜索（后端）+ **键集分页**（滚动加载或分页器）+ `新建/编辑`对话框（原文/翻译/词性下拉[GET /pos-presets]/备注）+ 删除 + `导入▾`/`导出▾`（CSV·JSON）。写操作/导入按 `project.term.manage` 显隐；导出对可查看者开放；翻译成员只读。
- **平台后台（`AdminView`）词性管理**：全局词性列表（增删改 + 拖拽排序）+ CSV/JSON 导入导出。仅平台管理员。
- **`api`**：`termsApi`（list/create/update/delete/import/export）、`posPresetsApi`（list；admin：manage/import/export）。
- 信息页「术语数」统计（A 暂隐）本阶段可点亮（GET terms 计数）。
- i18n 双语；样式少圆角、状态全称。

## 6. 导入导出格式

- **术语 CSV**：表头 `source_text,translation,pos,notes`（`pos`=词性名，导入时映射到 `pos_id`，未知→空 + 告警）。**术语 JSON**：`[{source_text, translation, pos, notes}]`。
- **词性 CSV**：`name,sort_order`。**词性 JSON**：`[{name, sort_order}]`。
- 术语导入 upsert（`(project_id, source_text, pos_id)`）；词性导入 upsert（`name`）。导出为当前全量。

## 7. 性能

- 术语列表**键集分页**（`(source_text, id)` 游标），禁用大 OFFSET；搜索走 trgm GIN。
- 导入分批事务（沿用上传批处理思路）；导出流式/分页拉取避免大内存。
- 术语规模通常百~千级，索引足够；E 的编辑器匹配复用 `source_text` trgm 索引。

## 8. 测试

- **单元**：CSV/JSON 解析与序列化（术语、词性）；导入 upsert 语义（命中更新/新插/未知词性告警）；键集游标。
- **db-test**：术语 CRUD + 键集分页 + 搜索；导入导出往返一致；权限门（非 `term.manage` 增删导入→403、非成员私有项目查看→403）；词性全局 CRUD + 平台权限门；`pos_id` 删除置空（ON DELETE SET NULL）。
- **前端**：CI build/lint。

## 9. 涉及文件

迁移 `0009_terminology.sql`（建表 + 默认词性种子）；`prts-core/permission.rs`（`project.term.manage` 节点 + 角色集）；`prts-db/terms.rs`、`prts-db/pos.rs`；`prts-api/routes/terms.rs`、`routes/pos.rs`（或并入 admin）、`mod.rs`、Swagger；前端（`ProjectGlossaryView`、术语对话框、`AdminView` 词性管理、`api` termsApi/posPresetsApi、`router`、`i18n`）；`docs/architecture.md`（补术语）。

## 10. 红线 / 未决

- 导入严格校验列/字段；参数化 SQL；键集分页不深翻；导入分批事务。
- 权限：写/导入过 `project.term.manage`，词性过 `platform.settings`；私有项目术语按可见性鉴权。
- **未决（实现时）**：词性**双语名**（当前单 `name`，默认集用中文；若英文界面需本地化再加 `name_en` 或 i18n key）；术语导入的“覆盖 vs 追加”是否需要用户可选（默认 upsert）；编辑器术语匹配在 E 落地（可能加 `GET /projects/{id}/terms/match?entry_id=` 或前端本地匹配）。
