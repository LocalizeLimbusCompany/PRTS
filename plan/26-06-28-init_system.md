# PRTS 初始化系统 · 规划文档（精修版）

> 本文件由 `26-06-28-init_system_raw.md` 精修而来，是 PRTS 项目的**权威蓝图**。
> 原始草稿保留于 `plan/26-06-28-init_system_raw.md` 作为历史。

| 项 | 值 |
| --- | --- |
| 项目 | **PRTS** · Process-Review-Translation System |
| 定位 | 开源版 Paratranz —— 公开、可扩展、高并发、线程安全、国际化的 L10N 协作平台 |
| 作者 | ZengXiaoPi |
| 日期 | 2026-06-28 |
| 状态 | 规划已定稿，待分阶段实施 |
| 代码仓库 | `git@github.com:LocalizeLimbusCompany/PRTS.git`（master） |
| 预计域名 | `prts.zeroasso.top` |
| 参考 | Paratranz（paratranz.cn）；ZOOT OAuth 文档见 `docs/external/oauth_integration.md` |

---

## 0. 决策记录（ADR 摘要）

本次规划与作者确认的全部关键决策，后续实现以此为准：

| # | 决策点 | 结论 |
| --- | --- | --- |
| D1 | 代码仓库 | `LocalizeLimbusCompany/PRTS`，首次以 `git push -u --force` 覆盖 master |
| D2 | 本次交付范围 | 仅**规划 + 基础文档**（不写业务代码） |
| D3 | 后端技术栈 | **axum + sqlx**（tokio / PostgreSQL / Redis） |
| D4 | 前端技术栈 | **Vue 3 + Quasar 2 + Vite + pnpm**（vue-i18n） |
| D5 | 搜索方案 | PG 全文(`tsvector`) + `pg_trgm` + `pgvector` 混合，RRF 融合；向量化走可插拔 `EmbeddingProvider`，默认 **Qwen 云 API** |
| D6 | 插件系统 | **Trait 抽象 + 编译期注册**（`AuthProvider` 等），ZOOT 作为内置 provider |
| D7 | CP 计分 | `CP = 权重 × Levenshtein 距离`；翻译/编辑 = 1，校对 = 0.3；**不设抗刷分机制** |
| D8 | 注册策略 | 开放注册；**管理后台可配 SMTP + 是否开启邮箱验证**；全局开关支持 `password+oauth` / `oauth-only` |
| D9 | 镜像库 | **GHCR**（ghcr.io） |
| D10 | 语言代码 | **BCP-47**（`en` / `ja` / `ko` / `zh-Hans` / `zh-Hant` …），界面显示本地化名称 |
| D11 | 项目可见性 | **默认公开**（游客只读浏览）+ **可设私有**（仅成员可见） |
| D12 | 编辑器并发 | **实时协作（WebSocket）** + 在线状态 + 保存乐观锁版本校验 |
| D13 | i18n 范围 | 前端 zh-CN / en；后端按 `Accept-Language` 返回本地化消息 |
| D14 | 部署 | 全程 Docker 化（compose + 各服务 Dockerfile），镜像推 GHCR |
| D15 | 词条状态 | 工作流枚举 + `locked`/`hidden` 正交标志位（见 §6） |

---

## 1. 愿景与范围

PRTS 是一个面向汉化组（及任意本地化团队）的**公开 L10N 平台**，让贡献者在线协作完成文本翻译、校对、审核，并以可控的权限、完整的历史与贡献度量进行项目管理。核心诉求：

- **现代化 / 可扩展 / 鲁棒**：清晰分层、插件化扩展点、完善的错误与日志体系。
- **高并发 / 线程安全 / 数据安全**：异步 Rust 栈、事务与乐观锁、最小权限、密钥隔离。
- **维护便利 / 国际化**：Monorepo + Docker + Swagger + 中英双语文档与界面。
- **面向用户的前端文案**：UI 文案面向译者，而非开发者。

**非目标（YAGNI，当前不做）**：运行时动态加载的外部插件（WASM/动态库）、机器翻译引擎自研、移动端原生 App、付费/计费体系。

---

## 2. 技术栈

