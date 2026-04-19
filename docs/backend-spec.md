# 后端详细规格

## 固定约定

- 后端 API 端口：`18080`
- 前端开发端口：`13000`
- API 前缀：`/api/v1`
- 时区统一使用 UTC 存库
- 前端展示时再按用户设置转换

## 数据库表设计

下面不是最终 SQL，而是第一版的表结构思路。

### users

字段建议：

- `id`
- `email`
- `username`
- `password_hash`
- `display_name`
- `preferred_locale`
- `preferred_source_language`
- `status`
- `created_at`
- `updated_at`

### organizations

- `id`
- `slug`
- `name`
- `description`
- `visibility`
- `created_by`
- `created_at`
- `updated_at`

### organization_members

- `id`
- `organization_id`
- `user_id`
- `role`
- `created_at`

### projects

- `id`
- `organization_id`
- `slug`
- `name`
- `description`
- `target_language`
- `visibility`
- `guest_policy`
- `created_by`
- `created_at`
- `updated_at`

### project_source_languages

- `id`
- `project_id`
- `language_code`
- `sort_order`

### project_members

- `id`
- `project_id`
- `user_id`
- `role_code`
- `created_at`

### permission_nodes

- `id`
- `code`
- `name`
- `description`

### project_roles

- `id`
- `project_id`
- `code`
- `name`
- `is_system`

### project_role_permissions

- `id`
- `project_role_id`
- `permission_node_id`

### project_member_permission_overrides

- `id`
- `project_id`
- `user_id`
- `permission_node_id`
- `effect`

说明：

- `effect` 取值建议：`allow` / `deny`

### project_member_document_rules

- `id`
- `project_id`
- `user_id`
- `permission_scope`
- `match_type`
- `match_value`
- `effect`

说明：

- `permission_scope` 例如 `document.edit`、`translation.edit`
- `match_type` 例如 `path_prefix`、`document_id`、`tag`
- `effect` 例如 `allow`、`deny`

### documents

- `id`
- `project_id`
- `path`
- `title`
- `current_version_no`
- `created_at`
- `updated_at`
- `updated_by`

唯一约束：

- `project_id + path`

### document_tags

- `id`
- `project_id`
- `code`
- `name`
- `color`
- `is_visible`
- `created_at`

### document_tag_bindings

- `id`
- `document_id`
- `tag_id`

### document_versions

- `id`
- `document_id`
- `version_no`
- `source_snapshot_hash`
- `import_job_id`
- `created_by`
- `created_at`

### translation_units

- `id`
- `project_id`
- `document_id`
- `key`
- `target_language`
- `target_text`
- `status`
- `comment`
- `version_no`
- `updated_by`
- `updated_at`

唯一约束：

- `document_id + key`

### translation_unit_sources

- `id`
- `translation_unit_id`
- `language_code`
- `text`

唯一约束：

- `translation_unit_id + language_code`

### translation_revisions

- `id`
- `translation_unit_id`
- `revision_no`
- `before_target_text`
- `after_target_text`
- `before_status`
- `after_status`
- `change_note`
- `changed_by`
- `changed_at`

### glossary_terms

- `id`
- `project_id`
- `source_term`
- `target_term`
- `source_language`
- `target_language`
- `note`
- `created_by`
- `created_at`
- `updated_at`

### translation_memory_entries

- `id`
- `project_id`
- `source_language`
- `target_language`
- `source_text`
- `target_text`
- `quality_score`
- `created_from_unit_id`
- `created_at`

### import_jobs

- `id`
- `project_id`
- `document_path`
- `status`
- `uploaded_by`
- `error_message`
- `started_at`
- `finished_at`
- `created_at`

### export_jobs

- `id`
- `project_id`
- `status`
- `file_path`
- `file_size`
- `expires_at`
- `requested_by`
- `error_message`
- `started_at`
- `finished_at`
- `created_at`

### audit_logs

- `id`
- `organization_id`
- `project_id`
- `user_id`
- `action`
- `resource_type`
- `resource_id`
- `detail_json`
- `created_at`

### platform_locales

- `id`
- `locale_code`
- `namespace`
- `message_key`
- `message_text`

## 接口设计

### 登录和用户

- `POST /api/v1/auth/login`
- `POST /api/v1/auth/logout`
- `GET /api/v1/me`
- `PATCH /api/v1/me/preferences`

`PATCH /api/v1/me/preferences` 需要支持：

- 平台界面语言
- 首选原文语言

### 组织

- `GET /api/v1/organizations`
- `POST /api/v1/organizations`
- `GET /api/v1/organizations/:organizationId`
- `PATCH /api/v1/organizations/:organizationId`
- `GET /api/v1/organizations/:organizationId/members`

### 项目

