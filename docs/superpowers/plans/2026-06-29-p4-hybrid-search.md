# P4 Hybrid Search + TM Suggestions — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade entry search from `ILIKE` to FTS (zhparser) + `pg_trgm` + `pgvector` three-way recall fused by RRF, with an optional-default-off embedding layer (Qwen, admin-toggleable) and a cross-project translation-memory suggestion panel in the editor.

**Architecture:** A new `0004` migration adds trigger-maintained `source_text` / `source_tsv` / `translation_tsv` / `embedding vector(1024)` columns. `prts-search` owns the `QwenProvider` and a pure RRF fuser; `prts-db` owns parameterized FTS/trgm/vector queries; `prts-api` exposes `GET /projects/{id}/search`, `GET /projects/{id}/entries/{entryId}/suggestions`, `GET|PUT /admin/settings/search`, and runs a background sweep worker that backfills embeddings. Everything degrades to FTS+trgm when embeddings are off (the default).

**Tech Stack:** Rust (axum, sqlx, tokio, reqwest), PostgreSQL 16 (pgvector + zhparser), Vue 3 + Quasar + Pinia, Docker/GHCR.

**Authoritative spec:** [`docs/superpowers/specs/2026-06-29-p4-hybrid-search-design.md`](../specs/2026-06-29-p4-hybrid-search-design.md). Read it before starting.

---

## Environment & workflow notes (read first)

- **DB-dependent tests run in CI, not locally** (no local Postgres). Pure-logic tests (`prts-search` unit tests) run locally. Tasks mark which is which.
- **CI Postgres must have zhparser.** Migration `0004` does `CREATE EXTENSION zhparser`, which the stock `pgvector/pgvector` image lacks. Phase 1 builds a custom image and points both compose and the CI db-tests job at it. **Do Phase 1 before any db-tests can pass.**
- **Adding crates pulls crates.io through the proxy** (`http.proxy = 127.0.0.1:10808`); ensure it's up, else rely on CI (direct) to verify. `--offline` only works for already-cached crates.
- **Windows + Defender** can block freshly-compiled test exes with `os error 5`; retry `cargo test` (short sleep + retry) if you hit it. Bash builds need `dangerouslyDisableSandbox: true`; first compile is slow — run in background and poll.
- **Per CLAUDE.md §6:** `cargo fmt && cargo clippy --all-targets && cargo test` before commit; frontend `pnpm lint && pnpm build`. Conventional Commits.
- A bash retry loop that ends in `echo` exits 0 even if cargo failed — grep the output for the real success marker.

## File structure map

**Backend (Rust)**
| Path | Action | Responsibility |
| --- | --- | --- |
| `backend/migrations/0004_search.sql` | create | zhparser ext + `prts_ts_config` + entries columns + trigger + indexes + backfill |
| `backend/crates/prts-common/src/config.rs` | modify | add `embedding.qwen.{api_key,base_url,model,dimensions}` |
| `backend/crates/prts-search/Cargo.toml` | modify | add `reqwest`, `pgvector`, `thiserror`, `serde_json`, `tracing` |
| `backend/crates/prts-search/src/lib.rs` | modify | re-export modules; keep `SearchFilters`/`SortBy`; add `SearchRequest`/`SearchHit` |
| `backend/crates/prts-search/src/qwen.rs` | create | `QwenProvider` + `EmbedError` |
| `backend/crates/prts-search/src/rrf.rs` | create | pure `rrf_fuse` + unit tests |
| `backend/crates/prts-db/src/search.rs` | create | `fts_search`, `trgm_search`, `vector_search`, `fetch_by_ids`, `suggestions` |
| `backend/crates/prts-db/src/search_settings.rs` | create | `SearchConfig` typed accessor over `settings` table |
| `backend/crates/prts-db/src/lib.rs` | modify | `pub mod search; pub mod search_settings;` |
| `backend/crates/prts-api/src/state.rs` | modify | add `embedder: Arc<Option<QwenProvider>>`, `search_rt: Arc<RwLock<SearchRuntime>>` |
| `backend/crates/prts-api/src/embed_worker.rs` | create | background sweep loop |
| `backend/crates/prts-api/src/routes/search.rs` | create | `GET /projects/{id}/search` |
| `backend/crates/prts-api/src/routes/suggestions.rs` | create | `GET /projects/{id}/entries/{entryId}/suggestions` |
| `backend/crates/prts-api/src/routes/admin_settings.rs` | create | `GET|PUT /admin/settings/search` |
| `backend/crates/prts-api/src/routes/mod.rs` | modify | register routes + OpenAPI paths |
| `backend/crates/prts-api/src/main.rs` | modify | build embedder, load runtime settings, spawn worker |
| `.env.example` | modify | `PRTS__EMBEDDING__QWEN__*` |

**Deploy**
| Path | Action | Responsibility |
| --- | --- | --- |
| `deploy/postgres.Dockerfile` | create | `pgvector/pgvector:pg16` + SCWS + zhparser |
| `deploy/docker-compose.yml` | modify | `postgres` → `build:` the custom image |
| `.github/workflows/ci.yml` | modify | build custom pg image; db-tests use it; push `prts-postgres` to GHCR |

**Frontend (Vue)**
| Path | Action | Responsibility |
| --- | --- | --- |
| `frontend/src/api/types.ts` | modify | `SearchHitDto`, `SuggestionDto`, `SearchSettingsDto` |
| `frontend/src/api/index.ts` | modify | `searchApi`, `suggestionsApi`, `adminSearchApi` |
| `frontend/src/components/SuggestionsPanel.vue` | create | ≤3 TM suggestion cards |
| `frontend/src/components/SearchFilters.vue` | create | advanced filter bar |
| `frontend/src/views/EditorView.vue` | modify | wire search + suggestions |
| `frontend/src/views/AdminView.vue` | modify | "搜索 / 向量化" settings section |
| `frontend/src/i18n/*` | modify | zh-CN + en strings |
| `docs/architecture.md` | modify | P4 section |

---

## Phase 0 — Config scaffolding

### Task 1: Add embedding config section to `prts-common`

**Files:**
- Modify: `backend/crates/prts-common/src/config.rs`
- Modify: `.env.example`

- [ ] **Step 1: Add the structs.** In `config.rs`, add below the existing settings structs:

```rust
/// 向量化（Embedding）配置。密钥仅经 env 注入，绝不下发前端。
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingSettings {
    #[serde(default)]
    pub qwen: QwenSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QwenSettings {
    /// Qwen API Key（仅 env：PRTS__EMBEDDING__QWEN__API_KEY）。空 = 未配置 → 降级。
    #[serde(default)]
    pub api_key: String,
    /// 向量维度，须与迁移 0004 的 vector(N) 一致。
    #[serde(default = "default_qwen_dimensions")]
    pub dimensions: usize,
}

fn default_qwen_dimensions() -> usize { 1024 }

impl Default for EmbeddingSettings {
    fn default() -> Self { Self { qwen: QwenSettings::default() } }
}
impl Default for QwenSettings {
    fn default() -> Self { Self { api_key: String::new(), dimensions: default_qwen_dimensions() } }
}
```

Add the field to `Settings`:

```rust
    #[serde(default)]
    pub embedding: EmbeddingSettings,
```

> Note: `model`/`base_url`/`batch` live in the DB `settings` table (Task 14), not env — only the secret `api_key` and the migration-fixed `dimensions` are env.

- [ ] **Step 2: Add a unit test.** Append to the `#[cfg(test)]` module in `config.rs` (or create one):

```rust
#[test]
fn embedding_defaults_are_safe() {
    let s = QwenSettings::default();
    assert_eq!(s.dimensions, 1024);
    assert!(s.api_key.is_empty(), "key must default empty so we degrade, not crash");
}
```

- [ ] **Step 3: Run the test.** Run: `cd backend && cargo test -p prts-common embedding_defaults_are_safe`
  Expected: PASS. (Retry once if Defender blocks the exe.)

- [ ] **Step 4: Update `.env.example`.** Append:

```dotenv
# 向量化（可选，默认关闭；开关与模型/URL 在管理后台配置）。仅密钥经 env：
PRTS__EMBEDDING__QWEN__API_KEY=
# 向量维度须与迁移 0004 的 vector(1024) 一致，一般不改：
PRTS__EMBEDDING__QWEN__DIMENSIONS=1024
```

- [ ] **Step 5: Commit.**

```bash
git add backend/crates/prts-common/src/config.rs .env.example
git commit -m "feat(search): add embedding config section (env-only api_key + dimensions)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase 1 — Migration, zhparser image, CI

### Task 2: Custom Postgres image with zhparser

**Files:**
- Create: `deploy/postgres.Dockerfile`
- Modify: `deploy/docker-compose.yml`

- [ ] **Step 1: Write the Dockerfile.** Create `deploy/postgres.Dockerfile`:

```dockerfile
# PRTS Postgres：pgvector 基础上叠加 SCWS + zhparser（中文全文分词）。
FROM pgvector/pgvector:pg16

