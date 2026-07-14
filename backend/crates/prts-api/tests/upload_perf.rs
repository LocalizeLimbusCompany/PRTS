//! 原子 replacement 的手动规模 verify。
//!
//! 运行：
//! `cargo test -p prts-api --features db-tests -- --ignored upload_perf --nocapture`
//! `PRTS_UPLOAD_PERF_N=200000` 用于生产目标 20 万词条；默认 20000 便于本地预检。
//! 同一过滤器还执行 `PRTS_UPLOAD_PERF_MB=100` 的 bounded parser 实测。常规 CI 只执行
//! 源码/设置合同，验证 500 文件、100MiB、2GiB、byte-zero attempt、取消/过期清理与
//! 30 天 restoration purge；未实际执行 ignored 测试时不得声称规模实测完成。
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

#[test]
fn upload_limits_and_byte_zero_attempt_lifecycle_are_fixed_contracts() {
    let settings = include_str!("../src/../../prts-db/src/upload_settings.rs");
    let routes = include_str!("../src/routes/uploads.rs");
    let uploads = include_str!("../src/../../prts-db/src/uploads.rs");
    assert!(settings.contains("max_files_per_batch: 500"));
    assert!(settings.contains("max_bytes_per_file: 100 * MEBIBYTE"));
    assert!(settings.contains("max_bytes_per_batch: 2 * GIBIBYTE"));
    assert!(settings.contains("upload_batch_expiry_hours: 24"));
    assert!(routes.contains("header::RANGE"));
    assert!(routes.contains("header::CONTENT_RANGE"));
    assert!(routes.contains("upload_resume_not_supported"));
    assert!(uploads.contains("COALESCE(max(attempt_number), 0) + 1"));
    assert!(uploads.contains("ORDER BY file.ordinal, attempt.attempt_number"));
    assert!(uploads.contains("current_attempt_id"));
    assert!(uploads.contains("processing_job_id"));
}

#[test]
fn cancellation_expiry_partial_success_and_cleanup_are_durable_contracts() {
    let uploads = include_str!("../src/../../prts-db/src/uploads.rs");
    let cleanup = include_str!("../src/jobs/cleanup_uploads.rs");
    assert!(uploads.contains("SET state = 'cancelling'"));
    assert!(
        uploads.contains("WHEN batch.state = 'cancelling' AND counts.active = 0 THEN 'cancelled'")
    );
    assert!(uploads.contains("WHEN counts.succeeded > 0 THEN 'partially_succeeded'"));
    assert!(
        uploads.contains("state IN ('draft', 'uploading', 'queued', 'processing', 'cancelling')")
    );
    assert!(uploads.contains("SET state = 'expired'"));
    assert!(uploads.contains("state IN ('failed', 'cancelled', 'expired', 'succeeded')"));
    assert!(cleanup.contains("list_cleanup_candidates"));
    assert!(cleanup.contains("mark_attempt_cleaned"));
}

#[test]
fn replacement_history_and_thirty_day_purge_remove_restoration_payload() {
    let replacement = include_str!("../src/jobs/process_upload.rs");
    let history = include_str!("../src/../../prts-core/src/file_history.rs");
    let purge = include_str!("../src/jobs/purge_deleted_files.rs");
    assert!(replacement.contains("apply_staged_replacement_tx"));
    assert!(replacement.contains("mark_processing_succeeded_tx"));
    assert!(history.contains("DEFAULT_RETENTION_DAYS: i64 = 30"));
    assert!(history.contains("plan_file_rollback"));
    assert!(history.contains("plan_folder_rollback"));
    assert!(purge.contains("purge_due_operation_tx"));
    assert!(purge.contains("task file/entry live refs"));
}

#[test]
fn stage8_recovery_wiring_resumes_durable_stages_without_leaking_internal_errors() {
    let language = include_str!("../src/jobs/repair_languages.rs");
    let upload = include_str!("../src/jobs/process_upload.rs");
    let cleanup = include_str!("../src/jobs/cleanup_uploads.rs");
    let reindex = include_str!("../src/jobs/reindex_project.rs");
    let file_purge = include_str!("../src/jobs/purge_deleted_files.rs");
    let project_purge = include_str!("../src/jobs/purge_project.rs");

    assert!(language.contains("UPDATE jobs SET stage = 'entries', progress_current = $2"));
    assert!(language.contains("tx.commit()"));
    assert!(upload.contains("begin_processing_tx"));
    assert!(upload.contains("stage_parsed_upload(&mut tx"));
    assert!(upload.contains("tx.commit()"));

    assert!(reindex.contains("payload = jsonb_set(payload, '{cursor}'"));
    assert!(reindex.contains("kind = 'primary_source_embedding_backfill'"));
    assert!(reindex.contains("embedding provider request failed"));
    assert!(reindex.contains("retryable: true"));

    assert!(file_purge.contains("purge_due_operation_tx"));
    assert!(file_purge.contains("tx.commit()"));
    assert!(file_purge.contains("retryable: true"));
    assert!(project_purge.contains("mark_external_cleanup_pending_tx"));
    assert!(project_purge.contains("if job.stage != \"external_cleanup_pending\""));
    assert!(project_purge.contains("external project cleanup failed"));
    assert!(project_purge.contains("retryable: true"));

    assert!(!upload.contains("message: format!"));
    assert!(!cleanup.contains("message: format!"));
    assert!(upload.contains("upload replacement database operation failed"));
    assert!(cleanup.contains("upload cleanup database operation failed"));
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
