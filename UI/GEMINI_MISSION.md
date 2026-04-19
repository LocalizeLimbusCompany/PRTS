# Gemini 前端任务书

你负责这个项目的前端设计与实现。

不要把它做成展示型官网，要把它做成真正高密度、高效率的翻译工作台。

下面这些信息视为固定要求，除非用户后续明确修改，否则不要自行改动。

## 固定信息

- 前端开发端口：`13000`
- 后端 API 端口：`18080`
- API 基础前缀：`/api/v1`
- 前端调用 API 时，默认后端地址：`http://localhost:18080`
- 这个系统是“公开的翻译协作平台”，不是企业 OA，也不是营销官网

## 固定技术栈

必须使用下面这套技术栈：

- React
- TypeScript
- Vite
- TanStack Router
- TanStack Query
- Zustand
- Tailwind CSS

不要自行改成 Next.js、Vue、Angular 或其他方案。

## 产品定位

这是一个支持多个组织、多个项目的翻译协作平台。

每个项目只负责一条翻译线，例如：

- `en/jp/kr -> cn`
- `en -> cn`
- `cn -> en`

如果要做另一条翻译线，就新建项目，而不是在同一个项目里混用。

## 前端必须支持的能力

- 多组织
- 多项目
- 平台自身 i18n
- 用户可设置首选原文语言
- 一个条目展示多个原文
- 其中一个原文突出显示
- 其他原文默认折叠，但可以展开
- 项目维度的术语库
- 项目维度的翻译记忆库
- 文档标签显示
- 文档标签筛选
- 版本历史查看
- 导入任务查看
- 导出任务查看
- 权限展示

## 不需要做的东西

- 机翻界面
- Git 同步界面
- 截图上下文
- 花哨的宣传页
- 偏“产品介绍网站”的设计

## 设计方向

整体气质要偏：

- 工具型
- 信息密度高
- 可长时间使用
- 清晰、克制、稳定

不要做成：

- SaaS 营销官网风
- 大留白卡片堆叠
- 只有视觉没有效率

建议方向：

- 桌面应用感更强
- 左右结构明确
- 列表、筛选、编辑区并存
- 对高频操作友好
- 尽量减少多余跳转

## 视觉风格

请直接采用下面这套固定风格，不要自由发挥成别的方向。

### 颜色

- 主背景：偏冷浅灰，不要纯白刺眼
- 主文字：高对比深灰
- 强调色：偏蓝，不要紫色
- 成功态：绿色
- 警告态：橙色
- 危险态：红色
- 标签色：低饱和，但要能区分

### 字体

优先考虑适合中日韩混排的字体方案。

不要只用最普通的默认西文字体栈。

### 组件气质

- 边框清晰
- 圆角适中，不要太大
- 阴影克制
- 用颜色和层级区分，而不是堆很多花哨装饰

## 建议目录结构

请按这个方向组织前端代码：

```text
src/
  app/
  routes/
  layouts/
  pages/
  features/
    auth/
    organization/
    project/
    document/
    translation/
    glossary/
    tm/
    version/
    jobs/
    permission/
    settings/
  components/
  hooks/
  store/
  lib/
  api/
  styles/
  i18n/
```

## 建议至少实现的文件

```text
src/app/main.tsx
src/app/router.tsx
src/layouts/app-shell.tsx
src/routes/login.tsx
src/routes/organizations.tsx
src/routes/projects.tsx
src/routes/project-overview.tsx
src/routes/project-documents.tsx
src/routes/project-workbench.tsx
src/routes/project-glossary.tsx
src/routes/project-tm.tsx
src/routes/project-history.tsx
src/routes/project-jobs.tsx
src/routes/settings.tsx
src/features/translation/components/translation-table.tsx
src/features/translation/components/translation-editor.tsx
src/features/document/components/document-sidebar.tsx
src/features/document/components/tag-filter.tsx
src/features/project/components/project-switcher.tsx
src/features/organization/components/organization-switcher.tsx
src/features/settings/components/language-preference-form.tsx
src/api/client.ts
src/store/preferences.ts
src/i18n/index.ts
```

## 页面和功能要求

### 1. 登录页

需要实现：

- 登录表单
- 登录失败提示
- 登录成功后进入组织或项目入口

使用接口：

- `POST /api/v1/auth/login`

请求示例：

```json
{
  "email": "admin@example.com",
  "password": "your-password"
}
```

成功返回示例：

```json
{
  "success": true,
  "data": {
    "token": "jwt-or-session-token",
    "user": {
      "id": "user_001",
      "displayName": "Doctor",
      "preferredLocale": "zh-CN",
      "preferredSourceLanguage": "jp"
    }
  },
  "error": null,
  "requestId": "req_login_001"
}
```

