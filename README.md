<div align="center">

# PRTS

**Process-Review-Translation System** · 开源的公开 L10N 协作平台

[简体中文](./README.md) · [English](./README.en.md)

</div>

---

PRTS 是一个面向汉化组与本地化团队的**公开、可扩展、高并发**的在线翻译协作平台——可理解为开源版的 [Paratranz](https://paratranz.cn)。贡献者在线完成**翻译、校对、审核**，平台提供权限管理、完整操作历史、混合搜索与贡献度量（CP）。

> 🚧 项目工作区大改造的功能阶段已完成，当前处于最终验证与发布准备。权威蓝图见 [`plan/26-06-28-init_system.md`](./plan/26-06-28-init_system.md)，架构与验证边界见 [`docs/architecture.md`](./docs/architecture.md)。

## ✨ 特性

- **项目工作区**：信息、文件、任务、术语、下载与管理分区；编辑器使用独立全屏路由。
- **项目 / 文件夹 / 文件 / 词条** 四级结构，以物化统计、键集分页和批处理面向单项目 20w+ 词条目标。
- **多源语言 → 单目标语言**（BCP-47，区分简繁），按个人偏好显示源文。
- **实时协作编辑器**（WebSocket）：在线状态、他人编辑提示、乐观锁防冲突。
- **结构化混合搜索**：POST tagged scope + PostgreSQL 全文检索 + 三元组模糊 + 可选向量语义（pgvector），RRF 融合和签名键集游标。
- **持久化流式上传**：500 文件 / 2GB 批次合同、100MB 单文件上限、byte-zero retry、逐文件原子 replacement、取消/过期清理与 30 天可恢复历史。
- **权限节点 RBAC**：平台级（总管理员/管理员/维护者）+ 项目级（拥有者/管理/校对/翻译）。
- **贡献分与排行榜**：在线翻译/编辑按 Levenshtein 距离 × 1.0、校对/审核按 × 0.3 精确计分，提供项目累计榜与平台总榜、UTC 月榜和周榜。
- **可插拔认证插件**：账号密码 + OAuth2（PKCE），内置 ZOOT 接入；支持「仅 OAuth」模式。
- **历史与审计**：业务 mutation 与 allowlisted 脱敏审计同事务 fail-closed；文件变更集支持回滚/恢复，项目删除采用 owner-only challenge + 24 小时延迟清除。
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

**已有卷不会重新执行 PostgreSQL init 脚本。** 旧版 PRTS 卷的数据库/表 owner 通常是
`prts`，不能只把 `.env` 改成默认 `prts_migrator` 后指望该角色自动出现。升级时请选择一种：

- 推荐就地沿用真实旧 owner：先查询 `pg_database` / `pg_tables` 确认 owner（通常为 `prts`），
  再把 `POSTGRES_MIGRATION_USER`、`POSTGRES_MIGRATION_PASSWORD` 和
  `PRTS__DATABASE__MIGRATION_URL` 指向这个**已存在且拥有旧对象**的账号；runtime URL 仍只使用
  新建的 `prts_runtime`。
- 如必须改为专用 `prts_migrator`，请以旧 owner 或数据库管理员连接目标数据库，只创建缺失
  role，使用 `psql` 的交互式 `\password` 设置本地密码，然后转移所有权（旧 owner 不是
  `prts` 时替换下列名称）：

  ```psql
  CREATE ROLE prts_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION;
  \password prts_migrator
  CREATE ROLE prts_runtime LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION;
  \password prts_runtime
  REASSIGN OWNED BY prts TO prts_migrator;
  ALTER DATABASE prts OWNER TO prts_migrator;
  ALTER SCHEMA public OWNER TO prts_migrator;
  ```

完成后再让 migration URL 使用 `prts_migrator`。不要在 shell 历史、README、compose 文件或
仓库内写真实密码；`\password` 会交互提示，实际凭据只保存在本地 `.env` / secret manager。

### 认证会话升级提示

本版本会拒绝旧版签发、未携带 `sid` 的 access JWT，且 refresh 会话改由 PostgreSQL 作为权威来源。
因此升级会让所有现有会话失效，相当于一次全量登出。生产部署必须安排维护窗口并提前通知用户；
升级完成后，用户重新登录即可建立新会话并恢复访问，无需重置账号或密码。

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

### 验证

安装 PowerShell 7 后可从仓库根运行：

```powershell
pwsh -File scripts/verify-project-workspace.ps1
```

默认只做静态/自动合同检查。数据库与昂贵规模验证必须显式使用脚本开关并准备 PostgreSQL/Redis；没有实际运行输出时，不应把 20 万词条或 100MB 场景写成实测结果。

兼容期内后端仍保留 deprecated 的旧内联 upload 与 GET search；新前端只使用 upload-batches 和结构化 POST search。

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

[MIT](./LICENSE)

---

预计部署域名：`prts.zeroasso.top`