| 层 | 选型 |
| --- | --- |
| 后端语言/运行时 | Rust + tokio（异步） |
| Web 框架 | axum |
| 数据层 | sqlx（编译期校验的异步 SQL，配 `sqlx migrate`） |
| 数据库 | PostgreSQL（含 `pg_trgm`、`pgvector` 扩展） |
| 缓存/会话/广播 | Redis（refresh token、限流、WS 跨实例 pub/sub） |
| API 文档 | utoipa + Swagger UI |
| 前端 | Vue 3 + Quasar 2 + Vite + pnpm + Pinia + vue-i18n |
| 实时 | WebSocket（axum） + Redis pub/sub |
| 向量化 | 可插拔 `EmbeddingProvider`，默认 Qwen 云 API |
| 部署 | Docker / docker-compose；镜像 GHCR；反代 nginx |
| CI/CD | GitHub Actions（lint + test + build + 推镜像） |

---

## 3. 仓库结构（Monorepo）

```
PRTS/
├─ backend/
│  ├─ Cargo.toml                 # workspace
│  ├─ crates/
│  │  ├─ prts-api/               # axum 路由、中间件、utoipa(Swagger)、WS 入口
│  │  ├─ prts-core/              # 领域逻辑：项目/文件/词条/CP/权限/历史
│  │  ├─ prts-auth/              # AuthProvider 插件框架 + 内置 providers(password/oauth2/zoot)
│  │  ├─ prts-search/            # 混合搜索 + EmbeddingProvider 抽象
│  │  ├─ prts-realtime/          # WebSocket 会话、在线状态、Redis pub/sub
│  │  ├─ prts-db/                # sqlx 连接池、查询、实体
│  │  └─ prts-common/            # 错误类型、配置、i18n、工具
│  └─ migrations/                # sqlx 迁移脚本
├─ frontend/
│  ├─ package.json               # pnpm
│  └─ src/                       # Vue3 + Quasar：pages / components / stores / i18n / api
├─ docs/
│  ├─ architecture.md            # 架构详述
│  └─ external/oauth_integration.md   # ZOOT 接入文档（已有）
├─ deploy/
│  ├─ docker-compose.yml         # backend + frontend + postgres + redis
│  ├─ docker-compose.dev.yml     # 本地开发覆盖
│  ├─ backend.Dockerfile
│  ├─ frontend.Dockerfile
│  └─ nginx/
├─ .github/workflows/            # CI：lint/test/build/push GHCR
├─ CLAUDE.md  AGENTS.md          # 面向 AI 协作（Vibe Coding）
├─ README.md  README.en.md       # 中 / 英
└─ .gitignore
```

---

## 4. 架构总览

```
            ┌─────────────────────────── 浏览器 (Vue3 + Quasar) ───────────────────────────┐
            │  REST (JSON)            WebSocket (实时编辑/在线状态)         Swagger UI         │
            └───────┬─────────────────────────┬───────────────────────────────┬─────────────┘
                    │                          │                               │
            ┌───────▼──────────────────────────▼───────────────────────────────▼─────────────┐
            │                           prts-api (axum)                                        │
            │  鉴权中间件 · 权限节点校验 · 限流 · 请求校验 · 错误→i18n · utoipa 文档            │
            └───┬─────────┬─────────────┬───────────────┬───────────────┬─────────────────────┘
                │         │             │               │               │
          prts-auth   prts-core    prts-search     prts-realtime     prts-db
          (Provider)  (领域逻辑)   (FTS+trgm       (WS+presence      (sqlx)
                                   +pgvector/RRF)   +Redis pubsub)
                │                       │                               │
                ▼                       ▼                               ▼
        外部 OAuth(ZOOT)         Qwen Embedding API              PostgreSQL · Redis
```

设计原则：每个 crate 单一职责、通过明确接口通信、可独立测试。领域逻辑（`prts-core`）不依赖具体 Web/DB 框架细节，便于替换与单测。

---

## 5. 数据模型

### 5.1 核心实体

- `user`：`id`、`username`、`email`、`password_hash?`、`avatar_url`、`description`、`uid`(展示用)、`joined_at`、`cp`(累计贡献分)。
- `external_account`：`user_id`、`provider`(github/zoot…)、`external_id`、`raw`(JSONB)；用于"关联账号"。
- `api_key`：`user_id`、`name`、`key_hash`、`scopes`、`created_at`、`last_used_at`（明文仅创建时展示一次）。
- `project`：`id`、`slug`、`name`、`visibility`(public/private)、`source_langs`(BCP-47 数组)、`target_lang`(BCP-47)、`owner_id`、`created_at`。
- `folder`：`id`、`project_id`、`parent_id?`(自引用树)、`name`、`path`。
- `file`：`id`、`project_id`、`folder_id?`、`name`、`format`、`stats`(词条数/各状态计数，物化)。
- `entry`（最小翻译单位）：
  - `id`、`file_id`、`key`、`original`(JSONB：`{ "<bcp47>": "源文本" }`)、`context`、`translation`、
  - `state`（工作流枚举，见 §6）、`locked`(bool)、`hidden`(bool)、
  - `version`(乐观锁递增)、`updated_by`、`updated_at`。
