# P4 · 混合搜索 + TM 建议 —— 设计 Spec

| 项 | 值 |
| --- | --- |
| 阶段 | P4（搜索）+ 翻译建议（TM），同一 spec |
| 基线 | `master` @ `a4ec807` |
| 日期 | 2026-06-29 · 作者 ZengXiaoPi · 设计协作 Claude |
| 前置阅读 | [`CLAUDE.md`](../../../CLAUDE.md)、蓝图 [`plan/26-06-28-init_system.md`](../../../plan/26-06-28-init_system.md) §12；原接力文件未随当前仓库保留 |

> **历史实现基线：** 本文件描述已落地 `0004_search.sql` 的 P4 设计，不是当前主源生命周期规范。其中 `source_langs[1]` 只说明旧迁移/旧触发器行为，已由 [`2026-07-10 规范总纲`](./2026-07-10-project-workspace-overhaul-design.md) 与计划中的 `0009_primary_source_search.sql` 明确取代；不得把该表达复制到新运行时代码。
>
> 其余设计约束以蓝图为准。实现阶段如再遇不确定 → 先问作者。

---

## 1. 范围

**本 spec 交付（P4）**
1. **混合搜索**：把现有 `ILIKE` 升级为 **FTS（zhparser 中文分词）+ `pg_trgm` + `pgvector` 三路召回 + RRF 融合**，新端点 `GET /projects/{id}/search`。
2. **TM 翻译建议**：编辑器译文面板下方展示 ≤3 条「与原文相似」的既有译文，跨项目（仅用户已加入项目）召回。
3. **向量化默认关闭**：管理后台可开关并配置向量化相关参数（密钥仅 env）。无向量时整链降级为 FTS + trgm。

**明确不在本 spec（拆为后续独立 spec，各自 spec→plan→实现）**
- **Spec B（编辑器 + 实时增强）**：保存按钮逻辑改造（未改译文→检查/审核键、已审核→灰）、左侧列表显示编辑者头像、管理/owner 强制保存。
- **Spec C（私信 / 通知子系统）**：点头像发消息 + 右上角通知提示（蓝图 §20 把通知列为 v1 未决，独立评估）。
- ja/ko 分词器；查询 embedding 的 Redis 缓存；批量上传 `UNNEST/COPY` 优化（P7）。

---

## 2. 决策摘要（已与作者确认）

| 维度 | 决定 | 不可逆性 / 备注 |
| --- | --- | --- |
| 嵌入模型 | Qwen **text-embedding-v4 @ 1024** | 维度迁移期固定为 `vector(1024)`，改维度=重迁移+全量重嵌 |
| 嵌入对象 | **主源语言**文本 `original ->> source_langs[0]` | 仅源文变化才重嵌；译文编辑不触发 |
| 嵌入生成 | **后台 sweep worker**，batch ≤10，失败退避 | 天然处理 20w 存量回填 |
| 向量索引 | **HNSW**（`vector_cosine_ops`） | 对全 NULL 即时建成，随嵌入到位填充 |
| 搜索端点 | 新 `GET /projects/{id}/search`，有界 top-K | `/entries`（键集浏览）保持不变 |
| 中文 FTS | **zhparser**（自定义 PG 镜像） | ja/ko 暂用 `simple`，由 trgm/向量兜底 |
| 向量开关 | **默认关闭**；管理后台开关 + 配置（非密参数） | 配置存 DB `settings` 表 |
| API Key | **仅 env**，后台只显示「已配置:是/否」 | 红线：绝不下发前端 |
| TM 建议来源 | **跨项目**，**仅用户已加入项目**，同 `target_lang` | 隐私边界 = membership |
| TM 建议门槛 | 状态 ≥ 已翻译 且 译文非空 | 排除自身词条 |
| TM 建议条数 | ≤ 3 | 后台 `tm_top_n` 可配（1..3） |

---

## 3. 数据模型与迁移 `0004_search.sql`

给 `entries` 增列（**全部由触发器维护**，应用层不手写）：

| 列 | 类型 | 用途 |
| --- | --- | --- |
| `source_text` | `TEXT NOT NULL DEFAULT ''` | 反范式化的主源语言文本，trgm/向量/FTS 源侧统一基底 |
| `source_tsv` | `tsvector` | 源侧 FTS（按主源语言选 config） |
| `translation_tsv` | `tsvector` | 译侧 FTS（按 `target_lang` 选 config） |
| `embedding` | `vector(1024)` NULL | 源文本语义向量；NULL = 待嵌入 |
| `embed_attempts` | `SMALLINT NOT NULL DEFAULT 0` | 失败退避计数，避免永久失败行被反复抓取 |

