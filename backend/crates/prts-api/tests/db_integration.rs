//! DB 集成测试：对真实 PostgreSQL 跑通仓储层 SQL 与迁移。
//!
//! 仅在 `db-tests` 特性下编译，并需要 runtime `DATABASE_URL` 与 owner
//! `MIGRATION_DATABASE_URL`。迁移和所有合同 DML 使用不同数据库角色。
//! 本地无 DB 时默认不编译；CI 会起 Postgres 服务后执行（见 .github/workflows/ci.yml）。
#![cfg(feature = "db-tests")]

use prts_db::{
    api_keys, auth_sessions, entries, files, jobs, memberships, messages, notifications, projects,
    settings, users,
};

static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

fn runtime_role() -> String {
    std::env::var("PRTS_TEST_RUNTIME_ROLE").unwrap_or_else(|_| "prts_runtime".to_string())
}

async fn ensure_migrated() {
    MIGRATED
        .get_or_init(|| async {
            let url =
                std::env::var("MIGRATION_DATABASE_URL").expect("MIGRATION_DATABASE_URL 未设置");
            let migration_pool = prts_db::connect_postgres(&url, 1)
                .await
                .expect("以 migration owner 连接 Postgres");
            let mut migration_conn = migration_pool.acquire().await.expect("获取 migration 连接");
            prts_db::run_migrations(&mut migration_conn, &runtime_role())
                .await
                .expect("以 migration owner 执行迁移");
            drop(migration_conn);
            migration_pool.close().await;
        })
        .await;
}

async fn pool() -> prts_db::Db {
    ensure_migrated().await;
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL 未设置");
    let pool = prts_db::connect_postgres(&url, 5)
        .await
        .expect("以 runtime role 连接 Postgres");
    prts_db::verify_runtime_role(&pool, &runtime_role())
        .await
        .expect("runtime role 必须与 migration owner 分离");
    pool
}

/// `0007` 必须同时建立追加式审计与持久化任务两项基础设施。
///
/// 该测试先于迁移编写，用于证明旧 schema 会明确报缺表，而不是因测试装配错误失败。
#[tokio::test]
async fn audit_jobs_tables_exist_after_migration() {
    let pool = pool().await;

    sqlx::query("SELECT id FROM audit_log LIMIT 0")
        .execute(&pool)
        .await
        .expect("0007 应创建 audit_log");
    sqlx::query("SELECT id FROM jobs LIMIT 0")
        .execute(&pool)
        .await
        .expect("0007 应创建 jobs");
}

/// 生成不会与并行 CI 或上次失败运行冲突的测试标识。
fn audit_jobs_unique(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时间应晚于 UNIX epoch")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

/// 创建任务外键测试所需的最小用户与项目。
async fn audit_jobs_project(pool: &prts_db::Db, prefix: &str) -> (i64, i64, String) {
    let suffix = audit_jobs_unique(prefix);
    let username = format!("u-{suffix}");
    let slug = format!("p-{suffix}");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-only-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(pool)
    .await
    .unwrap();
    let project_id: i64 = sqlx::query_scalar(
        "INSERT INTO projects (slug, name, source_langs, target_lang, owner_id)
         VALUES ($1, $1, ARRAY['en'], 'zh-Hans', $2) RETURNING id",
    )
    .bind(&slug)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap();
    (user_id, project_id, slug)
}

/// 审计记录只能追加，数据库层必须拒绝篡改与删除。
#[tokio::test]
async fn audit_jobs_audit_log_is_append_only_and_secret_payloads_are_rejected() {
    let pool = pool().await;
    let (current_role, table_owner): (String, String) = sqlx::query_as(
        "SELECT current_user::TEXT,
                pg_get_userbyid(class.relowner)::TEXT
         FROM pg_class AS class
         JOIN pg_namespace AS namespace ON namespace.oid = class.relnamespace
         WHERE namespace.nspname = 'public' AND class.relname = 'audit_log'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(current_role, runtime_role());
    assert_ne!(current_role, table_owner, "runtime 不得拥有 audit_log");
    let role_safety: (bool, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT NOT role.rolcreatedb,
                NOT role.rolcreaterole,
                NOT role.rolreplication,
                NOT role.rolbypassrls,
                NOT has_schema_privilege(current_user, 'public', 'CREATE')
         FROM pg_roles AS role
         WHERE role.rolname = current_user",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(role_safety, (true, true, true, true, true));
    let privileges: (bool, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT
             has_table_privilege(current_user, 'audit_log', 'SELECT'),
             has_table_privilege(current_user, 'audit_log', 'INSERT'),
             has_table_privilege(current_user, 'audit_log', 'UPDATE'),
             has_table_privilege(current_user, 'audit_log', 'DELETE'),
             has_table_privilege(current_user, 'audit_log', 'TRUNCATE')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(privileges, (true, true, false, false, false));

    let target = audit_jobs_unique("audit-target");
    let audit_id: i64 = sqlx::query_scalar(
        "INSERT INTO audit_log (
             actor_id, actor_kind, action, target_type, target_id,
             project_id_snapshot, payload, ip
         )
         VALUES (NULL, 'system', 'job.created', 'job', $1, NULL, $2, '127.0.0.1')
         RETURNING id",
    )
    .bind(&target)
    .bind(serde_json::json!({"kind": "test_job"}))
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query("UPDATE audit_log SET action = 'tampered' WHERE id = $1")
        .bind(audit_id)
        .execute(&pool)
        .await
        .expect_err("audit_log UPDATE 必须由数据库拒绝");

    sqlx::query("TRUNCATE audit_log")
        .execute(&pool)
        .await
        .expect_err("audit_log TRUNCATE 必须由 ACL 或数据库 trigger 拒绝");

    sqlx::query("DELETE FROM audit_log WHERE id = $1")
        .bind(audit_id)
        .execute(&pool)
        .await
        .expect_err("audit_log DELETE 必须由数据库拒绝");

    for payload in [
        serde_json::json!({"token": "raw"}),
        serde_json::json!({"refreshToken": "raw"}),
        serde_json::json!({"access_token": "raw"}),
        serde_json::json!({"apiKey": "raw"}),
        serde_json::json!({"password": "raw"}),
        serde_json::json!({"secret": "raw"}),
        serde_json::json!({"verifier": "raw"}),
        serde_json::json!({"code": "raw"}),
        serde_json::json!({"nested": [{"clientSecret": "raw"}]}),
    ] {
        let secret_error = sqlx::query(
            "INSERT INTO audit_log (
                 actor_id, actor_kind, action, target_type, target_id, payload
             ) VALUES (NULL, 'system', 'auth.failed', 'session', $1, $2)",
        )
        .bind(audit_jobs_unique("secret-audit"))
        .bind(payload)
        .execute(&pool)
        .await
        .expect_err("大小写与命名风格变化不得绕过 secret-key 检查");
        assert!(secret_error.as_database_error().is_some());
    }
}

/// 审计使用非级联 snapshot，用户和项目删除后仍能解释历史目标。
#[tokio::test]
async fn audit_jobs_audit_actor_and_project_snapshots_survive_source_deletion() {
    let pool = pool().await;
    let (user_id, project_id, slug) = audit_jobs_project(&pool, "audit-snapshot").await;
    let target = audit_jobs_unique("deleted-project");
    let audit_id: i64 = sqlx::query_scalar(
        "INSERT INTO audit_log (
             actor_id, actor_kind, action, target_type, target_id,
             project_id_snapshot, payload
         ) VALUES ($1, 'user', 'project.deleted', 'project', $2, $3, $4)
         RETURNING id",
    )
    .bind(user_id)
    .bind(&target)
    .bind(project_id)
    .bind(serde_json::json!({"slug": slug}))
    .fetch_one(&pool)
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

    let snapshots: (Option<i64>, Option<i64>, String) = sqlx::query_as(
        "SELECT actor_id, project_id_snapshot, payload->>'slug'
         FROM audit_log WHERE id = $1",
    )
    .bind(audit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(snapshots, (Some(user_id), Some(project_id), slug));
}

/// 两个 worker 并发领取时，`SKIP LOCKED` 必须给出不同任务。
#[tokio::test]
async fn audit_jobs_workers_claim_distinct_rows_concurrently() {
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "claim-project").await;
    let marker = audit_jobs_unique("claim");
    let job_ids: Vec<i64> = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, payload)
         VALUES ($1, $2, '{}'), ($1, $2, '{}') RETURNING id",
    )
    .bind(&marker)
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .unwrap();

    let eligible = vec![marker.clone()];
    let (claimed_a, claimed_b) = tokio::join!(
        jobs::claim_next_for_ids_and_kinds(&pool, "worker-a", 300, &[], &eligible, &job_ids,),
        jobs::claim_next_for_ids_and_kinds(&pool, "worker-b", 300, &[], &eligible, &job_ids,)
    );
    let claimed_a = claimed_a.unwrap().unwrap().id;
    let claimed_b = claimed_b.unwrap().unwrap().id;

    assert_ne!(claimed_a, claimed_b);
    assert!(job_ids.contains(&claimed_a));
    assert!(job_ids.contains(&claimed_b));

    sqlx::query("DELETE FROM jobs WHERE kind = $1")
        .bind(marker)
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

