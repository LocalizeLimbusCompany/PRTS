# PRTS 初始化系统 · 规划文档（精修版）

> 本文件由 `26-06-28-init_system_raw.md` 精修而来，是 PRTS 项目的**权威蓝图**。
> 原始草稿保留于 `plan/26-06-28-init_system_raw.md` 作为历史。
> 2026-07-10 项目工作区大改造的直接作者决定见 [`docs/superpowers/specs/2026-07-10-project-workspace-overhaul-design.md`](../docs/superpowers/specs/2026-07-10-project-workspace-overhaul-design.md)；该总纲是精确生命周期、状态矩阵、可见性谓词和 API truth table 的唯一规范源，对本蓝图相关摘要具有更高优先级。

| 项 | 值 |
| --- | --- |
| 项目 | **PRTS** · Process-Review-Translation System |
| 定位 | 开源版 Paratranz —— 公开、可扩展、高并发、线程安全、国际化的 L10N 协作平台 |
| 作者 | ZengXiaoPi |
| 日期 | 2026-06-28 |
| 状态 | 工作区大改造阶段 1–7.3 已实现；阶段 8 最终验证与发布准备中 |
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
| D16 | 排行榜口径 | 项目当前成员累计榜；平台累计总榜、UTC 自然月榜、周一开始的 UTC 自然周榜 |

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

- `user`：`id`、`username`、`email`、`password_hash?`、`password_change_required`、`avatar_url`、`description`、`uid`(展示用)、`joined_at`、`cp_tenths`（贡献分的精确 0.1 单位，`BIGINT`）。
- `external_account`：`user_id`、`provider`(github/zoot…)、`external_id`、`raw`(JSONB)；用于"关联账号"。
- `api_key`：`user_id`、`name`、`key_hash`、`scopes`、`created_at`、`last_used_at`（明文仅创建时展示一次）。
- `project`：`id`、`slug`、`name`、`visibility`(public/private)、`source_langs`(BCP-47 数组)、`primary_source_lang`、`target_lang`(BCP-47)、唯一 `owner_id`、搜索重建状态、头像 key、待删除时间、`created_at`。
- `folder`：`id`、`project_id`、`parent_id?`(自引用树)、`name`、`path`、软删除与 `deletion_change_set_id`。
- `file`：`id`、`project_id`、`folder_id?`、`name`、`format`、软删除与 `deletion_change_set_id`、`stats`（可见词条数/各状态计数，物化）。
- `entry`（最小翻译单位）：
  - `id`、`file_id`、`key`、`original`(JSONB：`{ "<bcp47>": "源文本" }`)、`translation`、
  - `state`（工作流枚举，见 §6）、`locked`(bool)、`hidden`(bool)、`deleted_at?`、
  - `version`(乐观锁递增)、`updated_by`、`updated_at`。
- `entry_version`：每次改动的快照（`entry_id`、`version`、`translation`、`state`、`editor_id`、`diff`、`created_at`）。
- `audit_log`：追加式、action allowlisted 的安全审计（见 §8）。
- `membership`：`project_id`、`user_id`、`role`(owner/manager/reviewer/translator)、`cp_tenths BIGINT NOT NULL DEFAULT 0`（当前成员的项目累计贡献分）。
- `platform_role`：`user_id`、`role`(super_admin/admin/maintainer)。
- `setting`：平台运行时配置（SMTP、注册开关、OAuth 模式、Embedding 开关、上传的文件数/单文件大小/批次大小/浏览器并发等），分类存储。
- `job`：上传、主源 lexical、Embedding、文件清理与项目清除的持久化状态/阶段/进度/租约/重试；`project_id` 可空并 `ON DELETE SET NULL`，删除后所需 project/media snapshot 存 payload。
- `task` / `task_file` / `task_baseline_entry`：immutable file/entry snapshot IDs + nullable live FKs（永久删除 SET NULL），历史基线可解释且 NULL live ref 退出分母。
- `term` / `pos_preset`：带任意合法 canonical `source_lang`/归档状态的项目术语与 zh-CN/en 全局词性；非当前主源语言只能 archived，legacy old-primary 保持 migration-ready。
- `project_stats` / `file_stats`：排除 hidden 与 soft-deleted 的可见状态计数。