ARG SCWS_VER=1.2.3
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        build-essential wget ca-certificates postgresql-server-dev-16 git; \
    # SCWS
    wget -O /tmp/scws.tar.bz2 "http://www.xunsearch.com/scws/down/scws-${SCWS_VER}.tar.bz2"; \
    mkdir -p /tmp/scws && tar -xjf /tmp/scws.tar.bz2 -C /tmp/scws --strip-components=1; \
    cd /tmp/scws && ./configure && make && make install; \
    # zhparser
    git clone --depth 1 https://github.com/amutu/zhparser.git /tmp/zhparser; \
    cd /tmp/zhparser && SCWS_HOME=/usr/local make && make install; \
    # cleanup build deps to keep image lean
    apt-get purge -y --auto-remove build-essential wget git postgresql-server-dev-16; \
    rm -rf /var/lib/apt/lists/* /tmp/scws* /tmp/zhparser; \
    ldconfig
```

> If `xunsearch.com` is unreachable in CI, mirror SCWS to GHCR or a release asset and change the URL. Note this in the PR if you switch sources.

- [ ] **Step 2: Point compose at the build.** In `deploy/docker-compose.yml`, replace the `postgres.image` line with:

```yaml
  postgres:
    build:
      context: ..
      dockerfile: deploy/postgres.Dockerfile
    image: ghcr.io/localizelimbuscompany/prts-postgres:latest # 预装 pgvector + zhparser
```

(Keep the existing `environment`, `volumes`, `healthcheck`, `restart`.)

- [ ] **Step 3: Build it locally to verify (background).** Run (background, poll): `docker build -f deploy/postgres.Dockerfile -t prts-postgres-test ..`
  Expected: image builds; final `ldconfig` succeeds. If build infra is unavailable locally, defer verification to CI (Task 4) and note it.

- [ ] **Step 4: Commit.**

```bash
git add deploy/postgres.Dockerfile deploy/docker-compose.yml
git commit -m "build(deploy): custom Postgres image with zhparser for Chinese FTS

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 3: Migration `0004_search.sql`

**Files:**
- Create: `backend/migrations/0004_search.sql`

- [ ] **Step 1: Write the migration.** Create the file with exactly:

```sql
-- 0004_search.sql — 混合搜索：FTS(zhparser) + trgm + 向量列 + 触发器维护。

-- 1) 中文分词扩展与文本搜索配置
CREATE EXTENSION IF NOT EXISTS zhparser;
CREATE TEXT SEARCH CONFIGURATION prts_zh (PARSER = zhparser);
ALTER TEXT SEARCH CONFIGURATION prts_zh ADD MAPPING FOR n,v,a,i,e,l,j WITH simple;

-- 2) BCP-47 语言码 → regconfig（IMMUTABLE，供触发器复用）
CREATE FUNCTION prts_ts_config(lang text) RETURNS regconfig
LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE
    WHEN lang LIKE 'zh%' THEN 'prts_zh'::regconfig
    WHEN lang LIKE 'en%' THEN 'english'::regconfig
    ELSE 'simple'::regconfig
  END
$$;

-- 3) entries 增列（触发器维护）
ALTER TABLE entries
  ADD COLUMN source_text     TEXT NOT NULL DEFAULT '',
  ADD COLUMN source_tsv      tsvector,
  ADD COLUMN translation_tsv tsvector,
  ADD COLUMN embedding       vector(1024),
  ADD COLUMN embed_attempts  SMALLINT NOT NULL DEFAULT 0;

-- 4) 维护触发器：算 source_text/tsv；仅源文变化才作废 embedding
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

-- 5) 索引
CREATE INDEX entries_source_tsv_idx       ON entries USING gin (source_tsv);
CREATE INDEX entries_translation_tsv_idx  ON entries USING gin (translation_tsv);
CREATE INDEX entries_source_trgm_idx      ON entries USING gin (source_text gin_trgm_ops);
CREATE INDEX entries_translation_trgm_idx ON entries USING gin (translation gin_trgm_ops);
CREATE INDEX entries_key_trgm_idx         ON entries USING gin (key gin_trgm_ops);
CREATE INDEX entries_embedding_hnsw_idx   ON entries USING hnsw (embedding vector_cosine_ops);

-- 6) 存量回填（embedding 保持 NULL → sweep worker 补）
UPDATE entries e SET
  source_text     = COALESCE(e.original ->> p.source_langs[1], ''),
  source_tsv      = to_tsvector(prts_ts_config(COALESCE(p.source_langs[1],'')), COALESCE(e.original ->> p.source_langs[1],'')),
  translation_tsv = to_tsvector(prts_ts_config(COALESCE(p.target_lang,'')), COALESCE(e.translation,''))
FROM projects p WHERE p.id = e.project_id;
```

> For a 20w+ existing table, the Step-6 backfill should be batched by `id` range in production. For this repo's current data volume a single statement is fine; leave a comment noting the batching option.

- [ ] **Step 2: Verify in CI** (no local Postgres). It is exercised by Task 5's db-tests once Task 4 wires the zhparser image. No local run.

- [ ] **Step 3: Commit.**

```bash
git add backend/migrations/0004_search.sql
git commit -m "feat(db): migration 0004 — FTS/trgm/vector columns + maintain trigger

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 4: CI uses the zhparser image for db-tests

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Inspect the current db-tests job.** Run: `git grep -n "db-tests\|pgvector\|services:" .github/workflows/`
  Note the job name and the `services.postgres.image` line.

- [ ] **Step 2: Build the custom image before the db-tests job and use it.** In the db-tests job, replace the `services:`-based Postgres with a step that builds and runs the custom image (services can't `build:`). Add before the test step:

```yaml
      - name: Build Postgres (zhparser) image
        run: docker build -f deploy/postgres.Dockerfile -t prts-postgres:ci .
      - name: Start Postgres + Redis
        run: |
          docker run -d --name pg -p 5432:5432 \
            -e POSTGRES_USER=prts -e POSTGRES_PASSWORD=prts -e POSTGRES_DB=prts \
            prts-postgres:ci
          docker run -d --name redis -p 6379:6379 redis:7-alpine
          for i in $(seq 1 30); do docker exec pg pg_isready -U prts && break; sleep 2; done
```

Keep the existing `DATABASE_URL`/`PRTS__DATABASE__URL` env pointing at `postgres://prts:prts@localhost:5432/prts`. Ensure the migrate + `cargo test -p prts-api --features db-tests` step still runs.

- [ ] **Step 3: Add a GHCR push for the image** (mirror the existing backend/frontend push job). Add `ghcr.io/localizelimbuscompany/prts-postgres:latest` build-and-push on `master`, reusing the existing GHCR login step.

- [ ] **Step 4: Commit & push to let CI verify Tasks 2–4.**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: build & use zhparser Postgres image for db-tests; push prts-postgres to GHCR

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

Expected (CI): db-tests job builds the image, migrations apply (including `0004`), existing tests stay green.

### Task 5: DB test — migration & trigger populate search columns

**Files:**
- Create/modify: `backend/crates/prts-db/tests/search_columns.rs` (or the existing db-tests location — match the current pattern found by `git grep -n "db-tests" backend`)

- [ ] **Step 1: Write the failing db-test.** Use the repo's existing db-test harness (pool fixture). Add:

```rust
// 仅在 db-tests feature 下编译；CI 提供带 zhparser 的 Postgres。
#![cfg(feature = "db-tests")]

#[sqlx::test(migrations = "../../migrations")]
async fn trigger_populates_source_text_and_tsv(pool: sqlx::PgPool) {
    // 建最小项目/文件/词条（zh-Hans 源 → en 目标）
    let project_id: i64 = sqlx::query_scalar(
        "INSERT INTO projects (slug,name,visibility,source_langs,target_lang,owner_id)
         VALUES ('p','P','public', ARRAY['zh-Hans'], 'en', NULL) RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let file_id: i64 = sqlx::query_scalar(
        "INSERT INTO files (project_id,name,format) VALUES ($1,'f.json','json') RETURNING id")
        .bind(project_id).fetch_one(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO entries (file_id,project_id,key,original,translation,state)
         VALUES ($1,$2,'k', '{\"zh-Hans\":\"今天天气很好\"}'::jsonb, 'nice weather', 'translated')")
        .bind(file_id).bind(project_id).execute(&pool).await.unwrap();

    let (src, lexemes): (String, i64) = sqlx::query_as(
        "SELECT source_text, length(source_tsv::text) FROM entries WHERE project_id=$1")
        .bind(project_id).fetch_one(&pool).await.unwrap();
    assert_eq!(src, "今天天气很好");
    assert!(lexemes > 0, "zhparser should segment Chinese into lexemes");
}
```

- [ ] **Step 2: Run in CI** (push). Expected: PASS — proving the trigger + zhparser config work end-to-end.

- [ ] **Step 3: Commit & push.**

```bash
git add backend/crates/prts-db/tests/search_columns.rs
git commit -m "test(db): verify 0004 trigger populates source_text + zhparser tsv

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

---

## Phase 2 — FTS + trgm hybrid search (vector OFF), `/search`, frontend

This phase ships a working hybrid search using only FTS+trgm — the default (vector-off) experience.

### Task 6: RRF fusion (pure function, TDD local)

**Files:**
- Create: `backend/crates/prts-search/src/rrf.rs`
- Modify: `backend/crates/prts-search/src/lib.rs` (add `pub mod rrf;`)

- [ ] **Step 1: Write the failing tests.** Create `rrf.rs`:

```rust
//! Reciprocal Rank Fusion：合并多路有序召回为单一排序。
use std::collections::HashMap;

const RRF_K: f64 = 60.0;

/// 一路召回：按相关度降序的 entry id。
pub type RankedIds = Vec<i64>;

/// 融合多路结果，返回按融合分降序的 (id, score)；并列按 id 升序（确定性）。
pub fn rrf_fuse(paths: &[RankedIds]) -> Vec<(i64, f64)> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for path in paths {
        for (rank, &id) in path.iter().enumerate() {
            *scores.entry(id).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut fused: Vec<(i64, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_in_two_paths_outranks_singletons() {
        let out = rrf_fuse(&[vec![7, 3, 1], vec![7, 9]]);
        assert_eq!(out[0].0, 7);
        assert!(out[0].1 > out[1].1);
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(rrf_fuse(&[]).is_empty());
    }

    #[test]
    fn ties_break_by_id_ascending() {
        let out = rrf_fuse(&[vec![5], vec![2]]); // 同分
        assert_eq!(out[0].0, 2);
        assert_eq!(out[1].0, 5);
    }
}
```

- [ ] **Step 2: Wire the module.** In `lib.rs` add `pub mod rrf;`.

- [ ] **Step 3: Run tests.** Run: `cd backend && cargo test -p prts-search rrf`
  Expected: 3 PASS. (Retry once on Defender `os error 5`.)

- [ ] **Step 4: Commit.**

```bash
git add backend/crates/prts-search/src/rrf.rs backend/crates/prts-search/src/lib.rs
git commit -m "feat(search): pure RRF fusion with deterministic tie-break

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 7: Search request/filter types in `prts-search`

**Files:**
- Modify: `backend/crates/prts-search/src/lib.rs`

- [ ] **Step 1: Extend the shared types.** Keep existing `SearchFilters`/`SortBy`; add the request and hit shapes:

```rust
/// 一次混合搜索请求（编排层输入）。
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub project_id: i64,
    pub query: String,                 // 主查询 q
    pub source_q: Option<String>,      // 定向源文子串过滤
    pub translation_q: Option<String>, // 定向译文子串过滤
    pub filters: SearchFilters,        // file_ids/folder_ids/states
    pub include_hidden: bool,
    pub sort: SortBy,
    pub per_path: i64,                 // 每路候选上限（默认 100）
    pub top_k: i64,                    // 融合后保留（默认 200）
}

/// 融合后的命中：entry id + 相关度分。
#[derive(Debug, Clone, Copy)]
pub struct SearchHit { pub id: i64, pub score: f64 }
```

- [ ] **Step 2: Build.** Run: `cd backend && cargo build -p prts-search`
  Expected: compiles.

- [ ] **Step 3: Commit.**

```bash
git add backend/crates/prts-search/src/lib.rs
git commit -m "feat(search): SearchRequest/SearchHit orchestration types

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 8: FTS + trgm queries in `prts-db`

**Files:**
- Create: `backend/crates/prts-db/src/search.rs`
- Modify: `backend/crates/prts-db/src/lib.rs` (add `pub mod search;`)

- [ ] **Step 1: Write the query functions.** Create `search.rs`. Each returns `Vec<i64>` ordered by per-path relevance. Filters are applied via dynamic SQL using `sqlx::QueryBuilder` (parameterized — never string-interpolate user input).

```rust
//! 三路召回的参数化查询。每路返回按相关度降序的 entry id（≤ per_path）。
use sqlx::{PgPool, QueryBuilder, Postgres};
use crate::models::Entry;

/// 公共过滤：project / file / state / hidden 可见性。push 到已开头的 WHERE。
fn push_filters(qb: &mut QueryBuilder<'_, Postgres>, file_ids: &[i64], states: &[String], include_hidden: bool) {
    if !file_ids.is_empty() {
        qb.push(" AND file_id = ANY(").push_bind(file_ids.to_vec()).push(")");
    }
    if !states.is_empty() {
        qb.push(" AND state = ANY(").push_bind(states.to_vec()).push(")");
    }
    if !include_hidden {
        qb.push(" AND hidden = FALSE");
    }
}

/// FTS：源/译两列各按其语言 config 匹配；ts_rank 求和排序。
/// src_cfg/tgt_cfg 由调用方按项目语言映射（用 prts_ts_config 的同义字符串）。
pub async fn fts_search(
    pool: &PgPool, project_id: i64, q: &str, src_lang: &str, tgt_lang: &str,
    file_ids: &[i64], states: &[String], include_hidden: bool, per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let mut qb = QueryBuilder::new(
        "SELECT id FROM entries, \
         plainto_tsquery(prts_ts_config(");
    qb.push_bind(src_lang.to_string()).push("), ").push_bind(q.to_string()).push(") AS sq, ");
    qb.push("plainto_tsquery(prts_ts_config(").push_bind(tgt_lang.to_string()).push("), ")
      .push_bind(q.to_string()).push(") AS tq WHERE project_id = ").push_bind(project_id);
    qb.push(" AND (source_tsv @@ sq OR translation_tsv @@ tq)");
    push_filters(&mut qb, file_ids, states, include_hidden);
    qb.push(" ORDER BY (ts_rank(source_tsv, sq) + ts_rank(translation_tsv, tq)) DESC LIMIT ")
      .push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}

/// trgm：源/译/键三列相似度取最大值排序。pg_trgm `%` 用默认阈值。
pub async fn trgm_search(
    pool: &PgPool, project_id: i64, q: &str,
    file_ids: &[i64], states: &[String], include_hidden: bool, per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let mut qb = QueryBuilder::new("SELECT id FROM entries WHERE project_id = ");
    qb.push_bind(project_id);
    qb.push(" AND (source_text % ").push_bind(q.to_string())
      .push(" OR translation % ").push_bind(q.to_string())
      .push(" OR key % ").push_bind(q.to_string()).push(")");
    push_filters(&mut qb, file_ids, states, include_hidden);
    qb.push(" ORDER BY GREATEST(similarity(source_text, ").push_bind(q.to_string())
      .push("), similarity(translation, ").push_bind(q.to_string())
      .push("), similarity(key, ").push_bind(q.to_string()).push(")) DESC LIMIT ")
      .push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}

/// 按 id 取整行，调用方负责按融合顺序重排。
pub async fn fetch_by_ids(pool: &PgPool, ids: &[i64]) -> Result<Vec<Entry>, sqlx::Error> {
    if ids.is_empty() { return Ok(vec![]); }
    sqlx::query_as::<_, Entry>("SELECT * FROM entries WHERE id = ANY($1)")
        .bind(ids.to_vec()).fetch_all(pool).await
}
```

> `vector_search` is added in Phase 3 (Task 16). Keeping it out now keeps Phase 2 vector-free.

- [ ] **Step 2: Wire the module.** In `prts-db/src/lib.rs` add `pub mod search;`.

- [ ] **Step 3: Add a db-test.** In the db-tests suite, seed two entries and assert `fts_search`/`trgm_search` return the expected id ordering for a query. (Mirror Task 5's fixture; assert the more-relevant id comes first.)

- [ ] **Step 4: Build locally + push for db-test.** Run: `cd backend && cargo build -p prts-db` (Expected: compiles). db-test verified in CI.

- [ ] **Step 5: Commit & push.**

```bash
git add backend/crates/prts-db/src/search.rs backend/crates/prts-db/src/lib.rs backend/crates/prts-db/tests/
git commit -m "feat(db): parameterized FTS + trgm recall queries

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

### Task 9: Search orchestrator in `prts-search`

**Files:**
- Create: `backend/crates/prts-search/src/orchestrator.rs`
- Modify: `backend/crates/prts-search/src/lib.rs` (`pub mod orchestrator;`)
- Modify: `backend/crates/prts-search/Cargo.toml` (ensure `tokio` with `rt`/`macros` and `prts-db` are deps; check workspace)

- [ ] **Step 1: Write the orchestrator.** It runs FTS + trgm (and, in Phase 3, vector) concurrently, fuses with RRF, fetches rows, reorders. Vector path is injected as an already-fetched `Option<Vec<i64>>` so this crate stays unaware of the embedder.

```rust
//! 混合检索编排：并行多路 → RRF → 取行 → 按分排序/截窗。
use crate::rrf::rrf_fuse;
use crate::{SearchHit, SortBy};
use prts_db::models::Entry;
use sqlx::PgPool;

pub struct OrchestratorInput<'a> {
    pub project_id: i64,
    pub q: &'a str,
    pub src_lang: &'a str,
    pub tgt_lang: &'a str,
    pub file_ids: &'a [i64],
    pub states: &'a [String],
    pub include_hidden: bool,
    pub per_path: i64,
    pub top_k: i64,
    pub sort: SortBy,
    /// Phase 3 注入的向量召回（已排序 id）；None = 向量路关闭/降级。
    pub vector_ids: Option<Vec<i64>>,
}

/// 返回按最终顺序排列的 (Entry, relevance_score)。
pub async fn run(pool: &PgPool, input: OrchestratorInput<'_>) -> Result<Vec<(Entry, f64)>, sqlx::Error> {
    let (fts, trgm) = tokio::join!(
        prts_db::search::fts_search(pool, input.project_id, input.q, input.src_lang, input.tgt_lang,
            input.file_ids, input.states, input.include_hidden, input.per_path),
        prts_db::search::trgm_search(pool, input.project_id, input.q,
            input.file_ids, input.states, input.include_hidden, input.per_path),
    );
    let mut paths = vec![fts?, trgm?];
    if let Some(v) = input.vector_ids { paths.push(v); }

    let fused = rrf_fuse(&paths);
    let hits: Vec<SearchHit> = fused.into_iter().take(input.top_k as usize)
        .map(|(id, score)| SearchHit { id, score }).collect();

    let ids: Vec<i64> = hits.iter().map(|h| h.id).collect();
    let rows = prts_db::search::fetch_by_ids(pool, &ids).await?;
    // 按 hits 顺序重排，并配相关度分
    let mut by_id: std::collections::HashMap<i64, Entry> = rows.into_iter().map(|e| (e.id, e)).collect();
    let mut out: Vec<(Entry, f64)> = hits.iter()
        .filter_map(|h| by_id.remove(&h.id).map(|e| (e, h.score))).collect();

    match input.sort {
        SortBy::Relevance => {}
        SortBy::Key => out.sort_by(|a, b| a.0.key.cmp(&b.0.key)),
        SortBy::UpdatedAt => out.sort_by(|a, b| b.0.updated_at.cmp(&a.0.updated_at)),
    }
    Ok(out)
}
```

- [ ] **Step 2: Wire module + deps.** Add `pub mod orchestrator;` to `lib.rs`. Ensure `prts-search/Cargo.toml` has `prts-db.workspace = true`, `tokio.workspace = true`, `sqlx.workspace = true`. (Add to `[workspace.dependencies]` only if missing.)

- [ ] **Step 3: Build.** Run: `cd backend && cargo build -p prts-search`
  Expected: compiles.

- [ ] **Step 4: Commit.**

```bash
git add backend/crates/prts-search/
git commit -m "feat(search): hybrid orchestrator (FTS+trgm now, vector slot ready)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 10: `GET /projects/{id}/search` endpoint

**Files:**
- Create: `backend/crates/prts-api/src/routes/search.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`

- [ ] **Step 1: Write the handler.** Mirror the access-control + DTO pattern from `routes/entries.rs` (use the same `MaybeUser`, project access lookup, `EntryDto`, `db_err`).

```rust
//! GET /projects/{id}/search — 混合搜索（FTS+trgm[+向量]）+ RRF。
use axum::{extract::{Path, Query, State}, Json};
use serde::{Deserialize, Serialize};
use crate::{error::ApiError, state::AppState, auth_ext::MaybeUser};
use super::entries::EntryDto;

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SearchQuery {
    /// 主查询（覆盖源/译/键）。
    pub q: Option<String>,
    pub source_q: Option<String>,
    pub translation_q: Option<String>,
    pub file_id: Option<i64>,
    /// 逗号分隔状态过滤。
    pub state: Option<String>,
    /// relevance | key | updated_at
    pub sort: Option<String>,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub include_hidden: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SearchHitDto {
    #[serde(flatten)]
    pub entry: EntryDto,
    /// 相关度分（RRF），用于前端展示排序权重。
    pub relevance: f64,
}

#[utoipa::path(get, path = "/projects/{id}/search", tag = "search",
    params(("id" = i64, Path, description = "项目 ID"), SearchQuery),
    responses((status = 200, body = [SearchHitDto]), (status = 400), (status = 404)))]
pub async fn search_entries(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(id): Path<i64>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHitDto>>, ApiError> {
    // 1) 项目访问 + 角色（沿用 entries.rs 的 access 逻辑）
    let access = crate::routes::projects::load_access(&state, user.as_ref(), id).await?;

    let main_q = q.q.clone().unwrap_or_default();
    if main_q.trim().is_empty() && q.source_q.is_none() && q.translation_q.is_none() {
        return Err(ApiError::bad_request("search requires q/source_q/translation_q"));
    }

    // 2) 解析状态、排序、分页（有界窗口）
    let states = crate::routes::entries::parse_states(q.state.as_deref());
    let include_hidden = q.include_hidden && access.has_node(crate::auth_ext::nodes::PROJECT_ENTRY_EDIT);
    let sort = match q.sort.as_deref() {
        Some("key") => prts_search::SortBy::Key,
        Some("updated_at") => prts_search::SortBy::UpdatedAt,
        _ => prts_search::SortBy::Relevance,
    };
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let offset = q.offset.unwrap_or(0).clamp(0, 200);
    let top_k = 200i64;

    // 3) 项目语言（FTS config）
    let (src_lang, tgt_lang) = crate::routes::projects::primary_langs(&state, id).await?;

    // 4) 向量路（Phase 3 填充；此处恒 None → 降级 FTS+trgm）
    let vector_ids: Option<Vec<i64>> = None;

    let file_ids: Vec<i64> = q.file_id.into_iter().collect();
    let rows = prts_search::orchestrator::run(&state.db, prts_search::orchestrator::OrchestratorInput {
        project_id: id, q: &main_q, src_lang: &src_lang, tgt_lang: &tgt_lang,
        file_ids: &file_ids, states: &states, include_hidden,
        per_path: 100, top_k, sort, vector_ids,
    }).await.map_err(crate::routes::db_err)?;

    let window: Vec<SearchHitDto> = rows.into_iter()
        .skip(offset as usize).take(limit as usize)
        .map(|(e, score)| SearchHitDto { entry: EntryDto::from(&e), relevance: score })
        .collect();
    Ok(Json(window))
}
```

> If helper functions referenced (`load_access`, `parse_states`, `primary_langs`, `ApiError::bad_request`) don't exist with these names, create thin wrappers in the matching module — check `routes/entries.rs` and `routes/projects.rs` first and reuse the real names. `parse_states` should be extracted from the existing inline CSV-parsing in `list_entries` (Task: refactor it into a shared `pub fn parse_states`).

- [ ] **Step 2: Register the route + OpenAPI.** In `routes/mod.rs` add `.routes(routes!(search::search_entries))` and register `search::search_entries` + `SearchHitDto` in the utoipa `OpenApi` derive paths/components.

- [ ] **Step 3: Build.** Run: `cd backend && cargo build -p prts-api`
  Expected: compiles.

- [ ] **Step 4: Add an API db-test.** In `prts-api` db-tests, seed entries and `GET /projects/{id}/search?q=...`; assert 200 + ordering + that `include_hidden` is permission-gated. Verified in CI.

- [ ] **Step 5: Commit & push.**

```bash
git add backend/crates/prts-api/src/routes/
git commit -m "feat(api): GET /projects/{id}/search — hybrid FTS+trgm with RRF (vector off)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

### Task 11: Frontend — advanced search filters + results

**Files:**
- Modify: `frontend/src/api/types.ts`, `frontend/src/api/index.ts`
- Create: `frontend/src/components/SearchFilters.vue`
- Modify: `frontend/src/views/EditorView.vue`
- Modify: `frontend/src/i18n/*`

- [ ] **Step 1: Add API types + client.** In `types.ts`:

```ts
export interface SearchHitDto extends EntryDto { relevance: number }
```

In `index.ts` add to (or near) `entriesApi`:

```ts
export const searchApi = {
  search(
    id: number,
    params: { q?: string; source_q?: string; translation_q?: string;
              file_id?: number; state?: string; sort?: string;
              offset?: number; limit?: number; include_hidden?: boolean },
  ) {
    return http.get<SearchHitDto[]>(`/projects/${id}/search`, { params }).then((r) => r.data)
  },
}
```

- [ ] **Step 2: Build the filter bar component.** Create `SearchFilters.vue` with Quasar inputs: a search `q` field, a state multi-select (`q-select multiple`), a file selector, and a sort dropdown (`relevance|key|updated_at`). Emit a `search` event with the params object. Use user-facing zh/en labels via `t(...)` (no developer jargon).

- [ ] **Step 3: Wire into the editor.** In `EditorView.vue`, when the filter bar emits `search` with a non-empty query, call `searchApi.search(props.id, params)` and render the returned hits in the left list (show a small relevance bar/percent). When the query is cleared, fall back to the existing `/entries` browse (`entriesApi.list`). Keep keyset browse untouched.

- [ ] **Step 4: i18n.** Add keys: `search.placeholder`, `search.state`, `search.sort.relevance|key|updated`, `search.file`, `search.noResults` in both `zh-CN` and `en` locale files.

- [ ] **Step 5: Verify.** Run: `cd frontend && pnpm lint && pnpm build`
  Expected: lint clean, build succeeds. Then manually: type a query, confirm ranked results; clear it, confirm browse returns.

- [ ] **Step 6: Commit.**

```bash
git add frontend/src/
git commit -m "feat(frontend): advanced search filters + ranked results in editor

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase 3 — Embedding provider, settings, admin endpoint, sweep worker, vector path

### Task 12: `QwenProvider` + `EmbedError` (TDD local for parsing)

**Files:**
- Create: `backend/crates/prts-search/src/qwen.rs`
- Modify: `backend/crates/prts-search/src/lib.rs` (`pub mod qwen;`)
- Modify: `backend/crates/prts-search/Cargo.toml` (add `reqwest`, `pgvector`, `thiserror`, `serde`, `serde_json`)

- [ ] **Step 1: Write provider + a parse test.** Create `qwen.rs`:

```rust
//! Qwen 向量化（DashScope OpenAI 兼容端点）。密钥仅 env；model/base_url 运行时传入。
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("http: {0}")] Http(String),
    #[error("api {0}: {1}")] Api(u16, String),
    #[error("parse: {0}")] Parse(String),
}

pub struct QwenProvider {
    http: reqwest::Client,
    api_key: String,
    dimensions: usize,
}

#[derive(Serialize)]
struct EmbedReq<'a> { model: &'a str, input: &'a [String], dimensions: usize }
#[derive(Deserialize)]
struct EmbedResp { data: Vec<EmbedDatum> }
#[derive(Deserialize)]
struct EmbedDatum { embedding: Vec<f32> }

impl QwenProvider {
    pub fn new(api_key: String, dimensions: usize) -> Self {
        Self { http: reqwest::Client::new(), api_key, dimensions }
    }
    pub fn dimensions(&self) -> usize { self.dimensions }

    /// 单批 ≤10；调用方分块。base_url/model 取自当前 settings 快照。
    pub async fn embed_batch(&self, base_url: &str, model: &str, texts: &[String])
        -> Result<Vec<Vec<f32>>, EmbedError>
    {
        let url = format!("{}/embeddings", base_url.trim_end_matches('/'));
        let resp = self.http.post(url).bearer_auth(&self.api_key)
            .json(&EmbedReq { model, input: texts, dimensions: self.dimensions })
            .send().await.map_err(|e| EmbedError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(EmbedError::Api(status.as_u16(), resp.text().await.unwrap_or_default()));
        }
        let parsed: EmbedResp = resp.json().await.map_err(|e| EmbedError::Parse(e.to_string()))?;
        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_openai_compatible_response() {
        let body = r#"{"data":[{"embedding":[0.1,0.2]},{"embedding":[0.3,0.4]}]}"#;
        let r: EmbedResp = serde_json::from_str(body).unwrap();
        assert_eq!(r.data.len(), 2);
        assert_eq!(r.data[0].embedding, vec![0.1, 0.2]);
    }
}
```

- [ ] **Step 2: Wire module + deps.** Add `pub mod qwen;` to `lib.rs`. In `prts-search/Cargo.toml`: `reqwest = { workspace = true }`, `pgvector = { version = "0.4", features = ["sqlx"] }` (verify latest in `[workspace.dependencies]`), `thiserror.workspace = true`, `serde.workspace = true`, `serde_json.workspace = true`. Add `pgvector`/`thiserror` to `[workspace.dependencies]` if absent.

- [ ] **Step 3: Run the parse test.** Run: `cd backend && cargo test -p prts-search parses_openai_compatible_response`
  Expected: PASS. (Proxy must be up for first dep fetch; else push and let CI compile.)

- [ ] **Step 4: Commit.**

```bash
git add backend/crates/prts-search/ backend/Cargo.toml
git commit -m "feat(search): QwenProvider (DashScope OpenAI-compat) + response parse test

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 13: Typed search settings accessor in `prts-db`

**Files:**
- Create: `backend/crates/prts-db/src/search_settings.rs`
- Modify: `backend/crates/prts-db/src/lib.rs` (`pub mod search_settings;`)

- [ ] **Step 1: Write the accessor.** Reads the `settings` table keys with defaults; writes via the existing `settings::set`.

```rust
//! 搜索/向量化运行时配置（存 settings 表，管理后台可改）。
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub embedding_enabled: bool,
    pub embedding_model: String,
    pub embedding_base_url: String,
    pub embedding_batch: i32,
    pub tm_enabled: bool,
    pub tm_min_similarity: f64,
    pub tm_top_n: i32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            embedding_enabled: false,
            embedding_model: "text-embedding-v4".into(),
            embedding_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            embedding_batch: 10,
            tm_enabled: true,
            tm_min_similarity: 0.30,
            tm_top_n: 3,
        }
    }
}

const KEY: &str = "search.config";

/// 读取（缺失返回默认）。
pub async fn get(pool: &PgPool) -> Result<SearchConfig, sqlx::Error> {
    match crate::settings::get(pool, KEY).await? {
        Some(v) => Ok(serde_json::from_value(v).unwrap_or_default()),
        None => Ok(SearchConfig::default()),
    }
}

/// 写入（校验后），clamp 危险字段。
pub async fn set(pool: &PgPool, mut cfg: SearchConfig, by: Option<i64>) -> Result<(), sqlx::Error> {
    cfg.embedding_batch = cfg.embedding_batch.clamp(1, 10);
    cfg.tm_top_n = cfg.tm_top_n.clamp(1, 3);
    cfg.tm_min_similarity = cfg.tm_min_similarity.clamp(0.0, 1.0);
    let v = serde_json::to_value(&cfg).unwrap();
    crate::settings::set(pool, KEY, &v, by).await
}
```

> Stored as one JSON blob under `search.config` (simpler than 7 keys; the spec's per-key table is logical, this is the physical encoding). If the existing `settings` repo signature differs, adapt the calls.

- [ ] **Step 2: Wire module + test.** Add `pub mod search_settings;`. Add a unit test asserting `SearchConfig::default().embedding_enabled == false` and that `set` clamps `embedding_batch` to ≤10 (pure, no DB — test the clamp by calling a small extracted `clamp` helper, or make `set`'s clamping a pure `fn normalize(cfg) -> cfg` and test that).

Refactor: extract `fn normalize(mut cfg: SearchConfig) -> SearchConfig` and test:

```rust
#[test]
fn normalize_clamps_dangerous_fields() {
    let n = normalize(SearchConfig { embedding_batch: 99, tm_top_n: 9, tm_min_similarity: 2.0, ..Default::default() });
    assert_eq!(n.embedding_batch, 10);
    assert_eq!(n.tm_top_n, 3);
    assert!((n.tm_min_similarity - 1.0).abs() < 1e-9);
}
```

- [ ] **Step 3: Run test.** Run: `cd backend && cargo test -p prts-db normalize_clamps`
  Expected: PASS.

- [ ] **Step 4: Commit.**

```bash
git add backend/crates/prts-db/src/search_settings.rs backend/crates/prts-db/src/lib.rs
git commit -m "feat(db): typed SearchConfig accessor over settings table

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 14: `AppState` — embedder + hot-swappable runtime

**Files:**
- Modify: `backend/crates/prts-api/src/state.rs`
- Modify: `backend/crates/prts-api/src/main.rs`

- [ ] **Step 1: Extend `AppState`.** Add fields:

```rust
    /// Qwen 向量化 provider（Some 当且仅当 env 配了 api_key）。
    pub embedder: std::sync::Arc<Option<prts_search::qwen::QwenProvider>>,
    /// 搜索运行时配置（管理后台可热改）。
    pub search_rt: std::sync::Arc<tokio::sync::RwLock<prts_db::search_settings::SearchConfig>>,
```

- [ ] **Step 2: Build them in `main.rs`.** After the DB pool is ready, before constructing `AppState`:

```rust
    // 向量化 provider：仅当 env 配了 key 才构造（决定 Some/None）。
    let embedder = std::sync::Arc::new(
        if settings.embedding.qwen.api_key.is_empty() {
            None
        } else {
            Some(prts_search::qwen::QwenProvider::new(
                settings.embedding.qwen.api_key.clone(),
                settings.embedding.qwen.dimensions,
            ))
        }
    );
    // 运行时搜索配置（从 settings 表加载，缺省默认）。
    let search_cfg = prts_db::search_settings::get(&db).await.unwrap_or_default();
    let search_rt = std::sync::Arc::new(tokio::sync::RwLock::new(search_cfg));
```

Add `embedder: embedder.clone()` and `search_rt: search_rt.clone()` to the `AppState { .. }` literal.

- [ ] **Step 3: Build.** Run: `cd backend && cargo build -p prts-api`
  Expected: compiles.

- [ ] **Step 4: Commit.**

```bash
git add backend/crates/prts-api/src/state.rs backend/crates/prts-api/src/main.rs
git commit -m "feat(api): embedder + hot-swappable search runtime in AppState

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 15: Admin settings endpoint `GET|PUT /admin/settings/search`

**Files:**
- Create: `backend/crates/prts-api/src/routes/admin_settings.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`

- [ ] **Step 1: Write handlers.** GET returns the config + `embedding_key_present` (derived from env; never the value). PUT validates, persists, and hot-swaps `search_rt`. Gate with the existing platform-admin guard (find it in `routes/admin.rs`).

```rust
//! GET|PUT /admin/settings/search — 向量化/搜索运行时配置（密钥不在此处）。
use axum::{extract::State, Json};
use serde::Serialize;
use prts_db::search_settings::SearchConfig;
use crate::{error::ApiError, state::AppState, auth_ext::AdminUser};

#[derive(Serialize, utoipa::ToSchema)]
pub struct SearchSettingsDto {
    #[serde(flatten)]
    pub config: SearchConfig,
    /// 是否已在 env 配置 Qwen API Key（不下发 key 值）。
    pub embedding_key_present: bool,
}

#[utoipa::path(get, path = "/admin/settings/search", tag = "admin",
    responses((status = 200, body = SearchSettingsDto), (status = 403)))]
pub async fn get_search_settings(
    State(state): State<AppState>, _admin: AdminUser,
) -> Result<Json<SearchSettingsDto>, ApiError> {
    let config = state.search_rt.read().await.clone();
    let embedding_key_present = !state.settings.embedding.qwen.api_key.is_empty();
    Ok(Json(SearchSettingsDto { config, embedding_key_present }))
}

#[utoipa::path(put, path = "/admin/settings/search", tag = "admin",
    request_body = SearchConfig,
    responses((status = 200, body = SearchSettingsDto), (status = 403)))]
pub async fn put_search_settings(
    State(state): State<AppState>, admin: AdminUser, Json(cfg): Json<SearchConfig>,
) -> Result<Json<SearchSettingsDto>, ApiError> {
    prts_db::search_settings::set(&state.db, cfg.clone(), Some(admin.id))
        .await.map_err(crate::routes::db_err)?;
    // 热替换运行时（重新读回规范化后的值）
    let fresh = prts_db::search_settings::get(&state.db).await.map_err(crate::routes::db_err)?;
    *state.search_rt.write().await = fresh.clone();
    let embedding_key_present = !state.settings.embedding.qwen.api_key.is_empty();
    Ok(Json(SearchSettingsDto { config: fresh, embedding_key_present }))
}
```

> If the admin guard extractor isn't named `AdminUser`, use the real one from `routes/admin.rs`. `SearchConfig` must `derive(utoipa::ToSchema)` — add that derive in Task 13's struct.

- [ ] **Step 2: Register routes + OpenAPI** in `routes/mod.rs`.

- [ ] **Step 3: Build + db-test.** Build locally; add a db-test: PUT with `embedding_batch=99` returns `10` (clamped) and `embedding_key_present=false` in CI env. Push for CI.

- [ ] **Step 4: Commit & push.**

```bash
git add backend/crates/prts-api/src/routes/
git commit -m "feat(api): admin GET|PUT /admin/settings/search (key stays env-only)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

### Task 16: Vector recall query + wire into orchestrator & `/search`

**Files:**
- Modify: `backend/crates/prts-db/src/search.rs` (add `vector_search`)
- Modify: `backend/crates/prts-api/src/routes/search.rs` (populate `vector_ids`)

- [ ] **Step 1: Add `vector_search`.** Append to `search.rs`:

```rust
use pgvector::Vector;

/// 向量召回：cosine 距离最近的 per_path 条（仅 embedding 非空）。
pub async fn vector_search(
    pool: &PgPool, project_id: i64, qvec: &[f32],
    file_ids: &[i64], states: &[String], include_hidden: bool, per_path: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let v = Vector::from(qvec.to_vec());
    let mut qb = QueryBuilder::new("SELECT id FROM entries WHERE project_id = ");
    qb.push_bind(project_id).push(" AND embedding IS NOT NULL");
    push_filters(&mut qb, file_ids, states, include_hidden);
    qb.push(" ORDER BY embedding <=> ").push_bind(v).push(" LIMIT ").push_bind(per_path);
    qb.build_query_scalar().fetch_all(pool).await
}
```

- [ ] **Step 2: Populate `vector_ids` in the handler.** Replace the `let vector_ids: Option<Vec<i64>> = None;` line in `search.rs` with:

```rust
    // 向量路：仅当 settings 开 + env 有 key。失败/关 → None（降级）。
    let vector_ids: Option<Vec<i64>> = {
        let rt = state.search_rt.read().await.clone();
        match (rt.embedding_enabled, state.embedder.as_ref()) {
            (true, Some(p)) => match p.embed_batch(&rt.embedding_base_url, &rt.embedding_model,
                                                   std::slice::from_ref(&main_q)).await {
                Ok(mut v) if !v.is_empty() => {
                    let qvec = v.remove(0);
                    prts_db::search::vector_search(&state.db, id, &qvec, &file_ids, &states,
                        include_hidden, 100).await.ok()
                }
                Ok(_) => None,
                Err(e) => { tracing::warn!("query embed failed, degrading: {e}"); None }
            },
            _ => None,
        }
    };
```

- [ ] **Step 3: Build.** Run: `cd backend && cargo build -p prts-api`
  Expected: compiles.

- [ ] **Step 4: DB-test the vector path.** Seed two entries, manually `UPDATE entries SET embedding = '[...]'::vector` with known vectors, and assert `vector_search` orders by cosine proximity to a query vector. (No real Qwen call — bind known vectors.) Push for CI.

- [ ] **Step 5: Commit & push.**

```bash
git add backend/crates/prts-db/src/search.rs backend/crates/prts-api/src/routes/search.rs
git commit -m "feat(search): pgvector recall wired into /search (degrades when off)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

### Task 17: Background sweep worker

**Files:**
- Create: `backend/crates/prts-api/src/embed_worker.rs`
- Modify: `backend/crates/prts-api/src/main.rs` (spawn) + `backend/crates/prts-api/src/lib.rs`/`mod` wiring

- [ ] **Step 1: Write the worker.** Re-reads config each loop so the admin toggle takes effect at runtime.

```rust
//! 后台嵌入 sweep：把 embedding IS NULL 的词条分批向量化。开关/配置每轮重读。
use std::{sync::Arc, time::Duration};
use prts_db::search_settings::SearchConfig;
use prts_search::qwen::QwenProvider;
use sqlx::PgPool;
use tokio::sync::RwLock;

const IDLE: Duration = Duration::from_secs(30);
const ACTIVE: Duration = Duration::from_secs(1);
const MAX_ATTEMPTS: i16 = 5;
const SELECT_LIMIT: i64 = 50;

pub fn spawn(db: PgPool, embedder: Arc<Option<QwenProvider>>, rt: Arc<RwLock<SearchConfig>>) {
    tokio::spawn(async move {
        loop {
            let cfg = rt.read().await.clone();
            let provider = match (cfg.embedding_enabled, embedder.as_ref()) {
                (true, Some(p)) => p,
                _ => { tokio::time::sleep(IDLE).await; continue; }
            };
            let rows: Vec<(i64, String)> = match sqlx::query_as(
                "SELECT id, source_text FROM entries
                 WHERE embedding IS NULL AND source_text <> '' AND embed_attempts < $1
                 ORDER BY id LIMIT $2")
                .bind(MAX_ATTEMPTS).bind(SELECT_LIMIT).fetch_all(&db).await
            {
                Ok(r) => r,
                Err(e) => { tracing::error!("sweep select failed: {e}"); tokio::time::sleep(IDLE).await; continue; }
            };
            if rows.is_empty() { tokio::time::sleep(IDLE).await; continue; }

            let batch = cfg.embedding_batch.clamp(1, 10) as usize;
            for chunk in rows.chunks(batch) {
                let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
                match provider.embed_batch(&cfg.embedding_base_url, &cfg.embedding_model, &texts).await {
                    Ok(vecs) => {
                        for ((id, captured), vec) in chunk.iter().zip(vecs) {
                            let v = pgvector::Vector::from(vec);
                            // 乐观：仅当 source_text 仍是抓取时的值才写，避免覆盖并发改源
                            let _ = sqlx::query(
                                "UPDATE entries SET embedding=$1, embed_attempts=0
                                 WHERE id=$2 AND source_text=$3")
                                .bind(v).bind(id).bind(captured).execute(&db).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("embed batch failed: {e}");
                        let ids: Vec<i64> = chunk.iter().map(|(id, _)| *id).collect();
                        let _ = sqlx::query(
                            "UPDATE entries SET embed_attempts = embed_attempts + 1 WHERE id = ANY($1)")
                            .bind(&ids).execute(&db).await;
                    }
                }
            }
            tokio::time::sleep(ACTIVE).await; // Qwen QPS 节流
        }
    });
}
```

- [ ] **Step 2: Spawn in `main.rs`.** Add `mod embed_worker;` and after building `embedder`/`search_rt`:

```rust
    crate::embed_worker::spawn(db.clone(), embedder.clone(), search_rt.clone());
```

- [ ] **Step 3: Build.** Run: `cd backend && cargo build -p prts-api`
  Expected: compiles.

- [ ] **Step 4: Commit.**

```bash
git add backend/crates/prts-api/src/embed_worker.rs backend/crates/prts-api/src/main.rs
git commit -m "feat(api): background embedding sweep worker (runtime-toggleable, backfills)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 18: Frontend — admin "搜索 / 向量化" settings

**Files:**
- Modify: `frontend/src/api/types.ts`, `frontend/src/api/index.ts`, `frontend/src/views/AdminView.vue`, `frontend/src/i18n/*`

- [ ] **Step 1: API types + client.**

```ts
export interface SearchConfigDto {
  embedding_enabled: boolean; embedding_model: string; embedding_base_url: string;
  embedding_batch: number; tm_enabled: boolean; tm_min_similarity: number; tm_top_n: number;
}
export interface SearchSettingsDto extends SearchConfigDto { embedding_key_present: boolean }
```

```ts
export const adminSearchApi = {
  get() { return http.get<SearchSettingsDto>('/admin/settings/search').then((r) => r.data) },
  put(cfg: SearchConfigDto) { return http.put<SearchSettingsDto>('/admin/settings/search', cfg).then((r) => r.data) },
}
```

- [ ] **Step 2: Add the settings section** in `AdminView.vue`: a toggle for `embedding_enabled`, inputs for model/base_url/batch, toggles/sliders for `tm_*`, and a **read-only** chip showing `embedding_key_present` ("已配置 Key:是/否"). **Never** render an input for the key. Save via `adminSearchApi.put`. Show a notice when `embedding_enabled` is on but `embedding_key_present` is false ("已开启但未配置密钥，将降级为 FTS+trgm").

- [ ] **Step 3: i18n** keys for all labels (zh-CN + en), user-facing wording.

- [ ] **Step 4: Verify.** Run: `cd frontend && pnpm lint && pnpm build`
  Expected: clean. Manually toggle and confirm persistence after reload.

- [ ] **Step 5: Commit.**

```bash
git add frontend/src/
git commit -m "feat(frontend): admin search/vectorization settings (key shown as present-only)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase 4 — TM suggestions

### Task 19: Suggestions query in `prts-db`

**Files:**
- Modify: `backend/crates/prts-db/src/search.rs`

- [ ] **Step 1: Add the suggestions query.** Membership-scoped, language-matched, state-gated, self-excluded. Vector ordering when the current entry has an embedding; else trgm.

```rust
/// TM 建议候选行（含来源项目名）。
#[derive(Debug, sqlx::FromRow)]
pub struct SuggestionRow {
    pub entry_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub source_text: String,
    pub translation: String,
    pub state: String,
    pub similarity: f64,
}

/// 向量版：当前词条已有 embedding。仅用户已加入项目 + 同 target_lang。
pub async fn suggestions_vector(
    pool: &PgPool, user_id: i64, target_lang: &str, cur_embedding: &[f32],
    cur_entry_id: i64, min_sim: f64, top_n: i64,
) -> Result<Vec<SuggestionRow>, sqlx::Error> {
    let v = pgvector::Vector::from(cur_embedding.to_vec());
    sqlx::query_as::<_, SuggestionRow>(
        "SELECT e.id AS entry_id, p.id AS project_id, p.name AS project_name,
                e.source_text, e.translation, e.state,
                1 - (e.embedding <=> $1) AS similarity
         FROM entries e
         JOIN projects p ON p.id = e.project_id
         JOIN memberships m ON m.project_id = p.id AND m.user_id = $2
         WHERE p.target_lang = $3
           AND e.state IN ('translated','questioned','checked','reviewed')
           AND e.translation <> '' AND e.source_text <> ''
           AND e.id <> $4 AND e.embedding IS NOT NULL
           AND (1 - (e.embedding <=> $1)) >= $5
         ORDER BY e.embedding <=> $1
         LIMIT $6")
        .bind(v).bind(user_id).bind(target_lang).bind(cur_entry_id).bind(min_sim).bind(top_n)
        .fetch_all(pool).await
}

/// trgm 版：向量关或当前词条无 embedding 时用源文相似度。
pub async fn suggestions_trgm(
    pool: &PgPool, user_id: i64, target_lang: &str, cur_source: &str,
    cur_entry_id: i64, min_sim: f64, top_n: i64,
) -> Result<Vec<SuggestionRow>, sqlx::Error> {
    sqlx::query_as::<_, SuggestionRow>(
        "SELECT e.id AS entry_id, p.id AS project_id, p.name AS project_name,
                e.source_text, e.translation, e.state,
                similarity(e.source_text, $1) AS similarity
         FROM entries e
         JOIN projects p ON p.id = e.project_id
         JOIN memberships m ON m.project_id = p.id AND m.user_id = $2
         WHERE p.target_lang = $3
           AND e.state IN ('translated','questioned','checked','reviewed')
           AND e.translation <> '' AND e.source_text <> ''
           AND e.id <> $4
           AND similarity(e.source_text, $1) >= $5
         ORDER BY similarity(e.source_text, $1) DESC
         LIMIT $6")
        .bind(cur_source).bind(user_id).bind(target_lang).bind(cur_entry_id).bind(min_sim).bind(top_n)
        .fetch_all(pool).await
}
```

> `state IN (...)` encodes "状态 ≥ 已翻译" (excludes only `untranslated`). Keep this list in sync with the `EntryState` enum.

- [ ] **Step 2: DB-test.** Seed entries across two projects the user belongs to + one they don't; assert suggestions exclude the non-member project, exclude self, respect target_lang and state. Push for CI.

- [ ] **Step 3: Commit & push.**

```bash
git add backend/crates/prts-db/src/search.rs
git commit -m "feat(db): membership-scoped TM suggestion queries (vector + trgm)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

### Task 20: Suggestions endpoint

**Files:**
- Create: `backend/crates/prts-api/src/routes/suggestions.rs`
- Modify: `backend/crates/prts-api/src/routes/mod.rs`

- [ ] **Step 1: Write the handler.** Requires an authenticated user (TM is per-user membership). Loads the current entry + project target_lang, picks vector vs trgm path.

```rust
//! GET /projects/{id}/entries/{entryId}/suggestions — 跨项目 TM 建议（仅用户已加入项目）。
use axum::{extract::{Path, State}, Json};
use serde::Serialize;
use crate::{error::ApiError, state::AppState, auth_ext::AuthUser};

#[derive(Serialize, utoipa::ToSchema)]
pub struct SuggestionDto {
    pub entry_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub source_text: String,
    pub translation: String,
    pub state: String,
    pub similarity: f64,
}

#[utoipa::path(get, path = "/projects/{id}/entries/{entry_id}/suggestions", tag = "search",
    params(("id" = i64, Path, description="项目 ID"), ("entry_id" = i64, Path, description="词条 ID")),
    responses((status = 200, body = [SuggestionDto]), (status = 401), (status = 404)))]
pub async fn entry_suggestions(
    State(state): State<AppState>, user: AuthUser,
    Path((id, entry_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<SuggestionDto>>, ApiError> {
    let cfg = state.search_rt.read().await.clone();
    if !cfg.tm_enabled { return Ok(Json(vec![])); }

    // 当前词条 + 项目目标语言（含 source_text / embedding 存在性）
    let cur = prts_db::entries::get(&state.db, id, entry_id).await
        .map_err(crate::routes::db_err)?.ok_or(ApiError::not_found("entry"))?;
    let (_src, tgt) = crate::routes::projects::primary_langs(&state, id).await?;

    let min = cfg.tm_min_similarity;
    let top = cfg.tm_top_n as i64;

    let rows = if cfg.embedding_enabled {
        // 取当前词条 embedding；NULL → 回落 trgm
        match prts_db::search::current_embedding(&state.db, entry_id).await.map_err(crate::routes::db_err)? {
            Some(vec) => prts_db::search::suggestions_vector(&state.db, user.id, &tgt, &vec, entry_id, min, top).await,
            None => prts_db::search::suggestions_trgm(&state.db, user.id, &tgt, &cur.source_text, entry_id, min, top).await,
        }
    } else {
        prts_db::search::suggestions_trgm(&state.db, user.id, &tgt, &cur.source_text, entry_id, min, top).await
    }.map_err(crate::routes::db_err)?;

    Ok(Json(rows.into_iter().map(|r| SuggestionDto {
        entry_id: r.entry_id, project_id: r.project_id, project_name: r.project_name,
        source_text: r.source_text, translation: r.translation, state: r.state, similarity: r.similarity,
    }).collect()))
}
```

> Add a tiny `prts_db::search::current_embedding(pool, entry_id) -> Result<Option<Vec<f32>>>` reading the row's `embedding` as `Option<pgvector::Vector>` and mapping to `Vec<f32>`. `cur.source_text` requires `Entry` to expose `source_text` — add the field to the `Entry` model + `SELECT` (it already exists post-0004; ensure the `models.rs` struct includes `source_text: String`).

- [ ] **Step 2: Register route + OpenAPI.** Add to `routes/mod.rs`.

- [ ] **Step 3: Build + db-test the endpoint** (CI). Build locally; push.

- [ ] **Step 4: Commit & push.**

```bash
git add backend/crates/prts-api/src/routes/ backend/crates/prts-db/src/
git commit -m "feat(api): GET entry suggestions endpoint (vector/trgm, TM-enabled gated)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

### Task 21: Frontend — suggestions panel below the translation panel

**Files:**
- Create: `frontend/src/components/SuggestionsPanel.vue`
- Modify: `frontend/src/api/types.ts`, `frontend/src/api/index.ts`, `frontend/src/views/EditorView.vue`, `frontend/src/i18n/*`

- [ ] **Step 1: API type + client.**

```ts
export interface SuggestionDto {
  entry_id: number; project_id: number; project_name: string;
  source_text: string; translation: string; state: string; similarity: number;
}
```

```ts
export const suggestionsApi = {
  forEntry(projectId: number, entryId: number) {
    return http.get<SuggestionDto[]>(`/projects/${projectId}/entries/${entryId}/suggestions`).then((r) => r.data)
  },
}
```

- [ ] **Step 2: Build `SuggestionsPanel.vue`.** Props: `suggestions: SuggestionDto[]`. Renders ≤3 Quasar cards, each showing source_text, translation, source project name, and a similarity percent badge. Emits `apply(translation: string)` on click. Empty state hidden (render nothing when list empty). Theme-aware, mobile-friendly.

```vue
<template>
  <div v-if="suggestions.length" class="suggestions q-mt-md">
    <div class="prts-label q-mb-xs">{{ t('suggestions.title') }}</div>
    <q-card v-for="s in suggestions" :key="s.entry_id" flat bordered
            class="q-pa-sm q-mb-xs cursor-pointer" @click="$emit('apply', s.translation)">
      <div class="row items-center no-wrap">
        <div class="col">
          <div class="prts-dim" style="font-size:12px">{{ s.source_text }}</div>
          <div class="prts-mono">{{ s.translation }}</div>
        </div>
        <q-badge outline>{{ Math.round(s.similarity * 100) }}%</q-badge>
      </div>
      <div class="prts-dim" style="font-size:11px">{{ s.project_name }}</div>
    </q-card>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { SuggestionDto } from '../api/types'
defineProps<{ suggestions: SuggestionDto[] }>()
defineEmits<{ (e: 'apply', translation: string): void }>()
const { t } = useI18n()
</script>
```

- [ ] **Step 3: Wire into `EditorView.vue`.** Below the translation `q-input` (after the textarea, around line 386), add `<SuggestionsPanel :suggestions="suggestions" @apply="onApplySuggestion" />`. On entry select, fetch: `suggestions.value = await suggestionsApi.forEntry(props.id, selected.value.id)` (wrap in try/catch, ignore failures silently — suggestions are non-critical). `onApplySuggestion(tr)` sets `draft.value = tr` (does **not** auto-save).

- [ ] **Step 4: i18n.** `suggestions.title = 翻译建议 / Suggestions`.

- [ ] **Step 5: Verify.** Run: `cd frontend && pnpm lint && pnpm build`
  Expected: clean. Manually: open an entry, confirm ≤3 suggestions appear, clicking fills the draft.

- [ ] **Step 6: Commit.**

```bash
git add frontend/src/
git commit -m "feat(frontend): TM suggestions panel below translation editor

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase 5 — Docs, perf verify, closure

### Task 22: Architecture docs + Swagger review

**Files:**
- Modify: `docs/architecture.md`

- [ ] **Step 1:** Add a "P4 混合搜索 + TM 建议" section to `docs/architecture.md`: the three-way recall + RRF, the embedding default-off/admin-config model, the sweep worker, and the suggestions flow. Reference the spec.
- [ ] **Step 2:** Open `/swagger-ui` (or read the generated OpenAPI) and confirm `/projects/{id}/search`, the suggestions path, and the admin settings paths appear with descriptions.
- [ ] **Step 3: Commit.**

```bash
git add docs/architecture.md
git commit -m "docs: document P4 hybrid search + TM suggestions architecture

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task 23: Perf verify (search latency)

**Files:**
- Create: `backend/crates/prts-api/tests/search_perf.rs` (feature-gated `db-tests`, `#[ignore]` by default)

- [ ] **Step 1:** Write an `#[ignore]` db-test that seeds N entries (parameter via env, default 50_000) with random `source_text` + deterministic embeddings, then times `/search` (or the orchestrator directly) over several queries and prints p50/p95. Assert it completes; log timings.
- [ ] **Step 2:** Document how to run it (`cargo test -p prts-api --features db-tests -- --ignored search_perf`) and the observed numbers in the PR / `docs/architecture.md`. Note the 20w target.
- [ ] **Step 3: Commit & push.**

```bash
git add backend/crates/prts-api/tests/search_perf.rs
git commit -m "test(perf): ignored search latency benchmark over seeded entries

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

### Task 24: Full local gate + phase closure

- [ ] **Step 1: Backend gate.** Run (retry on Defender): `cd backend && cargo fmt && cargo clippy --all-targets && cargo test`
  Expected: fmt clean, no clippy warnings, all non-db tests pass.
- [ ] **Step 2: Frontend gate.** Run: `cd frontend && pnpm lint && pnpm build`
  Expected: clean.
- [ ] **Step 3: Push and confirm CI green** (fmt/clippy/test + db-tests on zhparser image + frontend build + GHCR images for backend/frontend/postgres). Run: `git push` and watch CI.
- [ ] **Step 4: Commit the spec + plan docs** (deferred from brainstorming) alongside closure:

```bash
git add docs/superpowers/specs/2026-06-29-p4-hybrid-search-design.md docs/superpowers/plans/2026-06-29-p4-hybrid-search.md
git commit -m "docs(plan): P4 hybrid search + TM suggestions spec & plan

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
git push
```

- [ ] **Step 5:** Verify GHCR has updated `prts-backend`, `prts-frontend`, and the new `prts-postgres` images (CLAUDE.md §5 closure).

---

## Self-review (completed during planning)

**Spec coverage** — every spec section maps to a task:
- §3 schema/migration/trigger → Task 3; backfill in Task 3 Step 1.
- §4 zhparser image + CI → Tasks 2, 4.
- §5 QwenProvider → Task 12 (async_trait dropped; concrete provider — noted refinement).
- §6 config/settings/admin/degrade → Tasks 1, 13, 14, 15; degrade in Tasks 10 & 16.
- §7 sweep worker → Task 17.
- §8 hybrid + RRF + /search → Tasks 6, 7, 8, 9, 10, 16.
- §9 TM suggestions → Tasks 19, 20, 21.
- §10 frontend (search/suggestions/admin) → Tasks 11, 18, 21.
- §11 error handling/degrade → Tasks 10, 16, 17, 20 (silent suggestion failures, query-embed warn-degrade, worker backoff).
- §12 tests → unit (Tasks 1, 6, 12, 13); db (Tasks 5, 8, 10, 15, 16, 19, 20); perf (Task 23).
- §13 files → covered across tasks.

**Placeholder scan** — no TBD/"handle edge cases"/"similar to". Where existing symbol names are uncertain (admin guard extractor, `load_access`, `primary_langs`, `parse_states`), tasks explicitly say to find the real name in the named file and reuse/extract it — these are lookups, not placeholders.

**Type consistency** — `SearchConfig` (Task 13) is reused verbatim in Tasks 14/15/17; `QwenProvider::embed_batch(base_url, model, texts)` signature consistent across Tasks 12/16/17; `SuggestionRow`→`SuggestionDto` field names align (Tasks 19/20); `vector_ids: Option<Vec<i64>>` slot defined in Task 9, filled in Task 16. `Entry.source_text` field dependency flagged in Task 20.

**Known lookups the implementer must resolve against the live code** (not placeholders — concrete instructions given): exact names of the auth extractors (`AuthUser`/`AdminUser`/`MaybeUser`), the project-access helper, and the db-test harness/fixture macro. Each task names the file to check.