/// 过期租约可由另一 worker 接管，未过期租约不可被抢占。
#[tokio::test]
async fn audit_jobs_expired_lease_is_taken_over_and_active_lease_is_preserved() {
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "lease-project").await;
    let expired_kind = audit_jobs_unique("expired");
    let active_kind = audit_jobs_unique("active");
    let expired_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, state, payload, worker_id, lease_until)
         VALUES ($1, $2, 'running', '{}', 'dead-worker', now() - interval '1 minute')
         RETURNING id",
    )
    .bind(&expired_kind)
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let active_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, state, payload, worker_id, lease_until)
         VALUES ($1, $2, 'running', '{}', 'live-worker', now() + interval '1 hour')
         RETURNING id",
    )
    .bind(&active_kind)
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let eligible = vec![expired_kind.clone(), active_kind.clone()];
    let claimed = jobs::claim_next_for_ids_and_kinds(
        &pool,
        "replacement-worker",
        300,
        &[],
        &eligible,
        &[expired_id, active_id],
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(claimed.id, expired_id);
    let owner: String = sqlx::query_scalar("SELECT worker_id FROM jobs WHERE id = $1")
        .bind(expired_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(owner, "replacement-worker");

    sqlx::query("DELETE FROM jobs WHERE kind IN ($1, $2)")
        .bind(expired_kind)
        .bind(active_kind)
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

/// 最后一次允许的执行若在外部副作用后崩溃，过期租约只能被终结，不能再次执行。
#[tokio::test]
async fn audit_jobs_last_attempt_crash_is_failed_without_reclaim() {
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "last-attempt").await;
    let kind = audit_jobs_unique("last-attempt-kind");
    let exhausted_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (
             kind, project_id, state, payload, attempts, max_attempts, worker_id, lease_until
         ) VALUES ($1, $2, 'running', '{}', 1, 1, 'crashed-worker',
                   now() - interval '1 minute')
         RETURNING id",
    )
    .bind(&kind)
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let claimed = jobs::claim_next_for_ids_and_kinds(
        &pool,
        "replacement-worker",
        300,
        &[],
        std::slice::from_ref(&kind),
        &[exhausted_id],
    )
    .await
    .unwrap();
    assert!(claimed.is_none(), "耗尽任务不得重复外部副作用");
    let exhausted = jobs::find_by_id(&pool, exhausted_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(exhausted.state, "failed");
    assert_eq!(exhausted.attempts, 1);
    assert_eq!(
        exhausted.last_error_code.as_deref(),
        Some("job_attempts_exhausted")
    );
    assert!(exhausted.worker_id.is_none());
    assert!(exhausted.lease_until.is_none());

    assert!(jobs::claim_next_for_ids_and_kinds(
        &pool,
        "third-worker",
        300,
        &[],
        std::slice::from_ref(&kind),
        &[exhausted_id],
    )
    .await
    .unwrap()
    .is_none());

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(exhausted_id)
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

/// 只有当前持有者能续租；旧 worker 在接管后不能延长新租约。
#[tokio::test]
async fn audit_jobs_lease_renewal_requires_current_worker() {
    let pool = pool().await;
    let kind = audit_jobs_unique("renew");
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, state, payload, worker_id, lease_until)
         VALUES ($1, 'running', '{}', 'current-worker', now() + interval '1 minute')
         RETURNING id",
    )
    .bind(&kind)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(!jobs::renew_lease(&pool, job_id, "stale-worker", 300)
        .await
        .unwrap());
    assert!(jobs::renew_lease(&pool, job_id, "current-worker", 300)
        .await
        .unwrap());

    sqlx::query("UPDATE jobs SET lease_until = now() - interval '1 second' WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !jobs::renew_lease(&pool, job_id, "current-worker", 300)
            .await
            .unwrap(),
        "过期 lease 不得被原 worker 复活"
    );

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// 手动重试复用稳定 job id，并清理上次执行的租约与错误。
#[tokio::test]
async fn audit_jobs_manual_retry_reuses_same_id_and_increments_attempts() {
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "retry-project").await;
    let kind = audit_jobs_unique("retry");
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (
             kind, project_id, state, payload, attempts, max_attempts, worker_id, lease_until,
             last_error_code, last_error_message
         )
         VALUES ($1, $2, 'failed', '{}', 3, 3, 'failed-worker', now(), 'TEMP', 'temporary')
         RETURNING id",
    )
    .bind(&kind)
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let locked = jobs::find_by_id_for_update_tx(&mut tx, job_id)
        .await
        .unwrap()
        .unwrap();
    let delete_pool = pool.clone();
    let mut delete_task =
        tokio::spawn(async move { projects::delete(&delete_pool, project_id).await.unwrap() });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut delete_task)
            .await
            .is_err(),
        "project delete 应等待已锁定 job，避免 retry TOCTOU"
    );
    let retried = jobs::manual_retry_tx(&mut tx, locked.id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    assert!(delete_task.await.unwrap());

    assert_eq!(retried.id, job_id);
    assert_eq!(retried.attempts, 4);
    let detached = jobs::find_by_id(&pool, job_id).await.unwrap().unwrap();
    assert_eq!(detached.project_id, None);
    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// `0014` 前不引用未来列；调用者显式传入待删除项目集合完成暂停过滤。
#[tokio::test]
async fn audit_jobs_pending_deletion_filter_pauses_non_purge_jobs() {
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "pending-delete").await;
    let normal_kind = audit_jobs_unique("normal");
    let normal_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, payload) VALUES ($1, $2, '{}') RETURNING id",
    )
    .bind(&normal_kind)
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let purge_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, payload)
         VALUES ('project_purge', $1, $2) RETURNING id",
    )
    .bind(project_id)
    .bind(serde_json::json!({"project_id": project_id, "slug": "snapshot"}))
    .fetch_one(&pool)
    .await
    .unwrap();

    let eligible = vec![normal_kind.clone(), "project_purge".to_string()];
    let claimed = jobs::claim_next_for_ids_and_kinds(
        &pool,
        "purge-worker",
        300,
        &[project_id],
        &eligible,
        &[normal_id, purge_id],
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(claimed.id, purge_id);
    let normal_state: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(normal_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(normal_state, "paused");

    let pause_reason: Option<String> =
        sqlx::query_scalar("SELECT pause_reason FROM jobs WHERE id = $1")
            .bind(normal_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pause_reason.as_deref(), Some("project_pending_deletion"));

    let manually_paused_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, state, pause_reason, payload)
         VALUES ($1, $2, 'paused', 'manual', '{}') RETURNING id",
    )
    .bind(audit_jobs_unique("manual-pause"))
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        jobs::resume_project_jobs(&pool, project_id).await.unwrap(),
        1
    );
    let states: Vec<(i64, String, Option<String>)> =
        sqlx::query_as("SELECT id, state, pause_reason FROM jobs WHERE id IN ($1, $2) ORDER BY id")
            .bind(normal_id)
            .bind(manually_paused_id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(states.contains(&(normal_id, "queued".to_string(), None)));
    assert!(states.contains(&(
        manually_paused_id,
        "paused".to_string(),
        Some("manual".to_string())
    )));

    sqlx::query("DELETE FROM jobs WHERE project_id = $1")
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

/// 任务进度可在运行中持久化，且不可超过声明总量。
#[tokio::test]
async fn audit_jobs_progress_updates_are_bounded() {
    let pool = pool().await;
    let kind = audit_jobs_unique("progress");
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, state, payload, progress_current, progress_total)
         VALUES ($1, 'running', '{}', 0, 10) RETURNING id",
    )
    .bind(&kind)
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query(
        "UPDATE jobs
         SET worker_id = 'progress-worker', lease_until = now() + interval '5 minutes'
         WHERE id = $1",
    )
    .bind(job_id)
    .execute(&pool)
    .await
    .unwrap();
    let progress =
        jobs::update_progress(&pool, job_id, "progress-worker", 6, Some(10), "processing")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        (progress.progress_current, progress.progress_total),
        (6, Some(10))
    );

    sqlx::query("UPDATE jobs SET progress_current = 11 WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .expect_err("progress_current 不得超过 progress_total");
    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// 项目删除后 job 保留且外键置空；purge 仅凭不可变 payload snapshot 继续。
#[tokio::test]
async fn audit_jobs_project_delete_sets_null_and_purge_snapshot_survives() {
    let pool = pool().await;
    let (user_id, project_id, slug) = audit_jobs_project(&pool, "purge-snapshot").await;
    let deadline = "2026-07-11T00:00:00Z";
    let payload = serde_json::json!({
        "project_id": project_id,
        "slug": slug,
        "media_keys": ["projects/avatar.webp"],
        "temp_keys": ["uploads/pending.json"],
        "deadline": deadline
    });
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, payload)
         VALUES ('project_purge', $1, $2) RETURNING id",
    )
    .bind(project_id)
    .bind(&payload)
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
    let (live_project_id, stored_payload): (Option<i64>, serde_json::Value) =
        sqlx::query_as("SELECT project_id, payload FROM jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(live_project_id, None);
    assert_eq!(stored_payload, payload);

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// 项目删除后 detached non-purge 不可领取/重试，只有 purge worker 可依 snapshot 继续。
#[tokio::test]
async fn audit_jobs_detached_non_purge_is_not_claimed_or_retried() {
    let pool = pool().await;
    let (user_id, project_id, slug) = audit_jobs_project(&pool, "detached-job").await;
    let normal_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, payload)
         VALUES ('upload_process', $1, '{}') RETURNING id",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let failed_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, state, payload)
         VALUES ('upload_cleanup', $1, 'failed', '{}') RETURNING id",
    )
    .bind(project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let purge_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (kind, project_id, payload)
         VALUES ('project_purge', $1, $2) RETURNING id",
    )
    .bind(project_id)
    .bind(serde_json::json!({
        "project_id": project_id,
        "slug": slug,
        "media_keys": [],
        "temp_keys": [],
        "deadline": "2026-07-11T00:00:00Z"
    }))
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let locked = jobs::find_by_id_for_update_tx(&mut tx, failed_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(locked.project_id, None);
    assert!(jobs::manual_retry_tx(&mut tx, failed_id)
        .await
        .unwrap()
        .is_none());
    tx.commit().await.unwrap();

    let kinds = vec![
        "upload_process".to_string(),
        "upload_cleanup".to_string(),
        "project_purge".to_string(),
    ];
    let claimed = jobs::claim_next_for_ids_and_kinds(
        &pool,
        "detached-worker",
        300,
        &[],
        &kinds,
        &[normal_id, failed_id, purge_id],
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(claimed.id, purge_id);
    let normal_state: String = sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(normal_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(normal_state, "queued");

    sqlx::query("DELETE FROM jobs WHERE id IN ($1, $2, $3)")
        .bind(normal_id)
        .bind(failed_id)
        .bind(purge_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// 会话状态由 PostgreSQL 权威保存，支持签发、轮换、吊销和过期全生命周期。
#[tokio::test]
async fn audit_jobs_auth_session_state_machine_and_token_storage_contract() {
    let pool = pool().await;
    let username = audit_jobs_unique("session-user");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-only-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    let session_handle = audit_jobs_unique("session-handle");
    let family_handle = audit_jobs_unique("family-handle");
    let refresh_hash_text = prts_auth::token::sha256_hex("raw-refresh-token");
    let refresh_hash = auth_sessions::RefreshTokenHash::parse(refresh_hash_text.clone()).unwrap();
    assert!(auth_sessions::RefreshTokenHash::parse("raw-refresh-token".to_string()).is_err());
    assert!(
        auth_sessions::RefreshTokenHash::parse("cmF3LXJlZnJlc2gtdG9rZW4=".to_string()).is_err()
    );
    let mut tx = pool.begin().await.unwrap();
    let session = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: session_handle.clone(),
            family_handle: family_handle.clone(),
            user_id,
            refresh_token_hash: refresh_hash.clone(),
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    assert!(
        auth_sessions::lock_active_unexpired_by_refresh_hash_tx(&mut tx, &refresh_hash)
            .await
            .unwrap()
            .is_none(),
        "pending session 不得作为权威认证结果"
    );
    let session = auth_sessions::activate_pending_tx(&mut tx, session.id)
        .await
        .unwrap()
        .unwrap();
    let locked = auth_sessions::lock_active_unexpired_by_refresh_hash_tx(&mut tx, &refresh_hash)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(locked.id, session.id);
    let session = auth_sessions::begin_rotation_tx(&mut tx, session.id)
        .await
        .unwrap()
        .unwrap();
    let session = auth_sessions::revoke_unexpired_tx(&mut tx, session.id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    let session_id = session.id;

    let expired_hash = auth_sessions::RefreshTokenHash::parse(prts_auth::token::sha256_hex(
        "expired-raw-refresh-token",
    ))
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let expired = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("expired-handle"),
            family_handle: family_handle.clone(),
            user_id,
            refresh_token_hash: expired_hash,
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(1),
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    auth_sessions::expire_due_tx(&mut tx, expired.id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();

    sqlx::query(
        "INSERT INTO auth_sessions (
             session_handle, family_handle, user_id, refresh_token_hash, state, expires_at
         ) VALUES ($1, $2, $3, $4, 'pending', now() + interval '1 day')",
    )
    .bind(audit_jobs_unique("raw-hash-rejected"))
    .bind(audit_jobs_unique("raw-family"))
    .bind(user_id)
    .bind("cmF3LXJlZnJlc2gtdG9rZW4=")
    .execute(&pool)
    .await
    .expect_err("数据库必须拒绝非 SHA-256 hex refresh hash");

    sqlx::query("UPDATE auth_sessions SET state = 'unknown' WHERE id = $1")
        .bind(session_id)
        .execute(&pool)
        .await
        .expect_err("未知 auth session 状态必须由约束拒绝");

    let forbidden_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name IN ('auth_sessions', 'auth_session_intents', 'jobs', 'audit_log')
           AND column_name IN ('access_token', 'refresh_token', 'raw_access_token', 'raw_refresh_token')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(forbidden_columns, 0);

    let serialized_row: String =
        sqlx::query_scalar("SELECT row_to_json(s)::TEXT FROM auth_sessions AS s WHERE id = $1")
            .bind(session_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(serialized_row.contains(&refresh_hash_text));
    assert!(!serialized_row.contains("raw-access-token"));
    assert!(!serialized_row.contains("raw-refresh-token"));

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// rotation 的前后继关系必须可追踪，且删除后继时不会破坏历史会话行。
#[tokio::test]
async fn audit_jobs_auth_session_rotation_links_predecessor_and_successor() {
    let pool = pool().await;
    let username = audit_jobs_unique("rotation-user");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-only-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    let family = audit_jobs_unique("rotation-family");
    let mut tx = pool.begin().await.unwrap();
    let predecessor = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("predecessor"),
            family_handle: family.clone(),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("predecessor-raw-token"),
            )
            .unwrap(),
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let predecessor = auth_sessions::activate_pending_tx(&mut tx, predecessor.id)
        .await
        .unwrap()
        .unwrap();
    let predecessor = auth_sessions::begin_rotation_tx(&mut tx, predecessor.id)
        .await
        .unwrap()
        .unwrap();
    let successor = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("successor"),
            family_handle: family,
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("successor-raw-token"),
            )
            .unwrap(),
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            predecessor_id: Some(predecessor.id),
        },
    )
    .await
    .unwrap();
    let (predecessor, successor) =
        auth_sessions::complete_rotation_tx(&mut tx, predecessor.id, successor.id)
            .await
            .unwrap()
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(predecessor.successor_id, Some(successor.id));
    assert_eq!(successor.predecessor_id, Some(predecessor.id));
    assert_eq!(predecessor.state, auth_sessions::AuthSessionState::Revoked);
    assert_eq!(successor.state, auth_sessions::AuthSessionState::Active);

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// 每个 user/family 最多一个 active，会话激活与轮换必须走窄接口。
#[tokio::test]
async fn audit_jobs_auth_session_rejects_double_active_and_wrong_rotation_chain() {
    let pool = pool().await;
    let username = audit_jobs_unique("rotation-chain-user");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-only-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    let family = audit_jobs_unique("rotation-chain-family");
    let expires_at = chrono::Utc::now() + chrono::Duration::days(1);

    let mut tx = pool.begin().await.unwrap();
    let first = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("rotation-chain-first"),
            family_handle: family.clone(),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("rotation-chain-first-token"),
            )
            .unwrap(),
            expires_at,
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let _first = auth_sessions::activate_pending_tx(&mut tx, first.id)
        .await
        .unwrap()
        .unwrap();

    let duplicate = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("rotation-chain-duplicate"),
            family_handle: family.clone(),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("rotation-chain-duplicate-token"),
            )
            .unwrap(),
            expires_at,
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    assert!(
        auth_sessions::activate_pending_tx(&mut tx, duplicate.id)
            .await
            .is_err(),
        "partial unique index 必须拒绝同 family 双 active"
    );
    tx.rollback().await.unwrap();

    // 使用新事务构造两个 rotating predecessor；successor 精确链接 first，不能由 second 接管。
    let mut tx = pool.begin().await.unwrap();
    let first = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("wrong-chain-first"),
            family_handle: family.clone(),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("wrong-chain-first-token"),
            )
            .unwrap(),
            expires_at,
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let first = auth_sessions::activate_pending_tx(&mut tx, first.id)
        .await
        .unwrap()
        .unwrap();
    let first = auth_sessions::begin_rotation_tx(&mut tx, first.id)
        .await
        .unwrap()
        .unwrap();
    let second = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("wrong-chain-second"),
            family_handle: family.clone(),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("wrong-chain-second-token"),
            )
            .unwrap(),
            expires_at,
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let second = auth_sessions::activate_pending_tx(&mut tx, second.id)
        .await
        .unwrap()
        .unwrap();
    let second = auth_sessions::begin_rotation_tx(&mut tx, second.id)
        .await
        .unwrap()
        .unwrap();
    let successor = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("wrong-chain-successor"),
            family_handle: family,
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("wrong-chain-successor-token"),
            )
            .unwrap(),
            expires_at,
            predecessor_id: Some(first.id),
        },
    )
    .await
    .unwrap();
    sqlx::query("SAVEPOINT linked_activation_bypass")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE auth_sessions
         SET predecessor_id = NULL, state = 'active'
         WHERE id = $1",
    )
    .bind(successor.id)
    .execute(&mut *tx)
    .await
    .expect_err("linked pending 不得通过清空 predecessor 绕过 complete_rotation");
    sqlx::query("ROLLBACK TO SAVEPOINT linked_activation_bypass")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        auth_sessions::activate_pending_tx(&mut tx, successor.id)
            .await
            .unwrap()
            .is_none(),
        "带 predecessor 的 pending 不能走普通激活"
    );
    assert!(
        auth_sessions::complete_rotation_tx(&mut tx, second.id, successor.id)
            .await
            .unwrap()
            .is_none(),
        "successor.predecessor_id 不匹配时不得串链"
    );
    let unchanged: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, state, successor_id FROM auth_sessions WHERE id IN ($1, $2) ORDER BY id",
    )
    .bind(second.id)
    .bind(successor.id)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert!(unchanged.contains(&(second.id, "rotating".to_string(), None)));
    assert!(unchanged.contains(&(successor.id, "pending".to_string(), None)));
    tx.rollback().await.unwrap();

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// 权威 lookup/lock 只返回 active 且尚未过期的会话，并且公开值不携带 refresh hash。
#[tokio::test]
async fn audit_jobs_auth_authoritative_lookup_rejects_non_active_and_expired_rows() {
    let pool = pool().await;
    let username = audit_jobs_unique("lookup-user");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-only-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut expected_active = None;
    let mut rejected = Vec::new();
    for (label, state, expired) in [
        ("pending", "pending", false),
        ("active", "active", false),
        ("rotating", "rotating", false),
        ("revoked", "revoked", false),
        ("expired-state", "expired", false),
        ("expired-time", "active", true),
    ] {
        let handle = audit_jobs_unique(&format!("lookup-{label}"));
        let hash = auth_sessions::RefreshTokenHash::parse(prts_auth::token::sha256_hex(&format!(
            "lookup-{label}-raw-token"
        )))
        .unwrap();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO auth_sessions (
                 session_handle, family_handle, user_id, refresh_token_hash, state, expires_at
             ) VALUES ($1, $2, $3, $4, $5,
                       CASE WHEN $6 THEN now() - interval '1 second'
                            ELSE now() + interval '1 day' END)
             RETURNING id",
        )
        .bind(&handle)
        .bind(audit_jobs_unique(&format!("lookup-family-{label}")))
        .bind(user_id)
        .bind(hash.as_str())
        .bind(state)
        .bind(expired)
        .fetch_one(&pool)
        .await
        .unwrap();
        if label == "active" {
            expected_active = Some((id, handle, hash));
        } else {
            rejected.push((handle, hash));
        }
    }

    let (active_id, active_handle, active_hash) = expected_active.unwrap();
    let active = auth_sessions::find_active_unexpired_by_handle(&pool, &active_handle)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.id, active_id);
    assert_eq!(active.state, auth_sessions::AuthSessionState::Active);
    assert!(!format!("{active:?}").contains(active_hash.as_str()));

    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        auth_sessions::lock_active_unexpired_by_refresh_hash_tx(&mut tx, &active_hash)
            .await
            .unwrap()
            .unwrap()
            .id,
        active_id
    );
    for (handle, hash) in rejected {
        assert!(
            auth_sessions::find_active_unexpired_by_handle(&pool, &handle)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            auth_sessions::lock_active_unexpired_by_refresh_hash_tx(&mut tx, &hash)
                .await
                .unwrap()
                .is_none()
        );
    }
    tx.rollback().await.unwrap();

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// auth intent/outbox 使用租约领取，租约过期后复用同一 intent 行重试。
#[tokio::test]
async fn audit_jobs_auth_intent_lease_takeover_and_retry_reuse_same_id() {
    let pool = pool().await;
    let username = audit_jobs_unique("intent-user");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-only-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let session = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("intent-session"),
            family_handle: audit_jobs_unique("intent-family"),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("intent-raw-refresh-token"),
            )
            .unwrap(),
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let session = auth_sessions::activate_pending_tx(&mut tx, session.id)
        .await
        .unwrap()
        .unwrap();
    let intent = auth_sessions::enqueue_intent_tx(
        &mut tx,
        session.id,
        auth_sessions::AuthIntentPayload::RedisInvalidate {
            session_handle: session.session_handle.clone(),
        },
        2,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let session_id = session.id;
    let intent_id = intent.id;
    sqlx::query(
        "UPDATE auth_session_intents
         SET state = 'running', attempts = 1, worker_id = 'dead-worker',
             lease_until = now() - interval '1 minute'
         WHERE id = $1",
    )
    .bind(intent_id)
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        !auth_sessions::renew_intent_lease(&pool, intent_id, "dead-worker", 300)
            .await
            .unwrap(),
        "过期 intent lease 不得被原 worker 复活"
    );
    let claimed =
        auth_sessions::claim_intent_for_session(&pool, "replacement-worker", 300, session_id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(claimed.id, intent_id);
    assert!(
        !auth_sessions::renew_intent_lease(&pool, intent_id, "dead-worker", 300)
            .await
            .unwrap()
    );
    assert!(
        auth_sessions::renew_intent_lease(&pool, intent_id, "replacement-worker", 300)
            .await
            .unwrap()
    );
    let failed = auth_sessions::fail_intent(
        &pool,
        intent_id,
        "replacement-worker",
        "cache_unavailable",
        "redacted cache failure",
        1,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(failed.state, auth_sessions::AuthIntentState::Failed);
    let mut tx = pool.begin().await.unwrap();
    let retried = auth_sessions::retry_intent_tx(&mut tx, intent_id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(retried.id, intent_id);
    assert_eq!(retried.attempts, 3);

    sqlx::query(
        "INSERT INTO auth_session_intents (session_id, kind, payload)
         VALUES ($1, 'redis_populate', $2)",
    )
    .bind(session_id)
    .bind(serde_json::json!({"access_token": "raw-access-token"}))
    .execute(&pool)
    .await
    .expect_err("auth intent payload 不得接收 raw token 字段");

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// auth intent 的最后一次尝试崩溃后必须稳定失败，不能重复 Redis 外部副作用。
#[tokio::test]
async fn audit_jobs_auth_intent_last_attempt_crash_is_failed_without_reclaim() {
    let pool = pool().await;
    let username = audit_jobs_unique("intent-exhausted-user");
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash) VALUES ($1, 'test-only-hash') RETURNING id",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let session = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: audit_jobs_unique("intent-exhausted-session"),
            family_handle: audit_jobs_unique("intent-exhausted-family"),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("intent-exhausted-raw-token"),
            )
            .unwrap(),
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let session = auth_sessions::activate_pending_tx(&mut tx, session.id)
        .await
        .unwrap()
        .unwrap();
    let intent = auth_sessions::enqueue_intent_tx(
        &mut tx,
        session.id,
        auth_sessions::AuthIntentPayload::RedisInvalidate {
            session_handle: session.session_handle.clone(),
        },
        1,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    sqlx::query(
        "UPDATE auth_session_intents
         SET state = 'running', attempts = 1, worker_id = 'crashed-intent-worker',
             lease_until = now() - interval '1 minute'
         WHERE id = $1",
    )
    .bind(intent.id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(auth_sessions::claim_intent_for_session(
        &pool,
        "replacement-intent-worker",
        300,
        session.id,
    )
    .await
    .unwrap()
    .is_none());
    let persisted: (
        String,
        i32,
        Option<String>,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT state, attempts, worker_id, last_error_code, lease_until
             FROM auth_session_intents WHERE id = $1",
    )
    .bind(intent.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, "failed");
    assert_eq!(persisted.1, 1);
    assert!(persisted.2.is_none());
    assert_eq!(
        persisted.3.as_deref(),
        Some("auth_intent_attempts_exhausted")
    );
    assert!(persisted.4.is_none());

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

/// job payload 同样必须拒绝 raw token 字段，避免异步链路泄密。
#[tokio::test]
async fn audit_jobs_job_payload_rejects_raw_tokens() {
    let pool = pool().await;
    for payload in [
        serde_json::json!({"token": "raw"}),
        serde_json::json!({"refreshToken": "raw"}),
        serde_json::json!({"access_token": "raw"}),
        serde_json::json!({"apiKey": "raw"}),
        serde_json::json!({"password": "raw"}),
        serde_json::json!({"secret": "raw"}),
        serde_json::json!({"verifier": "raw"}),
        serde_json::json!({"code": "raw"}),
    ] {
        sqlx::query("INSERT INTO jobs (kind, payload) VALUES ('unsafe-test', $1)")
            .bind(payload)
            .execute(&pool)
            .await
            .expect_err("job payload 不得被命名风格绕过 secret-key 检查");
    }
}

/// repository 创建与完成边界只接受 kind-specific payload/result 类型。
#[tokio::test]
async fn audit_jobs_repository_uses_typed_payload_and_result() {
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "typed-job").await;
    let mut tx = pool.begin().await.unwrap();
    let created = jobs::create_tx(
        &mut tx,
        jobs::NewJob {
            kind: jobs::JobKind::UploadCleanup,
            project_id: Some(project_id),
            stage: "queued".to_string(),
            progress_total: None,
            max_attempts: 1,
            run_after: chrono::Utc::now(),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let claimed = jobs::claim_next_for_ids_and_kinds(
        &pool,
        "typed-job-worker",
        300,
        &[],
        &["upload_cleanup".to_string()],
        &[created.id],
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(claimed.id, created.id);
    let completed = jobs::complete(
        &pool,
        created.id,
        "typed-job-worker",
        jobs::JobResult::Completed,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(completed.result, Some(serde_json::json!({})));

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(created.id)
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

#[tokio::test]
async fn users_api_keys_settings_oauth_roundtrip() {
    let pool = pool().await;

    // 清理上次残留
    sqlx::query("DELETE FROM users WHERE username IN ('itest_user', 'itest_oauth')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM settings WHERE key = 'test.flag'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 账号密码用户 ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let user = users::create_password_user(
        &pool,
        "itest_user",
        Some("itest@example.com"),
        &hash,
        "active",
    )
    .await
    .unwrap();
    assert_eq!(user.username, "itest_user");
    assert!(user.password_hash.is_some());
    assert!(users::username_exists(&pool, "itest_user").await.unwrap());

    let found = users::find_by_username(&pool, "itest_user")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.id, user.id);

    let updated = users::update_profile(
        &pool,
        user.id,
        "hi",
        Some("http://x/a.png"),
        &["en".to_string(), "zh-Hans".to_string()],
    )
    .await
    .unwrap();
    assert_eq!(updated.description, "hi");
    assert_eq!(
        updated.translation_langs,
        vec!["en".to_string(), "zh-Hans".to_string()]
    );

    users::set_platform_role(&pool, user.id, Some("admin"))
        .await
        .unwrap();
    let admin = users::find_by_id(&pool, user.id).await.unwrap().unwrap();
    assert_eq!(admin.platform_role.as_deref(), Some("admin"));

    // —— API Key ——
    let key = prts_auth::token::generate_api_key();
    let rec = api_keys::create(&pool, user.id, "k", &key.hash, &key.display_prefix)
        .await
        .unwrap();
    let by_hash = api_keys::find_user_by_key_hash(&pool, &key.hash)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_hash.id, user.id);
    assert!(api_keys::revoke(&pool, user.id, rec.id).await.unwrap());
    assert!(!api_keys::revoke(&pool, user.id, rec.id).await.unwrap()); // 二次删除无效

    // —— 设置 ——
    settings::set(&pool, "test.flag", &serde_json::json!(true), Some(user.id))
        .await
        .unwrap();
    assert_eq!(
        settings::get(&pool, "test.flag").await.unwrap().unwrap(),
        serde_json::json!(true)
    );

    // —— OAuth 用户 + 关联账号 ——
    let extra = serde_json::json!({ "work_scope": "英翻" });
    let ouser = users::create_oauth_user(
        &pool,
        "itest_oauth",
        Some("http://x/b.png"),
        "zoot",
        "ext-123",
        &extra,
    )
    .await
    .unwrap();
    assert!(ouser.password_hash.is_none());
    let by_ext = users::find_by_external(&pool, "zoot", "ext-123")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_ext.id, ouser.id);
    let accounts = users::list_external_accounts(&pool, ouser.id)
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].provider, "zoot");
    assert_eq!(accounts[0].raw.get("work_scope").unwrap(), "英翻");

    // 清理
    sqlx::query("DELETE FROM users WHERE username IN ('itest_user', 'itest_oauth')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM settings WHERE key = 'test.flag'")
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn projects_files_entries_roundtrip() {
    let pool = pool().await;

    sqlx::query("DELETE FROM projects WHERE slug = 'itest-proj'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_owner'")
        .execute(&pool)
        .await
        .unwrap();

    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_owner", None, &hash, "active")
        .await
        .unwrap();

    // 项目 + 成员
    let proj = projects::create(
        &pool,
        "itest-proj",
        "ITest",
        "",
        "public",
        &["en".to_string(), "ja".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert(&pool, proj.id, owner.id, "owner")
        .await
        .unwrap();
    assert_eq!(
        memberships::find_role(&pool, proj.id, owner.id)
            .await
            .unwrap()
            .as_deref(),
        Some("owner")
    );
    assert!(projects::slug_exists(&pool, "itest-proj").await.unwrap());

    // 按路径上传（自动建文件夹/文件）
    let file = files::ensure_file_at_path(&pool, proj.id, "dialog/ch1.json")
        .await
        .unwrap();
    assert_eq!(file.path, "dialog/ch1.json");

    let initial = vec![
        entries::UploadEntry {
            key: "k1".to_string(),
            original: serde_json::json!({ "en": "Hello", "ja": "こんにちは" }),
            context: Some("greeting".to_string()),
            translation: None,
            state: None,
        },
        entries::UploadEntry {
            key: "k2".to_string(),
            original: serde_json::json!({ "en": "Bye" }),
            context: None,
            translation: None,
            state: None,
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &initial, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2);
    files::refresh_entry_count(&pool, file.id).await.unwrap();

    let listed = entries::list(&pool, proj.id, &entries::EntryFilter::default(), None, 100)
        .await
        .unwrap();
    assert_eq!(listed.len(), 2);
    let k1 = listed.iter().find(|e| e.key == "k1").unwrap().clone();
    assert_eq!(k1.state, "untranslated");

    // 乐观锁更新
    let updated = entries::update_translation(
        &pool,
        k1.id,
        k1.version,
        "你好",
        "translated",
        "translate",
        Some(owner.id),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(updated.translation, "你好");
    assert_eq!(updated.state, "translated");
    // 用过期版本号 → 冲突
    let conflict = entries::update_translation(
        &pool,
        k1.id,
        k1.version,
        "x",
        "translated",
        "edit",
        Some(owner.id),
    )
    .await
    .unwrap();
    assert!(conflict.is_none());

    // 重传：同 key 源文变化 → 保留译文、置未翻译、记历史
    let reupload = vec![entries::UploadEntry {
        key: "k1".to_string(),
        original: serde_json::json!({ "en": "Hello!", "ja": "こんにちは" }),
        context: None,
        translation: None,
        state: None,
    }];
    let stats2 = entries::bulk_upsert(&pool, file.id, proj.id, &reupload, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats2.updated, 1);
    let k1b = entries::get(&pool, proj.id, k1.id).await.unwrap().unwrap();
    assert_eq!(k1b.state, "untranslated"); // 已重置
    assert_eq!(k1b.translation, "你好"); // 译文保留
    let history = entries::list_versions(&pool, k1.id, 50).await.unwrap();
    assert!(history.iter().any(|v| v.kind == "source_update"));

    // 锁定标志
    let locked = entries::set_flags(&pool, proj.id, k1.id, Some(true), None)
        .await
        .unwrap()
        .unwrap();
    assert!(locked.locked);

    // 导出列表
    let export = entries::list_for_export(&pool, proj.id).await.unwrap();
    assert_eq!(export.len(), 2);

    // 级联清理
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证迁移 0004 的触发器：
/// - 插入中文源词条后，`source_text` 由触发器从 `original` JSON 取出；
/// - `source_tsv` 经 zhparser 分词，长度大于 0（zhparser 在 CI Postgres 镜像中可用）。
///
/// 仅在 CI 环境（带 zhparser 的 Postgres 镜像）运行；本地无 DB 时只编译不执行。
#[tokio::test]
async fn migration_0004_trigger_populates_source_text_and_zhparser_tsv() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-zh-search'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_zh_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner（owner_id NOT NULL REFERENCES users） ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_zh_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目：source_langs 首位为 'zh-Hans'，触发器用 source_langs[1] 提取 ——
    let proj = projects::create(
        &pool,
        "itest-zh-search",
        "ITest ZH Search",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "search/test.json")
        .await
        .unwrap();

    // —— 插入中文原文词条，state='translated' ——
    // original 的键必须与 source_langs[1]（即 'zh-Hans'）一致，触发器才能提取源文本。
    let batch = vec![entries::UploadEntry {
        key: "zh_test_key".to_string(),
        original: serde_json::json!({ "zh-Hans": "今天天气很好" }),
        context: None,
        translation: Some("nice weather".to_string()),
        state: Some("translated".to_string()),
    }];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 1, "应创建 1 条词条");

    // —— 验证触发器效果 ——
    // source_text：触发器从 original->>'zh-Hans' 填充
    // source_tsv：触发器对中文源文用 zhparser 分词（prts_zh 配置），CI 必有 zhparser
    let (source_text, tsv_text): (String, String) = sqlx::query_as(
        "SELECT source_text, source_tsv::text FROM entries WHERE project_id = $1 AND key = 'zh_test_key'",
    )
    .bind(proj.id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        source_text, "今天天气很好",
        "触发器应将 original->>'zh-Hans' 写入 source_text"
    );
    assert!(
        !tsv_text.is_empty(),
        "zhparser 应将中文分词写入 source_tsv（实际得到空串，请确认 zhparser 已安装）"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_zh_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 prts-db::search 的 trgm_search 和 fts_search：
/// - trgm_search 按三元组相似度召回匹配 "weather" 的词条；
/// - fts_search 按 plainto_tsquery 匹配英文译文中的 "weather"。
#[tokio::test]
async fn search_trgm_and_fts_recall() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-search-recall'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_search_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_search_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目：source_langs 首位 'zh-Hans'，目标语言 'en' ——
    let proj = projects::create(
        &pool,
        "itest-search-recall",
        "ITest Search Recall",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "search/recall.json")
        .await
        .unwrap();

    // —— 插入两条词条 ——
    let batch = vec![
        entries::UploadEntry {
            key: "w1".to_string(),
            original: serde_json::json!({ "zh-Hans": "今天天气不错" }),
            context: None,
            translation: Some("nice weather today".to_string()),
            state: Some("translated".to_string()),
        },
        entries::UploadEntry {
            key: "w2".to_string(),
            original: serde_json::json!({ "zh-Hans": "完全无关的内容" }),
            context: None,
            translation: Some("completely unrelated".to_string()),
            state: Some("translated".to_string()),
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2, "应创建 2 条词条");

    // —— 查询 w1/w2 的真实 id ——
    let w1_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'w1'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let w2_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'w2'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— trgm 召回：w1 的 translation 含 "weather"，应出现在结果中 ——
    let trgm_ids = prts_db::search::trgm_search(&pool, proj.id, "weather", &[], &[], false, 10)
        .await
        .unwrap();
    assert!(
        trgm_ids.contains(&w1_id),
        "trgm_search 应召回 w1（translation 含 'weather'），实际结果：{trgm_ids:?}"
    );
    assert!(
        !trgm_ids.contains(&w2_id),
        "无关词条不应出现在 'weather' 的 trgm 结果中，实际结果：{trgm_ids:?}"
    );

    // —— FTS 召回：英文 plainto_tsquery('english', 'weather') 应匹配 w1 的 translation_tsv ——
    let fts_ids = prts_db::search::fts_search(
        &pool,
        proj.id,
        "weather",
        "zh-Hans",
        "en",
        &[],
        &[],
        false,
        10,
    )
    .await
    .unwrap();
    assert!(
        fts_ids.contains(&w1_id),
        "fts_search 应召回 w1（translation_tsv 匹配 'weather'），实际结果：{fts_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_search_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 prts-search::orchestrator::run 端到端：
/// - 给定一个含已翻译词条的项目，orchestrator 应为匹配查询词的词条返回命中；
/// - 对无关词条不应出现在结果中；
/// - 结果中应包含 (Entry, relevance) 元组，relevance > 0。
#[tokio::test]
async fn search_orchestrator_returns_ranked_hits() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-orch-search'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_orch_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_orch_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目：源语言 'zh-Hans'，目标 'en' ——
    let proj = projects::create(
        &pool,
        "itest-orch-search",
        "ITest Orchestrator Search",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件并插入词条 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "orch/test.json")
        .await
        .unwrap();

    let batch = vec![
        entries::UploadEntry {
            key: "orch1".to_string(),
            original: serde_json::json!({ "zh-Hans": "明日之后" }),
            context: None,
            translation: Some("state of survival".to_string()),
            state: Some("translated".to_string()),
        },
        entries::UploadEntry {
            key: "orch2".to_string(),
            original: serde_json::json!({ "zh-Hans": "完全不相关词条" }),
            context: None,
            translation: Some("completely irrelevant entry".to_string()),
            state: Some("translated".to_string()),
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2, "应创建 2 条词条");

    let orch1_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'orch1'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let orch2_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'orch2'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— 调用 orchestrator.run ——
    let results = prts_search::orchestrator::run(
        &pool,
        prts_search::orchestrator::OrchestratorInput {
            project_id: proj.id,
            q: "survival",
            src_lang: "zh-Hans",
            tgt_lang: "en",
            file_ids: &[],
            states: &[],
            include_hidden: false,
            per_path: 100,
            top_k: 200,
            sort: prts_search::SortBy::Relevance,
            vector_ids: None,
        },
    )
    .await
    .unwrap();

    // 结果列表非空
    assert!(!results.is_empty(), "orchestrator 应返回至少一条命中");

    let hit_ids: Vec<i64> = results.iter().map(|(e, _)| e.id).collect();

    // "survival" 应命中 orch1（translation = "state of survival"）
    assert!(
        hit_ids.contains(&orch1_id),
        "orchestrator 应返回 orch1（translation 含 'survival'），实际结果 ids: {hit_ids:?}"
    );

    // 无关词条 orch2 不应出现在结果中
    assert!(
        !hit_ids.contains(&orch2_id),
        "无关词条 orch2 不应出现在 'survival' 的搜索结果中，实际结果 ids: {hit_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_orch_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 vector_search 的 cosine 距离排序：
/// - 播种两条词条，各手动 UPDATE embedding 为 1024 维已知向量；
/// - 查询向量贴近词条 A，断言 A 的 id 排在 B 之前。
///
/// 注：直接 UPDATE embedding 不改 original/source_text，故不触发清零触发器。
#[tokio::test]
async fn vector_search_returns_nearest_first() {
    use pgvector::Vector;

    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-vec-search'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_vec_owner'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建 owner ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let owner = users::create_password_user(&pool, "itest_vec_owner", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目 ——
    let proj = projects::create(
        &pool,
        "itest-vec-search",
        "ITest Vec Search",
        "",
        "public",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();

    // —— 建文件 + 插入两条词条 ——
    let file = files::ensure_file_at_path(&pool, proj.id, "vec/test.json")
        .await
        .unwrap();

    let batch = vec![
        entries::UploadEntry {
            key: "va".to_string(),
            original: serde_json::json!({ "en": "vector entry A" }),
            context: None,
            translation: None,
            state: None,
        },
        entries::UploadEntry {
            key: "vb".to_string(),
            original: serde_json::json!({ "en": "vector entry B" }),
            context: None,
            translation: None,
            state: None,
        },
    ];
    let stats = entries::bulk_upsert(&pool, file.id, proj.id, &batch, Some(owner.id))
        .await
        .unwrap();
    assert_eq!(stats.created, 2, "应创建 2 条词条");

    // —— 取真实 id ——
    let id_a: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'va'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let id_b: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'vb'")
            .bind(proj.id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— 构造 1024 维向量 ——
    // A 贴近 [1, 0, 0, ...0]，B 贴近 [0, 1, 0, ...0]
    let mut vec_a = vec![0.0_f32; 1024];
    vec_a[0] = 1.0;
    let mut vec_b = vec![0.0_f32; 1024];
    vec_b[1] = 1.0;

    // —— 直接 UPDATE embedding（不改 original，不触发清零触发器）——
    sqlx::query("UPDATE entries SET embedding = $1 WHERE id = $2")
        .bind(Vector::from(vec_a.clone()))
        .bind(id_a)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE entries SET embedding = $1 WHERE id = $2")
        .bind(Vector::from(vec_b))
        .bind(id_b)
        .execute(&pool)
        .await
        .unwrap();

    // —— 查询向量贴近 A，断言 A 排在 B 之前 ——
    let result_ids = prts_db::search::vector_search(&pool, proj.id, &vec_a, &[], &[], false, 10)
        .await
        .unwrap();

    assert!(!result_ids.is_empty(), "vector_search 应返回至少一条结果");
    let pos_a = result_ids.iter().position(|&x| x == id_a);
    let pos_b = result_ids.iter().position(|&x| x == id_b);
    assert!(
        pos_a.is_some() && pos_b.is_some(),
        "两条词条均应出现在结果中，实际：{result_ids:?}"
    );
    assert!(
        pos_a.unwrap() < pos_b.unwrap(),
        "A 应排在 B 之前（与查询向量 cosine 更近），实际顺序：{result_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_vec_owner'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 search_settings::set 规范化 + get 持久化圆环：
/// - 写入超出安全区间的 embedding_batch = 99、tm_top_n = 9；
/// - 规范化后持久化，再 get 取回，应得 clamped 值（10 和 3）。
#[tokio::test]
async fn search_settings_set_and_get_normalizes_values() {
    use prts_db::search_settings::{self, SearchConfig};

    let pool = pool().await;

    // 清理上次残留（key 固定为 "search.config"）
    sqlx::query("DELETE FROM settings WHERE key = 'search.config'")
        .execute(&pool)
        .await
        .unwrap();

    // 写入超范围值
    let cfg = SearchConfig {
        embedding_batch: 99,
        tm_top_n: 9,
        ..SearchConfig::default()
    };
    search_settings::set(&pool, cfg, None).await.unwrap();

    // 读取并断言规范化结果
    let got = search_settings::get(&pool).await.unwrap();
    assert_eq!(
        got.embedding_batch, 10,
        "embedding_batch 应被 clamp 到上限 10"
    );
    assert_eq!(got.tm_top_n, 3, "tm_top_n 应被 clamp 到上限 3");

    // 清理
    sqlx::query("DELETE FROM settings WHERE key = 'search.config'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 prts-db::notifications 仓储圆环（迁移 0005）：
/// - 为收件人 `create` 两条通知 → `unread_count` == 2；
/// - `mark_read` 其中一条 → `unread_count` == 1；
/// - `list` 按 id 降序返回（较新的在前）。
#[tokio::test]
async fn notifications_repository_roundtrip() {
    let pool = pool().await;

    // —— 清理上次残留（级联删除该用户的通知） ——
    sqlx::query("DELETE FROM users WHERE username = 'itest_notif_user'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建收件人 ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let user = users::create_password_user(&pool, "itest_notif_user", None, &hash, "active")
        .await
        .unwrap();

    // —— 创建两条通知 ——
    let n1 = notifications::create(
        &pool,
        user.id,
        "poke",
        &serde_json::json!({ "text": "first" }),
    )
    .await
    .unwrap();
    let n2 = notifications::create(
        &pool,
        user.id,
        "poke",
        &serde_json::json!({ "text": "second" }),
    )
    .await
    .unwrap();
    assert_eq!(n1.kind, "poke");
    assert!(n1.read_at.is_none());
    assert!(n2.id > n1.id, "IDENTITY 应递增，n2.id 应大于 n1.id");

    // —— 两条均未读 ——
    assert_eq!(
        notifications::unread_count(&pool, user.id).await.unwrap(),
        2,
        "创建后应有 2 条未读"
    );

    // —— 标记 n1 已读 → 剩 1 条未读 ——
    notifications::mark_read(&pool, user.id, &[n1.id])
        .await
        .unwrap();
    assert_eq!(
        notifications::unread_count(&pool, user.id).await.unwrap(),
        1,
        "标记一条已读后应剩 1 条未读"
    );

    // —— list 按 id 降序（较新的 n2 在前） ——
    let listed = notifications::list(&pool, user.id, None, 100)
        .await
        .unwrap();
    assert_eq!(listed.len(), 2, "应列出全部 2 条");
    assert_eq!(listed[0].id, n2.id, "list 应按 id 降序：较新的 n2 在最前");
    assert_eq!(listed[1].id, n1.id);
    // n1 已读、n2 仍未读
    let n1_listed = listed.iter().find(|n| n.id == n1.id).unwrap();
    let n2_listed = listed.iter().find(|n| n.id == n2.id).unwrap();
    assert!(n1_listed.read_at.is_some(), "n1 应已读");
    assert!(n2_listed.read_at.is_none(), "n2 应仍未读");

    // —— 键集游标：before_id = n2.id 只返回更旧的 n1 ——
    let older = notifications::list(&pool, user.id, Some(n2.id), 100)
        .await
        .unwrap();
    assert_eq!(older.len(), 1, "游标 before=n2 应只返回更旧的 n1");
    assert_eq!(older[0].id, n1.id);

    // —— 级联清理 ——
    sqlx::query("DELETE FROM users WHERE username = 'itest_notif_user'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 suggestions_trgm 的成员范围过滤：
/// - 用户 U 是项目 A 和 B 的成员，不是项目 C 的成员；
/// - 查询 A 中某词条的 trgm 建议：应包含 B 的跨项目词条，排除 C 及 A 自身被查词条。
#[tokio::test]
async fn suggestions_trgm_membership_scoped() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    for slug in ["itest-tm-a", "itest-tm-b", "itest-tm-c"] {
        sqlx::query("DELETE FROM projects WHERE slug = $1")
            .bind(slug)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM users WHERE username = 'itest_tm_user'")
        .execute(&pool)
        .await
        .unwrap();

    // —— 建用户 U ——
    let hash = prts_auth::password::hash_password("password123").unwrap();
    let u = users::create_password_user(&pool, "itest_tm_user", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目 A（U 是 owner，自动加入成员） ——
    let proj_a = projects::create(
        &pool,
        "itest-tm-a",
        "TM Test A",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        u.id,
    )
    .await
    .unwrap();
    // projects::create 已插入 owner 行，此处确保角色正确
    memberships::upsert(&pool, proj_a.id, u.id, "owner")
        .await
        .unwrap();

    // —— 建项目 B（U 作为 translator 加入） ——
    let proj_b = projects::create(
        &pool,
        "itest-tm-b",
        "TM Test B",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        u.id, // owner 也是 U，所以 U 在 B 中
    )
    .await
    .unwrap();
    memberships::upsert(&pool, proj_b.id, u.id, "translator")
        .await
        .unwrap();

    // —— 建项目 C（U 不是成员；由另一个 owner 创建，使用直接 INSERT） ——
    // 为了避免引入额外用户，直接插入项目并让 U 不加入成员表
    let proj_c: (i64,) = sqlx::query_as(
        "INSERT INTO projects (slug, name, description, visibility, source_langs, target_lang, owner_id)
         VALUES ('itest-tm-c', 'TM Test C', '', 'public', ARRAY['zh-Hans'], 'en', $1)
         RETURNING id",
    )
    .bind(u.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let proj_c_id = proj_c.0;
    // 明确不为 U 在 proj_c 插入 membership

    // —— 建文件 ——
    let file_a = files::ensure_file_at_path(&pool, proj_a.id, "tm/a.json")
        .await
        .unwrap();
    let file_b = files::ensure_file_at_path(&pool, proj_b.id, "tm/b.json")
        .await
        .unwrap();
    let file_c = files::ensure_file_at_path(&pool, proj_c_id, "tm/c.json")
        .await
        .unwrap();

    // 相同/相似源文：三个项目都用同一个英文原文，触发器提取 source_text
    let make_batch = |key: &str| {
        vec![entries::UploadEntry {
            key: key.to_string(),
            original: serde_json::json!({ "zh-Hans": "今天天气晴朗" }),
            context: None,
            translation: Some("the weather is sunny today".to_string()),
            state: Some("translated".to_string()),
        }]
    };

    let stats_a = entries::bulk_upsert(
        &pool,
        file_a.id,
        proj_a.id,
        &make_batch("tm_a1"),
        Some(u.id),
    )
    .await
    .unwrap();
    assert_eq!(stats_a.created, 1);

    let stats_b = entries::bulk_upsert(
        &pool,
        file_b.id,
        proj_b.id,
        &make_batch("tm_b1"),
        Some(u.id),
    )
    .await
    .unwrap();
    assert_eq!(stats_b.created, 1);

    let stats_c = entries::bulk_upsert(
        &pool,
        file_c.id,
        proj_c_id,
        &make_batch("tm_c1"),
        Some(u.id),
    )
    .await
    .unwrap();
    assert_eq!(stats_c.created, 1);

    // —— 取各词条真实 id ——
    let entry_a_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'tm_a1'")
            .bind(proj_a.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let entry_b_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'tm_b1'")
            .bind(proj_b.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let entry_c_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE project_id = $1 AND key = 'tm_c1'")
            .bind(proj_c_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // —— 调用 suggestions_trgm，以 A 的词条为查询基点 ——
    let results =
        prts_db::search::suggestions_trgm(&pool, u.id, "en", "今天天气晴朗", entry_a_id, 0.0, 10)
            .await
            .unwrap();

    let result_ids: Vec<i64> = results.iter().map(|r| r.entry_id).collect();

    // B 的词条应出现（跨项目，U 是成员）
    assert!(
        result_ids.contains(&entry_b_id),
        "B 项目词条应出现在建议中（U 是成员），实际：{result_ids:?}"
    );

    // C 的词条不应出现（U 不是 C 的成员）
    assert!(
        !result_ids.contains(&entry_c_id),
        "C 项目词条不应出现在建议中（U 不是成员），实际：{result_ids:?}"
    );

    // A 的被查词条本身不应出现（cur_entry_id 排除）
    assert!(
        !result_ids.contains(&entry_a_id),
        "被查词条自身不应出现在建议结果中，实际：{result_ids:?}"
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj_a.id).await.unwrap();
    projects::delete(&pool, proj_b.id).await.unwrap();
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(proj_c_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE username = 'itest_tm_user'")
        .execute(&pool)
        .await
        .unwrap();
}

/// 验证 poke 端点的成员门限逻辑（仓储层）：
/// - 建项目 + 两个成员（A=owner、B=translator）；
/// - A 向 B 创建 `poke` 通知 → B 有 1 条 `poke` 通知；
/// - 非成员 C 的成员查询返回 `None`（端点层据此拒绝，此处验证仓储层门限调用结果）。
#[tokio::test]
async fn poke_membership_gate_and_notification_created() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    sqlx::query("DELETE FROM projects WHERE slug = 'itest-poke-proj'")
        .execute(&pool)
        .await
        .unwrap();
    for uname in ["itest_poke_a", "itest_poke_b", "itest_poke_c"] {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(uname)
            .execute(&pool)
            .await
            .unwrap();
    }

    let hash = prts_auth::password::hash_password("password123").unwrap();

    // —— 建两个成员 A、B 和非成员 C ——
    let user_a = users::create_password_user(&pool, "itest_poke_a", None, &hash, "active")
        .await
        .unwrap();
    let user_b = users::create_password_user(&pool, "itest_poke_b", None, &hash, "active")
        .await
        .unwrap();
    let user_c = users::create_password_user(&pool, "itest_poke_c", None, &hash, "active")
        .await
        .unwrap();

    // —— 建项目，A 为 owner，B 为 translator ——
    let proj = projects::create(
        &pool,
        "itest-poke-proj",
        "ITest Poke Project",
        "",
        "public",
        &["en".to_string()],
        "zh-Hans",
        user_a.id,
    )
    .await
    .unwrap();
    memberships::upsert(&pool, proj.id, user_a.id, "owner")
        .await
        .unwrap();
    memberships::upsert(&pool, proj.id, user_b.id, "translator")
        .await
        .unwrap();

    // —— 验证 A 是成员，B 是成员，C 不是成员 ——
    let role_a = memberships::find_role(&pool, proj.id, user_a.id)
        .await
        .unwrap();
    assert!(role_a.is_some(), "A 应是项目成员");

    let role_b = memberships::find_role(&pool, proj.id, user_b.id)
        .await
        .unwrap();
    assert!(role_b.is_some(), "B 应是项目成员");

    let role_c = memberships::find_role(&pool, proj.id, user_c.id)
        .await
        .unwrap();
    assert!(role_c.is_none(), "C 不是项目成员，端点层应据此返回 400");

    // —— A poke B：创建通知 ——
    let payload = serde_json::json!({
        "from_user_id": user_a.id,
        "from_username": "itest_poke_a",
        "project_id": proj.id,
        "text": "嘿，看一下第 3 行",
    });
    let n = notifications::create(&pool, user_b.id, "poke", &payload)
        .await
        .unwrap();
    assert_eq!(n.kind, "poke", "通知类型应为 poke");
    assert_eq!(n.user_id, user_b.id, "收件人应为 B");
    assert!(n.read_at.is_none(), "新通知应未读");

    // —— B 有 1 条 poke 通知 ——
    let count = notifications::unread_count(&pool, user_b.id).await.unwrap();
    assert_eq!(count, 1, "B 应有 1 条未读通知");

    let listed = notifications::list(&pool, user_b.id, None, 10)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1, "B 的通知列表应有 1 条");
    assert_eq!(listed[0].kind, "poke");
    assert_eq!(
        listed[0].payload.get("text").and_then(|t| t.as_str()),
        Some("嘿，看一下第 3 行")
    );

    // —— 级联清理 ——
    projects::delete(&pool, proj.id).await.unwrap();
    for uname in ["itest_poke_a", "itest_poke_b", "itest_poke_c"] {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(uname)
            .execute(&pool)
            .await
            .unwrap();
    }
}

/// 验证 prts-db::messages 仓储圆环（迁移 0006）：
/// - 建 A、B 两个用户；A→B 两条、B→A 一条；
/// - `list_conversation(A, B)` 返回 3 条并按 id 降序（键集游标 before 生效）；
/// - `unread_count(B)` == 2（A→B 两条对 B 未读）；`mark_read(B, A)` 后 == 0；
/// - `list_threads(A)` 含与 B 的会话：username / 最后一条 / A 侧未读 == 1（B→A 那条）。
#[tokio::test]
async fn messages_repository_roundtrip() {
    let pool = pool().await;

    // —— 清理上次残留（级联删除其消息） ——
    for uname in ["itest_msg_a", "itest_msg_b"] {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(uname)
            .execute(&pool)
            .await
            .unwrap();
    }

    let hash = prts_auth::password::hash_password("password123").unwrap();
    let a = users::create_password_user(&pool, "itest_msg_a", None, &hash, "active")
        .await
        .unwrap();
    let b = users::create_password_user(&pool, "itest_msg_b", None, &hash, "active")
        .await
        .unwrap();

    // —— A→B 两条、B→A 一条 ——
    let m1 = messages::create(&pool, a.id, b.id, "hi B (1)")
        .await
        .unwrap();
    let m2 = messages::create(&pool, a.id, b.id, "hi B (2)")
        .await
        .unwrap();
    let m3 = messages::create(&pool, b.id, a.id, "hi A").await.unwrap();
    assert!(m2.id > m1.id && m3.id > m2.id, "IDENTITY 应递增");
    assert!(m1.read_at.is_none(), "新消息应未读");

    // —— 会话（A↔B）返回 3 条，按 id 降序（最新在前） ——
    let convo = messages::list_conversation(&pool, a.id, b.id, None, 100)
        .await
        .unwrap();
    assert_eq!(convo.len(), 3, "A↔B 会话应有 3 条消息");
    assert_eq!(
        convo[0].id, m3.id,
        "list_conversation 应按 id 降序：最新在前"
    );
    assert_eq!(convo[2].id, m1.id);

    // —— 键集游标：before = m3 只返回更旧的 m2、m1 ——
    let older = messages::list_conversation(&pool, a.id, b.id, Some(m3.id), 100)
        .await
        .unwrap();
    assert_eq!(older.len(), 2, "游标 before=m3 应只返回更旧的两条");
    assert_eq!(older[0].id, m2.id);

    // —— B 未读 2（A→B 两条） ——
    assert_eq!(
        messages::unread_count(&pool, b.id).await.unwrap(),
        2,
        "B 应有 2 条未读（A→B 两条）"
    );

    // —— B 标记与 A 的会话已读 → 0 ——
    messages::mark_read(&pool, b.id, a.id).await.unwrap();
    assert_eq!(
        messages::unread_count(&pool, b.id).await.unwrap(),
        0,
        "B 读完与 A 的会话后未读应为 0"
    );

    // —— A 的会话列表：含与 B 的会话，A 侧未读 == 1（B→A 那条 A 尚未读） ——
    let threads = messages::list_threads(&pool, a.id).await.unwrap();
    let with_b = threads
        .iter()
        .find(|t| t.other_user_id == b.id)
        .expect("A 的会话列表应含与 B 的会话");
    assert_eq!(with_b.username, "itest_msg_b");
    assert_eq!(with_b.last_content, "hi A", "最后一条应为 B→A 的 'hi A'");
    assert_eq!(with_b.last_sender_id, b.id);
    assert_eq!(with_b.unread, 1, "A 侧未读应为 1（B→A 未读那条）");

    // —— 级联清理 ——
    for uname in ["itest_msg_a", "itest_msg_b"] {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(uname)
            .execute(&pool)
            .await
            .unwrap();
    }
}

/// 验证私信「共享项目」门限（端点层用同一 EXISTS 交集查询）+ 会话取回（迁移 0006）：
/// - A、B 同属项目 P1（共享）；C 属另一项目 P2、与 A 无共享项目；
/// - 交集查询 EXISTS(A,B) == true、EXISTS(A,C) == false（端点据此放行 / 返回 403）；
/// - A→B 发送成功，`list_conversation(A,B)` 能取回该消息。
#[tokio::test]
async fn messages_share_project_gate() {
    let pool = pool().await;

    // —— 清理上次残留 ——
    for slug in ["itest-msg-p1", "itest-msg-p2"] {
        sqlx::query("DELETE FROM projects WHERE slug = $1")
            .bind(slug)
            .execute(&pool)
            .await
            .unwrap();
    }
    for uname in ["itest_msg_ga", "itest_msg_gb", "itest_msg_gc"] {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(uname)
            .execute(&pool)
            .await
            .unwrap();
    }

    let hash = prts_auth::password::hash_password("password123").unwrap();
    let a = users::create_password_user(&pool, "itest_msg_ga", None, &hash, "active")
        .await
        .unwrap();
    let b = users::create_password_user(&pool, "itest_msg_gb", None, &hash, "active")
        .await
        .unwrap();
    let c = users::create_password_user(&pool, "itest_msg_gc", None, &hash, "active")
        .await
        .unwrap();

    // —— P1：A、B 同项目 ——
    let p1 = projects::create(
        &pool,
        "itest-msg-p1",
        "MsgP1",
        "",
        "public",
        &["en".to_string()],
        "zh-Hans",
        a.id,
    )
    .await
    .unwrap();
    memberships::upsert(&pool, p1.id, a.id, "owner")
        .await
        .unwrap();
    memberships::upsert(&pool, p1.id, b.id, "translator")
        .await
        .unwrap();

    // —— P2：仅 C（A 不在其中） ——
    let p2 = projects::create(
        &pool,
        "itest-msg-p2",
        "MsgP2",
        "",
        "public",
        &["en".to_string()],
        "zh-Hans",
        c.id,
    )
    .await
    .unwrap();
    memberships::upsert(&pool, p2.id, c.id, "owner")
        .await
        .unwrap();

    // —— 共享项目交集查询（与端点 share_project 完全相同的 SQL） ——
    let share_ab: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships m1 JOIN memberships m2 ON m1.project_id = m2.project_id WHERE m1.user_id = $1 AND m2.user_id = $2)",
    )
    .bind(a.id)
    .bind(b.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(share_ab, "A、B 同属 P1，应共享项目（端点放行）");

    let share_ac: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships m1 JOIN memberships m2 ON m1.project_id = m2.project_id WHERE m1.user_id = $1 AND m2.user_id = $2)",
    )
    .bind(a.id)
    .bind(c.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!share_ac, "A、C 无共享项目，端点应返回 403");

    // —— A→B 发送成功并可取回 ——
    let m = messages::create(&pool, a.id, b.id, "hello from A")
        .await
        .unwrap();
    let convo = messages::list_conversation(&pool, a.id, b.id, None, 50)
        .await
        .unwrap();
    assert_eq!(convo.len(), 1, "A↔B 应有 1 条消息");
    assert_eq!(convo[0].id, m.id);
    assert_eq!(convo[0].content, "hello from A");

    // —— 级联清理 ——
    projects::delete(&pool, p1.id).await.unwrap();
    projects::delete(&pool, p2.id).await.unwrap();
    for uname in ["itest_msg_ga", "itest_msg_gb", "itest_msg_gc"] {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(uname)
            .execute(&pool)
            .await
            .unwrap();
    }
}