### 5.2 应对单项目 20w+ 词条的性能策略

- **键集分页（keyset / cursor）**：列表按 `(file_id, id)` 游标翻页，避免大 `OFFSET`。
- **索引**：`entry(file_id, state)`、`entry(key)`、`original/translation` 的 `tsvector` GIN、`pg_trgm` GIN、`pgvector` 的 IVFFlat/HNSW。
- **统计物化**：文件/项目按总纲 §3 的 effective visibility 集合增量维护；hidden overlay 不包含 entry tombstone、deleted file 或 deleted ancestor folder；正常读路径不实时 `COUNT(*)`/`GROUP BY entries`。
- **批量上传**：原始 JSON 文件流式进入后端临时卷，持久化 batch/job 后逐文件原子解析与完整替换；浏览器不解析内容。
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
| 总管理员 super_admin | 全部平台级能力；可任免管理员，并按平台管理能力跨项目操作；但不能冒充项目拥有者执行主源变化或项目删除 |
| 管理员 admin | 创建项目、按平台管理能力管理所有项目、管理平台设置；但不能冒充项目拥有者执行主源变化或项目删除 |
| 维护者 maintainer | 创建项目 |

### 7.2 项目级

| 角色 | 能力 |
| --- | --- |
| 拥有者 owner | 项目内全部，含主源变化、删除项目、改被锁词条；本轮不提供拥有者转让 |
| 管理 manager | 项目内管理能力，含改被锁词条；**不含主源变化、项目删除或拥有者转让** |
| 校对 reviewer | 设 `已检查/已审核`、修改已审核文本；翻译能力 |
| 翻译 translator | 编辑 `未翻译/已翻译/有疑问` 词条并设这三种状态；**不可**设 `已检查/已审核`，不可改被锁词条 |

> 补全了草稿中被截断的「翻译」角色定义。
>
> 2026-07-10 增补：`projects.owner_id` 是唯一拥有者。拥有者只可授予 manager/reviewer/translator，管理只可授予 reviewer/translator；任何端点不直接授予 owner，本轮不提供 owner 转让。API 返回 capabilities，前端不得从角色字符串自行推导。主源变化与项目删除只认 `owner_id`，平台管理员不能替代。

### 7.3 权限节点示例

`platform.admin.grant`、`platform.project.create`、`project.manage`、`project.delete`、`project.member.manage`、`project.entry.edit`、`project.entry.review`、`project.entry.lock`、`project.entry.hide`、`project.file.upload`、`project.download`、`project.history.view`、`project.history.rollback` …

---

## 8. 历史与审计

- 登录/登出/认证失败、成功令牌签发与所有业务 mutation 写入追加式 `audit_log`；普通读取不审计，敏感管理员导出/清除除外。
- 词条改动额外写 `entry_version`，展示**变更与差异（diff）**。
- 文件操作与重传写行为完整、字段 allowlisted 的 change set/delta；entry payload 永不捕获 context。成员可查看，owner/manager 可回滚。恢复 payload 只保留到 deleted file/folder 永久清除，之后不可恢复；audit 元数据无恢复正文。
- `audit_log` 保持追加式，不因项目清除而级联删除；项目清除后保留必要目标快照元数据。
- 审计 payload 按 action allowlist 脱敏，不存密码/hash、OAuth/token/API key、challenge answer、raw upload 或完整源文/译文。所有 repository writer 提供 transaction-scoped 接口，route 不写裸 SQL；业务 mutation 与审计同 PostgreSQL 事务。`0007` 建 DB-authoritative auth session/intents/outbox，只存 refresh hash/opaque handle；refresh/rotation/revoke 先查 DB state，Redis 仅缓存。issuance/rotation commit 后才返回 token，logout/revoke DB commit 即失效，Redis 清理失败由 durable worker 重试。
- 上传、主源重建、Embedding、文件保留期清理与项目清除使用持久化 `job`，记录阶段、进度、租约、错误和同一 job 重试。

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
- 会话：自签 **JWT access**（短期）+ **不透明 refresh**；refresh hash/state 以 PostgreSQL 为权威，Redis 仅缓存/加速，可吊销且 crash 后由 durable intent/outbox 收敛。
- 密码哈希用 Argon2id。

