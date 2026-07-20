# CLAUDE.md — AI 协作指南（PRTS）

本文件供 Claude / Claude Code 等 AI 协作者在本仓库工作时阅读。OpenAI 系工具请同时参考 `AGENTS.md`（内容保持同步）。

## 1. 这是什么

**PRTS**（Process-Review-Translation System）= 开源的公开 L10N 协作平台，对标 Paratranz。

- **权威蓝图**：[`plan/26-06-28-init_system.md`](./plan/26-06-28-init_system.md) —— 动手前**必读**。
- **架构详述**：[`docs/architecture.md`](./docs/architecture.md)。
- **外部接入**：[`docs/external/oauth_integration.md`](./docs/external/oauth_integration.md)（ZOOT OAuth2）。

## 2. 技术栈（已定，勿擅自更换）

- 后端：Rust · tokio · **axum** · **sqlx** · PostgreSQL（`pg_trgm`/`pgvector`）· Redis。
- 前端：Vue 3 · Quasar 2 · Vite · **pnpm** · Pinia · vue-i18n。
- 文档：utoipa + Swagger UI。部署：Docker / docker-compose · **GHCR** · nginx。

## 3. 仓库地图

```
backend/crates/
  prts-api        # axum 路由、中间件、WS 入口、utoipa(Swagger)
  prts-core       # 领域逻辑：项目/文件/词条/CP/权限/历史（不依赖 Web/DB 框架细节）
  prts-auth       # AuthProvider 插件框架 + 内置 providers（password/oauth2/zoot）
  prts-search     # 混合搜索 + EmbeddingProvider 抽象
  prts-realtime   # WebSocket 会话、在线状态、Redis pub/sub
  prts-db         # sqlx 连接池/查询/实体
  prts-common     # 错误、配置、i18n、工具
frontend/src/     # pages / components / stores / i18n / api
deploy/           # docker-compose、Dockerfile、nginx
```

## 4. 关键设计约束（务必遵守）

- **词条状态**：工作流枚举 `未翻译→已翻译→已检查→已审核`；`questioned`（有疑问标签）、`locked`、`hidden` 是**正交标志位**，独立于工作流。`locked=true` 时**仅项目「管理」「拥有者」可改**。
- **权限**：权限节点 RBAC。平台：总管理员/管理员/维护者；项目：拥有者/管理/校对/翻译。翻译只能设 `未翻译/已翻译`，并可独立增删有疑问标签。
- **CP**：`权重 × Levenshtein(prev, new)`，翻译/编辑 1、校对 0.3，**不加任何抗刷分逻辑**。
- **语言码**：BCP-47（`zh-Hans`/`zh-Hant`/`en`/`ja`/`ko`…）。多源语言 → 单目标语言。
- **搜索**：FTS + `pg_trgm` + `pgvector`，RRF 融合；向量化经 `EmbeddingProvider`（默认 Qwen），不可用时降级。
- **性能**：单项目可达 20w+ 词条 —— 列表用**键集分页**（禁用大 `OFFSET`），建好 GIN/向量索引，批量上传分批事务。
- **并发**：编辑器实时协作（WebSocket）；保存做**乐观锁版本校验**。
- **安全**：平台基础设施密钥仅经环境变量；用户/项目 AI API Key 只以 `PRTS__AI__MASTER_KEY` 加密后入库，**绝不下发明文到前端**；sqlx 参数化；全程 HTTPS；最小权限；所有操作写 `audit_log`。
- **认证**：插件化（Trait + 编译期注册）；默认构建不含 OAuth，启用 `zoot-oauth` 后 ZOOT 作为 OAuth2 provider 实例并支持 `password+oauth` / `oauth-only`。

## 5. 工作约定

- **提交**：Conventional Commits（`feat:`/`fix:`/`docs:`/`refactor:`/`test:`/`chore:`…），信息清晰。
- **注释**：所有代码需注释且符合规范。**前端文案面向用户，不是开发者。**
- **API**：每个端点都要进 utoipa/Swagger，附详尽描述。
- **i18n**：前端中英双语；后端按 `Accept-Language` 返回本地化消息（错误返回错误码 + 消息）。
- **测试 / verify**：写单元/集成测试与验证代码；涉及规模处做性能验证。
- **每阶段闭环**：测试 → verify → 规范提交 → 推 GitHub → 构建并推 **GHCR** 镜像。

## 6. 常用命令

```bash
# 后端
cd backend && cargo fmt && cargo clippy --all-targets && cargo test
cargo run -p prts-api
sqlx migrate run

# 前端
cd frontend && pnpm install && pnpm lint && pnpm test && pnpm dev

# 全栈
docker compose -f deploy/docker-compose.yml up -d
```

## 7. 红线 / Do-Not

- ❌ 不擅自更换已定技术栈或引入重型依赖替代既有选型。
- ❌ 不把任何密钥写入代码、前端或提交。
- ❌ 不用大 `OFFSET` 翻页；不在热路径做实时 `COUNT(*)`。
- ❌ 不给 CP 加抗刷分；不把 `locked/hidden` 混进 `state` 工作流枚举。
- ❌ 不绕过权限节点校验与审计留痕。

## 8. 最重要的一条

> **本系统不确定性很大：任何实现细节拿不准，先询问作者，不要擅自假设。**（来自原始计划）

每完成一项任务，按 §5「每阶段闭环」推送到 GitHub 与 GHCR。