**语言 → TS 配置映射**（`IMMUTABLE`，供触发器复用）：

```sql
CREATE EXTENSION IF NOT EXISTS zhparser;
CREATE TEXT SEARCH CONFIGURATION prts_zh (PARSER = zhparser);
ALTER TEXT SEARCH CONFIGURATION prts_zh ADD MAPPING FOR n,v,a,i,e,l,j WITH simple;

CREATE FUNCTION prts_ts_config(lang text) RETURNS regconfig
LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE
    WHEN lang LIKE 'zh%' THEN 'prts_zh'::regconfig
    WHEN lang LIKE 'en%' THEN 'english'::regconfig
    ELSE 'simple'::regconfig          -- ja/ko/其它：暂不分词，trgm+向量兜底
  END
$$;
```

**触发器**（`BEFORE INSERT OR UPDATE`）：join `projects` 取主源语言与目标语言，重算三列；**仅当 `source_text` 变化（或 INSERT）时**把 `embedding` 置 NULL 并重置 `embed_attempts`（这是「只嵌源文」的关键红利——译文连续编辑不会触发重嵌）：

```sql
CREATE FUNCTION entries_search_maintain() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE src_lang text; tgt_lang text; new_src text;
BEGIN
  SELECT p.source_langs[1], p.target_lang INTO src_lang, tgt_lang
    FROM projects p WHERE p.id = NEW.project_id;
  new_src := COALESCE(NEW.original ->> src_lang, '');
  IF TG_OP = 'INSERT' OR new_src IS DISTINCT FROM OLD.source_text THEN
    NEW.embedding := NULL;
    NEW.embed_attempts := 0;
  END IF;
  NEW.source_text     := new_src;
  NEW.source_tsv      := to_tsvector(prts_ts_config(COALESCE(src_lang,'')), new_src);
  NEW.translation_tsv := to_tsvector(prts_ts_config(COALESCE(tgt_lang,'')), COALESCE(NEW.translation,''));
  RETURN NEW;
END $$;

CREATE TRIGGER entries_search_maintain_trg
  BEFORE INSERT OR UPDATE ON entries
  FOR EACH ROW EXECUTE FUNCTION entries_search_maintain();
```

> 注：触发器对 `projects` 做主键查找（PG 会缓存，开销小）。译文-only 保存会顺带重算 `source_tsv`（略冗余但无害）。后续如需，可把项目语言反范式化到 entries 以省去查找——本期不做。

**索引**：

```sql
CREATE INDEX entries_source_tsv_idx       ON entries USING gin (source_tsv);
CREATE INDEX entries_translation_tsv_idx  ON entries USING gin (translation_tsv);
CREATE INDEX entries_source_trgm_idx      ON entries USING gin (source_text gin_trgm_ops);
CREATE INDEX entries_translation_trgm_idx ON entries USING gin (translation gin_trgm_ops);
CREATE INDEX entries_key_trgm_idx         ON entries USING gin (key gin_trgm_ops);
CREATE INDEX entries_embedding_hnsw_idx   ON entries USING hnsw (embedding vector_cosine_ops);
```

**存量回填**（迁移内，显式计算，不依赖触发器副作用；`embedding` 保持 NULL → sweep 补）：

```sql
UPDATE entries e SET
  source_text     = COALESCE(e.original ->> p.source_langs[1], ''),
  source_tsv      = to_tsvector(prts_ts_config(COALESCE(p.source_langs[1],'')), COALESCE(e.original ->> p.source_langs[1],'')),
  translation_tsv = to_tsvector(prts_ts_config(COALESCE(p.target_lang,'')), COALESCE(e.translation,''))
FROM projects p WHERE p.id = e.project_id;
```

> 20w+ 表：实现时按 `id` 区间分批 UPDATE，避免长事务/锁表。

---

## 4. 中文 FTS 与自定义 Postgres 镜像（zhparser）

- 新增 `deploy/postgres.Dockerfile`：`FROM pgvector/pgvector:pg16` → 编译安装 **SCWS + zhparser**（对应 PG16）。
- `deploy/docker-compose.yml`：`postgres` 服务由 `image:` 改为 `build:` 该 Dockerfile，并打 GHCR 标签 `ghcr.io/localizelimbuscompany/prts-postgres`；CI 构建并推送该镜像。
- **CI 改动（关键）**：现 db-tests 用 `pgvector/pgvector`（无 zhparser），迁移 `CREATE EXTENSION zhparser` 会失败。db-tests job 必须改用这个自定义镜像（build 或 pull GHCR）。
- 子项（实现默认，可在评审时否决）：分词扩展选 **zhparser**（最常用、配 SCWS）；备选 pg_jieba。