演示账号可先使用：

```text
admin@example.com / admin123
reviewer@example.com / reviewer123
```

### 2. 当前用户与设置页

需要实现：

- 当前用户信息显示
- 平台界面语言切换
- 首选原文语言切换

接口：

- `GET /api/v1/me`
- `PATCH /api/v1/me/preferences`

请求示例：

```json
{
  "preferredLocale": "zh-CN",
  "preferredSourceLanguage": "jp"
}
```

### 3. 组织列表页

需要实现：

- 组织列表
- 进入组织下项目列表
- 组织基础信息展示

接口：

- `GET /api/v1/organizations`
- `GET /api/v1/organizations/:organizationId`

### 4. 项目列表页

需要实现：

- 某组织下项目列表
- 项目卡片或项目表格
- 项目语言信息显示
- 项目可见性显示

接口：

- `GET /api/v1/organizations/:organizationId/projects`

项目列表示例返回：

```json
{
  "success": true,
  "data": {
    "items": [
      {
        "id": "proj_001",
        "name": "Arknights-CN",
        "slug": "arknights-cn",
        "sourceLanguages": ["en", "jp", "kr"],
        "targetLanguage": "cn",
        "visibility": "public"
      }
    ],
    "total": 1
  },
  "error": null,
  "requestId": "req_projects_001"
}
```

### 5. 项目概览页

需要实现：

- 项目基本信息
- 语言配置
- 文档数量
- 条目统计
- 状态统计
- 最近导入导出任务概览

接口：

- `GET /api/v1/projects/:projectId`

### 6. 文档列表页

需要实现：

- 文档列表
- 标签显示
- 标签筛选
- 按路径搜索
- 进入文档相关条目
- 文档版本入口

接口：

- `GET /api/v1/projects/:projectId/documents`
- `GET /api/v1/projects/:projectId/tags`
- `GET /api/v1/projects/:projectId/documents/:documentId/versions`

文档列表示例返回：

```json
{
  "success": true,
  "data": {
    "items": [
      {
        "id": "doc_001",
        "path": "StoryData/S001.json",
        "title": "S001",
        "currentVersionNo": 12,
        "tags": [
          {
            "id": "tag_001",
            "code": "main",
            "name": "主线",
            "color": "#4C88FF",
            "isVisible": true
          }
        ],
        "updatedAt": "2026-04-19T12:00:00Z",
        "updatedBy": {
          "id": "user_001",
          "name": "Amiya"
        }
      }
    ],
    "total": 1
  },
  "error": null,
  "requestId": "req_documents_001"
}
```

### 7. 翻译工作台

这是最重要的页面，优先级最高。

必须支持：

- 文档切换
- 标签筛选
- key 搜索
- 状态筛选
- 原文搜索
- 译文搜索
- 首选原文优先显示
- 其他原文折叠
- 当前译文编辑
- 状态显示
- 最近修改信息
- 历史入口
- 校对和批准操作
- 按权限显示或隐藏按钮

接口：

- `GET /api/v1/projects/:projectId/units`
- `GET /api/v1/projects/:projectId/units/:unitId`
- `PATCH /api/v1/projects/:projectId/units/:unitId`
- `POST /api/v1/projects/:projectId/units/:unitId/review`
- `POST /api/v1/projects/:projectId/units/:unitId/approve`
- `GET /api/v1/projects/:projectId/units/:unitId/history`

条目列表示例返回：

```json
{
  "success": true,
  "data": {
    "items": [
      {
        "id": "tu_001",
        "key": "story.line.001",
        "document": {
          "id": "doc_001",
          "path": "StoryData/S001.json",
          "tags": [
            {
              "code": "main",
              "name": "主线",
              "color": "#4C88FF",
              "isVisible": true
            }
          ]
        },
        "sources": {
          "en": "Doctor, wake up.",
          "jp": "ドクター、起きてください。",
          "kr": "박사님, 일어나세요."
        },
        "target": {
          "language": "cn",
          "text": "博士，醒醒。"
        },
        "status": "translated",
        "version": 8,
        "updatedAt": "2026-04-19T12:00:00Z",
        "updatedBy": {
          "id": "user_01",
          "name": "Amiya"
        },
        "permissions": {
          "canView": true,
          "canEdit": true,
          "canReview": false,
          "canApprove": false
        }
      }
    ],
    "page": 1,
    "pageSize": 50,
    "total": 1
  },
  "error": null,
  "requestId": "req_units_001"
}
```

更新译文请求示例：

```json
{
  "targetText": "博士，醒醒。",
  "status": "translated",
  "comment": "按当前语境微调"
}
```

校对请求示例：

```json
{
  "status": "reviewed",
  "comment": "语气已统一"
}
```

批准请求示例：