- `entry_version`：每次改动的快照（`entry_id`、`version`、`translation`、`state`、`editor_id`、`diff`、`created_at`）。
- `audit_log`：追加式全操作历史（见 §8）。
- `membership`：`project_id`、`user_id`、`role`(owner/manager/reviewer/translator)。
- `platform_role`：`user_id`、`role`(super_admin/admin/maintainer)。
- `setting`：平台配置（SMTP、注册开关、OAuth 模式、Embedding 开关等），分类存储。

### 5.2 应对单项目 20w+ 词条的性能策略

- **键集分页（keyset / cursor）**：列表按 `(file_id, id)` 游标翻页，避免大 `OFFSET`。
- **索引**：`entry(file_id, state)`、`entry(key)`、`original/translation` 的 `tsvector` GIN、`pg_trgm` GIN、`pgvector` 的 IVFFlat/HNSW。
- **统计物化**：文件/项目各状态计数增量维护（触发器或应用层），避免实时 `COUNT(*)`。
- **批量上传**：分批事务 + `COPY`；解析与入库分离，超大文件后台任务化。
- **只读浏览缓存**：公开项目的列表/统计走 Redis 缓存。

---

## 6. 词条状态模型（更新）

**工作流状态（`state` 枚举，单值）**：

```
未翻译 untranslated → 已翻译 translated → 有疑问 questioned / 已检查 checked → 已审核 reviewed
```

**正交标志位（独立于工作流）**：

| 标志 | 语义 | 谁能改 |
| --- | --- | --- |
| `locked` 已锁定 | 锁定该词条，独立于翻译流程 | **仅项目「管理」与「拥有者」** 可修改被锁词条 |
| `hidden` 已隐藏 | 从常规列表/翻译视图中隐藏 | 管理/拥有者设置 |

- 角色对工作流状态的设置权限由**权限节点**约束（见 §7）：翻译可设 `未翻译/已翻译/有疑问`；校对可设 `已检查/已审核` 并修改已审核文本。
- `locked=true` 时，除「管理」「拥有者」外任何人（含校对/翻译）均不可修改该词条的译文或状态。
- 上传/下载的 `state` 字段只映射工作流枚举；`locked`/`hidden` 为平台内独立属性。

---

## 7. 权限模型（平台 + 项目，权限节点 RBAC）

底层为**权限节点**，角色 = 节点集合，可细粒度调整。

### 7.1 平台级

| 角色 | 能力 |
| --- | --- |
| 总管理员 super_admin | 全部；可任免管理员 |
| 管理员 admin | 创建/删除/管理所有项目；平台设置 |
| 维护者 maintainer | 创建项目 |

### 7.2 项目级

| 角色 | 能力 |
| --- | --- |
| 拥有者 owner | 项目内全部，含删除项目、改被锁词条 |
| 管理 manager | 同拥有者，但**不可删除项目**；可改被锁词条 |
| 校对 reviewer | 设 `已检查/已审核`、修改已审核文本；翻译能力 |
| 翻译 translator | 编辑 `未翻译/已翻译/有疑问` 词条并设这三种状态；**不可**设 `已检查/已审核`，不可改被锁词条 |

> 补全了草稿中被截断的「翻译」角色定义。

### 7.3 权限节点示例

`platform.admin.grant`、`platform.project.create`、`project.manage`、`project.delete`、`project.member.manage`、`project.entry.edit`、`project.entry.review`、`project.entry.lock`、`project.entry.hide`、`project.file.upload`、`project.download`、`project.history.purge` …

---

## 8. 历史与审计

- **所有操作**（登录、上传、编辑、状态变更、权限变更、删除等）写入追加式 `audit_log`：`actor_id`、`action`、`target`(project/file/entry…)、`payload`(JSONB)、`ip`、`created_at`。
- 词条改动额外写 `entry_version`，展示**变更与差异（diff）**。
- 管理后台可**按时间段 + 按项目**清除操作历史（受 `project.history.purge` / 平台权限约束）。
- 设计为高写入：分区表（按月）或批量写入队列，避免阻塞主流程。

