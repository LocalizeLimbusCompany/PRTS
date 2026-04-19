# PRTS Paratranz Remake Translation System

<p align="left">
  <img src="https://img.shields.io/badge/Backend-Go_1.25+-00ADD8?style=flat-square&logo=go" alt="Go">
  <img src="https://img.shields.io/badge/Frontend-React_%2B_TypeScript-61DAFB?style=flat-square&logo=react" alt="React">
  <img src="https://img.shields.io/badge/Database-PostgreSQL-4169E1?style=flat-square&logo=postgresql" alt="Postgres">
  <img src="https://img.shields.io/badge/Cache-Redis-DC382D?style=flat-square&logo=redis" alt="Redis">
  <img src="https://img.shields.io/badge/Tooling-Docker-2496ED?style=flat-square&logo=docker" alt="Docker">
</p>

一个面向**公开协作场景**的现代翻译工作台。

它的重点不是机器翻译，也不是 Git 源码同步，而是专注于高效的翻译协作、精细的权限管理以及稳定的版本追踪。

### ✨ 核心特性

- 🏢 **多组织、多项目架构**：单一部署即可支持多个独立翻译组运行。
- 🌍 **多原文到单目标语言**：完美适配诸如 `EN/JP/KR -> CN` 的多源交叉翻译场景。
- 📦 **文档级 JSON 导入导出**：无缝对接游戏/软件等结构化本地化资源。
- 🏷️ **灵活的文档标签**：支持分类筛选与批量管理。
- 📜 **简单实用的版本历史**：清晰记录每一个翻译条目的变更轨迹（Who, When, What）。
- 🔐 **细粒度权限管控**：涵盖角色、单用户覆写、文档级范围限定以及多级审核流（译文 -> 校对 -> 批准）。

---

## 🏗️ 技术栈选型

**后端：**
- **Go** + **chi** (路由)
- **PostgreSQL** (核心数据) + **Redis** (缓存与会话)
- **Docker Compose** (编排)

**前端：**
- **React** + **TypeScript** + **Vite**
- **TanStack Router** (类型安全路由) + **TanStack Query** (服务端状态)
- **Zustand** (客户端状态)
- **Tailwind CSS** (样式原子化)

---

## 🚀 快速开始 (本地开发)

### 1. 环境准备
确保已安装 [Go 1.25.3+](https://go.dev/)、[Docker](https://www.docker.com/) 和 Docker Compose。

**克隆环境变量配置：**
```bash
# Linux / macOS
cp .env.example .env

# Windows PowerShell
Copy-Item .env.example .env
```

### 2. 启动基础设施与初始化数据库
启动依赖服务（PostgreSQL, Redis）并执行数据库自动迁移与种子数据填充：

```bash
# 启动依赖组件
docker compose up -d postgres redis

# 等待数据库就绪后，执行迁移与种子数据填充
go run ./cmd/migrate
```
*执行完毕后，数据库会自动创建表结构并写入演示用的组织、项目及账户数据。*

**🔑 默认演示账号：**
- 管理员：`admin@example.com` / `admin123`
- 校对员：`reviewer@example.com` / `reviewer123`

### 3. 启动应用

整理 Go 模块：
```bash
go mod tidy
```

开启两个终端分别启动 API 与后台 Worker：
```bash
# 终端 1: 启动 API 服务 (端口: 18080)
go run ./cmd/api

# 终端 2: 启动异步任务 Worker
go run ./cmd/worker
```

前端项目的启动请参考 `UI/GEMINI_README.md`，前端开发环境已在 `web/` 目录初始化完毕，默认端口为 `13000`。

---

## 📂 目录结构

```text
PRTS_TranslationSystem/
├── cmd/
│   ├── api/          # 后端 API 入口
│   ├── worker/       # 异步任务 Worker 入口
│   └── migrate/      # 数据库迁移工具
├── internal/
│   ├── app/          # 核心启动与装配逻辑
│   ├── config/       # 环境变量解析
│   ├── handlers/     # HTTP 请求处理
│   ├── middleware/   # 鉴权、日志等中间件
│   ├── platform/     # 基础工具与响应封装
│   └── store/        # 数据库仓储接口
├── db/
│   └── migrations/   # SQL 迁移文件
├── docs/             # 架构设计与需求文档
├── deploy/           # 生产环境部署脚本及 Compose 文件
├── web/              # 前端 React + Vite 项目代码
└── UI/               # 早期给 AI 助手提供的前端开发指令
```

---

## 🛠️ Docker Compose 完整启动

如果你不想在宿主机直接跑 Go，可以使用 Docker Compose 一键拉起完整环境（含 API、Worker、Postgres、Redis）：

```bash
docker compose up --build
```
*服务停止指令：`docker compose down`*

---

## 🚢 生产环境部署

本项目采用基于 **GitHub Actions + GHCR (GitHub Container Registry) + SSH** 的轻量级自动部署方案。
**核心原则：不在服务器上手动构建，也不手动同步源码。**

### 1. 服务器环境准备
```bash
# 创建部署目录并授权
sudo mkdir -p /opt/prts-translation-system
sudo chown -R $USER:$USER /opt/prts-translation-system
```

只需将仓库中的 3 个文件上传至服务器该目录下：
- `deploy/docker-compose.prod.yml`
- `deploy/deploy.sh`
- `.env` (需修改为生产环境配置，如真实的 `DATABASE_URL` 和 `POSTGRES_PASSWORD`)

### 2. GitHub Secrets 配置
在 GitHub 仓库设置中配置以下 Secrets，以激活 `deploy.yml` 工作流：
- `DEPLOY_HOST`: 服务器 IP 或域名
- `DEPLOY_USER`: SSH 登录用户名
- `DEPLOY_KEY`: SSH 私钥
- `DEPLOY_APP_DIR`: `/opt/prts-translation-system`

### 3. 自动化部署流程
每次将代码推送到 `main` (或 `master`) 分支后：
1. Actions 自动执行 `ci.yml` 验证代码。
2. Actions 构建最新的 Docker 镜像并推送至 `ghcr.io/<Your-Repo>`。
3. 通过 SSH 登录服务器执行 `deploy.sh`。
4. 服务器拉取最新镜像，执行平滑重启。

*如需更改镜像托管源，可在服务器的 `.env` 中指定 `APP_IMAGE=xxx`。*

---

<details>
<summary>💡 API 调试示例 (点击展开)</summary>

**健康检查**
```bash
curl http://localhost:18080/healthz
```

**获取登录 Token**
```bash
curl -X POST http://localhost:18080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"admin@example.com","password":"admin123"}'
```

**获取条目列表 (带 Token)**
```bash
curl -H "Authorization: Bearer <token>" \
  "http://localhost:18080/api/v1/projects/bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb/units?page=1&pageSize=50"
```

**上传 JSON 导入任务**
```bash
curl -X POST http://localhost:18080/api/v1/projects/bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb/imports \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d @example-import.json
```
</details>