### 9.3 用户页面

- 自身：描述、API-Key（创建/吊销，明文仅一次）、翻译语言偏好、修改密码。
- 管理员直接建号带持久化初始密码提醒；提醒不阻止使用，成功修改密码后清除。
- 展示：参与的项目、关联账号（先支持 GitHub）、UID、加入时间与真实累计 CP；管理列表不展示无业务意义的 CP 列。

---

## 10. CP（贡献分）

- 长期公式保持：`CP_gain = weight × levenshtein(prev_translation, new_translation)`
  - 首次翻译：`prev` 视为空串，`d = len(new_translation)`。
  - 权重：翻译/编辑 `1.0`，校对 `0.3`。
- **不设任何抗刷分机制**（无封顶、无短时合并、无每日上限）—— 按作者要求保持纯线性。
- 平台/项目累计 CP 使用 exact tenths integer：`users.cp_tenths BIGINT` 与 `memberships.cp_tenths BIGINT NOT NULL DEFAULT 0`，一单位代表 0.1 CP；只追加 `contribution_events` 支撑 UTC 自然月榜/周榜，不引入十进制 crate/sqlx decimal feature。
- 在线词条保存计分：目标状态为 `checked/reviewed` 时权重 0.3，其它状态为翻译/编辑权重 1.0；批量上传、回滚、恢复、系统任务固定 0 CP。项目展示当前成员累计榜；平台展示总榜、UTC 月榜和周一开始的 UTC 周榜。

---

## 11. 翻译语言

- 一个项目：**多源语言 + 单一目标语言**（如 `en, ja, ko -> zh-Hans`，或 `en -> zh-Hans`）。
- 语言码采用 **BCP-47**（`en` / `ja` / `ko` / `zh-Hans` / `zh-Hant` …），后端以 `language-tags` 统一校验/规范化：language 小写、script Titlecase、region 大写，variant/extension/private-use 按 parser 规范序列化；界面显示本地化名称。
- 项目 create/update 的 source/primary/target、上传 original JSON keys、term import/CRUD、search source selector 和用户语言偏好全部在入口规范化；invalid 或 canonical duplicate 拒绝。foundation durable repair 按批规范化 legacy project/entry/term/user 数据；冲突或 invalid 数据标 `needs_language_resolution`，禁用 search/普通语言 edits，只有 owner resolution UI/API 可显式处理，平台 admin 只有无正文诊断/retry。
- 每个 repair-ready 项目有非空 canonical `primary_source_lang` 且属于 `source_langs`；legacy unresolved 行只在条件约束下暂存且不进入普通 search/语言管理。`0008+0009`、language repair、primary search trigger/backfill 与 lexical worker 必须同一 foundation release 完成；`0009` exact JSON lookup 只处理 repair-ready 项目，之后才开放非首主源或已有项目更新。
- 已有项目只有唯一 `owner_id` 可改主源，真正变化 7 天冷却；相同值无副作用。lexical 与 embedding 使用独立 job/state；lexical ready 即恢复 FTS/trgm，provider 未配置标 degraded/skipped，配置 provider 失败只重试原 embedding stage。精确矩阵见总纲 §4.3。
- 移除当前主源必须同请求给替代值；项目已有词条后目标语言不可改。
- 用户可设置个人翻译语言偏好，翻译面板按偏好顺序展示源语言。