---

## 9. 认证与插件系统

### 9.1 AuthProvider 插件框架（Trait 抽象 + 编译期注册）

```rust
/// 认证提供方插件接口。各 provider 编译期注册到注册表，按配置启用。
trait AuthProvider {
    fn id(&self) -> &str;                 // "password" | "oauth2" | "zoot" ...
    fn kind(&self) -> ProviderKind;       // Password | OAuth2
    async fn begin(&self, ctx) -> BeginResult;     // 发起（如 OAuth 跳转 URL + PKCE）
    async fn complete(&self, ctx) -> Identity;     // 回调换取并归一化为平台身份
}
```

- 内置：`PasswordProvider`、通用 `OAuth2Provider`（Authorization Code + PKCE/S256）。
- **ZOOT = `OAuth2Provider` 的一个配置实例 + 映射器**：
  - `profile` → `username` / 头像 `picture` / `role`
  - `work` → 翻译类别 `work_scope` / `work_content`（如英翻/韩翻/日翻）
  - `external` → `github_id` 等 → 写入 `external_account`（关联账号）
- 全局模式：`password+oauth`（两者皆可）或 `oauth-only`（禁用密码）。

### 9.2 注册与会话

- 开放注册；**管理后台可配置 SMTP，并可开关"是否要求邮箱验证"**（未配 SMTP 即跳过验证）。
- 会话：自签 **JWT access**（短期）+ **不透明 refresh**（存 Redis，可吊销）。
- 密码哈希用 Argon2id。

### 9.3 用户页面

- 自身：描述、API-Key（创建/吊销，明文仅一次）、翻译语言偏好。
- 展示：参与的项目、关联账号（先支持 GitHub）、UID、加入时间、CP。

---

## 10. CP（贡献分）

- 触发：所有翻译、编辑、校对操作。
- 公式：`CP_gain = weight × levenshtein(prev_translation, new_translation)`
  - 首次翻译：`prev` 视为空串，`d = len(new_translation)`。
  - 权重：翻译/编辑 `1.0`，校对 `0.3`。
- **不设任何抗刷分机制**（无封顶、无短时合并、无每日上限）—— 按作者要求保持纯线性。
- 用户 `cp` 字段累加；提供项目贡献榜。

---

## 11. 翻译语言

- 一个项目：**多源语言 + 单一目标语言**（如 `en, ja, ko -> zh-Hans`，或 `en -> zh-Hans`）。
- 语言码采用 **BCP-47**（`en` / `ja` / `ko` / `zh-Hans` / `zh-Hant` …），界面显示本地化名称，天然区分简繁与地区，并支持其他主流语言。
- 用户可设置个人翻译语言偏好，翻译面板按偏好顺序展示源语言。

---

## 12. 混合搜索

- 三路召回 + **RRF（Reciprocal Rank Fusion）融合**：
  1. PG 全文检索（`tsvector`，按语言配置分词）
  2. `pg_trgm` 模糊匹配（容错/子串）
  3. `pgvector` 语义检索
- 向量化：`EmbeddingProvider` 抽象，默认 **Qwen 云 API**（密钥仅服务端；不可用时自动降级为 FTS+trgm）。
- **高级筛选**：所在文件/目录、词条状态、源/译文关键字、键名；多条件组合 + 多种排序方式。

---

## 13. 实时翻译编辑器（WebSocket）

- 布局：左侧词条列表（键集分页 + 搜索/筛选），中间翻译面板（按用户偏好显示源语言、上下文、键值）。
- 实时：WebSocket + Redis pub/sub 跨实例广播 —— 在线状态、"他人正在编辑此词条"、词条变更实时刷新。
- 并发安全：保存时**乐观锁版本校验**（`version` 不匹配则提示冲突）；`locked` 词条对非管理/拥有者只读。

---

## 14. 上传 / 下载

### 14.1 上传格式（修正草稿中的 JSON 错误）

```json
[
  {
    "key": "1234",
    "original": { "ko": "韩文原文", "ja": "日文原文" },
    "context": "可选上下文",
    "translation": "",
    "state": "untranslated"
  }
]
```

