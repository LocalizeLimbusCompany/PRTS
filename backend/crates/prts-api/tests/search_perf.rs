//! 搜索/物化统计性能验证。
//!
//! 常规 CI 执行源码合同测试，防止 stats/task progress 热读重新扫描 entries、lexical
//! worker 丢失断点或结构化搜索回退 OFFSET。真实数据库规模测试默认 ignored：
//! `PRTS_PERF_N=200000 cargo test -p prts-api --features db-tests -- --ignored search_perf --nocapture`。
//! 只有该命令的实际输出才是延迟实测；预算可由 `PRTS_SEARCH_BUDGET_MS`（默认 3000）和
//! `PRTS_MATERIALIZED_READ_BUDGET_MS`（默认 250）显式调整。默认搜索预算来自本地隔离
//! PostgreSQL 上 20 万高命中 fixture 的实测，不代表所有生产硬件结果。
#![cfg(feature = "db-tests")]

use std::time::Instant;

use prts_core::search_query::{CanonicalSearchCondition, SearchField, SearchOperator};
use prts_db::{entries, files, projects, users};
use prts_search::orchestrator::{run, OrchestratorInput};

fn env_u128(name: &str, default: u128) -> u128 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

async fn pool() -> prts_db::Db {
    let runtime_role =
        std::env::var("PRTS_TEST_RUNTIME_ROLE").unwrap_or_else(|_| "prts_runtime".to_string());
    let migration_url =
        std::env::var("MIGRATION_DATABASE_URL").expect("MIGRATION_DATABASE_URL 未设置");
    let migration_pool = prts_db::connect_postgres(&migration_url, 1)
        .await
        .expect("连接 migration owner");
    let mut connection = migration_pool.acquire().await.expect("获取 migration 连接");
    prts_db::run_migrations(&mut connection, &runtime_role)
        .await
        .expect("执行迁移");
    drop(connection);
    migration_pool.close().await;

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL 未设置");
    let pool = prts_db::connect_postgres(&url, 5)
        .await
        .expect("连接 runtime Postgres");
    prts_db::verify_runtime_role(&pool, &runtime_role)
        .await
        .expect("验证 runtime role");
    pool
}