---

## 12. 混合搜索

- 三路召回 + **RRF（Reciprocal Rank Fusion）融合**：
  1. PG 全文检索（`tsvector`，按语言配置分词）
  2. `pg_trgm` 模糊匹配（容错/子串）
  3. `pgvector` 语义检索
- 向量化：`EmbeddingProvider` 抽象，默认 **Qwen 云 API**（密钥仅服务端；不可用时自动降级为 FTS+trgm）。
- 主接口为结构化 `POST /projects/{id}/search`；旧 GET 适配同一 service 并保留一个兼容周期。
- 默认稳定分页 `(rrf_score DESC, entry_id ASC)`，opaque versioned cursor 绑定 URL project_id + query/filter/scope fingerprint + score/id；错误、跨项目或跨查询 cursor 400，limit 1..100，响应含 next_after。path 精确解析 file 或 segment-boundary folder subtree，禁 naive prefix。
- **高级筛选**：AND 条件；字段和操作符同既定范围；scope 使用总纲 §8.2 的 `type` tagged union（path/file/current_file/current_task 均带必需 payload），项目 route 校验归属/可见性/deletion；vector 默认 false。
- 普通搜索使用总纲 §3 effective visibility；owner/manager 的 include_hidden 只覆盖 hidden，绝不包含 entry/file/folder deletion。主源 lexical 重建中暂停，lexical ready 后先恢复 FTS+trgm。

---

## 13. 实时翻译编辑器（WebSocket）

- 布局：左侧词条列表（键集分页 + 搜索/筛选），中间翻译面板（按用户偏好显示源语言与键值）；词条模型不保留 context。
- 实时：WebSocket + Redis pub/sub 跨实例广播 —— 在线状态、"他人正在编辑此词条"、词条变更实时刷新。
- 并发安全：保存时**乐观锁版本校验**（`version` 不匹配则提示冲突）；`locked` 词条对非管理/拥有者只读。
- 右下恰好为状态下拉 + 一个智能按钮；服务端给 owner/manager 下发 `force_save_presence` capability，按钮只检查该 capability；强制保存仅越过 presence 占用提示，仍校验版本。
- 公开项目游客可进入只读编辑器，不保存、改状态、加入可写 presence、poke 或私信。

---

## 14. 上传 / 下载

### 14.1 上传格式（修正草稿中的 JSON 错误）

```json
[
  {
    "key": "1234",
    "original": { "ko": "韩文原文", "ja": "日文原文" },
    "translation": "",
    "state": "untranslated"
  }
]
```

- 最终 UI 选择一个或多个本地原始 JSON 文件，并从项目现有文件夹中选择目标目录（可选项目根目录）；不提供本地目录选择器或粘贴文本框，浏览器不解析内容。上传四项限制由设置 API 下发。V1 不支持 Range/offset 续传；重试在同 logical file 新建 attempt 并从 byte zero 开始，batch 可取消，未完成 batch 默认 24h 过期并由 durable cleanup 清 temp。
- 同路径重传是完整 replacement：缺失旧 key 可恢复软删除；既有平台译文保留；源文变化时状态重置未翻译；上传 translation/state 只 seed 从未存在的新 key。
- 每文件流式解析并原子提交，batch 允许部分成功；重复 key 拒绝该文件并返回位置。原始上传文件只作临时任务输入，不进入历史。
- `0008` foundation 仅预建 nullable deletion columns；legacy delete 仍硬删除并维护 stats，deletion_change_set_id 全 NULL，不提供恢复。`0010` 断言全 NULL并创建历史/FK后才切换：文件夹删除以同一 change-set 标记 subtree/descendant files，restore 只清本操作标记且不清 entry tombstone。FK 使用 RESTRICT/SET NULL；软删除默认 30 天，到期按业务行叶到根→items→sets 清除。