- `GET /api/v1/organizations/:organizationId/projects`
- `POST /api/v1/organizations/:organizationId/projects`
- `GET /api/v1/projects/:projectId`
- `PATCH /api/v1/projects/:projectId`
- `GET /api/v1/projects/:projectId/members`
- `PATCH /api/v1/projects/:projectId/members/:userId`

### 权限

- `GET /api/v1/projects/:projectId/roles`
- `GET /api/v1/projects/:projectId/permissions`
- `PATCH /api/v1/projects/:projectId/members/:userId/permissions`
- `PATCH /api/v1/projects/:projectId/members/:userId/document-rules`

### 文档

- `GET /api/v1/projects/:projectId/documents`
- `POST /api/v1/projects/:projectId/documents`
- `GET /api/v1/projects/:projectId/documents/:documentId`
- `PATCH /api/v1/projects/:projectId/documents/:documentId`
- `GET /api/v1/projects/:projectId/documents/:documentId/versions`

文档列表需要支持查询参数：

- `path`
- `tag`
- `updatedBy`

### 标签

- `GET /api/v1/projects/:projectId/tags`
- `POST /api/v1/projects/:projectId/tags`
- `PATCH /api/v1/projects/:projectId/tags/:tagId`
- `POST /api/v1/projects/:projectId/documents/:documentId/tags`
- `DELETE /api/v1/projects/:projectId/documents/:documentId/tags/:tagId`

### 翻译条目

- `GET /api/v1/projects/:projectId/units`
- `GET /api/v1/projects/:projectId/units/:unitId`
- `PATCH /api/v1/projects/:projectId/units/:unitId`
- `POST /api/v1/projects/:projectId/units/:unitId/review`
- `POST /api/v1/projects/:projectId/units/:unitId/approve`
- `GET /api/v1/projects/:projectId/units/:unitId/history`

条目列表要支持：

- `documentId`
- `documentPath`
- `tag`
- `key`
- `status`
- `sourceText`
- `targetText`
- `updatedBy`
- `page`
- `pageSize`

### 术语库

- `GET /api/v1/projects/:projectId/glossary`
- `POST /api/v1/projects/:projectId/glossary`
- `PATCH /api/v1/projects/:projectId/glossary/:termId`
- `DELETE /api/v1/projects/:projectId/glossary/:termId`

### 翻译记忆库

- `GET /api/v1/projects/:projectId/tm`
- `POST /api/v1/projects/:projectId/tm`
- `PATCH /api/v1/projects/:projectId/tm/:entryId`
- `DELETE /api/v1/projects/:projectId/tm/:entryId`

### 导入导出

- `POST /api/v1/projects/:projectId/imports`
- `GET /api/v1/projects/:projectId/imports`
- `POST /api/v1/projects/:projectId/exports`
- `GET /api/v1/projects/:projectId/exports`
- `GET /api/v1/projects/:projectId/exports/:exportJobId/download`

### 审计日志

- `GET /api/v1/projects/:projectId/audit-logs`

## 导入 JSON 格式

建议固定成下面这种：

```json
{
  "document": "StoryData/S001.json",
  "entries": [
    {
      "key": "story.line.001",
      "source": {
        "en": "Doctor, wake up.",
        "jp": "ドクター、起きてください。",
        "kr": "박사님, 일어나세요."
      },
      "target": {
        "cn": "博士，醒醒。"
      },
      "meta": {
        "status": "translated",
        "comment": ""
      }
    }
  ]
}
```

说明：

- `document` 是文档路径
- `entries` 是条目数组
- `source` 是多个原文字段
- `target` 只放项目目标语言
- `meta` 是可选元数据

## 导出 JSON 格式

每个文档导出成一个 JSON，结构和导入结构一致。

项目导出时：

- 先按文档生成多个 JSON
- 再打包进 zip

## 接口返回格式建议

统一返回格式建议：

```json
{
  "success": true,
  "data": {},
  "error": null,
  "requestId": "req_xxx"
}
```

错误时：

```json
{
  "success": false,
  "data": null,
  "error": {
    "code": "permission_denied",
    "message": "你没有权限执行该操作"
  },
  "requestId": "req_xxx"
}
```

## 权限判断建议

一次请求的权限判断顺序建议：

1. 判断是否具备基础角色权限
2. 应用用户单独允许或拒绝
3. 应用文档范围 allow/deny 规则
4. deny 优先于 allow

## 审计日志建议

至少记录以下动作：

- 创建组织
- 创建项目
- 修改项目设置
- 修改成员角色
- 修改权限覆盖
- 导入文档
- 导出项目
- 修改翻译
- 校对翻译
- 批准翻译
- 修改标签
- 修改术语库
- 修改翻译记忆库

## 后台任务建议

需要放进 worker 的任务：

- 导入处理
- 导出打包
- 导出清理
- 历史快照整理

## 第一版不做的事

- 机翻
- Git 同步
- 截图上下文
- 多候选译文投票
- 跨项目术语库
- 跨项目翻译记忆库
