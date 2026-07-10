<div align="center">

# PRTS

**Process-Review-Translation System** · 开源的公开 L10N 协作平台

[简体中文](./README.md) · [English](./README.en.md)

</div>

---

PRTS 是一个面向汉化组与本地化团队的**公开、可扩展、高并发**的在线翻译协作平台——可理解为开源版的 [Paratranz](https://paratranz.cn)。贡献者在线完成**翻译、校对、审核**，平台提供权限管理、完整操作历史、贡献度量（CP）与混合搜索。

> 🚧 项目处于初始化阶段，规划见 [`plan/26-06-28-init_system.md`](./plan/26-06-28-init_system.md)，架构见 [`docs/architecture.md`](./docs/architecture.md)。

## ✨ 特性

- **项目 / 文件夹 / 文件 / 词条** 四级结构，单项目可承载 20w+ 词条。
- **多源语言 → 单目标语言**（BCP-47，区分简繁），按个人偏好显示源文。
- **实时协作编辑器**（WebSocket）：在线状态、他人编辑提示、乐观锁防冲突。
- **混合搜索**：PostgreSQL 全文检索 + 三元组模糊 + 向量语义（pgvector），RRF 融合，高级筛选。
- **权限节点 RBAC**：平台级（总管理员/管理员/维护者）+ 项目级（拥有者/管理/校对/翻译）。
- **贡献分 CP**：按编辑距离计分。
- **可插拔认证插件**：账号密码 + OAuth2（PKCE），内置 ZOOT 接入；支持「仅 OAuth」模式。
- **完整历史与审计**：所有操作留痕，管理后台可按时间段 / 项目清除。
- **国际化**：前端中英双语，后端按 `Accept-Language` 返回本地化消息。
- **全程 Docker 化**，API 全量进 Swagger 文档。

## 🧱 技术栈

| 层 | 选型 |
| --- | --- |
| 后端 | Rust · tokio · **axum** · **sqlx** |
| 数据 | PostgreSQL（`pg_trgm` / `pgvector`）· Redis |
| 前端 | Vue 3 · Quasar 2 · Vite · pnpm · Pinia · vue-i18n |
| 文档 | utoipa + Swagger UI |
| 部署 | Docker / docker-compose · GHCR · nginx |

## 🚀 快速开始（Docker）

```bash
git clone git@github.com:LocalizeLimbusCompany/PRTS.git
cd PRTS
cp .env.example .env        # 按需填写数据库 / Redis / JWT / OAuth / Qwen 等
docker compose -f deploy/docker-compose.yml up -d
```

数据库迁移账号与应用 runtime 账号必须分离。新空卷会按 `.env` 中的
`POSTGRES_MIGRATION_*` / `POSTGRES_RUNTIME_*` 自动创建 runtime role，并由一次性
`migrate` 服务执行迁移；backend 只接收 runtime URL。升级已有 PostgreSQL 卷时，需先由
数据库管理员创建同名的非 superuser runtime login role，再按 `.env.example` 设置两组 URL；
两者相同或 runtime 仍拥有表时，服务会 fail closed。真实凭据只写本地环境变量，不提交仓库。

启动后：

- 前端：`http://localhost:8080`
- API + Swagger：`http://localhost:3000/swagger-ui`

## 🛠️ 本地开发

```bash
# 后端
cd backend
cargo run -p prts-api

# 前端
cd frontend
pnpm install
pnpm dev
```

详见 [`docs/architecture.md`](./docs/architecture.md) 与各 crate / 模块内文档。

## 📁 目录结构

```
backend/    Rust workspace（api / core / auth / search / realtime / db / common）
frontend/   Vue3 + Quasar 前端
docs/       架构与外部接入文档
deploy/     Docker / compose / nginx
plan/       规划文档
```

## 🤝 贡献

提交遵循 [Conventional Commits](https://www.conventionalcommits.org)；代码需通过 `fmt` / `clippy` / `lint` 与测试。面向 AI 协作者的说明见 [`CLAUDE.md`](./CLAUDE.md) 与 [`AGENTS.md`](./AGENTS.md)。

## 📄 许可证

待定（TBD）。

---

预计部署域名：`prts.zeroasso.top`