---

## 5. `EmbeddingProvider` + `QwenProvider`（`prts-search`）

扩展现有 trait（新增 async `embed`，保留 `id()`/`dimensions()`）：

```rust
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn id(&self) -> &str;
    fn dimensions(&self) -> usize;                 // = 1024
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
}
```

- `QwenProvider`：`reqwest`（已是 workspace 依赖，沿用 `prts-auth/oauth2.rs` 模式）POST `{base_url}/embeddings`（DashScope OpenAI 兼容），body `{ model, input: [..], dimensions: 1024 }`，`bearer api_key`；解析 `data[].embedding`。
- **批量 ≤10**：调用方（worker / 查询）按 10 分块。
- `EmbedError`：`Http` / `Api(status, msg)` / `Parse`。
- 新依赖：`async_trait`（轻量、标准）、`pgvector`（带 `sqlx` feature，用于 `Vec<f32>` ↔ `vector` 绑定）。
- **配置来源划分（消歧）**：`embed(texts)` 签名不带配置。`QwenProvider` 持 `reqwest` client + key（构造期从 **env** 读，决定 `AppState` 中 `Arc<Option<…>>` 的 `Some`/`None`）+ model/base_url/batch（来自 **settings 快照**）。settings 中这些非密参数变更时，**重建并热替换** AppState 内实例（Arc swap），故改动免重启生效；`embedding_enabled` 开关则由 worker/查询每轮重读。

---

## 6. 配置与降级

### 6.1 env（仅密钥 + 维度基线）
- `PRTS__EMBEDDING__QWEN__API_KEY`（**唯一秘密**，仅 env）。
- `PRTS__EMBEDDING__QWEN__DIMENSIONS`（默认 1024，须与迁移列一致；用于断言）。
- `prts-common` 加 `embedding.qwen` 段（默认值 + 上述键）；`.env.example` 补充。

### 6.2 DB `settings` 表（管理后台可改、免重启；`settings` 表与 `get/set/list_all` 已存在）

| key | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `search.embedding_enabled` | bool | **false** | 向量化总开关（生成 + 向量召回路） |
| `search.embedding_model` | string | `text-embedding-v4` | 模型名 |
| `search.embedding_base_url` | string | `https://dashscope.aliyuncs.com/compatible-mode/v1` | 兼容端点 |
| `search.embedding_batch` | int | 10 | 批大小（clamp 1..10） |
| `search.tm_enabled` | bool | true | TM 建议面板开关（trgm 也可用） |
| `search.tm_min_similarity` | float | 0.30 | 建议相似度阈值 |
| `search.tm_top_n` | int | 3 | 建议条数（clamp 1..3） |

- 类型化访问器（`prts-core`/`prts-db`）读取上述 key，缺失用默认。
- **provider 运行时**：从 settings 读 model/base_url/batch、从 env 读 key；`embedding_enabled && key_present` 才激活。
- 新增管理端点（权限 `platform.settings.manage` 或总管理员）：
  - `GET /admin/settings/search` → 返回上述非密字段 + `embedding_key_present: bool`（由 env 推导，**不含 key 值**）。
  - `PUT /admin/settings/search` → 校验 + 写入。
  - 进 utoipa/Swagger。

### 6.3 降级链
- 向量关（默认）或 key 缺失 → `/search` 与 TM 建议自动只走 **FTS + trgm**；sweep worker 空转。
- 向量开 + key 配好 → worker 回填 `embedding`，向量召回 + 语义 TM 生效。
- 迁移 `0004` 与索引在关态下零成本（HNSW 空、列全 NULL）。

---

## 7. 嵌入 sweep worker

- `main.rs` 在 DB 就绪后 `tokio::spawn`（与 realtime `Hub` 同级）；`EmbeddingProvider` 进 `AppState`（`Arc<Option<QwenProvider>>`，供 `/search`/建议查询时复用）。
- 循环（每轮重读开关，实现运行时启停）：