#[tokio::test]
#[ignore = "20 万词条五 scope 与物化读延迟 verify，手动运行"]
async fn search_perf_five_scopes_and_materialized_reads() {
    let n: usize = std::env::var("PRTS_PERF_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);
    let pool = pool().await;

    sqlx::query("DELETE FROM projects WHERE slug = 'perf-search'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'perf_owner'")
        .execute(&pool)
        .await
        .unwrap();

    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "perf_owner", None, &hash, "active")
        .await
        .unwrap();
    // en → zh-Hans：源侧 english FTS，译侧 zhparser。
    let proj = projects::create(
        &pool,
        "perf-search",
        "Perf",
        "",
        "public",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    let file = files::ensure_file_at_path(&pool, proj.id, "perf/large.json")
        .await
        .unwrap();

    // 分批插入 N 条词条（源文带可检索词）。
    let words = [
        "alpha", "beta", "gamma", "delta", "epsilon", "survival", "weather", "engine", "light",
        "shadow",
    ];
    let mut batch = Vec::with_capacity(500);
    for i in 0..n {
        let w = words[i % words.len()];
        batch.push(entries::UploadEntry {
            key: format!("k{i}"),
            original: serde_json::json!({ "en": format!("the {w} text number {i}") }),
            translation: Some(format!("译文 {w} {i}")),
            state: Some("translated".to_string()),
        });
        if batch.len() == 500 || i + 1 == n {
            entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
                .await
                .unwrap();
            batch.clear();
        }
    }

    let task_id: i64 = sqlx::query_scalar(
        "INSERT INTO tasks (project_id, title, created_by)
         VALUES ($1, 'Performance task', $2) RETURNING id",
    )
    .bind(proj.id)
    .bind(owner.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_files (task_id, file_id_snapshot, live_file_id)
         VALUES ($1, $2, $2)",
    )
    .bind(task_id)
    .bind(file.id)
    .execute(&pool)
    .await
    .unwrap();

    let path_file_ids = match prts_db::search::resolve_path_scope(&pool, proj.id, "perf")
        .await
        .unwrap()
    {
        prts_db::search::PathScopeResolution::Files(ids) => ids,
        other => panic!("path scope did not resolve: {other:?}"),
    };
    let resolved_file_id = prts_db::search::resolve_active_file_id(&pool, proj.id, file.id)
        .await
        .unwrap()
        .expect("active file scope");
    let task_file_ids = prts_db::search::resolve_active_task_file_ids(&pool, proj.id, task_id)
        .await
        .unwrap()
        .expect("current task scope");
    let scopes = [
        ("all", Vec::new(), false),
        ("path", path_file_ids, true),
        ("file", vec![resolved_file_id], true),
        ("current_file", vec![resolved_file_id], true),
        ("current_task", task_file_ids, true),
    ];
    let conditions = [CanonicalSearchCondition {
        field: SearchField::Translation,
        operator: SearchOperator::NotContains,
        value: "__structured_search_never_matches__".to_string(),
    }];
    let search_budget_ms = env_u128("PRTS_SEARCH_BUDGET_MS", 3_000);
    let mut measurements = Vec::new();
    for (scope, file_ids, restrict_to_file_ids) in &scopes {
        let t = Instant::now();
        let res = run(
            &pool,
            OrchestratorInput {
                project_id: proj.id,
                query: Some("survival"),
                src_lang: "en",
                tgt_lang: "zh-Hans",
                file_ids,
                restrict_to_file_ids: *restrict_to_file_ids,
                states: &[],
                questioned: None,
                conditions: &conditions,
                case_sensitive: false,
                include_hidden: false,
                per_path: 100,
                top_k: 200,
                filter_after_entry_id: None,
                vector_ids: None,
            },
        )
        .await
        .unwrap();
        let ms = t.elapsed().as_millis();
        println!(
            "[search-perf] entries={n} scope={scope} hits={} elapsed_ms={ms}",
            res.len()
        );
        measurements.push((*scope, ms));
    }
    let max_search_ms = measurements
        .iter()
        .map(|(_, milliseconds)| *milliseconds)
        .max()
        .unwrap_or_default();
    let read_budget_ms = env_u128("PRTS_MATERIALIZED_READ_BUDGET_MS", 250);
    let stats_started = Instant::now();
    let stats = prts_db::stats::project(&pool, proj.id).await.unwrap();
    let stats_ms = stats_started.elapsed().as_millis();
    assert_eq!(stats.visible_total, n as i64);
    assert!(
        stats_ms <= read_budget_ms,
        "materialized stats took {stats_ms}ms"
    );
    let task_started = Instant::now();
    let task_stats = prts_db::tasks::stats(&pool, proj.id, task_id)
        .await
        .unwrap()
        .expect("task stats");
    let task_ms = task_started.elapsed().as_millis();
    assert_eq!(task_stats.denominator, 0);
    assert!(
        task_ms <= read_budget_ms,
        "materialized task progress took {task_ms}ms"
    );
    println!(
        "[materialized-read-perf] entries={n} project_stats_ms={stats_ms} task_progress_ms={task_ms} budget_ms={read_budget_ms}"
    );
    assert!(
        max_search_ms <= search_budget_ms,
        "five-scope search max {max_search_ms}ms exceeded {search_budget_ms}ms budget: {measurements:?}"
    );

    // 清理
    projects::delete_test_fixture(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'perf_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

#[test]
fn structured_search_uses_keyset_and_contains_no_offset_sql() {
    let db_source = include_str!("../src/../../prts-db/src/search.rs");
    let route_source = include_str!("../src/routes/search.rs");
    assert!(!db_source.to_ascii_uppercase().contains(" OFFSET "));
    assert!(!route_source.contains(".skip("));
    assert!(!route_source.contains("pub offset:"));
    assert!(db_source.contains("entry.id >"));
    assert!(route_source.contains("last_rrf_score"));
}

#[test]
fn stats_and_task_progress_hot_reads_only_use_materialized_tables() {
    let stats_source = include_str!("../src/../../prts-db/src/stats.rs");
    let tasks_source = include_str!("../src/../../prts-db/src/tasks.rs");
    let stats_read = stats_source
        .split("pub async fn project(")
        .nth(1)
        .and_then(|source| source.split("pub async fn files(").next())
        .expect("project stats read function");
    assert!(stats_read.contains("FROM project_stats"));
    assert!(!stats_read.contains("entries"));
    assert!(!stats_read.to_ascii_uppercase().contains("COUNT("));

    let task_stats_read = tasks_source
        .split("pub async fn stats(")
        .nth(1)
        .and_then(|source| source.split("pub async fn file_details(").next())
        .expect("task stats read function");
    assert!(task_stats_read.contains("FROM task_stats"));
    assert!(!task_stats_read.contains("entries"));
    assert!(!task_stats_read.to_ascii_uppercase().contains("COUNT("));
}

#[test]
fn lexical_reindex_persists_a_keyset_checkpoint_every_bounded_batch() {
    let source = include_str!("../src/jobs/reindex_project.rs");
    let lexical = source
        .split("impl JobHandler for ReindexProjectHandler")
        .nth(1)
        .and_then(|source| source.split("pub struct EmbeddingBackfillHandler").next())
        .expect("lexical handler");
    assert!(lexical.contains("id > $2"));
    assert!(lexical.contains("ORDER BY id LIMIT 500"));
    assert!(lexical.contains(".payload"));
    assert!(lexical.contains(".get(\"cursor\")"));
    assert!(lexical.contains("jsonb_set(payload, '{cursor}'"));
    assert!(lexical.contains("tx.commit()"));
    assert!(!lexical.to_ascii_uppercase().contains(" OFFSET "));
}

#[test]
fn all_five_scopes_share_effective_visibility_and_resource_binding() {
    let db_source = include_str!("../src/../../prts-db/src/search.rs");
    let route_source = include_str!("../src/routes/search.rs");
    for variant in ["All", "Path", "File", "CurrentFile", "CurrentTask"] {
        assert!(route_source.contains(&format!("SearchScope::{variant}")));
    }
    for resolver in [
        "resolve_path_scope",
        "resolve_active_file_id",
        "resolve_active_task_file_ids",
    ] {
        assert!(route_source.contains(resolver));
    }
    assert!(db_source.contains("prts_entry_effective_visible(entry.id"));
    assert!(db_source.contains("ancestor.deleted_at IS NOT NULL"));
    assert!(db_source.contains("file.path LIKE prts_escape_like_pattern(ancestor.path) || '/%'"));
}