```json
{
  "status": "approved",
  "comment": "可发布"
}
```

### 8. 术语库页

需要实现：

- 术语列表
- 搜索
- 新增
- 编辑
- 删除

接口：

- `GET /api/v1/projects/:projectId/glossary`
- `POST /api/v1/projects/:projectId/glossary`
- `PATCH /api/v1/projects/:projectId/glossary/:termId`
- `DELETE /api/v1/projects/:projectId/glossary/:termId`

### 9. 翻译记忆页

需要实现：

- 翻译记忆列表
- 搜索
- 新增
- 编辑
- 删除

接口：

- `GET /api/v1/projects/:projectId/tm`
- `POST /api/v1/projects/:projectId/tm`
- `PATCH /api/v1/projects/:projectId/tm/:entryId`
- `DELETE /api/v1/projects/:projectId/tm/:entryId`

### 10. 历史与版本页

需要实现：

- 文档版本列表
- 条目历史列表
- 谁在什么时候改了什么

接口：

- `GET /api/v1/projects/:projectId/documents/:documentId/versions`
- `GET /api/v1/projects/:projectId/units/:unitId/history`

### 11. 导入导出任务页

需要实现：

- 导入任务列表
- 导出任务列表
- 导出下载入口
- 任务状态展示
- 失败原因展示

接口：

- `POST /api/v1/projects/:projectId/imports`
- `GET /api/v1/projects/:projectId/imports`
- `POST /api/v1/projects/:projectId/exports`
- `GET /api/v1/projects/:projectId/exports`
- `GET /api/v1/projects/:projectId/exports/:exportJobId/download`

导出任务返回示例：

```json
{
  "success": true,
  "data": {
    "items": [
      {
        "id": "exp_001",
        "status": "finished",
        "fileName": "arknights-cn-export-20260419.zip",
        "expiresAt": "2026-04-20T12:00:00Z",
        "downloadUrl": "/api/v1/projects/proj_001/exports/exp_001/download"
      }
    ]
  },
  "error": null,
  "requestId": "req_exports_001"
}
```

### 12. 权限管理页

需要实现：

- 成员列表
- 角色信息展示
- 权限节点展示
- 用户权限覆盖展示
- 文档范围规则展示

接口：

- `GET /api/v1/projects/:projectId/members`
- `GET /api/v1/projects/:projectId/roles`
- `GET /api/v1/projects/:projectId/permissions`
- `PATCH /api/v1/projects/:projectId/members/:userId/permissions`
- `PATCH /api/v1/projects/:projectId/members/:userId/document-rules`

### 13. 个人设置页

需要实现：

- 平台语言设置
- 首选原文语言设置
- 个人基础信息显示

接口：

- `GET /api/v1/me`
- `PATCH /api/v1/me/preferences`

## API 通用规则

### 请求头

前端默认请求头建议：

```http
Content-Type: application/json
Accept: application/json
Authorization: Bearer <token>
```

如果后端改成 Cookie Session，也要在请求层做好统一处理。

### 统一返回格式

成功时：

```json
{
  "success": true,
  "data": {},
  "error": null,
  "requestId": "req_xxx"
}
```

失败时：

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

### 列表接口约定

列表型接口优先按这个结构处理：

```json
{
  "success": true,
  "data": {
    "items": [],
    "page": 1,
    "pageSize": 50,
    "total": 0
  },
  "error": null,
  "requestId": "req_list_001"
}
```

### 常见错误码

前端至少要处理这些错误码：

- `unauthorized`
- `forbidden`
- `permission_denied`
- `not_found`
- `validation_error`
- `conflict`
- `internal_error`

## 权限显示规则

默认角色：

- owner
- admin
- reviewer
- translator
- guest

但前端不要把权限硬编码成“只认角色”。

因为后端支持：

- 权限节点
- 角色权限包
- 用户单独权限覆盖
- 文档级白名单/黑名单

所以前端应当以“当前用户拥有哪些操作权限”为准，而不是仅仅根据角色名字判断按钮是否显示。

## 文档与标签

文档路径可能像这样：

- `StoryData/S001.json`
- `StoryData/S001B.json`
- `StoryData/S002.json`

文档可以有 tag。

tag 在前端可以：

- 显示成 label
- 用于筛选
- 在某些页面隐藏不显示

## 交付物要求

你需要产出：

- 清晰的页面结构
- 明确的组件拆分
- 状态管理方案
- API 接入方案
- 关键交互说明
- 你对后端还需要补充的字段或接口要求

## 最重要的提醒

把它做成“翻译工作台”，不是“好看的网页”。

优先保证：

- 高密度信息展示
- 快速筛选
- 快速编辑
- 长时间使用不累
- 中日韩文本显示稳定