```text
loop:
  if !(embedding_enabled && key_present): sleep(IDLE=30s); continue
  rows = SELECT id, source_text FROM entries
         WHERE embedding IS NULL AND source_text <> '' AND embed_attempts < MAX(=5)
         ORDER BY id LIMIT 50
  if rows empty: sleep(IDLE); continue
  for chunk in rows.chunks(batch≤10):
    match provider.embed(texts):
      Ok(vecs): for (row,vec): UPDATE entries SET embedding=$vec, embed_attempts=0
                               WHERE id=$id AND source_text=$captured   -- 乐观防并发改源
      Err(_):   UPDATE entries SET embed_attempts = embed_attempts + 1 WHERE id = ANY($ids)
  sleep(ACTIVE≈1s)  -- 兼顾 Qwen QPS/配额限速
```

- 触发器在源文变化时把 `embed_attempts` 归零，故失败行在源文更新后会重试。
- **20w 初次回填**：受 Qwen 配额约束，靠 `ACTIVE` 节流分摊；属一次性后台过程，不阻塞主流程。

---

## 8. 混合检索 + RRF（`/search`）

### 8.1 端点
`GET /projects/{id}/search`（进 Swagger，详尽描述）。参数：

| 参数 | 说明 |
| --- | --- |
| `q` | 主查询（FTS+trgm+向量，覆盖 source/translation/key） |
| `source_q` / `translation_q` | 可选定向子串过滤（分别约束源/译，AND 叠加） |
| `file_id` / `folder_id` | 文件/目录范围 |
| `state` | 状态多选（CSV） |
| `sort` | `relevance`(默认) / `key` / `updated_at` |
| `offset` / `limit` | 有界窗口内翻页 |

- 约束：至少提供 `q`/`source_q`/`translation_q` 之一（纯浏览走 `/entries`）。
- 分页上限：每路候选 N=100；融合后 top-K=200；`limit` 默认 50、上限 100；`offset+limit ≤ 200`。**这是有界候选集内翻页,非全表深 OFFSET → 不踩红线。**

### 8.2 编排（`prts-search`）
1. 若向量启用 + key：`embed([q])` 取查询向量 `qvec`（一次调用）；否则跳过向量路。
2. **并行三路**（`tokio::join!`），每路取 top-N 的 `(id, rank)`（rank 为 1-based 名次）：
   - **FTS**：`source_tsv @@ plainto_tsquery(prts_ts_config(src_lang), $q)` ∪ `translation_tsv @@ plainto_tsquery(prts_ts_config(tgt_lang), $q)`，按 `ts_rank` 排序。
   - **trgm**：`GREATEST(similarity(source_text,$q), similarity(translation,$q), similarity(key,$q))`，`%` 阈值过滤。
   - **向量**（仅启用且 `qvec` 存在）：`ORDER BY embedding <=> $qvec`，`WHERE embedding IS NOT NULL`。
3. **RRF 融合**（`prts-search` 纯函数，单测覆盖）：`score(id) = Σ_path 1/(k + rank_path(id))`，`k=60`，等权。
4. 取融合 top-K；`sort != relevance` 时按对应字段重排候选集；`offset/limit` 截窗。
5. 按结果 id **保序批量取整行** → `EntryDto` + `relevance` 分。
- 各路 WHERE 内统一施加 `project_id` + 过滤 + 可见性（`hidden` 仅项目编辑可见），保证相关度有意义。
- `prts-db` 新增参数化查询：`fts_search` / `trgm_search` / `vector_search`（均 `LIMIT N`，无需键集）。

---

## 9. TM 翻译建议

### 9.1 端点
`GET /projects/{id}/entries/{entryId}/suggestions` → 返回 ≤ `tm_top_n`(默认3) 条（进 Swagger）。

返回项：`{ entry_id, project_id, project_name, source_text, translation, state, similarity }`。

### 9.2 召回逻辑
- 取当前词条 `source_text` 与所在项目 `target_lang`。
- 候选过滤（**隐私边界 = 用户已加入项目**）：

```text
entries e
  JOIN projects p   ON p.id = e.project_id
  JOIN memberships m ON m.project_id = p.id AND m.user_id = $current_user
WHERE p.target_lang = $current_target_lang
  AND e.state IN ('translated','questioned','checked','reviewed')   -- 状态≥已翻译
  AND e.translation <> ''
  AND e.source_text <> ''
  AND e.id <> $current_entry_id                                      -- 排除自身
ORDER BY <相似度>  LIMIT $tm_top_n
```

- **相似度**：向量启用且**当前词条已有 `embedding`** → `e.embedding <=> $current_embedding`（cosine）；否则 trgm `similarity(e.source_text, $current_source_text)`。
- 应用 `tm_min_similarity` 阈值。当前词条 `embedding` 为 NULL（尚未嵌入/向量关）时自动走 trgm，整端点优雅降级。