### 14.2 下载

- 项目详情页下载 zip：**保留文件夹结构**，每文件输出 JSON，仅含 `key` / `original` / `translation`。
- 普通导出使用总纲 §3 effective visibility；owner/manager 的 include_hidden 也不穿透 entry/file/folder deletion。

### 14.3 项目删除

- 删除 UI 先确认不可逆后果/24h/待删除只读，再要求完整项目 slug 精确匹配，之后才请求数学 challenge；两次确认只是前端门槛。仅唯一 `owner_id` 可通过绑定 user+project、短 TTL、一次性消费的 Redis 整数 challenge 安排删除；平台设置选择标准整数高数题或简单整数算术题，后端最终校验 owner 与答案。
- 正确答案安排 24 小时后的持久化 purge，不立即级联。待删除项目从列表消失、只读、普通 jobs 暂停；唯一 owner 可看倒计时并取消。
- 到期先在 DB 事务写 audit、detach/cancel 其它 jobs，按文件 30 天 purge 的兼容顺序显式清理全部 file history/tree，再清 tasks/terms/memberships 等关系并删除项目；不得依赖含糊 cascade 穿过 RESTRICT FK。purge job 以 nullable project_id 存活。提交后幂等清 media/temp，外部失败只重试同 job 且不复活项目；audit 保留无恢复正文的删除元数据。

---

## 15. 配置与安全

- **分层配置**：环境变量（密钥、连接串、媒体/临时卷路径）+ 数据库存储的运行时设置（SMTP、注册开关、OAuth 模式、Embedding、上传限制、文件保留期、删除题型等），并分类管理。
- 密钥（DB/Redis/JWT/Qwen/OAuth secret）仅经环境变量注入，**绝不下发前端**，`.gitignore` 完全覆盖 `.env`。
- 安全：全程 HTTPS、CSRF/CORS 策略、输入校验、SQL 参数化（sqlx）、限流、最小权限、审计留痕。
- 写 **verify 验证代码**（见 §17）。

---

## 16. API 与 Swagger

- 所有 API 归入 **utoipa/Swagger** 文档，含详尽描述，供内部协作。
- 用户可获取**自身权限范围内**的 API 文档，并用自己的 **API-Key** 调用（`key_hash` 存储、scope 限定）。
- 结构化搜索以 `POST /projects/{id}/search` 为主接口；OpenAPI 中 `SearchScope` 是拒绝未知字段、以 `type` 为 discriminator 的封闭 union，file/task ID 沿用 `BIGINT`/Rust `i64`。
- 旧内联 upload 与旧 GET search 保留一个兼容周期，并在 OpenAPI 标为 deprecated；新前端不再调用它们，生产入口按 method/path 观察调用量后再决定退役。
- 词条 schema 与 file history payload 不包含 `context`；所有 BCP-47 写入入口复用 `prts-core` 共享 canonicalizer。

---

## 17. 工程规范

- **代码全注释且符合规范**：后端 `cargo fmt` + `clippy`；前端 ESLint + Prettier；前端文案面向用户。
- **界面约束**：交互 UI 只用 Vue 3 + Quasar；浅/深主题、MDI、方角/2–4px；中文用 Noto Sans SC 同类 sans，JetBrains Mono 仅 code/key/number 且以 CJK sans fallback，不使用 SimSun/serif UI。
- **提交规范**：Conventional Commits（`feat:` / `fix:` / `docs:` / `refactor:` / `chore:` …）。
- **测试 + verify**：单元/集成测试；关键路径写验证脚本（含 20w 词条性能验证）。
- **`.gitignore` 完全**：Rust `target/`、Node `node_modules/`、构建产物、`.env`、IDE、日志等。
- **CLAUDE.md / AGENTS.md**：向 AI 协作者交代项目地图、约定、命令、注意事项。

---

## 18. 部署（Docker / GHCR）

