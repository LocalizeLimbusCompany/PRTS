# PRTS Translation System

一个面向公开协作场景的翻译平台。

它的重点不是机翻，也不是 Git 同步，而是：

- 多组织、多项目
- 多原文到单目标语言
- 文档级 JSON 导入导出
- 文档标签
- 简单实用的版本历史
- 细粒度权限

## 当前技术栈

后端固定为：

- Go
- chi
- PostgreSQL
- Redis
- Docker Compose

前端固定为：

- React
- TypeScript
- Vite
- TanStack Router
- TanStack Query
- Zustand
- Tailwind CSS

前端实现者请先看：

- [UI/GEMINI_README.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/UI/GEMINI_README.md)
- [UI/GEMINI_MISSION.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/UI/GEMINI_MISSION.md)
- [docs/frontend-handoff.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/docs/frontend-handoff.md)

后端规格请看：

- [docs/backend-architecture.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/docs/backend-architecture.md)
- [docs/backend-spec.md](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/docs/backend-spec.md)

## 固定端口

- 前端开发端口：`13000`
- 后端 API 端口：`18080`
- PostgreSQL 本地端口：`15432`
- Redis 本地端口：`16379`

## 目录说明

```text
cmd/
  api/        API 入口
  worker/     后台任务入口
internal/
  app/        启动逻辑
  config/     环境配置
  handlers/   HTTP 处理器
  httpserver/ 路由组装
  middleware/ 中间件
  platform/   通用返回和基础工具
db/
  migrations/ 数据库迁移
docs/         需求和架构文档
deploy/       生产部署文件
UI/           给 Gemini 的前端任务书
```

## 本地开发

### 1. 准备环境

需要先安装：

- Go 1.25.3 或更高版本
- Docker
- Docker Compose

复制环境变量：

```bash
cp .env.example .env
```

Windows PowerShell：

```powershell
Copy-Item .env.example .env
```

### 2. 启动依赖

```bash
docker compose up -d postgres redis
```

### 2.1 初始化数据库

```bash
go run ./cmd/migrate
```

Windows PowerShell 同样使用：

```powershell
go run ./cmd/migrate
```

### 3. 安装依赖并整理模块

```bash
go mod tidy
```

### 4. 启动 API

```bash
go run ./cmd/api
```

### 5. 启动 worker

另开一个终端：

```bash
go run ./cmd/worker
```

### 6. 验证接口

健康检查：

```bash
curl http://localhost:18080/healthz
```

接口元信息：

```bash
curl http://localhost:18080/api/v1/meta
```

登录演示账号：

```bash
curl -X POST http://localhost:18080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@example.com","password":"admin123"}'
```

组织列表示例：

```bash
curl http://localhost:18080/api/v1/organizations
```

项目列表示例：

```bash
curl http://localhost:18080/api/v1/organizations/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa/projects
```

翻译条目列表示例：

```bash
curl "http://localhost:18080/api/v1/projects/bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb/units?page=1&pageSize=50"
```

导入 JSON 示例：

```bash
curl -X POST http://localhost:18080/api/v1/projects/bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb/imports \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d @example-import.json
```

导出 zip 示例：

```bash
curl -X POST http://localhost:18080/api/v1/projects/bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb/exports \
  -H "Authorization: Bearer <token>"
```

## 使用 Docker Compose 本地完整启动

```bash
docker compose up --build
```

这会启动：

- API
- worker
- PostgreSQL
- Redis

## 生产部署

目标是“不手动复制项目代码到服务器”。

采用的方式是：

1. GitHub Actions 自动构建 Docker 镜像
2. 推送到 GHCR
3. 通过 SSH 在服务器上执行部署脚本
4. 服务器只拉新镜像并重启服务

也就是说，服务器上只需要保留：

- `deploy/docker-compose.prod.yml`
- `deploy/deploy.sh`
- `.env`

不需要你每次手动把整份代码复制过去。

## 生产服务器初始化

### 1. 服务器准备目录

```bash
sudo mkdir -p /opt/prts-translation-system
sudo chown -R $USER:$USER /opt/prts-translation-system
```

### 2. 上传部署文件

第一次只需要把下面几个文件放到服务器：

- `deploy/docker-compose.prod.yml`
- `deploy/deploy.sh`
- `.env`

建议目录结构：

```text
/opt/prts-translation-system/
  docker-compose.prod.yml
  deploy.sh
  .env
```

### 3. 服务器 `.env` 示例

```env
APP_NAME=prts-translation-system
APP_ENV=production
API_PORT=18080
DATABASE_URL=postgres://postgres:your-password@postgres:5432/prts_translation_system?sslmode=disable
REDIS_URL=redis://redis:6379/0
EXPORT_RETENTION=24h
EXPORT_CLEANUP_INTERVAL=1h
POSTGRES_PASSWORD=your-password
```

### 4. 首次部署命令

```bash
cd /opt/prts-translation-system
IMAGE_TAG=latest APP_DIR=/opt/prts-translation-system sh ./deploy.sh
```

## GitHub Actions 自动部署

仓库中已经准备了：

- [ci.yml](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/.github/workflows/ci.yml)
- [deploy.yml](D:/ZengXiaoPi/repo/PRTS_TranslationSystem/.github/workflows/deploy.yml)

需要在 GitHub Secrets 中配置：

- `DEPLOY_HOST`
- `DEPLOY_USER`
- `DEPLOY_KEY`
- `DEPLOY_APP_DIR`

部署流程是：

- 推送到 `main`
- Actions 构建镜像
- 推送到 `ghcr.io/<你的仓库名>`
- SSH 到服务器
- 执行 `deploy.sh`
- 服务器拉新镜像并重启

生产部署使用的镜像名默认为：

```text
ghcr.io/<你的 GitHub 仓库名>
```

如果你想换成别的镜像仓库，只需要在服务器 `.env` 或部署环境里设置：

```env
APP_IMAGE=ghcr.io/your-name/prts-translation-system
```

## 常用命令

整理依赖：

```bash
go mod tidy
```

编译 API：

```bash
go build ./cmd/api
```

编译 worker：

```bash
go build ./cmd/worker
```

启动本地完整环境：

```bash
docker compose up --build
```

停止本地环境：

```bash
docker compose down
```

查看服务日志：

```bash
docker compose logs -f api
docker compose logs -f worker
```

## 说明

当前仓库已经有：

- 后端最小可运行骨架
- PostgreSQL 初始化 SQL
- 演示种子数据
- organization / project / tag / document / version / translation unit / history 只读接口
- organization / project / document 的创建与更新接口
- 标签创建、更新、绑定、解绑接口
- 翻译条目更新、校对、批准接口
- 项目成员、角色、权限节点、权限覆写、文档范围规则接口
- 翻译条目的 `permissions` 字段已接入基础权限计算
- `auth/login`、`auth/logout`、`me`、`me/preferences` 已可用
- glossary / tm / imports / exports / audit-logs 接口已可用
- JSON 导入已真实写入文档、版本和翻译条目
- 项目导出已真实生成 zip 文件
- `go run ./cmd/migrate` 数据库迁移命令已可用
- 容器构建文件
- 本地开发方式
- 自动部署工作流
- 前端任务书

还没有开始实现：

- 数据库真实迁移
- 登录鉴权
- 项目、文档、翻译条目接口
- 权限计算
- 导入导出任务

这些就是下一阶段的实现重点。
