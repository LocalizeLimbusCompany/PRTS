//! 原子 replacement 的手动规模 verify。
//!
//! 运行：
//! `cargo test -p prts-api --features db-tests -- --ignored upload_perf --nocapture`
//! `PRTS_UPLOAD_PERF_N=200000` 用于生产目标 20 万词条；默认 20000 便于本地预检。
#![cfg(feature = "db-tests")]

use prts_core::upload_replacement::{EntryStatsDelta, ReplacementSummary};

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

#[test]
fn upload_worker_source_keeps_bounded_streaming_contract() {
    let source = include_str!("../src/jobs/process_upload.rs");
    assert!(source.contains("spawn_blocking"));
    assert!(source.contains("mpsc::channel(PARSER_CHANNEL_CAPACITY)"));
    assert!(source.contains("STAGING_BATCH_SIZE"));
    assert!(source.contains("PLAN_PAGE_SIZE"));
    assert!(!source.contains("read_to_end"));
    assert!(!source.contains("read_to_string"));
    assert!(!source.contains("serde_json::from_slice"));
}

#[tokio::test]
#[ignore = "20 万词条集合 replacement verify，手动运行"]
async fn upload_perf_stages_plans_in_bounded_cursor_pages() {
    let n: usize = std::env::var("PRTS_UPLOAD_PERF_N")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20_000);
    let pool = pool().await;
    let marker = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let username = format!("upload-perf-{marker}");
    let slug = format!("upload-perf-{marker}");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'perf-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    let project_id: i64 = sqlx::query_scalar(
        "INSERT INTO projects (slug, name, source_langs, target_lang, owner_id)
         VALUES ($1, $1, ARRAY['en'], 'zh-Hans', $2) RETURNING id",
    )
    .bind(&slug)
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let file = prts_db::files::ensure_file_at_path(&pool, project_id, "perf/large.json")
        .await
        .unwrap();

    let started = std::time::Instant::now();
    let mut tx = pool.begin().await.unwrap();
    prts_db::entries::create_replacement_temp_tables_tx(&mut tx)
        .await
        .unwrap();
    let mut batch = Vec::with_capacity(250);
    for ordinal in 0..n {
        batch.push(prts_db::entries::ReplacementStagedEntry {
            ordinal: ordinal as i64,
            key: format!("key-{ordinal:08}"),
            original: std::collections::BTreeMap::from([(
                "en".to_string(),
                format!("source text {ordinal}"),
            )]),
            translation: None,
            state: None,
        });
        if batch.len() == 250 || ordinal + 1 == n {
            prts_db::entries::stage_replacement_entries_tx(&mut tx, &batch)
                .await
                .unwrap();
            batch.clear();
        }
    }
    assert!(prts_db::entries::finalize_replacement_staging_tx(&mut tx)
        .await
        .unwrap()
        .is_none());
    prts_db::entries::lock_replacement_entries_tx(&mut tx, file.id)
        .await
        .unwrap();
    prts_db::entries::declare_replacement_input_cursor_tx(&mut tx, file.id)
        .await
        .unwrap();
    let effective_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT transaction_timestamp()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let mut missing_ordinal = n as i64;
    let mut summary = ReplacementSummary::default();
    let mut stats_delta = EntryStatsDelta::default();
    loop {
        let page = prts_db::entries::plan_and_stage_replacement_page_tx(
            &mut tx,
            &mut missing_ordinal,
            effective_at,
            500,
        )
        .await
        .unwrap();
        if !page.has_rows {
            break;
        }
        assert!(page.plan.transitions.len() <= 500);
        summary.inserted += page.plan.summary.inserted;
        summary.restored += page.plan.summary.restored;
        summary.source_changed += page.plan.summary.source_changed;
        summary.tombstoned += page.plan.summary.tombstoned;
        summary.unchanged += page.plan.summary.unchanged;
        stats_delta += page.plan.stats_delta;
    }
    let applied = prts_db::entries::apply_staged_replacement_tx(
        &mut tx,
        project_id,
        file.id,
        &file.path,
        user_id,
        summary,
        stats_delta,
        effective_at,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(applied.summary.inserted, n);
    assert_eq!(applied.stats_delta.visible_total, n as i64);
    println!(
        "[upload-perf] entries={n} elapsed_ms={}",
        started.elapsed().as_millis()
    );

    // 先删 file：files_stats_delete_trg 一次扣除物化统计，再由 FK cascade entries；
    // 禁止对 20 万 entries 逐行触发 stats decrement。
    sqlx::query("DELETE FROM files WHERE project_id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM file_change_items WHERE change_set_id IN (
             SELECT id FROM file_change_sets WHERE project_id = $1
         )",
    )
    .bind(project_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM file_change_sets WHERE project_id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}