- 至少需 `key` + `original`（建议含 `context`）。
- **key 相同 → 覆盖原文、`state` 重置为 `untranslated`，并在词条历史记录更改与差异（diff）。**

### 14.2 下载

- 项目详情页下载 zip：**保留文件夹结构**，每文件输出 JSON，仅含 `key` / `original` / `translation`。

---

## 15. 配置与安全

- **分层配置**：环境变量（密钥、连接串）+ 数据库存储的运行时设置（SMTP、注册开关、OAuth 模式、Embedding 开关等），并分类管理。
- 密钥（DB/Redis/JWT/Qwen/OAuth secret）仅经环境变量注入，**绝不下发前端**，`.gitignore` 完全覆盖 `.env`。
- 安全：全程 HTTPS、CSRF/CORS 策略、输入校验、SQL 参数化（sqlx）、限流、最小权限、审计留痕。
- 写 **verify 验证代码**（见 §17）。

---

## 16. API 与 Swagger

- 所有 API 归入 **utoipa/Swagger** 文档，含详尽描述，供内部协作。
- 用户可获取**自身权限范围内**的 API 文档，并用自己的 **API-Key** 调用（`key_hash` 存储、scope 限定）。

---

## 17. 工程规范

- **代码全注释且符合规范**：后端 `cargo fmt` + `clippy`；前端 ESLint + Prettier；前端文案面向用户。
- **提交规范**：Conventional Commits（`feat:` / `fix:` / `docs:` / `refactor:` / `chore:` …）。
- **测试 + verify**：单元/集成测试；关键路径写验证脚本（含 20w 词条性能验证）。
- **`.gitignore` 完全**：Rust `target/`、Node `node_modules/`、构建产物、`.env`、IDE、日志等。
- **CLAUDE.md / AGENTS.md**：向 AI 协作者交代项目地图、约定、命令、注意事项。

---

## 18. 部署（Docker / GHCR）

- `docker-compose`：`backend` + `frontend(nginx)` + `postgres(含扩展)` + `redis`；另有 `docker-compose.dev.yml` 供本地开发。
- 各服务独立 Dockerfile（后端多阶段构建静态二进制；前端构建后 nginx 托管）。
- **每完成一个阶段：测试 → verify → 提交(规范) → 推 GitHub → 构建并推 GHCR 镜像**，供生产（`prts.zeroasso.top`）拉取。

---

## 19. 实施路线图

> 每阶段验收闭环：通过测试与 verify → 规范提交 → 推 GitHub → 推 GHCR 镜像。

| 阶段 | 内容 |
| --- | --- |
| **P0 基础设施** | Monorepo、Cargo workspace、axum 骨架、sqlx + 迁移、Vue/Quasar 脚手架、docker-compose、CI、utoipa/Swagger、配置/日志/错误/i18n 骨架 |
| **P1 账号与权限** | 注册/登录（密码 + JWT）、AuthProvider 框架 + ZOOT 插件、权限节点 RBAC、平台/项目角色、用户主页、API-Key |
| **P2 项目与文件系统** | 项目/文件夹/文件/词条 模型、上传（解析 + key 覆盖 + 历史 diff）、下载导出、词条 CRUD、状态机 + locked/hidden |
| **P3 实时编辑器** | 词条列表（键集分页 + 搜索）、翻译面板、WebSocket 实时协作与在线状态、乐观锁 |
| **P4 混合搜索** | FTS + pg_trgm + pgvector（Qwen Embedding 可插拔）+ RRF + 高级筛选 |
| **P5 历史与审计** | 全操作审计日志、按时间/项目清除、词条历史与差异 |
| **P6 CP 与贡献** | 编辑距离计分、用户 CP、项目贡献榜 |
| **P7 完善** | i18n 全量、文档（README 中英 + docs）、安全加固、20w 词条性能压测、verify |

---

## 20. 未决细节（实现时再与作者确认）

- 权限节点的**完整清单**与各角色默认集合的最终勾选。
- 贡献榜的展示口径（时间范围、按项目/全平台）。
- 上传支持的**文件格式集合**（先 JSON；是否需要 i18n properties / PO / CSV 等导入器）。
- 邮件模板与多语言；通知体系是否纳入 v1。
- `pgvector` 索引类型（IVFFlat vs HNSW）与维度，依所选 Qwen 模型确定。

> 遵循草稿要求：实现细节有不确定的，一律先询问作者。
