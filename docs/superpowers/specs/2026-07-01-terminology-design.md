# 术语（Terminology）· 工作流 D — 校准版设计 Spec

| 项 | 值 |
| --- | --- |
| 历史阶段 | 2026-07-01 大改造工作流 D |
| 校准日期 | 2026-07-10 |
| 规范总纲 | [`2026-07-10-project-workspace-overhaul-design.md`](./2026-07-10-project-workspace-overhaul-design.md) |

> 2026-07-01 版本确定了项目级术语、平台 POS、CSV/JSON 与 reviewer 管理权。本文件保留术语工作流细节；主源切换的精确发布门和 lexical/embedding 状态只以规范总纲 §4 为准。

## 1. 范围与权限

- 术语属于项目，保存真实源语言，不假定英语。
- owner/manager/reviewer 通过 `manage_terms` capability 创建、修改、删除、导入和迁移；其它项目可见者只读/导出。
- 全局 POS 只有平台管理员可管理；maintainer、项目 owner 和普通成员均不可写。
- 所有 mutation 写追加式 audit；私有项目术语遵循项目可见性。

## 2. 数据模型

计划迁移 `0012_terminology.sql`：

```text
pos_presets
  id, name_zh_cn, name_en, sort_order, created_at, updated_at
  CHECK(name_zh_cn 非空 OR name_en 非空)

terms
  id, project_id, source_lang, source_text, translation, notes,
  pos_id, archived_at, created_by, created_at, updated_at
```

唯一键为：

```sql
UNIQUE NULLS NOT DISTINCT (project_id, source_lang, source_text, pos_id)
```

因此同语言、同原文、同 POS 只能一条，`pos_id=NULL` 也不会重复；不同 POS 允许同形术语。

POS 响应按 `Accept-Language` 优先返回 zh-CN 或 en 名称，缺少当前语言时回退到另一名称。内置默认 POS 同时提供两种名称。

## 3. active、归档与迁移

- 任意合法 canonical BCP-47 `source_lang` 都可存储，不要求属于项目 `source_langs`。
- active set = `source_lang = projects.primary_source_lang AND archived_at IS NULL`。非当前主源语言术语只能 archived；创建/更新若请求 `archived=false` 且 source_lang 不是当前 primary，返回稳定校验错误，不得静默改成 archived。
- 主源变化事务中，旧主源 active terms 设置 archived_at；新主源已有归档术语取消归档。legacy old-primary 术语继续保留为 archived/migration-ready。
- 归档术语停止搜索匹配和编辑器建议，但仍可在术语区查看、导出和人工迁移。
- “迁移术语”复制/映射到当前主源并按唯一键 upsert，不篡改旧归档记录；源文本变化必须由用户在预览中确认。
- 混合列表/导出包含 current + archived，始终显式返回 `source_lang` 和 `archived`。

## 4. API

### 项目术语

- `GET /projects/{id}/terms?q=&set=current|archived|mixed&after=&limit=`：键集分页，原文/译文搜索。
- `POST /projects/{id}/terms`：创建，必须带 BCP-47 `source_lang`。
- `PUT /projects/{id}/terms/{term_id}`：修改。
- `DELETE /projects/{id}/terms/{term_id}`：删除。
- `GET /projects/{id}/terms/matches?entry_id=`：只返回当前主源 active matches，供 E 使用。
- `POST /projects/{id}/terms/migrate`：把选定归档术语预览并 upsert 到当前主源。
- `GET /projects/{id}/terms/export?format=csv|json&set=...`：流式导出。

### POS

- `GET /pos-presets`：按 locale 返回名称与两种原始名称。
- `POST/PUT/DELETE /admin/pos-presets[/{id}]`：仅平台管理员。
- POS CSV/JSON 同样使用预览确认导入与流式导出。

全部端点进入 Swagger，错误返回 code + 本地化 message。

所有 term CRUD/migrate/import 的 `source_lang` 先调用共享 `language-tags` canonicalizer；language 小写、script Titlecase、region 大写，variant/extension/private-use 按 parser 输出。无效 tag 与 canonicalization 后重复拒绝，但合法 canonical tag 不受项目 `source_langs` 限制。`archived=false` 只允许当前 primary；非主源 active 请求稳定失败。`needs_language_resolution` 项目禁用普通 term mutation，不能借术语端点绕过 owner resolution。

## 5. CSV/JSON 预览确认

### 5.1 格式

- 术语 CSV：`source_lang,source_text,translation,pos,notes,archived`。
- 术语 JSON：对象数组，字段同 CSV。
- POS CSV：`name_zh_cn,name_en,sort_order`。
- POS JSON：对象数组，字段同 CSV。

### 5.2 两阶段导入

1. `POST .../imports/preview` 解析文件，先 canonicalize source_lang，再返回行预览、created/updated 数、错误、警告和一次性 token，不写业务表。token 由 CSPRNG 生成、熵至少 128 bit、TTL 15 分钟，绑定 `actor_id + project_id + import_kind(term|pos) + canonical content digest`。
2. `POST .../imports/{token}/confirm` 原子校验并一次性消费 token，重新检查当前 permission 与项目状态，然后在事务内按唯一键 upsert并写 audit。actor/project/kind/digest 不匹配、过期、重放、并发二次消费或权限撤销全部拒绝且不写业务表。

未知 POS 置 `pos_id=NULL` 并返回带行号的 warning，不拒绝其它合法行。无效 BCP-47、缺少 source_text、重复输入唯一键冲突，以及 `archived=false` 的非主源行都在预览中明确标出；合法非项目 source-set tag 的 archived 行允许通过。

## 6. 前端与编辑器联动

- `ProjectTermsView` 提供 current/archived/mixed、键集加载、搜索、CRUD、迁移、预览导入与 CSV/JSON 导出。
- POS 下拉按 locale 显示并回退；未知 POS 的预览行显示警告。
- E 必须实现 active term 高亮与建议，不再作为可选范围：匹配当前主源文本；点击时替换 selection，无 selection 时插入 cursor；只改本地 draft，不保存、不改 state。
- 前端完全依据 capabilities 控制写操作，覆盖 zh-CN/en、浅/深主题、MDI 和小圆角。

## 7. 性能与验收

- 术语列表使用 `(source_text,id)` 或稳定 id cursor，禁止大 OFFSET；搜索走 trgm 索引。
- 导入分批解析、确认时事务 upsert；导出流式响应，避免全量内存拼接。
- 测试覆盖 NULL POS 唯一、同形异性、任意合法 canonical source_lang 可存、非主源 active 稳定拒绝、主源归档/激活、legacy old-primary archived/migration-ready、canonical duplicate 拒绝、混合导出、未知 POS warning、双语 fallback、token 绑定/15 分钟过期/重放/并发消费/权限撤销、权限和私有可见性。
- CSV/JSON preview→confirm→export 往返保持 source_lang、archived、POS 与备注。
- E 验收必须包含 active-only 匹配、selection/cursor 插入和不改变状态。
- 阶段结束执行测试、verify、Conventional Commit、推 master、等待 CI 与 GHCR。
