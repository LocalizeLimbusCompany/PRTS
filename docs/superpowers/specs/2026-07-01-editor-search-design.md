# 编辑器（工作台 + 搜索重构 + 删 context）· 工作流 E — 校准版设计 Spec

| 项 | 值 |
| --- | --- |
| 历史阶段 | 2026-07-01 大改造工作流 E |
| 校准日期 | 2026-07-10 |
| 规范总纲 | [`2026-07-10-project-workspace-overhaul-design.md`](./2026-07-10-project-workspace-overhaul-design.md) |

> 2026-07-01 版本确认了智能动作、搜索位置、AND 条件、删 context 与 MDI。本文件保留编辑器交互细节；精确 search scope DTO、资源验证、可见性和 context/history 清理契约只以规范总纲 §3、§8 为准。

## 1. 范围

工作流 E 完成：

1. 删除词条 `context` 的数据库、DTO、上传、前端、测试和权威文档引用；
2. 右下“状态下拉 + 一个智能按钮”；
3. 列表上方快捷搜索、状态菜单和高级结构化搜索；
4. active 术语高亮/建议；
5. 公开项目游客只读编辑器；
6. 本人 editing avatar 展示但不提供 poke/DM。

## 2. 智能动作

删除左下状态 combobox。右下角恰好保留状态下拉与一个主按钮，按钮真值表固定为：

| 条件 | 标签 | 保存后的状态 |
| --- | --- | --- |
| dirty 且当前 `untranslated` | 翻译 | `translated` |
| dirty 且当前为其它状态 | 保存 | 状态不变 |
| clean、当前 `translated`、有 `review_entry` capability | 检查 | `checked` |
| clean、当前 `checked`、有 `review_entry` capability | 审核 | `reviewed` |
| 他人 presence 占用、本人有 `force_save_presence` capability | 强制保存 | 按 dirty 规则；仍校验 expected version |
| 其它 | 禁用 | 无请求 |

- 状态下拉列出完整工作流状态，无设置能力的选项置灰。
- 服务端继续校验 capability、状态机、locked 和乐观锁版本。
- owner 与 manager 获得 `force_save_presence`；前后端只检查 capability。“强制保存”只越过 presence 占用提示，不绕过 version mismatch；过期版本仍返回 409。
- 自己正在编辑的列表行显示自己的头像；点击自己不显示 poke 或私信菜单。

## 3. 公开游客只读

- 公开项目的 editor 路由不要求登录；匿名可读取项目、文件、词条、普通搜索和术语只读数据。
- 匿名不建立可写 presence，不发送 editing、poke、DM 或其它协作事件。
- 匿名不显示/调用保存、状态、locked、hidden、history rollback 等 mutation。
- 私有项目匿名访问仍拒绝；登录成员能力由 API `capabilities` 决定，前端不从角色字符串推断。

## 4. context 清理

计划迁移 `0013_editor_search.sql` 不改写已应用的 `0003`，并在同一迁移中完成：

- `ALTER TABLE entries DROP COLUMN context`，并从 `prts-db::Entry`、UploadEntry、API DTO、Swagger、前端类型和 editor UI 移除字段；
- 创建/更新结构化 POST search 所需 metadata、indexes 和 functions；
- 从既有 `file_change_items.before/after` 的 entry JSONB payload scrub `context` key。

从 B 的 file-history 首次发布起，entry change-set payload 就只能序列化 `key/original/translation/state/locked/hidden/deleted_at`；不得等到 `0013` 才停止捕获 context。

- 旧上传兼容期若请求仍携带 context，反序列化层忽略未知字段，不写库、不回显。
- 权威蓝图与 architecture 同步删除“保留/展示上下文”的描述。

## 5. 快捷与高级搜索

### 5.1 快捷搜索

- 输入框位于词条列表上方。
- IME composing 期间 Enter/Shift+Enter 不触发。
- `Enter`：全项目 scope；`Shift+Enter`：当前文件 scope。
- 筛选图标菜单只提供当前文件单一 state 选择和“高级筛选”入口。

### 5.2 结构化 POST

`POST /projects/{id}/search` 请求模型：

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

字段：

- `source:<bcp47>`：指定源语言；tag 先通过共享 `language-tags` canonicalizer 规范化，invalid/非项目 source set 拒绝；
- `source_any`：任意源语言；
- `translation`；
- `key`。

操作符仅：`contains`、`not_contains`、`starts_with`、`ends_with`、`equals`。所有 conditions 为 AND，不支持 regex，也不提供 OR 分组。

scope 的 JSON 形状：