### 9.3 前端
- 译文输入框下方挂建议列表（`EditorView.vue` 译文 `q-input` 之后）；选中词条时拉取。
- 每条卡片：源文 + 译文 + 相似度 + 来源项目名；点击 → **填入 `draft`**（用户确认后再保存，不自动提交）。
- Quasar 组件、中英 i18n 文案、亮/深主题、移动端适配（沿用现有约定）。

---

## 10. 前端

| 区域 | 改动 |
| --- | --- |
| 编辑器搜索 | 搜索框升级为高级筛选（文件/目录、状态多选、源/译关键字、排序）；有查询走 `/search` 并展示相关度,清空回落 `/entries` 浏览 |
| 编辑器建议 | §9.3 建议面板 |
| 管理后台 | 新增「搜索 / 向量化」设置区：开关 `embedding_enabled`、编辑 model/base_url/batch、`tm_enabled`/`min_similarity`/`top_n`；只读显示「已配置 Key:是/否」(**绝不输入/回显 key**) |
| api/types/store | `searchApi`、`suggestionsApi`、admin settings 读写；类型补充 |

---

## 11. 错误处理与降级（汇总）

- Qwen 调用失败：worker 累加 `embed_attempts`（退避，达 MAX 暂停该行）；`/search` 查询期 embed 失败 → 跳过向量路、记 warn、继续返回 FTS+trgm 结果（不 500）。
- 乐观锁：sweep 写 embedding 带 `source_text=$captured` 守卫，避免覆盖并发改源。
- 配置非法（PUT settings）：校验并返回错误码 + 本地化消息（按 `Accept-Language`）。
- key 缺失但开关开：视为降级（FTS+trgm），后台 `embedding_key_present=false` 提示。

---

## 12. 测试与 verify

**单元（无 DB）**
- RRF 融合：排序、并列、某路缺席项的合并。
- `prts_ts_config` 映射、嵌入输入构造（从 `original` JSONB 取主源语言）、settings 默认值/clamp、降级判定（`enabled && key_present`）。
- `QwenProvider` 请求构造 + 响应解析（mock/fixture，**不打真 API**）。

**DB 集成（CI，`--features db-tests`，自定义 zhparser 镜像）**
- 迁移可应用；触发器正确填充 `source_text`/`source_tsv`/`translation_tsv`（含一条中文样本验证 zhparser 切出多词元）。
- `fts_search`/`trgm_search`/`vector_search` 在播种数据上返回预期 id；**向量测试直接插入已知向量**（不依赖真 provider），验证 cosine 排序。
- RRF 端到端排序；降级路径（`embedding` 全 NULL → 仅 FTS+trgm）。
- TM 建议：membership + 语言 + 状态过滤正确、排除自身。

**perf verify**
- 播种 Nw 词条（含向量）测 `/search` 延迟，记录方法与 20w 目标（脚本或 `--ignored` 基准）。

---

## 13. 涉及文件

- **改**：`prts-search`（trait+QwenProvider+RRF+编排）、`prts-common/config`、`prts-db`（entries 查询、settings 访问器、models）、`prts-api`（`/search`、`/suggestions`、`/admin/settings/search`、`main.rs` worker、`AppState`、Swagger）、迁移 `0004_search.sql`、`deploy/postgres.Dockerfile`+compose+CI、`.env.example`、前端（搜索 UI/建议面板/admin 设置/api/types/store/i18n）、`docs/architecture.md`。
- **新依赖**：`async_trait`、`pgvector`（sqlx feature）。

---

## 14. 红线核对

- ✅ 键集浏览 `/entries` 保留；`/search` 有界 top-K 非全表深翻。
- ✅ 密钥仅 env、绝不下发前端；后台仅显示 present 布尔。
- ✅ sqlx 全参数化；最小权限（admin 设置端点权限受控）。
- ✅ 不引入除 zhparser（已批准）外的重依赖；`async_trait`/`pgvector` 为轻量标准件。
- ✅ `locked/hidden` 仍为正交标志，未混入搜索状态枚举。
- ⏭ 审计留痕（P5）、CP（P6）不在本期。

---

## 15. 后续 spec 衔接

实现完 P4 后，按接力文档建议顺序进入 **Spec B（编辑器+实时增强：保存逻辑/列表头像/强制保存）** 与 **Spec C（私信+通知）**，各自独立 brainstorm→plan→实现→闭环。