- `docker-compose`：`backend` + `frontend(nginx)` + `postgres(含扩展)` + `redis`，并挂载 media/upload-temp 持久卷；另有 `docker-compose.dev.yml` 供本地开发。
- 各服务独立 Dockerfile（后端多阶段构建静态二进制；前端构建后 nginx 托管）。
- **每完成一个阶段：测试 → verify → 提交(规范) → 推 GitHub → 构建并推 GHCR 镜像**，供生产（`prts.zeroasso.top`）拉取。
- 阶段 8 使用 `scripts/verify-project-workspace.ps1` 区分默认静态/自动合同、可选 DB 检查与显式手动规模实测；未实际执行的昂贵场景不得声明为通过。
- 特性分支可完成提交、推送和 CI 验证；真正合并/推送 `master`、发布 GHCR 或部署生产前必须经过明确发布确认，禁止 force push。

---

## 19. 实施路线图

> 每阶段验收闭环：通过测试与 verify → 规范提交 → 推 GitHub → 推 GHCR 镜像。
>
> 原始 P0–P7 用于系统首轮建设。2026-07-10 工作区大改造按 [`docs/superpowers/plans/2026-07-10-project-workspace-overhaul.md`](../docs/superpowers/plans/2026-07-10-project-workspace-overhaul.md) 执行：基线/文档 → foundation（含不可拆分 `0008+0009`、trigger/backfill worker）→ A route/UI exposure → B → C → D → E → F → 全量发布。

| 阶段 | 内容 |
| --- | --- |
| **P0 基础设施** | Monorepo、Cargo workspace、axum 骨架、sqlx + 迁移、Vue/Quasar 脚手架、docker-compose、CI、utoipa/Swagger、配置/日志/错误/i18n 骨架 |
| **P1 账号与权限** | 注册/登录（密码 + JWT）、AuthProvider 框架 + ZOOT 插件、权限节点 RBAC、平台/项目角色、用户主页、API-Key |
| **P2 项目与文件系统** | 项目/文件夹/文件/词条 模型、上传（解析 + key 覆盖 + 历史 diff）、下载导出、词条 CRUD、状态机 + locked/hidden |
| **P3 实时编辑器** | 词条列表（键集分页 + 搜索）、翻译面板、WebSocket 实时协作与在线状态、乐观锁 |
| **P4 混合搜索** | FTS + pg_trgm + pgvector（Qwen Embedding 可插拔）+ RRF + 高级筛选 |
| **P5 历史与审计** | 追加式安全审计、词条历史、文件行为 change set/delta 与保留期内可逆回滚 |
| **P6 CP 与贡献** | 按既定编辑距离公式原子计分；项目累计榜，以及平台总榜、UTC 自然月榜与自然周榜 |
| **P7 完善** | i18n 全量、文档（README 中英 + docs）、安全加固、20w 词条性能压测、verify |

当前项目工作区改造已完成 foundation 与 A–F（阶段 1–7.3）的实现和特性分支 CI；阶段 8 依次完成契约/文档、规模/故障恢复/安全验证与兼容发布准备。生产结果只以实际保存的 verify、CI、Docker health、Swagger 和部署冒烟输出为准。

---

## 20. 后续范围边界

- 权限节点以服务端 capabilities 和 2026-07-10 owner/manager/terms/tasks/history 规则为当前基线；扩大能力须另作作者决定。
- 排行榜口径已确认：项目当前成员累计榜；平台累计总榜、UTC 自然月榜、周一开始的 UTC 自然周榜。
- 工作区上传本轮只支持 PRTS JSON 文件；properties/PO/其它格式属于独立导入器范围。
- SMTP 邮件模板与更广泛的通知策略不属于本轮工作区改造；现有通知/私信功能不得回退。
- 当前搜索使用 HNSW 与 1024 维 Qwen 配置；更换索引或维度须迁移数据并单列 ADR。

> 遵循草稿要求：实现细节有不确定的，一律先询问作者。
