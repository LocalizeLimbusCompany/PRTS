# 任务（Tasks）· 工作流 C — 校准版设计 Spec

| 项 | 值 |
| --- | --- |
| 历史阶段 | 2026-07-01 大改造工作流 C |
| 校准日期 | 2026-07-10 |
| 规范总纲 | [`2026-07-10-project-workspace-overhaul-design.md`](./2026-07-10-project-workspace-overhaul-design.md) |

> 2026-07-01 版本确认了标题、Markdown、多对多文件与快照进度方向。精确有效可见性与 `current_task` tagged scope 由规范总纲 §3、§6、§8.2 定义；本文保留任务工作流细节。

## 1. 范围与权限

任务包含标题、Markdown 介绍、多对多文件关系、基线快照和进度。owner/manager 通过 `manage_tasks` capability 创建、修改、删除、增移文件；其它项目可见者只读。

Markdown 只保存源文，使用共享净化组件渲染。所有 mutation 与文件集合变化写 audit。

## 2. 基线快照

### 2.1 加入文件

文件加入任务时，在同一事务创建新的 `task_files.id`，并把当时满足以下条件的 entry id 写入 `task_baseline_entries`：

```text
file_id = joined_active_file
state = untranslated
effective_visible(entry, file, include_hidden=false)
```

不能只保存数量。保存 ID 后才能正确处理隐藏、删除、恢复、状态回退和 key 重传。

### 2.2 有效分母与完成数

- 有效分母 = snapshot IDs 中当前满足规范 `effective_visible(..., false)` 的数量。
- 完成数 = 有效分母中当前 state 不等于 `untranslated` 的数量。
- hidden、entry tombstone、file/folder delete 使 snapshot entry 暂时离开分母；只有符合各自恢复规则时才返回。file/folder restore 不清除 reupload tombstone。
- 完成 entry 退回 `untranslated` 时，完成数和百分比同步回退。
- 文件加入后的新 entry 不进入旧 snapshot。
- 从任务移除文件会删除该 task_file 与 snapshot；重新加入创建新 task_file id 和新 snapshot。
- 有效分母为 0 时，任务显示 100% 与“无需处理”。文件、文件夹和项目自身的空进度仍显示“—”。

## 3. 数据模型

计划迁移 `0011_tasks.sql`：

```text
tasks
  id, project_id, title, description_markdown, created_by, created_at, updated_at

task_files
  id, task_id, file_id_snapshot BIGINT NOT NULL,
  live_file_id BIGINT NULL REFERENCES files(id) ON DELETE SET NULL,
  added_by, added_at
  UNIQUE(task_id, live_file_id) WHERE live_file_id IS NOT NULL

task_baseline_entries
  task_file_id REFERENCES task_files(id) ON DELETE CASCADE,
  entry_id_snapshot BIGINT NOT NULL,
  live_entry_id BIGINT NULL REFERENCES entries(id) ON DELETE SET NULL
  PRIMARY KEY(task_file_id, entry_id_snapshot)

task_stats
  task_id, effective_baseline, completed, updated_at
```

`task_stats` 由 task file、entry state/hidden/tombstone 和 file/folder deletion exposure 变化集合更新。任务列表正常读路径不扫描项目全部 entries 做实时 `COUNT(*)`；离线 rebuild/verify 使用规范 effective-visible predicate。

文件软删除不会删除 task_files 或 snapshot，因此恢复后 live relationship 与分母自然返回。文件/entry 永久清除时，`live_file_id/live_entry_id` 由 FK 置 NULL，snapshot ID 仍保留以解释历史基线；NULL live refs 永久退出有效分母。显式从任务移除文件才删除 task_file，并由 CASCADE 删除该成员关系自己的 baseline rows。

## 4. API

- `GET /projects/{id}/tasks`：键集列表，返回标题、介绍摘要、有效分母、完成数、百分比、文件数和 capabilities。
- `POST /projects/{id}/tasks`：创建任务；owner/manager。
- `GET /projects/{id}/tasks/{task_id}`：详情、净化所需源 Markdown、统计和当前文件集合。
- `PUT /projects/{id}/tasks/{task_id}`：原子保存标题、Markdown 与期望 file_ids；新增文件建立 snapshot，既有文件保留 snapshot，缺席文件移除。
- `DELETE /projects/{id}/tasks/{task_id}`：删除任务与 snapshot；写 audit。

所有端点进入 Swagger，使用稳定错误码与 Accept-Language 本地化消息。前端不从 role 推断管理权。

## 5. 当前任务搜索

“在此任务翻译”进入编辑器并传 task_id。E 发送 `{ "type": "current_task", "task_id": ... }`，项目 search route 验证 task 属于 URL project 且 task/project 对 caller 可见，再使用任务当前 active `task_files` 搜索 effective-visible entries：

- 不限于 snapshot IDs；
- `include_hidden` 只覆盖 hidden；entry/file/folder deletion 始终排除；
- 支持多状态、结构化条件和 vector=false 默认规则；
- 文件移出任务后立即不再属于该 scope。

## 6. 前端

- `TaskListView`：标题、净化摘要、完成/分母、进度和文件数；零基线显示“无需处理”。
- `TaskDetailView`：净化 Markdown、进度、共享文件浏览器、“在此任务翻译”和 capability 控制的管理入口。
- `TaskManageView`：标题、Markdown 编辑/预览、文件勾选。勾文件夹只展开为保存时的后代 file ids。
- 点任务文件进入全屏 editor，携带 file 与 task 参数。
- zh-CN/en、Quasar、MDI、小圆角与浅/深主题全部沿用共享基础。

## 7. 验收

- 数据库测试覆盖 snapshot 条件、隐藏/删除离开、恢复返回、完成/回退、新 entry 排除、移除重加和零基线。
- 随机变更后 task_stats 等于离线 snapshot join 校验；列表 SQL 不做项目 entries 全表实时聚合。
- 权限测试覆盖 owner/manager 写、reviewer/translator/游客只读、私有非成员拒绝。
- Markdown XSS、文件集合协调、当前任务搜索范围和前端 capability 均有测试。
- 阶段结束执行测试、verify、Conventional Commit、推 master、等待 CI 与 GHCR。
