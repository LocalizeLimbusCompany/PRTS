# PRTS 架构文档

> 本文是面向开发者的架构详述，与权威蓝图 [`plan/26-06-28-init_system.md`](../plan/26-06-28-init_system.md) 配套。决策依据见蓝图 §0「决策记录」。

## 1. 分层与 crate 职责

| Crate | 职责 | 不应包含 |
| --- | --- | --- |
| `prts-api` | axum 路由、中间件（鉴权/权限/限流/CORS）、WS 入口、utoipa 文档聚合、错误→HTTP/i18n 映射 | 领域规则 |
| `prts-core` | 领域逻辑：项目/文件/词条/状态机/CP/权限判定/历史。**纯逻辑，不依赖 axum/sqlx 细节** | HTTP、SQL 方言 |
| `prts-auth` | `AuthProvider` 框架 + `password`/`oauth2`/`zoot` 实现、JWT 签发/校验、API-Key | 业务权限 |
| `prts-search` | 混合检索编排（FTS+trgm+vector、RRF）、`EmbeddingProvider` 抽象与 Qwen 实现 | 路由 |
| `prts-realtime` | WS 连接管理、房间（按文件/项目）、在线状态、Redis pub/sub 广播 | 持久化 |
| `prts-db` | sqlx 连接池、迁移、查询、实体映射 | 领域规则 |
| `prts-common` | 错误类型、分层配置加载、i18n、通用工具 | 任何具体业务 |

**依赖方向**：`api → {core, auth, search, realtime} → {db, common}`；`core` 不反向依赖 `api`。

## 2. 请求生命周期（REST）

```
请求 → CORS/限流 → 鉴权(JWT/API-Key→Identity) → 权限节点校验 → 处理器
     → prts-core 领域逻辑 → prts-db(sqlx 事务) → 响应(DTO)
     → 错误统一映射为 {code, message(i18n), details}
     → 写 audit_log（旁路，不阻塞主流程）
```

## 3. 关键数据流

### 3.1 ZOOT 登录（OAuth2 + PKCE）

```
前端「用 ZOOT 登录」
 → prts-auth(zoot.begin)：生成 state+PKCE，存 Redis，返回 authorize URL
 → 浏览器跳 ZOOT 授权 → 回调带 code
 → zoot.complete：校验 state → /oauth/token 换 token → /oauth/userinfo
 → 映射：profile(username/picture/role)、work(work_scope/content→翻译类别)、external(github_id→external_account)
 → upsert user → 签发 PRTS 自有 JWT(access)+refresh(Redis)
```

ZOOT 端点与字段详见 [`external/oauth_integration.md`](./external/oauth_integration.md)。其它 OAuth 源只需以不同配置实例化通用 `OAuth2Provider`。

### 3.2 文件上传

```
上传 JSON [{key, original:{lang:text}, context, translation?, state?}]
 → 校验 + 分批解析
 → 对每个 key：存在则比对 → 覆盖原文、state=untranslated、写 entry_version(diff)
                 不存在则插入
 → COPY/批量事务入库 → 增量更新文件/项目状态计数 → 写 audit_log
```

### 3.3 混合搜索（P4）

**列与索引**（迁移 `0004`，触发器维护）：`entries` 增 `source_text`（主源语言文本）、`source_tsv`/`translation_tsv`（按语言选 `regconfig`，中文经 **zhparser**）、`embedding vector(1024)`、`embed_attempts`。索引：tsv GIN、`source_text`/`translation`/`key` 的 trgm GIN、`embedding` 的 HNSW(cosine)。触发器仅在**源文变化**时作废 `embedding`（译文编辑不触发重嵌）。

`GET /projects/{id}/search`：

```
q + 过滤(file/状态/排序) + 可见性(hidden 需编辑权限)
 → 查询期对 q 取一次 embedding（向量启用且配 key 时）
 → 并行三路：FTS(source_tsv/translation_tsv) | trgm(source_text/translation/key) | pgvector kNN
 → RRF 融合(k=60, 每路 top-100) → 有界 top-200 → offset/limit 窗口 → 取行返回(含 relevance)
（向量默认关闭 / key 缺失 / 调用失败 → 自动降级为 FTS+trgm）
```

**向量化**：`EmbeddingProvider`（默认 `QwenProvider`，DashScope OpenAI 兼容端点）。**默认关闭**；管理后台 `GET|PUT /admin/settings/search` 开关并配置 model/base_url/batch 与 TM 参数（存 `settings` 表 `search.config`，运行时热生效）；**API Key 仅经 env，绝不下发前端**（后台只显示「已配置:是/否」）。后台 **sweep worker** 分批回填 `embedding IS NULL` 的词条（batch ≤10，失败退避），覆盖 20w 存量，不阻塞主流程。

**TM 翻译建议** `GET /projects/{id}/entries/{entry_id}/suggestions`：从**当前用户已加入**、`target_lang` 一致的项目里，按源文相似度（向量 cosine，关则 trgm）召回状态≥已翻译、译文非空的词条（排除自身），≤ `tm_top_n`(默认 3) 条，供编辑器译文面板下方展示、点击填入。

### 3.4 实时编辑

```
进入文件 → WS 加入房间(file_id)
 → 广播 presence；编辑中广播「正在编辑」
 → 保存：乐观锁校验 version
        命中→更新+version+1+entry_version+CP+audit，向房间广播变更
        冲突→返回最新版本，前端提示合并
 → locked=true 的词条仅 manager/owner 可写
多实例：节点间经 Redis pub/sub 同步房间事件
```

## 4. 数据模型（ER 摘要）

```
user 1─* external_account
user 1─* api_key
user *─* project   (membership: role=owner|manager|reviewer|translator)
user 1─* platform_role (super_admin|admin|maintainer)

project 1─* folder (parent_id 自引用树)
project 1─* file
folder  1─* file
file    1─* entry
entry   1─* entry_version

audit_log  (actor_id, action, target, payload, created_at)  -- 追加式
setting    (分类的平台运行时配置：SMTP/注册/OAuth 模式/Embedding…)
```

`entry`：`key`、`original`(JSONB `{bcp47:text}`)、`context`、`translation`、`state`(枚举)、`locked`、`hidden`、`version`。

## 5. 性能要点（20w+ 词条）

- 列表：键集分页 `WHERE (file_id,id) > (:f,:cursor) ORDER BY id LIMIT n`。
- 索引：`entry(file_id,state)`、`entry(key)`、`tsvector` GIN、`pg_trgm` GIN、`pgvector` HNSW/IVFFlat。
- 计数：状态计数物化于 `file.stats` / `project`，增量维护，避免实时 `COUNT(*)`。
- 写放大：`audit_log` 按月分区或队列批写。

## 6. 配置与密钥

- 启动期配置（连接串、JWT 密钥、OAuth secret、Qwen key）→ **环境变量 / `.env`（不入库、不进前端）**。
- 运行时可调配置（SMTP、是否邮箱验证、注册开关、OAuth 模式、Embedding 开关）→ `setting` 表，管理后台维护。

## 7. 部署拓扑

```
nginx ──┬── frontend(静态, Quasar 构建)
        └── /api, /ws → backend(axum)
backend ── PostgreSQL(pg_trgm+pgvector)
        └── Redis(session/refresh/限流/WS pubsub)
镜像：GHCR；编排：deploy/docker-compose.yml
```

## 8. 测试策略

- `prts-core` 纯逻辑单测（状态机、权限判定、CP 计分、RRF）。
- API 集成测试（鉴权、权限、上传覆盖、搜索）。
- 性能验证：20w 词条的上传与分页/搜索基准。
- verify 脚本随阶段交付。