- `{ "type": "all" }` 全项目；
- `{ "type": "path", "path": "chapter/01" }` 指定 active 文件夹/路径；
- `{ "type": "file", "file_id": 41 }` 指定任意 active file；
- `{ "type": "current_file", "file_id": 41 }` 明确指定当前编辑 file；
- `{ "type": "current_task", "task_id": 73 }` 指定 task 当前 active files，不限 snapshot IDs。

file/task ID 沿用现有 PostgreSQL `BIGINT`/Rust `i64`；path 仍为 string，不做 UUID migration。tagged union 拒绝未知字段，因此 `{ "type": "all", "file_id": 41 }`、variant 多余字段、缺 payload、未知 type 和错误 ID 类型都必须返回 400。

项目 route 先用共享 file-path canonicalizer 规范化 path，再验证 path/file/task 属于 URL project 且对 caller 可见；`current_task` 还验证 task/project 可见。deleted file/folder/ancestor/task 一律排除。服务端不从 session 或其它 query 参数推断 current context。

path 解析按 segment boundary：精确解析为 active file 时仅该 file；解析为 active folder 时包含 active descendant files，只允许 exact path 或 `folder/` subtree，禁止 naive prefix。歧义、跨项目与 deleted ancestor 稳定拒绝。

### 5.3 P4 管线与兼容

- query 继续进入现有 FTS + trgm + 可选 pgvector + RRF；conditions/scope/states 在召回和取行时一致应用。
- `vector=false` 是默认，不调用 EmbeddingProvider。provider 缺失/失败时安全降级词法路径。
- 普通搜索使用规范 `effective_visible`。只有 owner/manager 有 `include_hidden` capability；它只覆盖 hidden，永不包含 tombstone/deleted file/folder；越权请求 true 返回 403。
- 主源 lexical 重建期间返回稳定 `PROJECT_SEARCH_REBUILDING` 与 job 引用；lexical ready 后恢复 FTS/trgm，即使 embedding 仍 degraded。
- 旧 `GET /projects/{id}/search` 映射到同一 service：有 i64 `file_id` 时只映射为 `file {file_id}`，否则只映射为 `all`；绝不制造 `current_file/current_task`。保留一个兼容周期，OpenAPI 标 deprecated，响应加入 Deprecation/Sunset；不得维护第二套 SQL。
- POST 唯一默认排序为 `(rrf_score DESC, entry_id ASC)`，limit 默认 50、允许 1..=100；响应包含 `items` 与 `next_after`。opaque `after` cursor 版本化并绑定 URL `project_id`、canonical query/filter/scope fingerprint、最后 score+id；错误/未知版本/跨 project 或跨查询 cursor 返回 400。新增 sort 必须逐一声明稳定 tie-break。

## 6. 术语高亮与建议

- 只请求 D 定义的当前主源 active terms；归档或其它 source_lang 不匹配。
- 对当前 primary source 文本做可定位高亮，并在译文区展示 translation、POS、notes。
- 点击建议：有 selection 时替换 selection；无 selection 时插入 cursor。
- 点击只更新本地 translation draft，不发送保存请求、不改变 state、不获取 CP。

## 7. 前端结构

- `SearchBar.vue`：快捷输入、IME、安全快捷键、scope chip。
- `AdvancedFilterDialog.vue`：AND 条件、五种操作符、五种 scope、多状态、include_hidden、vector。
- `TermSuggestions.vue`：active matches 与插入行为。
- `saveButton.ts`：纯函数实现真值表；EditorView 只消费 mode/label/color/icon/disabled。
- 图标统一 MDI；中文字体、2–4px 圆角、浅/深主题和 zh-CN/en 使用共享基础。

## 8. 验收

- `saveButton` 覆盖 dirty/state/review/manage/presence/version 组合；force + stale version 必须 409。
- 状态下拉按 capability 灰显，服务端拒绝伪造越权 state。
- 搜索测试覆盖各 field/op、AND、tagged union 五 variant、i64 ID、path file/folder/segment-boundary、缺 payload/未知 type/未知字段、跨项目/deleted path-file-task、多状态、hidden overlay、vector 降级、稳定 score+id keyset、next_after、limit 边界、cursor tamper/version/fingerprint mismatch、同过滤跨 URL project cursor 400 和 GET file/all 映射。
- Enter/Shift+Enter 在 composition 期间不触发；普通按键触发正确 scope。
- context 在代码、API schema、数据库、历史 JSONB 和权威文档中移除；B 首发历史不捕获它，旧上传带字段仍可兼容。
- 游客公开只读、私有拒绝、无 WS mutation；术语点击不自动保存/改状态。
- 阶段结束执行测试、verify、Conventional Commit、推 master、等待 CI 与 GHCR。
