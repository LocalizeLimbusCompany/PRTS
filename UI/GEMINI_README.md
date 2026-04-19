# Gemini 前端入口说明

你是这个项目的前端实现者。

开始动手之前，必须先看下面这些文档，顺序不要乱：

1. [UI/GEMINI_MISSION.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/UI/GEMINI_MISSION.md)
2. [docs/frontend-handoff.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/docs/frontend-handoff.md)
3. [docs/backend-spec.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/docs/backend-spec.md)
4. [docs/backend-architecture.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/docs/backend-architecture.md)
5. [AGENTS.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/AGENTS.md)
6. [README.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/README.md)

## 你的工作方式

- 以 `UI/GEMINI_MISSION.md` 为主
- 不要自行更改固定技术栈
- 不要把产品做成营销官网
- 要把它做成真正可用的翻译工作台
- 如果发现后端接口字段不够，请列出你需要补充的字段

## 固定前端技术栈

- React
- TypeScript
- Vite
- TanStack Router
- TanStack Query
- Zustand
- Tailwind CSS

## 固定联调信息

- 前端开发端口：`13000`
- 后端 API 端口：`18080`
- API 基础前缀：`/api/v1`
- 默认后端地址：`http://localhost:18080`

## 你最该关注的页面

- 翻译工作台
- 文档列表与标签筛选
- 版本历史
- 导入导出任务
- 权限展示与控制

其中“翻译工作台”优先级最高。

现在后端已经具备：

- 组织、项目、文档、标签、翻译条目、历史的读取接口
- 组织、项目、文档的基础创建和更新接口
- 翻译条目更新、校对、批准接口
- 项目成员、角色、权限节点、权限覆写、文档范围规则接口
- 翻译条目的 `permissions` 字段已经开始根据角色、权限覆写和文档规则计算
- glossary / tm / imports / exports / audit-logs 接口
- auth/login、auth/logout、me、me/preferences
- JSON 导入和项目 zip 导出已经可以真实工作

所以 Gemini 现在就可以开始做前端，不需要继续等待。
