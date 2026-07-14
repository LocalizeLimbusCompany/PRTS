//! 搜索性能基准（手动运行，默认 `#[ignore]`，不在常规 CI 执行）。
//!
//! 运行：`cargo test -p prts-api --features db-tests -- --ignored search_perf --nocapture`
//! 规模经环境变量 `PRTS_PERF_N` 调整（默认 2000；生产目标量级 20w，按需调大）。
//! 验证：HNSW/GIN 索引 + 有界 top-K + 键集浏览下，单项目大规模搜索延迟可接受。
#![cfg(feature = "db-tests")]

use std::time::Instant;

use prts_core::search_query::{CanonicalSearchCondition, SearchField, SearchOperator};
use prts_db::{entries, files, projects, users};
use prts_search::orchestrator::{run, OrchestratorInput};

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
#[ignore = "性能基准，手动运行"]
async fn search_perf_orchestrator() {
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
    let file = files::ensure_file_at_path(&pool, proj.id, "perf.json")
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

    // 计时若干次查询（向量关，走 FTS + trgm）。
    let queries = ["survival", "weather", "engine light", "alpha"];
    let conditions = [CanonicalSearchCondition {
        field: SearchField::Translation,
        operator: SearchOperator::NotContains,
        value: "__structured_search_never_matches__".to_string(),
    }];
    let mut max_ms = 0u128;
    for q in queries {
        let t = Instant::now();
        let res = run(
            &pool,
            OrchestratorInput {
                project_id: proj.id,
                query: Some(q),
                src_lang: "en",
                tgt_lang: "zh-Hans",
                file_ids: &[],
                restrict_to_file_ids: false,
                states: &[],
                conditions: &conditions,
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
        max_ms = max_ms.max(ms);
        println!("[perf] N={n} q={q:?} hits={} {ms}ms", res.len());
    }
    println!("[perf] N={n} max={max_ms}ms（目标量级 20w；HNSW/GIN 索引 + 有界 top-K）");

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
