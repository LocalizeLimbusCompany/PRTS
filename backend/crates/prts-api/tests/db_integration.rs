//! DB 集成测试：对真实 PostgreSQL 跑通仓储层 SQL 与迁移。
//!
//! 仅在 `db-tests` 特性下编译，并需要 runtime `DATABASE_URL` 与 owner
//! `MIGRATION_DATABASE_URL`。迁移和所有合同 DML 使用不同数据库角色。
//! 本地无 DB 时默认不编译；CI 会起 Postgres 服务后执行（见 .github/workflows/ci.yml）。
#![cfg(feature = "db-tests")]
// binary-only production modules are imported selectively below; unused endpoints remain intentional.
#![allow(dead_code)]

// 直接复用二进制 crate 的真实 handler/composition 模块，使集成测试能够在不复制
// route/repository 实现的前提下验证 HTTP 边界。PRTS 当前只有 binary target；这些
// `path` 模块在 Task 1.2 完成前提供测试入口，后续若抽出 library target 可直接删去。
#[path = "../src/appsettings.rs"]
mod appsettings;
#[path = "../src/auth/mod.rs"]
mod auth;
#[path = "../src/dto.rs"]
mod dto;
#[path = "../src/error.rs"]
mod error;
#[path = "../src/job_retry.rs"]
mod job_retry;
#[path = "../src/job_worker.rs"]
mod job_worker;
#[path = "../src/jobs/mod.rs"]
mod jobs;
#[path = "../src/media.rs"]
mod media;
#[path = "../src/search_settings_worker.rs"]
mod search_settings_worker;
#[path = "../src/state.rs"]
mod state;

#[path = "../src/routes/admin.rs"]
mod admin_routes;
#[path = "../src/routes/admin_settings.rs"]
mod admin_settings_routes;
#[path = "../src/routes/auth.rs"]
mod auth_routes;
#[path = "../src/routes/entries.rs"]
mod entries_routes;
#[path = "../src/routes/files.rs"]
mod files_routes;
#[path = "../src/routes/language_resolution.rs"]
mod language_resolution_routes;
#[path = "../src/routes/messages.rs"]
mod messages_routes;
#[path = "../src/routes/meta.rs"]
mod meta_routes;
#[path = "../src/routes/notifications.rs"]
mod notifications_routes;
#[path = "../src/routes/project_media.rs"]
mod project_media_routes;
#[path = "../src/routes/projects.rs"]
mod projects_routes;
#[path = "../src/routes/users.rs"]
mod users_routes;
#[path = "../src/routes/uploads.rs"]
mod uploads_routes;

/// 将数据库错误映射为真实 handler 使用的统一 API 错误。
fn db_err(error: prts_db::DbError) -> error::ApiError {
    prts_common::Error::internal(format!("db error: {error}")).into()
}

/// `entries` handler 的兄弟模块依赖；保持与生产 route 组合函数相同的解析语义。
fn parse_states(value: Option<&str>) -> Vec<String> {
    value
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|state| prts_core::EntryState::parse(state).is_some())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

use prts_db::{
    api_keys, auth_sessions, entries, files, jobs as db_jobs, memberships, messages, notifications,
    projects, settings, users,
};

static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
static SEARCH_SETTINGS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static UPLOAD_SETTINGS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static UPLOAD_LIFECYCLE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
            // 测试专用故障注入：trigger 只在当前连接显式设置 GUC 时拒绝 audit INSERT。
            // 因此专用 failure pool 可稳定触发 fail-closed，其他并行测试连接完全不受影响。
            sqlx::query(
                "CREATE OR REPLACE FUNCTION prts_test_reject_audit_insert()
                 RETURNS trigger LANGUAGE plpgsql AS $$
                 BEGIN
                     IF current_setting('prts.test_fail_audit', true) = 'on' THEN
                         RAISE EXCEPTION 'injected audit insert failure' USING ERRCODE = '57014';
                     END IF;
                     RETURN NEW;
                 END;
                 $$",
            )
            .execute(&mut *migration_conn)
            .await
            .expect("安装连接隔离的审计故障函数");
            sqlx::query("DROP TRIGGER IF EXISTS prts_test_reject_audit_insert ON audit_log")
                .execute(&mut *migration_conn)
                .await
                .expect("重置审计故障 trigger");
            sqlx::query(
                "CREATE TRIGGER prts_test_reject_audit_insert
                 BEFORE INSERT ON audit_log FOR EACH ROW
                 EXECUTE FUNCTION prts_test_reject_audit_insert()",
            )
            .execute(&mut *migration_conn)
            .await
            .expect("安装审计故障 trigger");
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

// ======================== Task 1.2 audit contracts ========================

/// 一个现有 route/auth 写入口及其必须提交的审计 action。
///
/// 此清单是 Task 1.2 的唯一测试 inventory：内部统计 writer 归属于触发它的业务 action，
/// 普通读取则列在 `UNAUDITED_READS`，不会因“覆盖所有 writer”而误加读取审计。
#[derive(Debug, Clone, Copy)]
struct AuditedEntrypoint {
    entrypoint: &'static str,
    action: &'static str,
    allowed_payload_keys: &'static [&'static str],
}

/// 每个稳定 action 的目标合同；与 route entrypoint 清单分离，且 action 必须唯一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetIdPolicy {
    Numeric,
    CompositeNumeric,
    Constant(&'static str),
    OpaqueNonEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectSnapshotPolicy {
    None,
    SameAsNumericTarget,
    Required,
}

#[derive(Debug, Clone, Copy)]
struct AuditActionContract {
    action: &'static str,
    target_type: &'static str,
    target_id_policy: TargetIdPolicy,
    project_snapshot_policy: ProjectSnapshotPolicy,
    /// 有多少 route/auth entrypoint 合法地产生该 action；用于显式容纳 `auth.failed` 多入口。
    expected_count: usize,
}

const AUDITED_ENTRYPOINTS: &[AuditedEntrypoint] = &[
    AuditedEntrypoint {
        entrypoint: "routes::auth::register",
        action: "auth.registered",
        allowed_payload_keys: &["method", "status"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::login.success",
        action: "auth.login_succeeded",
        allowed_payload_keys: &["method"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::login.failure",
        action: "auth.login_failed",
        allowed_payload_keys: &["method", "reason_code"],
    },
    AuditedEntrypoint {
        entrypoint: "auth::extract::presented_failure",
        action: "auth.failed",
        allowed_payload_keys: &["method", "reason_code"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::refresh.failure",
        action: "auth.failed",
        allowed_payload_keys: &["method", "reason_code"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::refresh",
        action: "auth.refresh_rotated",
        allowed_payload_keys: &["session_handle", "predecessor_handle", "expires_at"],
    },
    AuditedEntrypoint {
        entrypoint: "auth::session::refresh.token_issue",
        action: "auth.token_issued",
        allowed_payload_keys: &["session_handle", "method", "expires_at"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::logout",
        action: "auth.logged_out",
        allowed_payload_keys: &["session_handle", "revoked_sessions"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::logout.noop",
        action: "auth.failed",
        allowed_payload_keys: &["method", "reason_code"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::oauth_callback.success",
        action: "auth.oauth_succeeded",
        allowed_payload_keys: &["provider", "new_user"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::auth::oauth_callback.failure",
        action: "auth.oauth_failed",
        allowed_payload_keys: &["provider", "reason_code"],
    },
    AuditedEntrypoint {
        entrypoint: "auth::session::issue",
        action: "auth.token_issued",
        allowed_payload_keys: &["session_handle", "method", "expires_at"],
    },
    AuditedEntrypoint {
        entrypoint: "main/auth::maybe_bootstrap_super_admin",
        action: "auth.bootstrap_role_granted",
        allowed_payload_keys: &["role"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::users::update_me",
        action: "user.profile_updated",
        allowed_payload_keys: &["changed_fields", "translation_lang_count"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::users::create_api_key",
        action: "api_key.created",
        allowed_payload_keys: &["name", "prefix"],
    },
    AuditedEntrypoint {
        entrypoint: "auth::extract::api_key_touch",
        action: "api_key.used",
        allowed_payload_keys: &["prefix"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::users::revoke_api_key",
        action: "api_key.revoked",
        allowed_payload_keys: &["prefix"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::admin::update_settings",
        action: "settings.updated",
        allowed_payload_keys: &["keys", "count"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::admin_settings::put_search_settings",
        action: "search_settings.updated",
        allowed_payload_keys: &["changed_fields"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::admin_settings::put_upload_settings",
        action: "upload_settings.updated",
        allowed_payload_keys: &["changed_fields"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::admin::grant_role",
        action: "user.platform_role_changed",
        allowed_payload_keys: &["previous_role", "new_role"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::projects::create_project",
        action: "project.created",
        allowed_payload_keys: &["slug", "visibility", "source_langs", "target_lang"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::projects::update_project",
        action: "project.updated",
        allowed_payload_keys: &["changed_fields", "visibility"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::project_media::upload_project_avatar",
        action: "project.avatar_updated",
        allowed_payload_keys: &["content_type", "encoded_bytes", "replaced"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::project_media::delete_project_avatar",
        action: "project.avatar_deleted",
        allowed_payload_keys: &["had_avatar"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::projects::change_primary_source",
        action: "project.primary_source_changed",
        allowed_payload_keys: &[
            "previous_primary_source",
            "new_primary_source",
            "source_language_count",
            "lexical_job_id",
        ],
    },
    AuditedEntrypoint {
        entrypoint: "routes::language_resolution::resolve_project_languages",
        action: "project.language_resolution_completed",
        allowed_payload_keys: &[
            "issue_count",
            "source_language_count",
            "primary_source_language",
            "target_language",
        ],
    },
    AuditedEntrypoint {
        entrypoint: "routes::language_resolution::retry_admin_language_repair",
        action: "project.language_repair_retried",
        allowed_payload_keys: &["job_id", "previous_state"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::projects::delete_project",
        action: "project.deleted",
        allowed_payload_keys: &["slug"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::projects::add_member",
        action: "membership.upserted",
        allowed_payload_keys: &["member_id", "previous_role", "new_role"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::projects::remove_member",
        action: "membership.removed",
        allowed_payload_keys: &["member_id", "previous_role"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::entries::upload",
        action: "entries.uploaded",
        allowed_payload_keys: &["file_id", "path", "created", "updated", "unchanged"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::uploads::create_batch",
        action: "upload.batch_created",
        allowed_payload_keys: &["file_count", "total_bytes"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::uploads::receive_attempt.started",
        action: "upload.attempt_started",
        allowed_payload_keys: &["batch_id", "batch_file_id"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::uploads::receive_attempt.received",
        action: "upload.attempt_received",
        allowed_payload_keys: &["batch_id", "batch_file_id", "bytes_received"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::uploads::receive_attempt.failed",
        action: "upload.attempt_failed",
        allowed_payload_keys: &["batch_id", "batch_file_id", "bytes_received", "error_code"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::uploads::complete_batch",
        action: "upload.batch_queued",
        allowed_payload_keys: &["file_count"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::uploads::retry_file",
        action: "upload.file_retried",
        allowed_payload_keys: &["batch_id", "batch_file_id", "attempt_number"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::uploads::cancel_batch",
        action: "upload.batch_cancelled",
        allowed_payload_keys: &["file_count"],
    },
    AuditedEntrypoint {
        entrypoint: "jobs::cleanup_uploads::expire_due",
        action: "upload.batch_expired",
        allowed_payload_keys: &["file_count"],
    },
    AuditedEntrypoint {
        entrypoint: "jobs::cleanup_uploads::mark_attempt_cleaned",
        action: "upload.attempt_cleaned",
        allowed_payload_keys: &["batch_id", "batch_file_id"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::files::delete_file",
        action: "file.deleted",
        allowed_payload_keys: &["path", "entry_count"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::files::delete_folder",
        action: "folder.deleted",
        allowed_payload_keys: &["path", "file_count", "entry_count"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::entries::update_entry",
        action: "entry.updated",
        allowed_payload_keys: &[
            "previous_version",
            "new_version",
            "previous_state",
            "new_state",
        ],
    },
    AuditedEntrypoint {
        entrypoint: "routes::entries::set_entry_flags",
        action: "entry.flags_updated",
        allowed_payload_keys: &["locked", "hidden"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::entries::export_project",
        action: "project.exported",
        allowed_payload_keys: &["file_count", "entry_count", "include_hidden"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::notifications::mark_read",
        action: "notification.marked_read",
        allowed_payload_keys: &["notification_ids", "count", "all"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::notifications::poke",
        action: "poke.sent",
        allowed_payload_keys: &[
            "recipient_id",
            "project_id",
            "notification_id",
            "text_length",
        ],
    },
    AuditedEntrypoint {
        entrypoint: "routes::messages::send",
        action: "message.sent",
        allowed_payload_keys: &["recipient_id", "message_id", "content_length"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::messages::mark_read",
        action: "message.marked_read",
        allowed_payload_keys: &["other_user_id", "count"],
    },
    AuditedEntrypoint {
        entrypoint: "routes::jobs::retry_job",
        action: "job.retried",
        allowed_payload_keys: &["kind", "previous_attempts", "new_attempts"],
    },
];

const AUDIT_ACTION_CONTRACTS: &[AuditActionContract] = &[
    AuditActionContract {
        action: "auth.registered",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "auth.login_succeeded",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "auth.login_failed",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "auth.failed",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 3,
    },
    AuditActionContract {
        action: "auth.refresh_rotated",
        target_type: "auth_session",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "auth.logged_out",
        target_type: "auth_session",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "auth.oauth_succeeded",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "auth.oauth_failed",
        target_type: "oauth_identity",
        target_id_policy: TargetIdPolicy::OpaqueNonEmpty,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "auth.token_issued",
        target_type: "auth_session",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 2,
    },
    AuditActionContract {
        action: "auth.bootstrap_role_granted",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "user.profile_updated",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "api_key.created",
        target_type: "api_key",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "api_key.used",
        target_type: "api_key",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "api_key.revoked",
        target_type: "api_key",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "settings.updated",
        target_type: "settings",
        target_id_policy: TargetIdPolicy::Constant("platform"),
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "search_settings.updated",
        target_type: "settings",
        target_id_policy: TargetIdPolicy::Constant("search.config"),
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload_settings.updated",
        target_type: "settings",
        target_id_policy: TargetIdPolicy::Constant("upload.config"),
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "user.platform_role_changed",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.created",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.updated",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.avatar_updated",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.avatar_deleted",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.primary_source_changed",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.language_resolution_completed",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.language_repair_retried",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.deleted",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "membership.upserted",
        target_type: "membership",
        target_id_policy: TargetIdPolicy::CompositeNumeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "membership.removed",
        target_type: "membership",
        target_id_policy: TargetIdPolicy::CompositeNumeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "entries.uploaded",
        target_type: "file",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.batch_created",
        target_type: "upload_batch",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.attempt_started",
        target_type: "upload_attempt",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.attempt_received",
        target_type: "upload_attempt",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.attempt_failed",
        target_type: "upload_attempt",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.batch_queued",
        target_type: "upload_batch",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.file_retried",
        target_type: "upload_attempt",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.batch_cancelled",
        target_type: "upload_batch",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.batch_expired",
        target_type: "upload_batch",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "upload.attempt_cleaned",
        target_type: "upload_attempt",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "file.deleted",
        target_type: "file",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "folder.deleted",
        target_type: "folder",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "entry.updated",
        target_type: "entry",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "entry.flags_updated",
        target_type: "entry",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "project.exported",
        target_type: "project",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::SameAsNumericTarget,
        expected_count: 1,
    },
    AuditActionContract {
        action: "notification.marked_read",
        target_type: "user",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "poke.sent",
        target_type: "notification",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
    AuditActionContract {
        action: "message.sent",
        target_type: "message",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "message.marked_read",
        target_type: "conversation",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::None,
        expected_count: 1,
    },
    AuditActionContract {
        action: "job.retried",
        target_type: "job",
        target_id_policy: TargetIdPolicy::Numeric,
        project_snapshot_policy: ProjectSnapshotPolicy::Required,
        expected_count: 1,
    },
];

/// 现有被审计 repository writer。每项在 GREEN 后必须有 `&mut PgConnection`/`*_tx`
/// 入口；这里保留公共函数级清单，避免遗漏不直接暴露 route 的邮箱验证与内部统计写入。
const REPOSITORY_WRITERS: &[&str] = &[
    "users::create_password_user",
    "users::update_profile",
    "users::set_platform_role",
    "users::mark_email_verified",
    "users::create_oauth_user",
    "projects::create",
    "projects::create_with_primary_tx",
    "projects::update",
    "projects::set_avatar_tx",
    "projects::clear_avatar_tx",
    "projects::change_primary_source_tx",
    "projects::delete",
    "files::ensure_file_at_path",
    "files::get_or_create_folder",
    "files::delete_file",
    "files::delete_folder",
    "files::refresh_entry_count",
    "entries::bulk_upsert",
    "entries::update_translation",
    "entries::set_flags",
    "memberships::upsert",
    "memberships::remove",
    "settings::set",
    "search_settings::set",
    "api_keys::create",
    "api_keys::touch_last_used",
    "api_keys::revoke",
    "notifications::create",
    "notifications::mark_read",
    "messages::create",
    "messages::mark_read",
    "jobs::manual_retry",
    "upload_settings::set_tx",
    "upload_settings::set_locked_tx",
    "language_resolution::complete_owner_resolution_tx",
    "language_resolution::update_entry_original_tx",
    "language_resolution::retry_failed_project_repair_tx",
    "uploads::create_batch_tx",
    "uploads::claim_attempt_for_receive_tx",
    "uploads::mark_attempt_received_tx",
    "uploads::fail_attempt_tx",
    "uploads::queue_batch_tx",
    "uploads::retry_file_tx",
    "uploads::cancel_batch_tx",
    "uploads::expire_due",
    "uploads::mark_attempt_cleaned",
];

/// DB-authoritative session 与 durable intent/outbox 的全部现有写边界。
const AUTH_SESSION_WRITERS: &[&str] = &[
    "session::issue",
    "session::refresh",
    "session::revoke",
    "auth_sessions::create_pending_tx",
    "auth_sessions::activate_pending_tx",
    "auth_sessions::begin_rotation_tx",
    "auth_sessions::revoke_unexpired_tx",
    "auth_sessions::expire_due_tx",
    "auth_sessions::complete_rotation_tx",
    "auth_sessions::enqueue_intent_tx",
    "auth_sessions::claim_intent",
    "auth_sessions::renew_intent_lease",
    "auth_sessions::complete_intent",
    "auth_sessions::reschedule_intent",
    "auth_sessions::fail_intent_permanently",
    "auth_sessions::retry_intent_tx",
];

/// 普通读取必须保持零审计；敏感项目导出不在此列。
const UNAUDITED_READS: &[&str] = &[
    "users::me",
    "users::get_user",
    "users::my_accounts",
    "users::list_api_keys",
    "admin::get_settings",
    "admin_settings::get_search_settings",
    "admin_settings::get_upload_settings",
    "meta::upload_config",
    "projects::list_projects",
    "projects::get_project",
    "project_media::get_project_avatar",
    "projects::list_members",
    "files::get_tree",
    "entries::list_entries",
    "entries::get_entry",
    "entries::entry_history",
    "notifications::list",
    "notifications::unread_count",
    "messages::list_threads",
    "messages::conversation",
    "messages::unread_count",
    "language_resolution::get_project_language_resolution",
    "language_resolution::list_admin_language_resolutions",
    "uploads::get_batch",
    "uploads::list_cleanup_candidates",
];

#[test]
fn audit_contract_inventory_covers_every_existing_writer_with_typed_payloads() {
    use std::collections::HashSet;

    assert_eq!(
        REPOSITORY_WRITERS.len(),
        46,
        "repository writer inventory 发生漂移"
    );
    assert_eq!(
        AUTH_SESSION_WRITERS.len(),
        16,
        "auth/session writer inventory 发生漂移"
    );
    assert_eq!(UNAUDITED_READS.len(), 25, "普通读取 inventory 发生漂移");
    assert_eq!(AUDITED_ENTRYPOINTS.len(), 51, "审计入口 inventory 发生漂移");
    assert_eq!(
        AUDIT_ACTION_CONTRACTS.len(),
        48,
        "action 合同 inventory 发生漂移"
    );

    let writers: HashSet<_> = REPOSITORY_WRITERS.iter().copied().collect();
    assert_eq!(writers.len(), REPOSITORY_WRITERS.len(), "writer 不得重复");
    let entrypoints: HashSet<_> = AUDITED_ENTRYPOINTS
        .iter()
        .map(|entry| entry.entrypoint)
        .collect();
    assert_eq!(entrypoints.len(), AUDITED_ENTRYPOINTS.len());
    let auth_writers: HashSet<_> = AUTH_SESSION_WRITERS.iter().copied().collect();
    assert_eq!(auth_writers.len(), AUTH_SESSION_WRITERS.len());
    let actions: HashSet<_> = AUDIT_ACTION_CONTRACTS
        .iter()
        .map(|contract| contract.action)
        .collect();
    assert_eq!(
        actions.len(),
        AUDIT_ACTION_CONTRACTS.len(),
        "每个 action 必须只有一个目标合同"
    );

    for contract in AUDIT_ACTION_CONTRACTS {
        let entrypoint_count = AUDITED_ENTRYPOINTS
            .iter()
            .filter(|entry| entry.action == contract.action)
            .count();
        assert_eq!(
            entrypoint_count, contract.expected_count,
            "{} 的 entrypoint 数量与合同不符",
            contract.action
        );
    }

    for entry in AUDITED_ENTRYPOINTS {
        assert!(
            !entry.allowed_payload_keys.is_empty(),
            "{} 缺 typed allowlist",
            entry.action
        );
        for key in entry.allowed_payload_keys {
            assert!(
                !audit_contract_key_is_sensitive(key),
                "{} allowlist 含敏感键 {key}",
                entry.action
            );
        }
    }
}

/// 所有 audit before snapshot 都必须可在调用方事务内加锁读取，避免 route 先读后写竞态。
#[tokio::test]
async fn audit_contract_repositories_expose_transaction_local_locked_snapshots() {
    let state = audit_contract_state().await;
    let mut tx = state.db.begin().await.unwrap();

    let _ = users::find_by_id_for_update_tx(&mut tx, 0).await.unwrap();
    let _ = users::find_by_username_for_update_tx(&mut tx, "missing")
        .await
        .unwrap();
    let _ = projects::find_by_id_for_update_tx(&mut tx, 0)
        .await
        .unwrap();
    let _ = entries::get_for_update_tx(&mut tx, 0, 0).await.unwrap();
    let _ = memberships::find_role_tx(&mut tx, 0, 0).await.unwrap();
    let _ = memberships::count_role_tx(&mut tx, 0, "owner")
        .await
        .unwrap();
    let _ = files::find_file_for_update_tx(&mut tx, 0, 0).await.unwrap();
    let _ = files::find_folder_for_update_tx(&mut tx, 0, 0)
        .await
        .unwrap();
    let _ = files::folder_tree_counts_tx(&mut tx, 0, "/").await.unwrap();
    let _ = prts_db::search_settings::get_for_update_tx(&mut tx)
        .await
        .unwrap();

    tx.rollback().await.unwrap();
}

/// 项目行锁必须串行化成员管理，即使目标 membership 尚不存在也不能绕过锁。
#[tokio::test]
async fn audit_contract_project_lock_serializes_membership_upserts_for_missing_rows() {
    let state = audit_contract_state().await;
    let actor = audit_contract_create_user(&state.db, "audit-member-lock-owner", None).await;
    let member = audit_contract_create_user(&state.db, "audit-member-lock-target", None).await;
    let project = projects::create(
        &state.db,
        &format!("audit-member-lock-{}", actor.id),
        "Audit member lock",
        "",
        "private",
        &["en".to_string()],
        "zh-Hans",
        actor.id,
    )
    .await
    .unwrap();

    let mut first = state.db.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *first)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut first, project.id)
        .await
        .unwrap()
        .unwrap();
    let second_pool = state.db.clone();
    let project_id = project.id;
    let member_id = member.id;
    let second = tokio::spawn(async move {
        let mut tx = second_pool.begin().await.unwrap();
        projects::find_by_id_for_update_tx(&mut tx, project_id)
            .await
            .unwrap()
            .unwrap();
        memberships::upsert_tx(&mut tx, project_id, member_id, "translator")
            .await
            .unwrap();
        tx.commit().await.unwrap();
    });
    audit_contract_wait_for_postgres_block(
        &state.db,
        blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &second,
        "第二个成员写事务必须等待同一项目行锁",
    )
    .await;
    first.rollback().await.unwrap();
    second.await.unwrap();

    projects::delete(&state.db, project.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE id = ANY($1::BIGINT[])")
        .bind(&[actor.id, member.id][..])
        .execute(&state.db)
        .await
        .unwrap();
}

/// 两个真实搜索设置 PUT 必须按进入顺序提交并发布，最终 DB/runtime 同为后一次更新。
#[tokio::test]
async fn audit_contract_concurrent_search_puts_keep_db_and_runtime_in_commit_order() {
    use axum::extract::State;
    use axum::Json;
    use std::time::Duration;

    let _search_settings_guard = SEARCH_SETTINGS_TEST_LOCK.lock().await;
    let state = audit_contract_state().await;
    let actor = audit_contract_create_user(
        &state.db,
        "audit-search-settings-order",
        Some("super_admin"),
    )
    .await;
    let previous = prts_db::search_settings::get(&state.db).await.unwrap();
    let first = prts_db::search_settings::SearchConfig {
        embedding_enabled: false,
        embedding_model: format!("audit-search-first-{}", actor.id),
        embedding_base_url: "https://first.invalid/v1".to_string(),
        embedding_batch: 4,
        tm_enabled: true,
        tm_min_similarity: 0.41,
        tm_top_n: 1,
    };
    let second = prts_db::search_settings::SearchConfig {
        embedding_enabled: false,
        embedding_model: format!("audit-search-second-{}", actor.id),
        embedding_base_url: "https://second.invalid/v1".to_string(),
        embedding_batch: 5,
        tm_enabled: false,
        tm_min_similarity: 0.52,
        tm_top_n: 2,
    };

    let mut commit_blocker = state.db.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *commit_blocker)
        .await
        .unwrap();
    prts_db::search_settings::get_for_update_tx(&mut commit_blocker)
        .await
        .unwrap();
    let first_put = {
        let state = state.clone();
        let user = audit_contract_current_user(&actor);
        let body = admin_settings_routes::SearchConfigDto {
            embedding_enabled: first.embedding_enabled,
            embedding_model: first.embedding_model.clone(),
            embedding_base_url: first.embedding_base_url.clone(),
            embedding_batch: first.embedding_batch,
            tm_enabled: first.tm_enabled,
            tm_min_similarity: first.tm_min_similarity,
            tm_top_n: first.tm_top_n,
        };
        tokio::spawn(async move {
            admin_settings_routes::put_search_settings(State(state), user, Json(body)).await
        })
    };
    audit_contract_wait_for_postgres_block(
        &state.db,
        blocker_pid,
        "SELECT pg_advisory_xact_lock($1)",
        &first_put,
        "第一次 PUT 应等待 DB advisory lock",
    )
    .await;
    let second_put = tokio::spawn(admin_settings_routes::put_search_settings(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(admin_settings_routes::SearchConfigDto {
            embedding_enabled: second.embedding_enabled,
            embedding_model: second.embedding_model.clone(),
            embedding_base_url: second.embedding_base_url.clone(),
            embedding_batch: second.embedding_batch,
            tm_enabled: second.tm_enabled,
            tm_min_similarity: second.tm_min_similarity,
            tm_top_n: second.tm_top_n,
        }),
    ));
    commit_blocker.rollback().await.unwrap();

    let Json(first_response) = tokio::time::timeout(Duration::from_secs(5), first_put)
        .await
        .expect("释放 advisory lock 后第一次 PUT 应完成")
        .unwrap()
        .expect_api("第一次搜索设置 PUT 成功");
    let Json(second_response) = tokio::time::timeout(Duration::from_secs(5), second_put)
        .await
        .expect("第一次发布完成后第二次 PUT 应完成")
        .unwrap()
        .expect_api("第二次搜索设置 PUT 成功");
    assert_eq!(first_response.config.embedding_model, first.embedding_model);
    assert_eq!(
        second_response.config.embedding_model,
        second.embedding_model
    );
    assert_eq!(
        prts_db::search_settings::get(&state.db).await.unwrap(),
        second
    );
    assert_eq!(*state.search_rt.read().await, second);

    prts_db::search_settings::set(&state.db, previous.clone(), Some(actor.id))
        .await
        .unwrap();
    *state.search_rt.write().await = previous;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(actor.id)
        .execute(&state.db)
        .await
        .unwrap();
}

/// 请求在 worker 已进入 DB 事务后取消，已入队更新仍必须完成 DB/audit/runtime 收敛。
#[tokio::test]
async fn audit_contract_cancelled_search_put_still_converges_db_runtime_and_audit() {
    use std::time::Duration;

    let _search_settings_guard = SEARCH_SETTINGS_TEST_LOCK.lock().await;
    let state = audit_contract_state().await;
    let actor = audit_contract_create_user(
        &state.db,
        "audit-search-settings-cancel",
        Some("super_admin"),
    )
    .await;
    let before_db = prts_db::search_settings::get(&state.db).await.unwrap();
    let before_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_log
         WHERE actor_id = $1 AND action = 'search_settings.updated'",
    )
    .bind(actor.id)
    .fetch_one(&state.db)
    .await
    .unwrap();

    let expected = prts_db::search_settings::SearchConfig {
        embedding_enabled: false,
        embedding_model: format!("cancelled-search-{}", actor.id),
        embedding_base_url: "https://cancelled.invalid/v1".to_string(),
        embedding_batch: 2,
        tm_enabled: false,
        tm_min_similarity: 0.77,
        tm_top_n: 1,
    };
    let mut commit_blocker = state.db.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *commit_blocker)
        .await
        .unwrap();
    prts_db::search_settings::get_for_update_tx(&mut commit_blocker)
        .await
        .unwrap();
    let response = state
        .search_settings_updater
        .enqueue(actor.id, expected.clone())
        .await
        .expect("更新成功入队");
    let response_waiter = tokio::spawn(response);
    audit_contract_wait_for_postgres_block(
        &state.db,
        blocker_pid,
        "SELECT pg_advisory_xact_lock($1)",
        &response_waiter,
        "搜索设置 worker 应已进入 DB 事务并等待 advisory lock",
    )
    .await;
    let runtime_read = tokio::time::timeout(Duration::from_millis(100), state.search_rt.read())
        .await
        .expect("DB 锁竞争期间 runtime 读取不得被搜索设置事务阻塞");
    drop(runtime_read);
    response_waiter.abort();
    let waiter_error = response_waiter.await.expect_err("模拟请求等待方必须已取消");
    assert!(waiter_error.is_cancelled());
    commit_blocker.rollback().await.unwrap();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if prts_db::search_settings::get(&state.db).await.unwrap() == expected
                && *state.search_rt.read().await == expected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("接收端丢弃后 worker 仍应完成 DB/runtime 发布");
    let after_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_log
         WHERE actor_id = $1 AND action = 'search_settings.updated'",
    )
    .bind(actor.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(after_audit, before_audit + 1);

    prts_db::search_settings::set(&state.db, before_db.clone(), Some(actor.id))
        .await
        .unwrap();
    *state.search_rt.write().await = before_db;

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(actor.id)
        .execute(&state.db)
        .await
        .unwrap();
}

/// 所有现有项目业务 mutation 都必须在项目锁后、业务写前重新加载授权快照。
#[test]
fn audit_contract_project_mutations_reauthorize_inside_locked_transaction() {
    fn function_body<'a>(source: &'a str, start: &str, end: Option<&str>) -> &'a str {
        let tail = source
            .split_once(start)
            .unwrap_or_else(|| panic!("缺少函数 {start}"))
            .1;
        end.map_or(tail, |end| {
            tail.split_once(end)
                .unwrap_or_else(|| panic!("缺少函数结束标记 {end}"))
                .0
        })
    }

    let projects_source = include_str!("../src/routes/projects.rs");
    let entries_source = include_str!("../src/routes/entries.rs");
    let files_source = include_str!("../src/routes/files.rs");
    let notifications_source = include_str!("../src/routes/notifications.rs");
    let project_media_source = include_str!("../src/routes/project_media.rs");
    let job_retry_source = include_str!("../src/job_retry.rs");
    for (body, writer) in [
        (
            function_body(
                entries_source,
                "pub async fn upload(",
                Some("// ============================= 词条"),
            ),
            "entries::bulk_upsert_tx",
        ),
        (
            function_body(
                projects_source,
                "pub async fn change_primary_source(",
                Some("fn primary_source_change_error("),
            ),
            "projects::change_primary_source_tx",
        ),
        (
            function_body(
                projects_source,
                "pub async fn update_project(",
                Some("/// 删除项目"),
            ),
            "projects::update_tx",
        ),
        (
            function_body(
                projects_source,
                "pub async fn delete_project(",
                Some("/// 成员对外表示"),
            ),
            "projects::delete_tx",
        ),
        (
            function_body(
                project_media_source,
                "pub async fn upload_project_avatar(",
                Some("/// 删除项目头像"),
            ),
            "projects::set_avatar_tx",
        ),
        (
            function_body(
                project_media_source,
                "pub async fn delete_project_avatar(",
                Some("/// 按项目可见性读取头像"),
            ),
            "projects::clear_avatar_tx",
        ),
        (
            function_body(
                projects_source,
                "pub async fn add_member(",
                Some("/// 移除项目成员"),
            ),
            "memberships::upsert_tx",
        ),
        (
            function_body(projects_source, "pub async fn remove_member(", None),
            "memberships::remove_tx",
        ),
        (
            function_body(
                entries_source,
                "pub async fn update_entry(",
                Some("/// 设置标志请求"),
            ),
            "entries::update_translation_tx",
        ),
        (
            function_body(
                entries_source,
                "pub async fn set_entry_flags(",
                Some("/// 词条历史项"),
            ),
            "entries::set_flags_tx",
        ),
        (
            function_body(
                files_source,
                "pub async fn delete_file(",
                Some("/// 删除文件夹"),
            ),
            "files::delete_file_tx",
        ),
        (
            function_body(
                files_source,
                "pub async fn delete_folder(",
                Some("#[cfg(test)]"),
            ),
            "files::delete_folder_tx",
        ),
        (
            function_body(notifications_source, "pub async fn poke(", None),
            "notifications::create_tx",
        ),
        (
            function_body(job_retry_source, "pub(crate) async fn retry_job(", None),
            "jobs::manual_retry_tx",
        ),
    ] {
        let project_lock = body
            .find("projects::find_by_id_for_update_tx")
            .expect("项目 mutation 必须先锁项目");
        let reauthorization = body
            .find("paccess::load_locked_tx")
            .expect("项目 mutation 必须在锁后重验权限");
        let business_write = body.find(writer).expect("项目 mutation 业务 writer 存在");
        assert!(project_lock < reauthorization);
        assert!(reauthorization < business_write);
    }
}

/// 上传、文件删除与文件夹删除都必须等待项目行锁；文件夹审计计数对应实际级联子树。
#[tokio::test]
async fn audit_contract_file_tree_routes_share_project_lock_and_count_deleted_subtree() {
    use axum::extract::{Path, State};
    use axum::Json;

    let state = audit_contract_state().await;
    let actor =
        audit_contract_create_user(&state.db, "audit-file-tree-lock", Some("maintainer")).await;
    let Json(project) = projects_routes::create_project(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(projects_routes::CreateProjectReq {
            name: format!("Audit file tree lock {}", actor.id),
            slug: Some(format!("audit-file-tree-lock-{}", actor.id)),
            description: None,
            visibility: Some("private".to_string()),
            source_langs: vec!["en".to_string()],
            primary_source_lang: None,
            target_lang: "zh-Hans".to_string(),
        }),
    )
    .await
    .expect_api("创建文件树锁测试项目");

    let mut upload_blocker = state.db.begin().await.unwrap();
    let upload_blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *upload_blocker)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut upload_blocker, project.id)
        .await
        .unwrap()
        .unwrap();
    let blocked_upload = {
        let state = state.clone();
        let user = audit_contract_current_user(&actor);
        tokio::spawn(async move {
            entries_routes::upload(
                State(state),
                user,
                Path(project.id),
                Json(audit_contract_upload_req(
                    "lock/file.json",
                    &[("file-entry", "one")],
                )),
            )
            .await
        })
    };
    audit_contract_wait_for_postgres_block(
        &state.db,
        upload_blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &blocked_upload,
        "上传必须等待项目行锁",
    )
    .await;
    upload_blocker.rollback().await.unwrap();
    let Json(file_to_delete) = blocked_upload
        .await
        .unwrap()
        .expect_api("释放项目锁后上传成功");

    let mut file_delete_blocker = state.db.begin().await.unwrap();
    let file_delete_blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *file_delete_blocker)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut file_delete_blocker, project.id)
        .await
        .unwrap()
        .unwrap();
    let blocked_file_delete = {
        let state = state.clone();
        let user = audit_contract_current_user(&actor);
        tokio::spawn(async move {
            files_routes::delete_file(
                State(state),
                user,
                Path((project.id, file_to_delete.file_id)),
            )
            .await
        })
    };
    audit_contract_wait_for_postgres_block(
        &state.db,
        file_delete_blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &blocked_file_delete,
        "文件删除必须等待项目行锁",
    )
    .await;
    file_delete_blocker.rollback().await.unwrap();
    blocked_file_delete
        .await
        .unwrap()
        .expect_api("释放项目锁后文件删除成功");
    assert!(
        files::find_file(&state.db, project.id, file_to_delete.file_id)
            .await
            .unwrap()
            .is_none()
    );

    let Json(first_subtree_file) = entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
        Json(audit_contract_upload_req(
            "cascade/one.json",
            &[("one", "one")],
        )),
    )
    .await
    .expect_api("创建第一个待级联文件");
    let Json(second_subtree_file) = entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
        Json(audit_contract_upload_req(
            "cascade/nested/two.json",
            &[("two-a", "two a"), ("two-b", "two b")],
        )),
    )
    .await
    .expect_api("创建第二个待级联文件");
    let folder_id = files::list_folders(&state.db, project.id)
        .await
        .unwrap()
        .into_iter()
        .find(|folder| folder.path == "cascade")
        .expect("待级联顶层文件夹存在")
        .id;

    let mut folder_delete_blocker = state.db.begin().await.unwrap();
    let folder_delete_blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *folder_delete_blocker)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut folder_delete_blocker, project.id)
        .await
        .unwrap()
        .unwrap();
    let blocked_folder_delete = {
        let state = state.clone();
        let user = audit_contract_current_user(&actor);
        tokio::spawn(async move {
            files_routes::delete_folder(State(state), user, Path((project.id, folder_id))).await
        })
    };
    audit_contract_wait_for_postgres_block(
        &state.db,
        folder_delete_blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &blocked_folder_delete,
        "文件夹删除必须等待项目行锁",
    )
    .await;
    folder_delete_blocker.rollback().await.unwrap();
    blocked_folder_delete
        .await
        .unwrap()
        .expect_api("释放项目锁后文件夹删除成功");

    let remaining_file_ids: Vec<i64> = files::list_files(&state.db, project.id)
        .await
        .unwrap()
        .into_iter()
        .map(|file| file.id)
        .collect();
    assert!(!remaining_file_ids.contains(&first_subtree_file.file_id));
    assert!(!remaining_file_ids.contains(&second_subtree_file.file_id));
    assert!(files::list_folders(&state.db, project.id)
        .await
        .unwrap()
        .into_iter()
        .all(|folder| folder.path != "cascade" && !folder.path.starts_with("cascade/")));

    let folder_audit =
        audit_contract_rows_for_action_target(&state.db, "folder.deleted", &folder_id.to_string())
            .await;
    assert_eq!(folder_audit.len(), 1);
    assert_eq!(folder_audit[0].project_id, Some(project.id));
    assert_eq!(folder_audit[0].payload["file_count"], 2);
    assert_eq!(folder_audit[0].payload["entry_count"], 3);

    projects::delete(&state.db, project.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(actor.id)
        .execute(&state.db)
        .await
        .unwrap();
}

/// 文件树 mutation 等待项目锁期间被撤权时，锁内授权快照必须拒绝陈旧权限。
#[tokio::test]
async fn audit_contract_upload_revalidates_permission_after_project_lock() {
    use axum::extract::{Path, State};
    use axum::response::IntoResponse;
    use axum::Json;

    let state = audit_contract_state().await;
    let owner = audit_contract_create_user(&state.db, "audit-lock-revoke-owner", None).await;
    let manager = audit_contract_create_user(&state.db, "audit-lock-revoke-manager", None).await;
    let project = projects::create(
        &state.db,
        &format!("audit-lock-revoke-{}", owner.id),
        "Audit lock revoke",
        "",
        "private",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert(&state.db, project.id, owner.id, "owner")
        .await
        .unwrap();
    memberships::upsert(&state.db, project.id, manager.id, "manager")
        .await
        .unwrap();

    let mut revocation = state.db.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revocation)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut revocation, project.id)
        .await
        .unwrap()
        .unwrap();
    memberships::remove_tx(&mut revocation, project.id, manager.id)
        .await
        .unwrap();

    let blocked_upload = tokio::spawn(entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&manager),
        Path(project.id),
        Json(audit_contract_upload_req(
            "revoked/file.json",
            &[("entry", "source")],
        )),
    ));
    audit_contract_wait_for_postgres_block(
        &state.db,
        blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &blocked_upload,
        "上传应在锁后重新读取最新权限",
    )
    .await;
    revocation.commit().await.unwrap();

    let denied = blocked_upload
        .await
        .unwrap()
        .expect_err_api("撤权提交后不得继续上传");
    assert_eq!(
        denied.into_response().status(),
        axum::http::StatusCode::FORBIDDEN
    );
    assert!(files::list_files(&state.db, project.id)
        .await
        .unwrap()
        .is_empty());

    memberships::upsert(&state.db, project.id, manager.id, "manager")
        .await
        .unwrap();
    let mut project_revocation = state.db.begin().await.unwrap();
    let project_blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *project_revocation)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut project_revocation, project.id)
        .await
        .unwrap()
        .unwrap();
    memberships::remove_tx(&mut project_revocation, project.id, manager.id)
        .await
        .unwrap();
    let denied_name = format!("Revoked manager update {}", manager.id);
    let blocked_project_update = tokio::spawn(projects_routes::update_project(
        State(state.clone()),
        audit_contract_current_user(&manager),
        Path(project.id),
        Json(projects_routes::UpdateProjectReq {
            name: Some(denied_name.clone()),
            description: None,
            visibility: None,
            source_langs: None,
            target_lang: None,
        }),
    ));
    audit_contract_wait_for_postgres_block(
        &state.db,
        project_blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &blocked_project_update,
        "项目更新应在锁后重新读取最新权限",
    )
    .await;
    project_revocation.commit().await.unwrap();
    assert_eq!(
        blocked_project_update
            .await
            .unwrap()
            .expect_err_api("撤权提交后不得更新项目")
            .into_response()
            .status(),
        axum::http::StatusCode::FORBIDDEN
    );
    assert_ne!(
        projects::find_by_id(&state.db, project.id)
            .await
            .unwrap()
            .unwrap()
            .name,
        denied_name
    );

    let target = audit_contract_create_user(&state.db, "audit-lock-revoke-target", None).await;
    memberships::upsert(&state.db, project.id, manager.id, "manager")
        .await
        .unwrap();
    let mut member_revocation = state.db.begin().await.unwrap();
    let member_blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *member_revocation)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut member_revocation, project.id)
        .await
        .unwrap()
        .unwrap();
    memberships::remove_tx(&mut member_revocation, project.id, manager.id)
        .await
        .unwrap();
    let blocked_member_add = tokio::spawn(projects_routes::add_member(
        State(state.clone()),
        audit_contract_current_user(&manager),
        Path(project.id),
        Json(projects_routes::AddMemberReq {
            username: target.username.clone(),
            role: "translator".to_string(),
        }),
    ));
    audit_contract_wait_for_postgres_block(
        &state.db,
        member_blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &blocked_member_add,
        "成员添加应在锁后重新读取最新权限",
    )
    .await;
    member_revocation.commit().await.unwrap();
    assert_eq!(
        blocked_member_add
            .await
            .unwrap()
            .expect_err_api("撤权提交后不得添加成员")
            .into_response()
            .status(),
        axum::http::StatusCode::FORBIDDEN
    );
    assert!(memberships::find_role(&state.db, project.id, target.id)
        .await
        .unwrap()
        .is_none());

    let Json(uploaded) = entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(audit_contract_upload_req(
            "revoked/entry.json",
            &[("entry", "source")],
        )),
    )
    .await
    .expect_api("拥有者创建撤权词条夹具");
    let entry: prts_db::models::Entry = sqlx::query_as(
        "SELECT * FROM entries WHERE project_id = $1 AND file_id = $2 AND key = 'entry'",
    )
    .bind(project.id)
    .bind(uploaded.file_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    memberships::upsert(&state.db, project.id, manager.id, "manager")
        .await
        .unwrap();
    let mut entry_revocation = state.db.begin().await.unwrap();
    let entry_blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *entry_revocation)
        .await
        .unwrap();
    projects::find_by_id_for_update_tx(&mut entry_revocation, project.id)
        .await
        .unwrap()
        .unwrap();
    memberships::remove_tx(&mut entry_revocation, project.id, manager.id)
        .await
        .unwrap();
    let blocked_entry_update = tokio::spawn(entries_routes::update_entry(
        State(state.clone()),
        audit_contract_current_user(&manager),
        Path((project.id, entry.id)),
        Json(entries_routes::UpdateEntryReq {
            translation: "revoked translation".to_string(),
            state: "translated".to_string(),
            version: entry.version,
        }),
    ));
    audit_contract_wait_for_postgres_block(
        &state.db,
        entry_blocker_pid,
        "SELECT * FROM projects WHERE id = $1 FOR UPDATE",
        &blocked_entry_update,
        "词条更新应在锁后重新读取最新权限",
    )
    .await;
    entry_revocation.commit().await.unwrap();
    assert_eq!(
        blocked_entry_update
            .await
            .unwrap()
            .expect_err_api("撤权提交后不得更新词条")
            .into_response()
            .status(),
        axum::http::StatusCode::FORBIDDEN
    );
    assert_ne!(
        entries::get(&state.db, project.id, entry.id)
            .await
            .unwrap()
            .unwrap()
            .translation,
        "revoked translation"
    );

    projects::delete(&state.db, project.id).await.unwrap();
    sqlx::query("DELETE FROM users WHERE id = ANY($1::BIGINT[])")
        .bind(&[owner.id, manager.id, target.id][..])
        .execute(&state.db)
        .await
        .unwrap();
}

#[test]
fn audit_contract_unavailable_error_is_stable_and_bilingual() {
    use prts_common::i18n::{localize, Locale};

    assert_eq!(
        localize("AUDIT_UNAVAILABLE", Locale::ZhCn),
        "审计服务暂不可用"
    );
    assert_eq!(
        localize("AUDIT_UNAVAILABLE", Locale::En),
        "Audit service unavailable"
    );
}

/// 规范化 payload key 后检查全部秘密与正文禁区。
fn audit_contract_key_is_sensitive(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "token",
        "accesstoken",
        "refreshtoken",
        "authorization",
        "password",
        "passwordhash",
        "hash",
        "secret",
        "clientsecret",
        "apikey",
        "apikeyhash",
        "code",
        "oauthcode",
        "verifier",
        "codeverifier",
        "challengeanswer",
        "raw",
        "rawbody",
        "body",
        "original",
        "translation",
        "sourcetext",
        "content",
        "context",
        "text",
    ]
    .iter()
    .any(|forbidden| normalized == *forbidden)
}

/// 真实 handler 测试共用的应用状态；DB/Redis 都使用 CI 提供的真实服务。
async fn audit_contract_state() -> state::AppState {
    ensure_migrated().await;
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL 未设置");
    let db = prts_db::connect_postgres(&database_url, 8)
        .await
        .expect("连接 runtime PostgreSQL");
    audit_contract_state_with_db(db).await
}

/// 每条新连接都启用 test-only GUC；只有该 pool 的 audit INSERT 会被 fixture trigger 拒绝。
async fn audit_contract_failing_audit_db() -> prts_db::Db {
    ensure_migrated().await;
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL 未设置");
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                sqlx::query("SELECT set_config('prts.test_fail_audit', 'on', false)")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&database_url)
        .await
        .expect("连接审计故障专用 runtime pool");
    prts_db::verify_runtime_role(&db, &runtime_role())
        .await
        .expect("故障 pool 仍必须使用 runtime role");
    db
}

/// 创建隔离 Redis ACL 用户，仅拒绝指定命令；用于真实 commit/Redis 边界故障。
async fn audit_contract_restricted_redis(
    admin_cache: &prts_db::Cache,
    denied_command: &str,
) -> (prts_db::Cache, String) {
    let suffix = audit_jobs_unique("acl")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    let username = format!("audit{suffix}");
    let password = format!("Pass{suffix}");
    let mut admin = admin_cache.clone();
    let _: () = redis::cmd("ACL")
        .arg("SETUSER")
        .arg(&username)
        .arg("reset")
        .arg("on")
        .arg(format!(">{password}"))
        .arg("~*")
        .arg("+@all")
        .arg(format!("-{denied_command}"))
        .query_async(&mut admin)
        .await
        .expect("创建隔离 Redis ACL 用户");
    let base = std::env::var("PRTS__REDIS__URL").expect("PRTS__REDIS__URL 未设置");
    let authority = base
        .strip_prefix("redis://")
        .expect("CI Redis URL 使用 redis://");
    let url = format!("redis://{username}:{password}@{authority}");
    let cache = prts_db::connect_redis(&url)
        .await
        .expect("连接受限 Redis ACL 用户");
    (cache, username)
}

async fn audit_contract_delete_redis_acl_user(admin_cache: &prts_db::Cache, username: &str) {
    let mut admin = admin_cache.clone();
    let _: i64 = redis::cmd("ACL")
        .arg("DELUSER")
        .arg(username)
        .query_async(&mut admin)
        .await
        .expect("清理隔离 Redis ACL 用户");
}

async fn audit_contract_state_with_db(db: prts_db::Db) -> state::AppState {
    use std::sync::Arc;

    let redis_url = std::env::var("PRTS__REDIS__URL").expect("PRTS__REDIS__URL 未设置");
    let cache = prts_db::connect_redis(&redis_url)
        .await
        .expect("连接真实 Redis");
    let realtime = prts_realtime::Hub::new(&redis_url)
        .await
        .expect("构造真实 realtime hub");
    let settings = Arc::new(
        prts_common::config::Settings::load_from("__audit_contract_missing_config__")
            .expect("加载测试配置"),
    );
    let (job_worker, _job_worker_handle) = job_worker::spawn(
        db.clone(),
        jobs::JobRegistry::new(Vec::new()),
        Arc::new(job_worker::NoPendingDeletions),
    );
    let search_config = prts_db::search_settings::get(&db)
        .await
        .expect("加载当前搜索运行时配置");
    let search_rt = Arc::new(tokio::sync::RwLock::new(search_config));
    let (search_settings_updater, _search_settings_worker) =
        search_settings_worker::spawn(db.clone(), search_rt.clone());
    state::AppState {
        db,
        cache,
        settings,
        media: Arc::new(media::LocalMediaStore::new(
            std::env::temp_dir().join(format!("prts-audit-media-{}", audit_jobs_unique("state"))),
        )),
        zoot: Arc::new(None),
        realtime,
        embedder: Arc::new(None),
        search_rt,
        search_settings_updater,
        job_worker,
    }
}

async fn audit_contract_create_user(
    db: &prts_db::Db,
    prefix: &str,
    platform_role: Option<&str>,
) -> prts_db::models::User {
    let username = format!("u-{}", audit_jobs_unique(prefix));
    let hash =
        prts_auth::password::hash_password("audit-contract-password").expect("测试密码可哈希");
    let user = users::create_password_user(db, &username, None, &hash, "active")
        .await
        .expect("创建隔离测试用户");
    if let Some(role) = platform_role {
        users::set_platform_role(db, user.id, Some(role))
            .await
            .expect("设置测试平台角色");
        users::find_by_id(db, user.id)
            .await
            .unwrap()
            .expect("测试用户仍存在")
    } else {
        user
    }
}

fn audit_contract_upload_req(
    path: impl Into<String>,
    entries: &[(&str, &str)],
) -> entries_routes::UploadReq {
    entries_routes::UploadReq {
        path: path.into(),
        entries: entries
            .iter()
            .map(|(key, original)| entries_routes::UploadEntryDto {
                key: (*key).to_string(),
                original: serde_json::json!({"en": original}),
                context: None,
                translation: None,
                state: None,
            })
            .collect(),
    }
}

fn audit_contract_current_user(user: &prts_db::models::User) -> auth::CurrentUser {
    auth::CurrentUser {
        id: user.id,
        platform_role: user
            .platform_role
            .as_deref()
            .and_then(prts_core::PlatformRole::parse),
    }
}

async fn audit_contract_wait_for_postgres_block<T>(
    db: &prts_db::Db,
    blocker_pid: i32,
    expected_query: &str,
    task: &tokio::task::JoinHandle<T>,
    message: &str,
) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let blocked_pid: Option<i32> = sqlx::query_scalar(
                "SELECT activity.pid
                 FROM pg_stat_activity AS activity
                 WHERE activity.datname = current_database()
                   AND activity.pid <> $1
                   AND activity.wait_event_type = 'Lock'
                   AND $1 = ANY(pg_blocking_pids(activity.pid))
                   AND activity.query LIKE '%' || $2 || '%'
                 ORDER BY activity.query_start
                 LIMIT 1",
            )
            .bind(blocker_pid)
            .bind(expected_query)
            .fetch_optional(db)
            .await
            .unwrap();
            if blocked_pid.is_some() {
                assert!(!task.is_finished(), "{message}");
                break;
            }
            assert!(!task.is_finished(), "{message}: mutation 提前结束");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{message}: 未观察到 PostgreSQL 阻塞证据"));
}

/// `ApiError` 刻意不暴露内部细节且没有 `Debug`；测试只需报告调用边界名称。
trait AuditContractApiResultExt<T> {
    fn expect_api(self, message: &str) -> T;
    fn expect_err_api(self, message: &str) -> error::ApiError;
}

impl<T> AuditContractApiResultExt<T> for Result<T, error::ApiError> {
    fn expect_api(self, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(_) => panic!("{message}"),
        }
    }

    fn expect_err_api(self, message: &str) -> error::ApiError {
        match self {
            Ok(_) => panic!("{message}"),
            Err(error) => error,
        }
    }
}

#[derive(Debug)]
struct ObservedAudit {
    action: String,
    target_type: String,
    target_id: String,
    project_id: Option<i64>,
    payload: serde_json::Value,
}

async fn audit_contract_rows_for_actor(db: &prts_db::Db, actor_id: i64) -> Vec<ObservedAudit> {
    sqlx::query_as::<_, (String, String, String, Option<i64>, serde_json::Value)>(
        "SELECT action, target_type, target_id, project_id_snapshot, payload
         FROM audit_log WHERE actor_id = $1 ORDER BY id",
    )
    .bind(actor_id)
    .fetch_all(db)
    .await
    .expect("读取测试 actor 的审计")
    .into_iter()
    .map(
        |(action, target_type, target_id, project_id, payload)| ObservedAudit {
            action,
            target_type,
            target_id,
            project_id,
            payload,
        },
    )
    .collect()
}

async fn audit_contract_rows_for_action_target(
    db: &prts_db::Db,
    action: &str,
    target_id: &str,
) -> Vec<ObservedAudit> {
    sqlx::query_as::<_, (String, String, String, Option<i64>, serde_json::Value)>(
        "SELECT action, target_type, target_id, project_id_snapshot, payload
         FROM audit_log WHERE action = $1 AND target_id = $2 ORDER BY id",
    )
    .bind(action)
    .bind(target_id)
    .fetch_all(db)
    .await
    .expect("读取 action/target 审计")
    .into_iter()
    .map(
        |(action, target_type, target_id, project_id, payload)| ObservedAudit {
            action,
            target_type,
            target_id,
            project_id,
            payload,
        },
    )
    .collect()
}

fn audit_contract_assert_actions(rows: &[ObservedAudit], expected: &[&str]) {
    let mut expected_counts = std::collections::HashMap::new();
    for action in expected {
        *expected_counts.entry(*action).or_insert(0_usize) += 1;
    }
    for (action, expected_count) in expected_counts {
        let observed_count = rows.iter().filter(|row| row.action == action).count();
        assert_eq!(
            observed_count, expected_count,
            "审计 action {action} 数量不符"
        );
    }
}

fn audit_contract_assert_exact_targets(rows: &[ObservedAudit], expected: &[(&str, String)]) {
    for (action, target_id) in expected {
        let matching: Vec<_> = rows
            .iter()
            .filter(|row| row.action == *action && row.target_id == *target_id)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "action {action} 必须恰好指向 target {target_id}"
        );
    }
}

#[test]
fn audit_contract_exact_targets_reject_numeric_but_wrong_ids() {
    let rows = vec![ObservedAudit {
        action: "message.sent".to_string(),
        target_type: "message".to_string(),
        target_id: "41".to_string(),
        project_id: None,
        payload: serde_json::json!({}),
    }];
    let rejected = std::panic::catch_unwind(|| {
        audit_contract_assert_exact_targets(&rows, &[("message.sent", "42".to_string())]);
    });
    assert!(rejected.is_err(), "数值合法但错误的 target id 必须被拒绝");
}

fn audit_contract_assert_payloads_are_typed_and_redacted(
    rows: &[ObservedAudit],
    forbidden_values: &[&str],
) {
    for row in rows {
        let inventory = AUDITED_ENTRYPOINTS
            .iter()
            .find(|entry| entry.action == row.action)
            .unwrap_or_else(|| panic!("action {} 没有 typed inventory", row.action));
        let object = row
            .payload
            .as_object()
            .unwrap_or_else(|| panic!("{} payload 必须是 object", row.action));
        for key in object.keys() {
            assert!(
                inventory.allowed_payload_keys.contains(&key.as_str()),
                "{} payload 出现未 allowlist 字段 {key}",
                row.action
            );
        }
        let contract = AUDIT_ACTION_CONTRACTS
            .iter()
            .find(|contract| contract.action == row.action)
            .unwrap_or_else(|| panic!("action {} 没有目标合同", row.action));
        assert_eq!(
            row.target_type, contract.target_type,
            "{} target_type",
            row.action
        );
        match contract.target_id_policy {
            TargetIdPolicy::Numeric => {
                row.target_id
                    .parse::<i64>()
                    .unwrap_or_else(|_| panic!("{} target_id 必须为整数", row.action));
            }
            TargetIdPolicy::CompositeNumeric => {
                let parts: Vec<_> = row.target_id.split(':').collect();
                assert_eq!(parts.len(), 2, "{} target_id 必须是 a:b", row.action);
                for part in parts {
                    part.parse::<i64>()
                        .unwrap_or_else(|_| panic!("{} composite target 必须为整数", row.action));
                }
            }
            TargetIdPolicy::Constant(expected) => assert_eq!(row.target_id, expected),
            TargetIdPolicy::OpaqueNonEmpty => assert!(!row.target_id.is_empty()),
        }
        match contract.project_snapshot_policy {
            ProjectSnapshotPolicy::None => assert!(row.project_id.is_none()),
            ProjectSnapshotPolicy::SameAsNumericTarget => assert_eq!(
                row.project_id,
                Some(
                    row.target_id
                        .parse::<i64>()
                        .expect("project target 必须为整数")
                )
            ),
            ProjectSnapshotPolicy::Required => assert!(row.project_id.is_some()),
        }
        audit_contract_assert_json_has_no_sensitive_keys(&row.payload);
        let serialized = row.payload.to_string();
        for forbidden in forbidden_values {
            assert!(
                !serialized.contains(forbidden),
                "{} payload 泄露秘密或正文 marker {forbidden}",
                row.action
            );
        }
    }
}

fn audit_contract_assert_json_has_no_sensitive_keys(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                assert!(
                    !audit_contract_key_is_sensitive(key),
                    "审计 payload 出现秘密/正文键 {key}"
                );
                audit_contract_assert_json_has_no_sensitive_keys(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                audit_contract_assert_json_has_no_sensitive_keys(item);
            }
        }
        _ => {}
    }
}

async fn audit_contract_assert_unavailable(error: error::ApiError) {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("读取错误响应体");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("错误响应为 JSON");
    assert_eq!(body["code"], "AUDIT_UNAVAILABLE");
    assert_eq!(body["message"], "审计服务暂不可用");
}

/// 认证注册把用户、active session、审计与 Redis outbox 作为一个 DB 提交；审计失败不返回 token。
#[tokio::test]
async fn audit_contract_registration_rolls_back_user_and_token_issuance_when_audit_fails() {
    use axum::extract::State;
    use axum::Json;

    let settings_db = pool().await;
    settings::set(
        &settings_db,
        appsettings::AUTH_OAUTH_ONLY,
        &serde_json::json!(false),
        None,
    )
    .await
    .unwrap();
    settings::set(
        &settings_db,
        appsettings::AUTH_REGISTRATION_OPEN,
        &serde_json::json!(true),
        None,
    )
    .await
    .unwrap();
    let failing_db = audit_contract_failing_audit_db().await;
    let state = audit_contract_state_with_db(failing_db).await;
    let username = format!("u-{}", audit_jobs_unique("reg"));
    let password_marker = "REGISTER_PASSWORD_MUST_NEVER_ENTER_AUDIT";
    let result = auth_routes::register(
        State(state.clone()),
        Json(auth_routes::RegisterReq {
            username: username.clone(),
            email: Some(format!("{username}@example.invalid")),
            password: password_marker.to_string(),
        }),
    )
    .await;
    let error = match result {
        Ok(_) => panic!("审计失败时注册不得激活用户或返回 token"),
        Err(error) => error,
    };
    audit_contract_assert_unavailable(error).await;

    let observation_db = pool().await;
    assert!(
        users::find_by_username(&observation_db, &username)
            .await
            .unwrap()
            .is_none(),
        "审计失败必须回滚 password 用户落库"
    );
    let leaked: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM audit_log
             WHERE payload::TEXT LIKE '%' || $1 || '%'
         )",
    )
    .bind(password_marker)
    .fetch_one(&observation_db)
    .await
    .unwrap();
    assert!(!leaked, "失败审计也不得泄露 password");
    sqlx::query("DELETE FROM settings WHERE key = ANY($1::TEXT[])")
        .bind(
            &[
                appsettings::AUTH_OAUTH_ONLY,
                appsettings::AUTH_REGISTRATION_OPEN,
            ][..],
        )
        .execute(&settings_db)
        .await
        .unwrap();
}

/// 普通业务 mutation 同样 fail closed；项目与 owner membership 不能部分提交。
#[tokio::test]
async fn audit_contract_project_creation_rolls_back_business_rows_when_audit_fails() {
    use axum::extract::State;
    use axum::Json;

    let failing_db = audit_contract_failing_audit_db().await;
    let state = audit_contract_state_with_db(failing_db).await;
    let actor =
        audit_contract_create_user(&state.db, "audit-project-rollback", Some("maintainer")).await;
    let slug = format!("audit-project-rollback-{}", actor.id);
    let result = projects_routes::create_project(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(projects_routes::CreateProjectReq {
            name: "Audit rollback project".to_string(),
            slug: Some(slug.clone()),
            description: Some("BUSINESS_BODY_MUST_ROLL_BACK".to_string()),
            visibility: Some("private".to_string()),
            source_langs: vec!["en".to_string()],
            primary_source_lang: None,
            target_lang: "zh-Hans".to_string(),
        }),
    )
    .await;
    let error = match result {
        Ok(_) => panic!("审计失败时项目 handler 不得返回成功"),
        Err(error) => error,
    };
    audit_contract_assert_unavailable(error).await;

    let observation_db = pool().await;
    assert!(
        projects::find_by_slug(&observation_db, &slug)
            .await
            .unwrap()
            .is_none(),
        "project 与 owner membership 必须随 audit 一起回滚"
    );
}

/// 成功登录必须先提交 DB-authoritative active session、审计与 cache-populate outbox 才返回 raw token。
#[tokio::test]
async fn audit_contract_login_commits_active_session_audit_and_outbox_before_returning_tokens() {
    use axum::extract::State;
    use axum::Json;

    let state = audit_contract_state().await;
    let username = format!("u-{}", audit_jobs_unique("audit-login-success"));
    let password_marker = "LOGIN_PASSWORD_MUST_NOT_ENTER_AUDIT";
    let password_hash = prts_auth::password::hash_password(password_marker).unwrap();
    let user = users::create_password_user(&state.db, &username, None, &password_hash, "active")
        .await
        .unwrap();

    let Json(tokens) = auth_routes::login(
        State(state.clone()),
        Json(auth_routes::LoginReq {
            username,
            password: password_marker.to_string(),
        }),
    )
    .await
    .expect_api("正确密码登录成功");

    let refresh_hash = prts_auth::token::sha256_hex(&tokens.refresh_token);
    let sessions: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, state, session_handle, family_handle
         FROM auth_sessions WHERE refresh_token_hash = $1",
    )
    .bind(&refresh_hash)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(sessions.len(), 1, "返回 refresh 前必须持久化唯一 session");
    assert_eq!(sessions[0].1, "active");
    assert!(sessions[0].2.len() >= 16);
    assert!(sessions[0].3.len() >= 16);

    let queued_populate: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM auth_session_intents
         WHERE session_id = $1 AND kind = 'redis_populate' AND state = 'queued'",
    )
    .bind(sessions[0].0)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(
        queued_populate, 1,
        "active session 必须带 durable cache outbox"
    );

    let rows = audit_contract_rows_for_actor(&state.db, user.id).await;
    audit_contract_assert_actions(&rows, &["auth.login_succeeded", "auth.token_issued"]);
    audit_contract_assert_exact_targets(
        &rows,
        &[
            ("auth.login_succeeded", user.id.to_string()),
            ("auth.token_issued", sessions[0].0.to_string()),
        ],
    );
    audit_contract_assert_payloads_are_typed_and_redacted(
        &rows,
        &[
            password_marker,
            &password_hash,
            &tokens.access_token,
            &tokens.refresh_token,
            &refresh_hash,
        ],
    );
}

/// 成功登录的 audit INSERT 失败时，既不能激活 session，也不能把 token 返回调用方。
#[tokio::test]
async fn audit_contract_login_returns_503_without_active_session_when_audit_fails() {
    use axum::extract::State;
    use axum::Json;

    let failing_db = audit_contract_failing_audit_db().await;
    let state = audit_contract_state_with_db(failing_db).await;
    let username = format!("u-{}", audit_jobs_unique("audit-login-fail"));
    let password = "valid-login-password";
    let password_hash = prts_auth::password::hash_password(password).unwrap();
    let user = users::create_password_user(&state.db, &username, None, &password_hash, "active")
        .await
        .unwrap();
    let result = auth_routes::login(
        State(state.clone()),
        Json(auth_routes::LoginReq {
            username,
            password: password.to_string(),
        }),
    )
    .await;
    let error = match result {
        Ok(_) => panic!("审计失败时成功登录不得返回 token"),
        Err(error) => error,
    };
    audit_contract_assert_unavailable(error).await;
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM auth_sessions WHERE user_id = $1 AND state = 'active'",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(active, 0);
}

/// 失败凭证也同步写脱敏 audit；只有审计本身失败时才把原 401 收敛为通用 503。
#[tokio::test]
async fn audit_contract_failed_password_login_is_audited_and_audit_failure_hides_auth_result() {
    use axum::extract::State;
    use axum::response::IntoResponse;
    use axum::Json;

    let state = audit_contract_state().await;
    let username = format!("u-{}", audit_jobs_unique("audit-login-denied"));
    let password_hash = prts_auth::password::hash_password("correct-password").unwrap();
    let user = users::create_password_user(&state.db, &username, None, &password_hash, "active")
        .await
        .unwrap();
    let wrong_password = "WRONG_PASSWORD_MUST_NOT_ENTER_AUDIT";
    let denied = auth_routes::login(
        State(state.clone()),
        Json(auth_routes::LoginReq {
            username: username.clone(),
            password: wrong_password.to_string(),
        }),
    )
    .await
    .expect_err_api("错误密码必须拒绝");
    assert_eq!(
        denied.into_response().status(),
        axum::http::StatusCode::UNAUTHORIZED
    );
    let rows =
        audit_contract_rows_for_action_target(&state.db, "auth.login_failed", &user.id.to_string())
            .await;
    assert_eq!(rows.len(), 1, "每次失败认证必须同步审计一次");
    audit_contract_assert_payloads_are_typed_and_redacted(&rows, &[wrong_password, &password_hash]);

    let failing_db = audit_contract_failing_audit_db().await;
    let failing_state = audit_contract_state_with_db(failing_db).await;
    let denied = auth_routes::login(
        State(failing_state),
        Json(auth_routes::LoginReq {
            username,
            password: wrong_password.to_string(),
        }),
    )
    .await;
    let error = match denied {
        Ok(_) => panic!("错误密码不得认证成功"),
        Err(error) => error,
    };
    audit_contract_assert_unavailable(error).await;
}

async fn audit_contract_login_fixture(
    state: &state::AppState,
    prefix: &str,
) -> (prts_db::models::User, auth_routes::TokenResponse) {
    use axum::extract::State;
    use axum::Json;

    let username = format!("u-{}", audit_jobs_unique(prefix));
    let password = "fixture-login-password";
    let hash = prts_auth::password::hash_password(password).unwrap();
    let user = users::create_password_user(&state.db, &username, None, &hash, "active")
        .await
        .unwrap();
    let Json(tokens) = auth_routes::login(
        State(state.clone()),
        Json(auth_routes::LoginReq {
            username,
            password: password.to_string(),
        }),
    )
    .await
    .expect_api("fixture 登录成功");
    (user, tokens)
}

/// refresh rotation 必须在一个事务中撤销 predecessor、激活 successor，并写 audit/outbox。
#[tokio::test]
async fn audit_contract_refresh_rotation_has_one_active_successor_and_redacted_outbox() {
    use axum::extract::State;
    use axum::Json;

    let state = audit_contract_state().await;
    let (user, first) = audit_contract_login_fixture(&state, "audit-refresh").await;
    let old_hash = prts_auth::token::sha256_hex(&first.refresh_token);
    let Json(second) = auth_routes::refresh(
        State(state.clone()),
        Json(auth_routes::RefreshReq {
            refresh_token: first.refresh_token.clone(),
        }),
    )
    .await
    .expect_api("refresh rotation 成功");
    let new_hash = prts_auth::token::sha256_hex(&second.refresh_token);

    let sessions: Vec<(i64, String, String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT id, refresh_token_hash, state, predecessor_id, successor_id
         FROM auth_sessions WHERE user_id = $1 ORDER BY id",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(sessions.len(), 2, "rotation 只建立一个 successor");
    let predecessor = sessions
        .iter()
        .find(|row| row.1 == old_hash)
        .expect("predecessor hash 存在");
    let successor = sessions
        .iter()
        .find(|row| row.1 == new_hash)
        .expect("successor hash 存在");
    assert_eq!(predecessor.2, "revoked");
    assert_eq!(successor.2, "active");
    assert_eq!(predecessor.4, Some(successor.0));
    assert_eq!(successor.3, Some(predecessor.0));
    assert_eq!(
        sessions.iter().filter(|row| row.2 == "active").count(),
        1,
        "同 family 不得双 active"
    );

    let intents: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT kind, payload FROM auth_session_intents
         WHERE session_id = ANY($1::BIGINT[]) ORDER BY id",
    )
    .bind(&[predecessor.0, successor.0][..])
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert!(intents.iter().any(|intent| intent.0 == "redis_invalidate"));
    assert!(intents.iter().any(|intent| intent.0 == "redis_populate"));
    let serialized_intents = serde_json::to_string(&intents).unwrap();
    for forbidden in [
        first.refresh_token.as_str(),
        second.refresh_token.as_str(),
        first.access_token.as_str(),
        second.access_token.as_str(),
        old_hash.as_str(),
        new_hash.as_str(),
    ] {
        assert!(!serialized_intents.contains(forbidden));
    }

    let rows = audit_contract_rows_for_actor(&state.db, user.id).await;
    audit_contract_assert_actions(
        &rows,
        &[
            "auth.refresh_rotated",
            "auth.token_issued",
            "auth.token_issued",
        ],
    );
    audit_contract_assert_exact_targets(
        &rows,
        &[
            ("auth.refresh_rotated", successor.0.to_string()),
            ("auth.token_issued", successor.0.to_string()),
        ],
    );
    audit_contract_assert_payloads_are_typed_and_redacted(
        &rows,
        &[
            &first.refresh_token,
            &second.refresh_token,
            &first.access_token,
            &second.access_token,
            &old_hash,
            &new_hash,
        ],
    );
}

/// Refresh 必须在 rotation 事务内取得对外用户快照，提交后不得再依赖可失败的 DB 查询。
#[tokio::test]
async fn audit_contract_refresh_returns_transactional_secret_free_user_snapshot() {
    let state = audit_contract_state().await;
    let (user, first) = audit_contract_login_fixture(&state, "audit-refresh-snapshot").await;

    let refreshed = auth::session::refresh(&state, &first.refresh_token)
        .await
        .expect("refresh rotation 成功");

    assert_eq!(refreshed.user.id, user.id);
    assert_eq!(refreshed.user.username, user.username);
    assert!(!refreshed.tokens.access_token.is_empty());
    let serialized = serde_json::to_value(&refreshed.user).expect("用户快照可序列化");
    let object = serialized.as_object().expect("用户快照必须是 object");
    assert!(!object.contains_key("password_hash"));
    assert!(!object.contains_key("email_verified"));
    assert!(!object.contains_key("status"));
}

/// 并发复用同一 predecessor 只能一个 refresh 成功，且 DB 最终只有一个 active successor。
#[tokio::test]
async fn audit_contract_concurrent_refresh_never_creates_two_active_successors() {
    use axum::extract::State;
    use axum::Json;

    let state = audit_contract_state().await;
    let (user, first) = audit_contract_login_fixture(&state, "audit-refresh-race").await;
    let raw = first.refresh_token;
    let (left, right) = tokio::join!(
        auth_routes::refresh(
            State(state.clone()),
            Json(auth_routes::RefreshReq {
                refresh_token: raw.clone(),
            }),
        ),
        auth_routes::refresh(
            State(state.clone()),
            Json(auth_routes::RefreshReq { refresh_token: raw }),
        )
    );
    assert_eq!(
        usize::from(left.is_ok()) + usize::from(right.is_ok()),
        1,
        "同一个 refresh predecessor 必须恰有一个 rotation 成功"
    );
    let (active, total): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE state = 'active'), COUNT(*)
         FROM auth_sessions WHERE user_id = $1",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(active, 1);
    assert_eq!(total, 2, "失败竞争者不得留下 pending/rotating 垃圾行");
}

/// rotation audit 失败时不得返回 successor，predecessor 必须仍是唯一 active。
#[tokio::test]
async fn audit_contract_refresh_audit_failure_keeps_predecessor_active_and_returns_503() {
    use axum::extract::State;
    use axum::Json;

    let normal_state = audit_contract_state().await;
    let (user, first) = audit_contract_login_fixture(&normal_state, "audit-refresh-fail").await;
    let old_hash = prts_auth::token::sha256_hex(&first.refresh_token);
    let failing_db = audit_contract_failing_audit_db().await;
    let failing_state = audit_contract_state_with_db(failing_db).await;
    let result = auth_routes::refresh(
        State(failing_state),
        Json(auth_routes::RefreshReq {
            refresh_token: first.refresh_token,
        }),
    )
    .await;
    let error = match result {
        Ok(_) => panic!("rotation audit 失败不得返回 successor token"),
        Err(error) => error,
    };
    audit_contract_assert_unavailable(error).await;

    let sessions: Vec<(String, String)> =
        sqlx::query_as("SELECT refresh_token_hash, state FROM auth_sessions WHERE user_id = $1")
            .bind(user.id)
            .fetch_all(&normal_state.db)
            .await
            .unwrap();
    assert_eq!(sessions, vec![(old_hash, "active".to_string())]);
}

/// 即使 Redis 仍残留旧 refresh，DB revoked 状态也必须否决认证。
#[tokio::test]
async fn audit_contract_stale_redis_refresh_cannot_bypass_revoked_db_session() {
    use axum::extract::State;
    use axum::response::IntoResponse;
    use axum::Json;

    let state = audit_contract_state().await;
    let (user, tokens) = audit_contract_login_fixture(&state, "audit-stale-redis").await;
    let refresh_hash =
        auth_sessions::RefreshTokenHash::parse(prts_auth::token::sha256_hex(&tokens.refresh_token))
            .unwrap();
    let mut tx = state.db.begin().await.unwrap();
    let session =
        match auth_sessions::lock_active_unexpired_by_refresh_hash_tx(&mut tx, &refresh_hash)
            .await
            .unwrap()
        {
            Some(session) => session,
            None => {
                // 让旧实现也能抵达 stale-cache 断言；GREEN 后会直接复用 login 创建的 session。
                let pending = auth_sessions::create_pending_tx(
                    &mut tx,
                    auth_sessions::NewAuthSession {
                        session_handle: format!("session-handle-{}", audit_jobs_unique("stale")),
                        family_handle: format!("family-handle-{}", audit_jobs_unique("stale")),
                        user_id: user.id,
                        refresh_token_hash: refresh_hash.clone(),
                        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                        predecessor_id: None,
                    },
                )
                .await
                .unwrap();
                auth_sessions::activate_pending_tx(&mut tx, pending.id)
                    .await
                    .unwrap()
                    .unwrap()
            }
        };
    auth_sessions::revoke_unexpired_tx(&mut tx, session.id)
        .await
        .unwrap()
        .expect("权威 session 可吊销");
    tx.commit().await.unwrap();

    // 故意不 DEL Redis，模拟 invalidate worker crash/stale cache。
    let denied = auth_routes::refresh(
        State(state),
        Json(auth_routes::RefreshReq {
            refresh_token: tokens.refresh_token,
        }),
    )
    .await
    .expect_err_api("revoked DB session 必须拒绝 stale Redis");
    assert_eq!(
        denied.into_response().status(),
        axum::http::StatusCode::UNAUTHORIZED
    );
}

/// 新签发 access JWT 必须绑定 DB session；logout/revoke 后不能继续靠 JWT 签名通过。
#[tokio::test]
async fn audit_contract_revoked_db_session_immediately_denies_bound_access_jwt() {
    use axum::extract::FromRequestParts;
    use axum::http::{header, Request};
    use axum::response::IntoResponse;

    let state = audit_contract_state().await;
    let (user, tokens) = audit_contract_login_fixture(&state, "audit-access-session-revoke").await;
    let claims = prts_auth::jwt::decode(&tokens.access_token, state.jwt_secret()).unwrap();
    let session_handle = claims.sid.expect("新签发 access JWT 必须携带 sid");
    let session = auth_sessions::find_active_unexpired_by_handle(&state.db, &session_handle)
        .await
        .unwrap()
        .expect("login 已提交 active DB session");
    assert_eq!(session.user_id, user.id);

    let mut tx = state.db.begin().await.unwrap();
    auth_sessions::revoke_unexpired_tx(&mut tx, session.id)
        .await
        .unwrap()
        .expect("active session 可立即吊销");
    tx.commit().await.unwrap();

    let (mut parts, _) = Request::builder()
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", tokens.access_token),
        )
        .body(())
        .unwrap()
        .into_parts();
    let denied = auth::CurrentUser::from_request_parts(&mut parts, &state)
        .await
        .expect_err_api("revoked DB session 必须否决仍未过期的绑定 access JWT");
    assert_eq!(
        denied.into_response().status(),
        axum::http::StatusCode::UNAUTHORIZED
    );
}

/// 已提供但无效的凭证必须先写脱敏 `auth.failed`；无凭证仍可按公开游客处理。
#[tokio::test]
async fn audit_contract_presented_invalid_credentials_are_audited_before_denial_or_guest() {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use std::collections::HashSet;
    use tower::ServiceExt;

    async fn protected(_user: auth::CurrentUser) -> StatusCode {
        StatusCode::OK
    }

    async fn public(auth::MaybeUser(user): auth::MaybeUser) -> StatusCode {
        if user.is_some() {
            StatusCode::OK
        } else {
            StatusCode::NO_CONTENT
        }
    }

    // 使用“任何 audit INSERT 都失败”的连接池证明真正无凭证路径根本不会触发审计；
    // 不读取全局计数，避免并行测试插入同一 action 造成竞态。
    let no_audit_state =
        audit_contract_state_with_db(audit_contract_failing_audit_db().await).await;
    let no_audit_app = Router::new()
        .route("/public", get(public))
        .layer(axum::middleware::from_fn(error::localize_audit_errors))
        .with_state(no_audit_state);
    let missing = no_audit_app
        .oneshot(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NO_CONTENT);

    let state = audit_contract_state().await;
    let app = Router::new()
        .route("/protected", get(protected))
        .route("/public", get(public))
        .layer(axum::middleware::from_fn(error::localize_audit_errors))
        .with_state(state.clone());
    let started_at = chrono::Utc::now();

    let (active_user, active_tokens) =
        audit_contract_login_fixture(&state, "audit-invalid-jwt").await;
    let base_claims =
        prts_auth::jwt::decode(&active_tokens.access_token, state.jwt_secret()).unwrap();
    let sid_none = prts_auth::jwt::encode(
        &prts_auth::jwt::Claims {
            sid: None,
            ..base_claims.clone()
        },
        state.jwt_secret(),
    );
    let expired = prts_auth::jwt::encode(
        &prts_auth::jwt::Claims {
            exp: chrono::Utc::now().timestamp() - 120,
            ..base_claims.clone()
        },
        state.jwt_secret(),
    );
    let other = audit_contract_create_user(&state.db, "audit-jwt-mismatch", None).await;
    let mismatch = prts_auth::jwt::encode(
        &prts_auth::jwt::Claims {
            sub: other.id,
            ..base_claims.clone()
        },
        state.jwt_secret(),
    );

    let (revoked_user, revoked_tokens) =
        audit_contract_login_fixture(&state, "audit-jwt-revoked").await;
    let revoked_claims =
        prts_auth::jwt::decode(&revoked_tokens.access_token, state.jwt_secret()).unwrap();
    let revoked_handle = revoked_claims.sid.clone().unwrap();
    let revoked_session =
        auth_sessions::find_active_unexpired_by_handle(&state.db, &revoked_handle)
            .await
            .unwrap()
            .unwrap();
    let mut tx = state.db.begin().await.unwrap();
    auth_sessions::revoke_unexpired_tx(&mut tx, revoked_session.id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();

    let (inactive_user, inactive_tokens) =
        audit_contract_login_fixture(&state, "audit-jwt-inactive").await;
    sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
        .bind(inactive_user.id)
        .execute(&state.db)
        .await
        .unwrap();

    let invalid_jwt = "INVALID_JWT_MUST_NOT_ENTER_AUDIT".to_string();
    let invalid_api_key = "prts_INVALID_API_KEY_MUST_NOT_ENTER_AUDIT".to_string();
    for (credential, expected_status) in [
        (invalid_jwt.clone(), StatusCode::UNAUTHORIZED),
        (invalid_api_key.clone(), StatusCode::UNAUTHORIZED),
        (sid_none.clone(), StatusCode::UNAUTHORIZED),
        (expired.clone(), StatusCode::UNAUTHORIZED),
        (mismatch.clone(), StatusCode::UNAUTHORIZED),
        (
            revoked_tokens.access_token.clone(),
            StatusCode::UNAUTHORIZED,
        ),
        (inactive_tokens.access_token.clone(), StatusCode::FORBIDDEN),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected_status);
    }

    let optional_invalid = app
        .oneshot(
            Request::builder()
                .uri("/public")
                .header(header::AUTHORIZATION, "Bearer prts_OPTIONAL_INVALID")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(optional_invalid.status(), StatusCode::NO_CONTENT);

    let rows = sqlx::query_as::<_, (String, String, String, Option<i64>, serde_json::Value)>(
        "SELECT action, target_type, target_id, project_id_snapshot, payload
         FROM audit_log
         WHERE action = 'auth.failed' AND created_at >= $1
         ORDER BY id",
    )
    .bind(started_at)
    .fetch_all(&state.db)
    .await
    .unwrap()
    .into_iter()
    .map(
        |(action, target_type, target_id, project_id, payload)| ObservedAudit {
            action,
            target_type,
            target_id,
            project_id,
            payload,
        },
    )
    .collect::<Vec<_>>();
    let observed: HashSet<_> = rows
        .iter()
        .map(|row| {
            (
                row.payload["method"].as_str().unwrap().to_string(),
                row.payload["reason_code"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    for expected in [
        ("jwt", "invalid_credential"),
        ("api_key", "invalid_credential"),
        ("jwt", "missing_session"),
        ("jwt", "token_expired"),
        ("jwt", "user_mismatch"),
        ("jwt", "session_inactive"),
        ("jwt", "account_inactive"),
    ] {
        assert!(
            observed.contains(&(expected.0.to_string(), expected.1.to_string())),
            "缺少失败认证审计 {expected:?}；实际={observed:?}"
        );
    }
    audit_contract_assert_payloads_are_typed_and_redacted(
        &rows,
        &[
            &invalid_jwt,
            &invalid_api_key,
            &sid_none,
            &expired,
            &mismatch,
            &revoked_tokens.access_token,
            &inactive_tokens.access_token,
        ],
    );
    assert!(rows
        .iter()
        .any(|row| row.target_id == active_user.id.to_string()));
    assert!(rows.iter().any(|row| row.target_id == other.id.to_string()));
    assert!(rows
        .iter()
        .any(|row| row.target_id == revoked_user.id.to_string()));
    assert!(rows
        .iter()
        .any(|row| row.target_id == inactive_user.id.to_string()));
}

/// 失败审计不可持久化时必须覆盖原 401/guest，并按请求语言返回通用 503。
#[tokio::test]
async fn audit_contract_presented_invalid_credential_audit_failure_is_bilingual_503() {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn protected(_user: auth::CurrentUser) -> StatusCode {
        StatusCode::OK
    }

    async fn public(auth::MaybeUser(user): auth::MaybeUser) -> StatusCode {
        if user.is_some() {
            StatusCode::OK
        } else {
            StatusCode::NO_CONTENT
        }
    }

    let failing_db = audit_contract_failing_audit_db().await;
    let state = audit_contract_state_with_db(failing_db).await;
    let user = audit_contract_create_user(&state.db, "audit-failed-sid-none", None).await;
    let now = chrono::Utc::now().timestamp();
    let sid_none = prts_auth::jwt::encode(
        &prts_auth::jwt::Claims {
            sub: user.id,
            iat: now,
            exp: now + 600,
            typ: "access".to_string(),
            sid: None,
        },
        state.jwt_secret(),
    );
    let app = Router::new()
        .route("/protected", get(protected))
        .route("/public", get(public))
        .layer(axum::middleware::from_fn(error::localize_audit_errors))
        .with_state(state);

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NO_CONTENT);

    for (credential, locale, expected_message) in [
        (
            "prts_INVALID_AUDIT_FAILURE".to_string(),
            "en",
            "Audit service unavailable",
        ),
        (
            "INVALID_JWT_AUDIT_FAILURE".to_string(),
            "zh-CN",
            "审计服务暂不可用",
        ),
        (sid_none, "en-US", "Audit service unavailable"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                    .header(header::ACCEPT_LANGUAGE, locale)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "AUDIT_UNAVAILABLE");
        assert_eq!(body["message"], expected_message);
    }
}

/// DB commit 已成功时，Redis populate 失败由 outbox 重放，不能撤销 session 或吞掉 raw token。
#[tokio::test]
async fn audit_contract_token_issuance_survives_redis_populate_failure_via_durable_outbox() {
    use axum::extract::State;
    use axum::Json;

    let mut state = audit_contract_state().await;
    let admin_cache = state.cache.clone();
    let (restricted_cache, acl_user) = audit_contract_restricted_redis(&admin_cache, "set").await;
    state.cache = restricted_cache;
    let username = format!("u-{}", audit_jobs_unique("audit-redis-set-fail"));
    let password = "redis-set-failure-login";
    let hash = prts_auth::password::hash_password(password).unwrap();
    let user = users::create_password_user(&state.db, &username, None, &hash, "active")
        .await
        .unwrap();
    let result = auth_routes::login(
        State(state.clone()),
        Json(auth_routes::LoginReq {
            username,
            password: password.to_string(),
        }),
    )
    .await;
    audit_contract_delete_redis_acl_user(&admin_cache, &acl_user).await;
    let Json(tokens) = result.expect_api("Redis populate 失败不应推翻已提交的 token issuance");
    assert!(!tokens.access_token.is_empty());
    assert!(!tokens.refresh_token.is_empty());

    let (active, queued): (i64, i64) = sqlx::query_as(
        "SELECT
             (SELECT COUNT(*) FROM auth_sessions
              WHERE user_id = $1 AND state = 'active'),
             (SELECT COUNT(*) FROM auth_session_intents AS intent
              JOIN auth_sessions AS session ON session.id = intent.session_id
              WHERE session.user_id = $1 AND intent.kind = 'redis_populate'
                AND intent.state = 'queued')",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!((active, queued), (1, 1));
}

/// Redis 暂时不可用时，同一 intent 必须保持 queued 并按有界退避重试，不进入 failed 扫描恢复。
#[tokio::test]
async fn audit_contract_transient_auth_outbox_requeues_same_intent_until_redis_returns() {
    let state = audit_contract_state().await;
    let user = audit_contract_create_user(&state.db, "audit-outbox-recovery", None).await;
    let raw_refresh = "OUTBOX_RAW_REFRESH_MUST_NOT_ENTER_PAYLOAD";
    let refresh_hash = prts_auth::token::sha256_hex(raw_refresh);
    let session_handle = format!("outbox-session-{}", audit_jobs_unique("recover"));
    let mut tx = state.db.begin().await.unwrap();
    let pending = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: session_handle.clone(),
            family_handle: audit_jobs_unique("outbox-family"),
            user_id: user.id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(refresh_hash.clone())
                .unwrap(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let active = auth_sessions::activate_pending_tx(&mut tx, pending.id)
        .await
        .unwrap()
        .unwrap();
    let intent = auth_sessions::enqueue_intent_tx(
        &mut tx,
        active.id,
        auth_sessions::AuthIntentPayload::RedisPopulate {
            session_handle: session_handle.clone(),
            expires_at: active.expires_at,
        },
        i32::MAX,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let admin_cache = state.cache.clone();
    let (restricted_cache, acl_user) = audit_contract_restricted_redis(&admin_cache, "set").await;
    assert_eq!(
        auth::session::process_one_outbox_intent(
            &state.db,
            &restricted_cache,
            "outbox-recovery-worker-1",
            Some(active.id),
        )
        .await
        .unwrap(),
        auth::session::OutboxProcessOutcome::Rescheduled
    );
    audit_contract_delete_redis_acl_user(&admin_cache, &acl_user).await;

    let queued: (
        i64,
        String,
        i32,
        chrono::DateTime<chrono::Utc>,
        Option<String>,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT id, state, attempts, run_after, last_error_code, payload
             FROM auth_session_intents WHERE id = $1",
    )
    .bind(intent.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(queued.0, intent.id);
    assert_eq!(queued.1, "queued");
    assert_eq!(queued.2, 1);
    assert!(
        queued.3 > chrono::Utc::now(),
        "暂时失败后必须持久化有界退避"
    );
    assert_eq!(queued.4.as_deref(), Some("redis_unavailable"));
    let serialized_payload = queued.5.to_string();
    assert!(!serialized_payload.contains(raw_refresh));
    assert!(!serialized_payload.contains(&refresh_hash));

    assert!(
        auth::session::process_one_outbox_intent(
            &state.db,
            &admin_cache,
            "outbox-recovery-worker-2",
            Some(active.id),
        )
        .await
        .unwrap()
            == auth::session::OutboxProcessOutcome::Idle,
        "cooldown 未到不得 busy-loop 重领"
    );
    sqlx::query(
        "UPDATE auth_session_intents SET run_after = now() - interval '1 second' WHERE id = $1",
    )
    .bind(intent.id)
    .execute(&state.db)
    .await
    .unwrap();
    assert_eq!(
        auth::session::process_one_outbox_intent(
            &state.db,
            &admin_cache,
            "outbox-recovery-worker-3",
            Some(active.id),
        )
        .await
        .unwrap(),
        auth::session::OutboxProcessOutcome::Completed
    );

    let recovered: (i64, String, i32, Option<String>) = sqlx::query_as(
        "SELECT id, state, attempts, last_error_code
         FROM auth_session_intents WHERE id = $1",
    )
    .bind(intent.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(recovered, (intent.id, "succeeded".to_string(), 2, None));
    let mut cache = admin_cache.clone();
    let cached_user: Option<i64> = redis::cmd("GET")
        .arg(format!("auth_session:{session_handle}"))
        .query_async(&mut cache)
        .await
        .unwrap();
    assert_eq!(cached_user, Some(user.id));
    let _: i64 = redis::cmd("DEL")
        .arg(format!("auth_session:{session_handle}"))
        .query_async(&mut cache)
        .await
        .unwrap();
}

/// Redis 在最后一次预算内仍失败时，intent 必须进入 failed，不能永久留在 queued。
#[tokio::test]
async fn audit_contract_auth_outbox_last_redis_failure_exhausts_intent() {
    let state = audit_contract_state().await;
    let user = audit_contract_create_user(&state.db, "audit-outbox-exhausted", None).await;
    let session_handle = format!("outbox-exhausted-{}", audit_jobs_unique("session"));
    let mut tx = state.db.begin().await.unwrap();
    let pending = auth_sessions::create_pending_tx(
        &mut tx,
        auth_sessions::NewAuthSession {
            session_handle: session_handle.clone(),
            family_handle: audit_jobs_unique("outbox-exhausted-family"),
            user_id: user.id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("outbox-exhausted-refresh"),
            )
            .unwrap(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            predecessor_id: None,
        },
    )
    .await
    .unwrap();
    let active = auth_sessions::activate_pending_tx(&mut tx, pending.id)
        .await
        .unwrap()
        .unwrap();
    let intent = auth_sessions::enqueue_intent_tx(
        &mut tx,
        active.id,
        auth_sessions::AuthIntentPayload::RedisPopulate {
            session_handle,
            expires_at: active.expires_at,
        },
        1,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (restricted_cache, acl_user) = audit_contract_restricted_redis(&state.cache, "set").await;
    assert_eq!(
        auth::session::process_one_outbox_intent(
            &state.db,
            &restricted_cache,
            "outbox-exhausted-worker",
            Some(active.id),
        )
        .await
        .unwrap(),
        auth::session::OutboxProcessOutcome::PermanentlyFailed
    );
    audit_contract_delete_redis_acl_user(&state.cache, &acl_user).await;

    let persisted: (String, i32, Option<String>) = sqlx::query_as(
        "SELECT state, attempts, last_error_code
         FROM auth_session_intents WHERE id = $1",
    )
    .bind(intent.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(persisted.0, "failed");
    assert_eq!(persisted.1, 1);
    assert_eq!(
        persisted.2.as_deref(),
        Some("auth_intent_attempts_exhausted")
    );
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.id)
        .execute(&state.db)
        .await
        .unwrap();
}

/// logout 的 DB revoke 一经提交立即生效；Redis DEL 失败只留下 durable invalidate retry。
#[tokio::test]
async fn audit_contract_logout_remains_immediate_when_redis_invalidate_fails() {
    use axum::extract::State;
    use axum::response::IntoResponse;
    use axum::Json;

    let normal_state = audit_contract_state().await;
    let admin_cache = normal_state.cache.clone();
    let (user, tokens) =
        audit_contract_login_fixture(&normal_state, "audit-logout-redis-fail").await;
    let (restricted_cache, acl_user) = audit_contract_restricted_redis(&admin_cache, "del").await;
    let mut logout_state = audit_contract_state_with_db(normal_state.db.clone()).await;
    logout_state.cache = restricted_cache;
    let logout_result = auth_routes::logout(
        State(logout_state),
        Json(auth_routes::RefreshReq {
            refresh_token: tokens.refresh_token.clone(),
        }),
    )
    .await;
    audit_contract_delete_redis_acl_user(&admin_cache, &acl_user).await;
    logout_result.expect_api("Redis DEL 失败不应推翻 DB logout commit");

    let refresh_hash = prts_auth::token::sha256_hex(&tokens.refresh_token);
    let (session_id, state_name, invalidate_count): (i64, String, i64) = sqlx::query_as(
        "SELECT session.id, session.state,
                (SELECT COUNT(*) FROM auth_session_intents AS intent
                 WHERE intent.session_id = session.id
                   AND intent.kind = 'redis_invalidate'
                   AND intent.state = 'queued')
         FROM auth_sessions AS session WHERE session.refresh_token_hash = $1",
    )
    .bind(refresh_hash)
    .fetch_one(&normal_state.db)
    .await
    .unwrap();
    assert_eq!(state_name, "revoked");
    assert_eq!(invalidate_count, 1);

    // 即使 DEL 失败后缓存仍残留，DB revoked 必须立刻否决 refresh。
    let denied = auth_routes::refresh(
        State(normal_state.clone()),
        Json(auth_routes::RefreshReq {
            refresh_token: tokens.refresh_token,
        }),
    )
    .await
    .expect_err_api("logout DB commit 后 refresh 必须立即失效");
    assert_eq!(
        denied.into_response().status(),
        axum::http::StatusCode::UNAUTHORIZED
    );

    let rows = audit_contract_rows_for_actor(&normal_state.db, user.id).await;
    audit_contract_assert_actions(&rows, &["auth.logged_out"]);
    audit_contract_assert_exact_targets(&rows, &[("auth.logged_out", session_id.to_string())]);
}

/// 幂等 logout 的 unknown/revoked/expired refresh 仍是一次已提供但失败的认证并须 fail closed。
#[tokio::test]
async fn audit_contract_logout_noop_is_audited_and_audit_failure_returns_503() {
    use axum::extract::State;
    use axum::Json;

    let state = audit_contract_state().await;
    let started_at = chrono::Utc::now();
    let unknown = "UNKNOWN_REFRESH_MUST_NOT_ENTER_AUDIT".to_string();
    auth_routes::logout(
        State(state.clone()),
        Json(auth_routes::RefreshReq {
            refresh_token: unknown.clone(),
        }),
    )
    .await
    .expect_api("unknown refresh logout 保持幂等 204");

    let (_, revoked_tokens) = audit_contract_login_fixture(&state, "audit-logout-noop").await;
    auth_routes::logout(
        State(state.clone()),
        Json(auth_routes::RefreshReq {
            refresh_token: revoked_tokens.refresh_token.clone(),
        }),
    )
    .await
    .expect_api("首次 logout 吊销 active session");
    auth_routes::logout(
        State(state.clone()),
        Json(auth_routes::RefreshReq {
            refresh_token: revoked_tokens.refresh_token.clone(),
        }),
    )
    .await
    .expect_api("revoked refresh logout 保持幂等 204");

    let (_, expired_tokens) = audit_contract_login_fixture(&state, "audit-logout-expired").await;
    let expired_hash = prts_auth::token::sha256_hex(&expired_tokens.refresh_token);
    sqlx::query(
        "UPDATE auth_sessions SET expires_at = now() - interval '1 second'
         WHERE refresh_token_hash = $1",
    )
    .bind(&expired_hash)
    .execute(&state.db)
    .await
    .unwrap();
    auth_routes::logout(
        State(state.clone()),
        Json(auth_routes::RefreshReq {
            refresh_token: expired_tokens.refresh_token.clone(),
        }),
    )
    .await
    .expect_api("expired refresh logout 保持幂等 204");

    let rows = sqlx::query_as::<_, (String, String, String, Option<i64>, serde_json::Value)>(
        "SELECT action, target_type, target_id, project_id_snapshot, payload
         FROM audit_log
         WHERE action = 'auth.failed' AND created_at >= $1
           AND payload->>'method' = 'refresh'
         ORDER BY id",
    )
    .bind(started_at)
    .fetch_all(&state.db)
    .await
    .unwrap()
    .into_iter()
    .map(
        |(action, target_type, target_id, project_id, payload)| ObservedAudit {
            action,
            target_type,
            target_id,
            project_id,
            payload,
        },
    )
    .collect::<Vec<_>>();
    assert!(rows.len() >= 3, "unknown/revoked/expired 均须写失败审计");
    assert!(rows
        .iter()
        .all(|row| row.payload["reason_code"] == "invalid_refresh"));
    audit_contract_assert_payloads_are_typed_and_redacted(
        &rows,
        &[
            &unknown,
            &revoked_tokens.refresh_token,
            &expired_tokens.refresh_token,
            &expired_hash,
        ],
    );

    let failing_db = audit_contract_failing_audit_db().await;
    let failing_state = audit_contract_state_with_db(failing_db).await;
    let error = auth_routes::logout(
        State(failing_state),
        Json(auth_routes::RefreshReq {
            refresh_token: "UNKNOWN_REFRESH_AUDIT_FAILURE".to_string(),
        }),
    )
    .await
    .expect_err_api("noop logout 的审计失败必须返回 503");
    audit_contract_assert_unavailable(error).await;
}

/// 普通 read 跨用户/项目/文件/词条/通知/私信均保持零审计；敏感 export 已单独覆盖。
#[tokio::test]
async fn audit_contract_ordinary_reads_do_not_append_audit_rows() {
    use axum::extract::{Path, Query, State};

    let state = audit_contract_state().await;
    let actor =
        audit_contract_create_user(&state.db, "audit-read-actor", Some("super_admin")).await;
    let other = audit_contract_create_user(&state.db, "audit-read-other", None).await;
    let project = projects::create(
        &state.db,
        &format!("audit-read-project-{}", actor.id),
        "Audit Read Project",
        "",
        "private",
        &["en".to_string()],
        "zh-Hans",
        actor.id,
    )
    .await
    .unwrap();
    memberships::upsert(&state.db, project.id, actor.id, "owner")
        .await
        .unwrap();
    memberships::upsert(&state.db, project.id, other.id, "translator")
        .await
        .unwrap();
    let file = files::ensure_file_at_path(&state.db, project.id, "read/item.json")
        .await
        .unwrap();
    entries::bulk_upsert(
        &state.db,
        file.id,
        project.id,
        &[entries::UploadEntry {
            key: "read-key".to_string(),
            original: serde_json::json!({"en": "read fixture"}),
            context: None,
            translation: None,
            state: None,
        }],
        Some(actor.id),
    )
    .await
    .unwrap();
    let entry = entries::list(
        &state.db,
        project.id,
        &entries::EntryFilter::default(),
        None,
        1,
    )
    .await
    .unwrap()
    .remove(0);
    notifications::create(&state.db, actor.id, "read-fixture", &serde_json::json!({}))
        .await
        .unwrap();
    messages::create(&state.db, other.id, actor.id, "read fixture")
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE actor_id = $1")
        .bind(actor.id)
        .fetch_one(&state.db)
        .await
        .unwrap();

    let _ = users_routes::me(State(state.clone()), audit_contract_current_user(&actor))
        .await
        .expect_api("读取当前用户成功");
    let _ = admin_routes::get_settings(State(state.clone()), audit_contract_current_user(&actor))
        .await
        .expect_api("读取平台设置成功");
    let _ = projects_routes::get_project(
        State(state.clone()),
        auth::MaybeUser(Some(audit_contract_current_user(&actor))),
        Path(project.id),
    )
    .await
    .expect_api("读取项目成功");
    let _ = files_routes::get_tree(
        State(state.clone()),
        auth::MaybeUser(Some(audit_contract_current_user(&actor))),
        Path(project.id),
    )
    .await
    .expect_api("读取文件树成功");
    let _ = entries_routes::get_entry(
        State(state.clone()),
        auth::MaybeUser(Some(audit_contract_current_user(&actor))),
        Path((project.id, entry.id)),
    )
    .await
    .expect_api("读取词条成功");
    let _ = notifications_routes::list(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Query(notifications_routes::ListQuery {
            before: None,
            limit: Some(10),
        }),
    )
    .await
    .expect_api("读取通知成功");
    let _ = messages_routes::conversation(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(other.id),
        Query(messages_routes::ConversationQuery {
            before: None,
            limit: Some(10),
        }),
    )
    .await
    .expect_api("读取私信成功");

    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE actor_id = $1")
        .bind(actor.id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(after, before, "普通 read 不得追加 audit_log");
}

/// 用户、自助 API key、平台设置与角色写入必须由真实 handler 产生同 actor 审计。
#[tokio::test]
async fn audit_contract_users_admin_settings_and_api_keys_are_audited_and_redacted() {
    use axum::extract::{FromRequestParts, Path, State};
    use axum::http::{header, Request};
    use axum::Json;
    use std::collections::HashMap;

    let _search_settings_guard = SEARCH_SETTINGS_TEST_LOCK.lock().await;
    let state = audit_contract_state().await;
    let actor = audit_contract_create_user(&state.db, "audit-admin", Some("super_admin")).await;
    let target = audit_contract_create_user(&state.db, "audit-role-target", None).await;
    let previous_search = prts_db::search_settings::get(&state.db).await.unwrap();
    let profile_marker = "FULL_PROFILE_CONTENT_MUST_NOT_ENTER_AUDIT";
    let setting_marker = "FULL_SETTING_VALUE_MUST_NOT_ENTER_AUDIT";
    let endpoint_marker = "AUDIT_EMBEDDING_ENDPOINT_VALUE";

    let Json(updated_profile) = users_routes::update_me(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(users_routes::UpdateMeReq {
            description: Some(profile_marker.to_string()),
            avatar_url: Some("https://example.invalid/avatar.png".to_string()),
            translation_langs: Some(vec!["en".to_string(), "zh-Hans".to_string()]),
        }),
    )
    .await
    .expect_api("更新个人资料成功");

    let Json(created_key) = users_routes::create_api_key(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(users_routes::CreateApiKeyReq {
            name: "audit-contract-key".to_string(),
        }),
    )
    .await
    .expect_api("创建 API key 成功");

    // 走真实 CurrentUser extractor，覆盖 API-key touch writer，而不是直接调用 mock。
    let (mut parts, _) = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {}", created_key.key))
        .body(())
        .unwrap()
        .into_parts();
    let extracted = auth::CurrentUser::from_request_parts(&mut parts, &state)
        .await
        .expect_api("API key 可认证");
    assert_eq!(extracted.id, actor.id);

    let mut changed_settings = HashMap::new();
    changed_settings.insert(
        format!("audit.contract.{}", actor.id),
        serde_json::json!({"enabled": true, "value": setting_marker}),
    );
    admin_routes::update_settings(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(admin_routes::UpdateSettingsReq {
            settings: changed_settings,
        }),
    )
    .await
    .expect_api("更新平台设置成功");

    let _ = admin_settings_routes::put_search_settings(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(admin_settings_routes::SearchConfigDto {
            embedding_enabled: false,
            embedding_model: "text-embedding-v4".to_string(),
            embedding_base_url: format!("https://{endpoint_marker}.invalid/v1"),
            embedding_batch: 3,
            tm_enabled: true,
            tm_min_similarity: 0.4,
            tm_top_n: 2,
        }),
    )
    .await
    .expect_api("更新搜索设置成功");

    admin_routes::grant_role(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(target.id),
        Json(admin_routes::GrantRoleReq {
            role: Some("maintainer".to_string()),
        }),
    )
    .await
    .expect_api("授予平台角色成功");

    users_routes::revoke_api_key(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(created_key.id),
    )
    .await
    .expect_api("吊销 API key 成功");

    let rows = audit_contract_rows_for_actor(&state.db, actor.id).await;
    audit_contract_assert_actions(
        &rows,
        &[
            "user.profile_updated",
            "api_key.created",
            "api_key.used",
            "settings.updated",
            "search_settings.updated",
            "user.platform_role_changed",
            "api_key.revoked",
        ],
    );
    audit_contract_assert_exact_targets(
        &rows,
        &[
            ("user.profile_updated", updated_profile.id.to_string()),
            ("api_key.created", created_key.id.to_string()),
            ("api_key.used", created_key.id.to_string()),
            ("api_key.revoked", created_key.id.to_string()),
            ("user.platform_role_changed", target.id.to_string()),
            ("settings.updated", "platform".to_string()),
            ("search_settings.updated", "search.config".to_string()),
        ],
    );
    audit_contract_assert_payloads_are_typed_and_redacted(
        &rows,
        &[
            profile_marker,
            setting_marker,
            endpoint_marker,
            &created_key.key,
        ],
    );
    prts_db::search_settings::set(&state.db, previous_search.clone(), Some(actor.id))
        .await
        .unwrap();
    *state.search_rt.write().await = previous_search;
}

/// 上传设置管理、公开只读 DTO 与 fail-closed 审计共享同一持久化真值。
#[tokio::test]
async fn upload_settings_routes_enforce_permissions_and_audit_rollback() {
    use axum::extract::State;
    use axum::Json;

    let _upload_settings_guard = UPLOAD_SETTINGS_TEST_LOCK.lock().await;
    let state = audit_contract_state().await;
    let admin = audit_contract_create_user(&state.db, "upload-settings-admin", Some("admin")).await;
    let ordinary = audit_contract_create_user(&state.db, "upload-settings-user", None).await;
    let previous = prts_db::upload_settings::get(&state.db).await.unwrap();

    let denied = admin_settings_routes::get_upload_settings(
        State(state.clone()),
        audit_contract_current_user(&ordinary),
    )
    .await
    .expect_err_api("普通用户不得读取管理端上传设置");
    assert_eq!(denied.code(), "forbidden");

    let Json(public_config) = meta_routes::upload_config(State(state.clone()))
        .await
        .expect_api("公开 meta 可读取上传客户端限制");
    assert_eq!(
        public_config.max_files_per_batch,
        previous.max_files_per_batch
    );
    assert_eq!(
        public_config.max_bytes_per_file,
        previous.max_bytes_per_file
    );
    assert_eq!(
        public_config.max_bytes_per_batch,
        previous.max_bytes_per_batch
    );
    assert_eq!(
        public_config.client_concurrency,
        previous.client_concurrency
    );

    let updated = dto::upload::UploadConfigDto {
        max_files_per_batch: 640,
        max_bytes_per_file: 96 * 1024 * 1024,
        max_bytes_per_batch: 3 * 1024 * 1024 * 1024,
        client_concurrency: 5,
    };
    let failing_state = audit_contract_state_with_db(audit_contract_failing_audit_db().await).await;
    let audit_failure = admin_settings_routes::put_upload_settings(
        State(failing_state),
        audit_contract_current_user(&admin),
        Json(updated.clone()),
    )
    .await
    .expect_err_api("上传设置审计失败必须回滚");
    audit_contract_assert_unavailable(audit_failure).await;
    assert_eq!(
        prts_db::upload_settings::get(&state.db).await.unwrap(),
        previous
    );

    let Json(saved) = admin_settings_routes::put_upload_settings(
        State(state.clone()),
        audit_contract_current_user(&admin),
        Json(updated),
    )
    .await
    .expect_api("管理员更新上传设置");
    assert_eq!(saved.client_concurrency, 5);
    let rows = audit_contract_rows_for_actor(&state.db, admin.id).await;
    audit_contract_assert_actions(&rows, &["upload_settings.updated"]);
    audit_contract_assert_exact_targets(
        &rows,
        &[("upload_settings.updated", "upload.config".to_string())],
    );
    let upload_audit = rows
        .iter()
        .find(|row| row.action == "upload_settings.updated")
        .expect("上传设置审计存在");
    let payload = upload_audit
        .payload
        .as_object()
        .expect("上传设置审计为对象");
    assert_eq!(payload.len(), 1);
    assert!(payload.contains_key("changed_fields"));

    let mut tx = state.db.begin().await.unwrap();
    prts_db::upload_settings::set_tx(&mut tx, &previous, Some(admin.id))
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// 可选认证只能吞掉无凭证/无效凭证，不能把 API-key touch 的审计故障降级成游客。
#[tokio::test]
async fn audit_contract_optional_auth_propagates_api_key_audit_unavailable() {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn public_endpoint(auth::MaybeUser(user): auth::MaybeUser) -> StatusCode {
        if user.is_some() {
            StatusCode::OK
        } else {
            StatusCode::NO_CONTENT
        }
    }

    let normal_state = audit_contract_state().await;
    let actor = audit_contract_create_user(&normal_state.db, "audit-maybe-user", None).await;
    let generated = prts_auth::token::generate_api_key();
    api_keys::create(
        &normal_state.db,
        actor.id,
        "optional-auth-audit-failure",
        &generated.hash,
        &generated.display_prefix,
    )
    .await
    .unwrap();

    let failing_db = audit_contract_failing_audit_db().await;
    let failing_state = audit_contract_state_with_db(failing_db).await;
    let app = Router::new()
        .route("/public", get(public_endpoint))
        .layer(axum::middleware::from_fn(error::localize_audit_errors))
        .with_state(failing_state);
    let anonymous = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::NO_CONTENT);
    let invalid = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/public")
                .header(header::AUTHORIZATION, "Bearer prts_invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::SERVICE_UNAVAILABLE);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/public")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", generated.plaintext),
                )
                .header(header::ACCEPT_LANGUAGE, "en")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "AUDIT_UNAVAILABLE");
    assert_eq!(body["message"], "Audit service unavailable");
}

/// 手动重试的任务状态与审计必须同事务；审计失败返回稳定 503 且不消耗重试预算。
#[tokio::test]
async fn audit_contract_job_retry_audit_failure_rolls_back_and_returns_503() {
    let normal_state = audit_contract_state().await;
    let owner = audit_contract_create_user(&normal_state.db, "audit-job-retry-owner", None).await;
    let project = projects::create(
        &normal_state.db,
        &format!("audit-job-retry-{}", owner.id),
        "Audit job retry",
        "",
        "private",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert(&normal_state.db, project.id, owner.id, "owner")
        .await
        .unwrap();
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (
             kind, project_id, state, stage, payload, attempts, max_attempts,
             last_error_code, last_error_message, finished_at
         ) VALUES (
             'upload_process', $1, 'failed', 'processing', '{}', 2, 2,
             'provider_unavailable', 'redacted test failure', now()
         ) RETURNING id",
    )
    .bind(project.id)
    .fetch_one(&normal_state.db)
    .await
    .unwrap();

    let failing_state = audit_contract_state_with_db(audit_contract_failing_audit_db().await).await;
    let error = job_retry::retry_job(&failing_state, &audit_contract_current_user(&owner), job_id)
        .await
        .expect_err_api("审计失败不得重新排队任务");
    audit_contract_assert_unavailable(error).await;

    let persisted: (String, i32, i32, Option<String>) = sqlx::query_as(
        "SELECT state, attempts, max_attempts, last_error_code FROM jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_one(&normal_state.db)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "failed".to_string(),
            2,
            2,
            Some("provider_unavailable".to_string())
        )
    );

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job_id)
        .execute(&normal_state.db)
        .await
        .unwrap();
    projects::delete(&normal_state.db, project.id)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(owner.id)
        .execute(&normal_state.db)
        .await
        .unwrap();
}

/// Owner resolution 必须验证当前候选、锁后鉴权，并在审计失败时整事务回滚。
#[tokio::test]
async fn language_resolution_owner_selection_permissions_and_audit_rollback_are_atomic() {
    use axum::extract::{Path, State};
    use axum::Json;

    let state = audit_contract_state().await;
    let owner = audit_contract_create_user(&state.db, "language-resolution-owner", None).await;
    let manager = audit_contract_create_user(&state.db, "language-resolution-manager", None).await;
    let project = projects::create(
        &state.db,
        &format!("language-resolution-{}", owner.id),
        "Language resolution",
        "",
        "private",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert(&state.db, project.id, owner.id, "owner")
        .await
        .unwrap();
    memberships::upsert(&state.db, project.id, manager.id, "manager")
        .await
        .unwrap();
    let Json(uploaded) = entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(audit_contract_upload_req(
            "resolution/main.json",
            &[("conflict", "owner-selected-source")],
        )),
    )
    .await
    .expect_api("创建 resolution 测试词条");
    let entry_id: i64 =
        sqlx::query_scalar("SELECT id FROM entries WHERE file_id = $1 AND key = 'conflict'")
            .bind(uploaded.file_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    let unresolved_original = serde_json::json!({
        "en": "owner-selected-source",
        "EN": "alternate-source"
    });
    sqlx::query(
        "UPDATE projects SET language_repair_state = 'needs_language_resolution' WHERE id = $1",
    )
    .bind(project.id)
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query("UPDATE entries SET original = $2 WHERE id = $1")
        .bind(entry_id)
        .bind(&unresolved_original)
        .execute(&state.db)
        .await
        .unwrap();
    let issue_id: i64 = sqlx::query_scalar(
        "INSERT INTO language_resolution_issues (
             project_id, entry_id, entity_type, entity_id_snapshot, issue_kind,
             raw_tag, canonical_tag, metadata
         ) VALUES ($1, $2, 'entry', $2::TEXT, 'conflicting_original_keys',
                   'EN', 'en', jsonb_build_object('key_count', 2))
         RETURNING id",
    )
    .bind(project.id)
    .bind(entry_id)
    .fetch_one(&state.db)
    .await
    .unwrap();

    let hidden_from_manager = language_resolution_routes::get_project_language_resolution(
        State(state.clone()),
        audit_contract_current_user(&manager),
        Path(project.id),
    )
    .await
    .expect_err_api("manager 不得读取 owner resolution 详情");
    assert_eq!(hidden_from_manager.code(), "forbidden");
    let Json(owner_view) = language_resolution_routes::get_project_language_resolution(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
    )
    .await
    .expect_api("owner 可读取当前冲突候选");
    assert_eq!(owner_view.issues.len(), 1);
    assert!(owner_view.issues[0]
        .current_values
        .contains(&"owner-selected-source".to_string()));
    assert!(owner_view.issues[0]
        .current_values
        .contains(&"alternate-source".to_string()));

    let request = || language_resolution_routes::ResolveProjectLanguagesReq {
        source_langs: vec!["en".to_string()],
        primary_source_lang: "en".to_string(),
        target_lang: "zh-Hans".to_string(),
        issues: vec![language_resolution_routes::IssueResolutionReq {
            issue_id,
            canonical_tag: Some("en".to_string()),
            selected_value: Some("owner-selected-source".to_string()),
        }],
    };
    let denied = language_resolution_routes::resolve_project_languages(
        State(state.clone()),
        audit_contract_current_user(&manager),
        Path(project.id),
        Json(request()),
    )
    .await
    .expect_err_api("manager 不得代替 owner 处理语言歧义");
    assert_eq!(denied.code(), "forbidden");

    let invalid = language_resolution_routes::resolve_project_languages(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(language_resolution_routes::ResolveProjectLanguagesReq {
            issues: vec![language_resolution_routes::IssueResolutionReq {
                issue_id,
                canonical_tag: Some("en".to_string()),
                selected_value: Some("not-a-current-candidate".to_string()),
            }],
            ..request()
        }),
    )
    .await
    .expect_err_api("owner 选择必须来自锁定后的当前候选");
    assert_eq!(invalid.code(), "bad_request");

    let jobs_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE project_id = $1 AND kind = 'language_repair'",
    )
    .bind(project.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    let failing_state = audit_contract_state_with_db(audit_contract_failing_audit_db().await).await;
    let failed = language_resolution_routes::resolve_project_languages(
        State(failing_state),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(request()),
    )
    .await
    .expect_err_api("审计失败必须回滚完整 resolution 事务");
    audit_contract_assert_unavailable(failed).await;
    let rolled_back: (
        String,
        serde_json::Value,
        Option<chrono::DateTime<chrono::Utc>>,
        i64,
    ) = sqlx::query_as(
        "SELECT project.language_repair_state, entry.original, issue.resolved_at,
                    (SELECT count(*) FROM jobs
                     WHERE project_id = project.id AND kind = 'language_repair')
             FROM projects AS project
             JOIN entries AS entry ON entry.id = $2
             JOIN language_resolution_issues AS issue ON issue.id = $3
             WHERE project.id = $1",
    )
    .bind(project.id)
    .bind(entry_id)
    .bind(issue_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(rolled_back.0, "needs_language_resolution");
    assert_eq!(rolled_back.1, unresolved_original);
    assert!(rolled_back.2.is_none());
    assert_eq!(rolled_back.3, jobs_before);

    language_resolution_routes::resolve_project_languages(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(request()),
    )
    .await
    .expect_api("owner resolution 成功排入 repair job");
    let completed: (String, serde_json::Value, bool, i64) = sqlx::query_as(
        "SELECT project.language_repair_state, entry.original,
                issue.resolved_at IS NOT NULL, job.id
         FROM projects AS project
         JOIN entries AS entry ON entry.id = $2
         JOIN language_resolution_issues AS issue ON issue.id = $3
         JOIN jobs AS job ON job.id = project.language_repair_job_id
         WHERE project.id = $1 AND job.kind = 'language_repair' AND job.state = 'queued'",
    )
    .bind(project.id)
    .bind(entry_id)
    .bind(issue_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(completed.0, "repairing");
    assert_eq!(
        completed.1,
        serde_json::json!({"en": "owner-selected-source"})
    );
    assert!(completed.2);
    assert!(completed.3 > 0);
}

/// Admin 只能看到 metadata 摘要，并且只可重试失败的同一 repair job。
#[tokio::test]
async fn language_resolution_admin_visibility_and_retry_are_metadata_only_and_state_safe() {
    use axum::extract::{Path, Query, State};

    let state = audit_contract_state().await;
    let owner = audit_contract_create_user(&state.db, "language-admin-owner", None).await;
    let admin = audit_contract_create_user(&state.db, "language-admin", Some("admin")).await;
    let project = projects::create(
        &state.db,
        &format!("language-admin-{}", owner.id),
        "Private language repair",
        "private-body-marker-must-not-leak",
        "private",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert(&state.db, project.id, owner.id, "owner")
        .await
        .unwrap();
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (
             kind, project_id, state, stage, payload, attempts, max_attempts,
             last_error_code, last_error_message, finished_at
         ) VALUES (
             'language_repair', $1, 'failed', 'entries', '{}', 2, 2,
             'language_resolution_required', 'private-job-marker-must-not-leak', now()
         ) RETURNING id",
    )
    .bind(project.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE projects SET language_repair_state = 'needs_language_resolution',
             language_repair_job_id = $2
         WHERE id = $1",
    )
    .bind(project.id)
    .bind(job_id)
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO language_resolution_issues (
             project_id, entity_type, entity_id_snapshot, issue_kind, raw_tag, metadata
         ) VALUES ($1, 'project', $1::TEXT, 'invalid_tag',
                   'private-raw-tag-marker', jsonb_build_object('reason', 'private-reason-marker'))",
    )
    .bind(project.id)
    .execute(&state.db)
    .await
    .unwrap();

    let axum::Json(summaries) = language_resolution_routes::list_admin_language_resolutions(
        State(state.clone()),
        audit_contract_current_user(&admin),
        Query(language_resolution_routes::AdminResolutionQuery {
            after_project_id: None,
            limit: Some(100),
        }),
    )
    .await
    .expect_api("管理员读取语言修复摘要");
    let summary = summaries
        .iter()
        .find(|summary| summary.project_id == project.id)
        .expect("摘要包含目标项目");
    assert_eq!(summary.issue_count, 1);
    let serialized = serde_json::to_string(summary).unwrap();
    for marker in [
        "private-body-marker-must-not-leak",
        "private-job-marker-must-not-leak",
        "private-raw-tag-marker",
        "private-reason-marker",
    ] {
        assert!(!serialized.contains(marker));
    }

    let blocked = language_resolution_routes::retry_admin_language_repair(
        State(state.clone()),
        audit_contract_current_user(&admin),
        Path(project.id),
    )
    .await
    .expect_err_api("管理员不得通过 retry 绕过 owner resolution");
    assert_eq!(blocked.code(), "conflict");

    let retry_project = projects::create(
        &state.db,
        &format!("language-retry-{}", owner.id),
        "Repair infrastructure retry",
        "",
        "private",
        &["en".to_string()],
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert(&state.db, retry_project.id, owner.id, "owner")
        .await
        .unwrap();
    let retry_job_id: i64 = sqlx::query_scalar(
        "INSERT INTO jobs (
             kind, project_id, state, stage, payload, attempts, max_attempts,
             last_error_code, last_error_message, finished_at
         ) VALUES (
             'language_repair', $1, 'failed', 'entries', '{}', 2, 2,
             'database_unavailable', 'redacted database failure', now()
         ) RETURNING id",
    )
    .bind(retry_project.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE projects SET language_repair_state = 'repairing', language_repair_job_id = $2
         WHERE id = $1",
    )
    .bind(retry_project.id)
    .bind(retry_job_id)
    .execute(&state.db)
    .await
    .unwrap();

    let failing_state = audit_contract_state_with_db(audit_contract_failing_audit_db().await).await;
    let audit_failure = language_resolution_routes::retry_admin_language_repair(
        State(failing_state),
        audit_contract_current_user(&admin),
        Path(retry_project.id),
    )
    .await
    .expect_err_api("管理员 repair retry 的审计失败必须回滚");
    audit_contract_assert_unavailable(audit_failure).await;
    let still_failed: (String, i32, i32, Option<String>) = sqlx::query_as(
        "SELECT state, attempts, max_attempts, last_error_code FROM jobs WHERE id = $1",
    )
    .bind(retry_job_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(
        still_failed,
        (
            "failed".to_string(),
            2,
            2,
            Some("database_unavailable".to_string())
        )
    );

    language_resolution_routes::retry_admin_language_repair(
        State(state.clone()),
        audit_contract_current_user(&admin),
        Path(retry_project.id),
    )
    .await
    .expect_api("管理员重试失败的 repair job");
    let retried: (i64, String, i32, i32, Option<String>) = sqlx::query_as(
        "SELECT id, state, attempts, max_attempts, last_error_code FROM jobs WHERE id = $1",
    )
    .bind(retry_job_id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(retried.0, retry_job_id);
    assert_eq!(retried.1, "queued");
    assert_eq!(retried.2, 3);
    assert_eq!(retried.3, 4);
    assert!(retried.4.is_none());

    let duplicate = language_resolution_routes::retry_admin_language_repair(
        State(state.clone()),
        audit_contract_current_user(&admin),
        Path(retry_project.id),
    )
    .await
    .expect_err_api("queued repair job 不能被伪装成再次重试成功");
    assert_eq!(duplicate.code(), "conflict");
    let job_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE project_id = $1 AND kind = 'language_repair'",
    )
    .bind(retry_project.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(job_count, 1);
}

/// 项目、成员、旧上传、词条编辑/flags、文件删除与敏感导出必须走真实事务审计。
#[tokio::test]
async fn audit_contract_projects_files_entries_memberships_and_export_are_audited_and_redacted() {
    use axum::extract::{Path, State};
    use axum::Json;

    let state = audit_contract_state().await;
    let actor =
        audit_contract_create_user(&state.db, "audit-project-owner", Some("maintainer")).await;
    let member = audit_contract_create_user(&state.db, "audit-project-member", None).await;
    let original_marker = "FULL_ORIGINAL_SOURCE_TEXT_MUST_NOT_ENTER_AUDIT";
    let seeded_translation_marker = "FULL_SEEDED_TRANSLATION_MUST_NOT_ENTER_AUDIT";
    let updated_translation_marker = "FULL_UPDATED_TRANSLATION_MUST_NOT_ENTER_AUDIT";
    let context_marker = "FULL_ENTRY_CONTEXT_MUST_NOT_ENTER_AUDIT";
    let description_marker = "FULL_PROJECT_DESCRIPTION_MUST_NOT_ENTER_AUDIT";
    let slug = format!("audit-project-{}", actor.id);

    let Json(project) = projects_routes::create_project(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Json(projects_routes::CreateProjectReq {
            name: format!("Audit Project {}", actor.id),
            slug: Some(slug.clone()),
            description: Some(description_marker.to_string()),
            visibility: Some("private".to_string()),
            source_langs: vec!["en".to_string()],
            primary_source_lang: None,
            target_lang: "zh-Hans".to_string(),
        }),
    )
    .await
    .expect_api("创建项目成功");

    projects_routes::add_member(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
        Json(projects_routes::AddMemberReq {
            username: member.username.clone(),
            role: "translator".to_string(),
        }),
    )
    .await
    .expect_api("添加项目成员成功");

    let Json(uploaded) = entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
        Json(entries_routes::UploadReq {
            path: "dialog/main.json".to_string(),
            entries: vec![entries_routes::UploadEntryDto {
                key: "audit-key".to_string(),
                original: serde_json::json!({"en": original_marker}),
                context: Some(context_marker.to_string()),
                translation: Some(seeded_translation_marker.to_string()),
                state: Some("translated".to_string()),
            }],
        }),
    )
    .await
    .expect_api("旧上传入口成功");

    let entry = entries::list(
        &state.db,
        project.id,
        &entries::EntryFilter {
            file_id: Some(uploaded.file_id),
            ..Default::default()
        },
        None,
        10,
    )
    .await
    .unwrap()
    .into_iter()
    .next()
    .expect("上传产生词条");

    let _ = entries_routes::update_entry(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path((project.id, entry.id)),
        Json(entries_routes::UpdateEntryReq {
            translation: updated_translation_marker.to_string(),
            state: "translated".to_string(),
            version: entry.version,
        }),
    )
    .await
    .expect_api("编辑词条成功");
    let _ = entries_routes::set_entry_flags(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path((project.id, entry.id)),
        Json(entries_routes::SetFlagsReq {
            locked: Some(true),
            hidden: Some(true),
        }),
    )
    .await
    .expect_api("设置词条 flags 成功");

    // 敏感读取例外：项目导出必须审计，普通列表/详情读取在另一合同中断言零审计。
    entries_routes::export_project(
        State(state.clone()),
        auth::MaybeUser(Some(audit_contract_current_user(&actor))),
        Path(project.id),
    )
    .await
    .expect_api("项目导出成功");

    let Json(file_to_delete) = entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
        Json(entries_routes::UploadReq {
            path: "delete-file/item.json".to_string(),
            entries: vec![entries_routes::UploadEntryDto {
                key: "delete-file-key".to_string(),
                original: serde_json::json!({"en": original_marker}),
                context: None,
                translation: None,
                state: None,
            }],
        }),
    )
    .await
    .expect_api("创建待删文件成功");
    files_routes::delete_file(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path((project.id, file_to_delete.file_id)),
    )
    .await
    .expect_api("删除文件成功");

    let Json(folder_upload) = entries_routes::upload(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
        Json(entries_routes::UploadReq {
            path: "delete-folder/nested/item.json".to_string(),
            entries: vec![entries_routes::UploadEntryDto {
                key: "delete-folder-key".to_string(),
                original: serde_json::json!({"en": original_marker}),
                context: None,
                translation: None,
                state: None,
            }],
        }),
    )
    .await
    .expect_api("创建待删文件夹成功");
    let folder_id = files::list_folders(&state.db, project.id)
        .await
        .unwrap()
        .into_iter()
        .find(|folder| folder.path == "delete-folder")
        .expect("顶层待删文件夹存在")
        .id;
    files_routes::delete_folder(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path((project.id, folder_id)),
    )
    .await
    .expect_api("删除文件夹成功");

    let _ = projects_routes::update_project(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
        Json(projects_routes::UpdateProjectReq {
            name: Some(format!("Updated Audit Project {}", actor.id)),
            description: Some(description_marker.to_string()),
            visibility: Some("public".to_string()),
            source_langs: None,
            target_lang: None,
        }),
    )
    .await
    .expect_api("更新项目成功");
    projects_routes::remove_member(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path((project.id, member.id)),
    )
    .await
    .expect_api("移除项目成员成功");
    projects_routes::delete_project(
        State(state.clone()),
        audit_contract_current_user(&actor),
        Path(project.id),
    )
    .await
    .expect_api("删除项目成功");

    let rows = audit_contract_rows_for_actor(&state.db, actor.id).await;
    audit_contract_assert_actions(
        &rows,
        &[
            "project.created",
            "membership.upserted",
            "entries.uploaded",
            "entries.uploaded",
            "entries.uploaded",
            "entry.updated",
            "entry.flags_updated",
            "project.exported",
            "file.deleted",
            "folder.deleted",
            "project.updated",
            "membership.removed",
            "project.deleted",
        ],
    );
    audit_contract_assert_exact_targets(
        &rows,
        &[
            ("project.created", project.id.to_string()),
            (
                "membership.upserted",
                format!("{}:{}", project.id, member.id),
            ),
            ("entries.uploaded", uploaded.file_id.to_string()),
            ("entries.uploaded", file_to_delete.file_id.to_string()),
            ("entries.uploaded", folder_upload.file_id.to_string()),
            ("entry.updated", entry.id.to_string()),
            ("entry.flags_updated", entry.id.to_string()),
            ("project.exported", project.id.to_string()),
            ("file.deleted", file_to_delete.file_id.to_string()),
            ("folder.deleted", folder_id.to_string()),
            ("project.updated", project.id.to_string()),
            (
                "membership.removed",
                format!("{}:{}", project.id, member.id),
            ),
            ("project.deleted", project.id.to_string()),
        ],
    );
    for row in &rows {
        if row.project_id.is_some() {
            assert_eq!(row.project_id, Some(project.id));
        }
        assert!(!row.target_type.is_empty());
        assert!(!row.target_id.is_empty());
    }
    audit_contract_assert_payloads_are_typed_and_redacted(
        &rows,
        &[
            original_marker,
            seeded_translation_marker,
            updated_translation_marker,
            context_marker,
            description_marker,
        ],
    );
}

/// 项目头像遵循公开/私有可见性，并在替换与删除时同步媒体、元数据和类型化审计。
#[tokio::test]
async fn project_avatar_lifecycle_enforces_visibility_and_audits_mutations() {
    use axum::body::Body;
    use axum::extract::{Path, State};
    use axum::http::{header, Request, StatusCode};
    use axum::Json;

    fn webp(red: u8) -> Vec<u8> {
        let pixels = vec![red, 20, 30, 255].repeat(64 * 64);
        let mut bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .encode(&pixels, 64, 64, image::ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    fn upload_request(bytes: Vec<u8>) -> Request<Body> {
        Request::builder()
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CONTENT_LENGTH, bytes.len())
            .body(Body::from(bytes))
            .unwrap()
    }

    let state = audit_contract_state().await;
    let owner = audit_contract_create_user(&state.db, "avatar-owner", Some("maintainer")).await;
    let outsider = audit_contract_create_user(&state.db, "avatar-outsider", None).await;
    let Json(project) = projects_routes::create_project(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Json(projects_routes::CreateProjectReq {
            name: format!("Avatar project {}", owner.id),
            slug: Some(format!("avatar-project-{}", owner.id)),
            description: None,
            visibility: Some("public".to_string()),
            source_langs: vec!["en".to_string()],
            primary_source_lang: None,
            target_lang: "zh-Hans".to_string(),
        }),
    )
    .await
    .expect_api("创建头像测试项目");

    let first = webp(40);
    let second = webp(180);
    assert_eq!(
        project_media_routes::upload_project_avatar(
            State(state.clone()),
            Path(project.id),
            audit_contract_current_user(&owner),
            upload_request(first.clone()),
        )
        .await
        .expect_api("首次上传头像"),
        StatusCode::NO_CONTENT
    );
    let public_response = project_media_routes::get_project_avatar(
        State(state.clone()),
        Path(project.id),
        auth::MaybeUser(None),
    )
    .await
    .expect_api("公开项目允许游客读取头像");
    assert_eq!(public_response.status(), StatusCode::OK);
    assert_eq!(
        axum::body::to_bytes(public_response.into_body(), usize::MAX)
            .await
            .unwrap(),
        first
    );

    sqlx::query("UPDATE projects SET visibility = 'private' WHERE id = $1")
        .bind(project.id)
        .execute(&state.db)
        .await
        .unwrap();
    let hidden = project_media_routes::get_project_avatar(
        State(state.clone()),
        Path(project.id),
        auth::MaybeUser(Some(audit_contract_current_user(&outsider))),
    )
    .await
    .expect_err_api("私有项目对外部用户隐藏头像");
    assert_eq!(hidden.code(), "not_found");

    project_media_routes::upload_project_avatar(
        State(state.clone()),
        Path(project.id),
        audit_contract_current_user(&owner),
        upload_request(second.clone()),
    )
    .await
    .expect_api("替换项目头像");
    assert_eq!(
        state
            .media
            .read(&media::project_avatar_key(project.id))
            .await
            .unwrap(),
        second
    );
    project_media_routes::delete_project_avatar(
        State(state.clone()),
        Path(project.id),
        audit_contract_current_user(&owner),
    )
    .await
    .expect_api("删除项目头像");
    let stored = projects::find_by_id(&state.db, project.id)
        .await
        .unwrap()
        .unwrap();
    assert!(stored.avatar_key.is_none());
    assert!(state
        .media
        .read(&media::project_avatar_key(project.id))
        .await
        .is_err());

    let rows = audit_contract_rows_for_actor(&state.db, owner.id).await;
    let avatar_rows: Vec<_> = rows
        .iter()
        .filter(|row| row.action.starts_with("project.avatar_"))
        .collect();
    assert_eq!(avatar_rows.len(), 3);
    assert_eq!(avatar_rows[0].action, "project.avatar_updated");
    assert_eq!(avatar_rows[0].payload["replaced"], false);
    assert_eq!(avatar_rows[1].action, "project.avatar_updated");
    assert_eq!(avatar_rows[1].payload["replaced"], true);
    assert_eq!(avatar_rows[2].action, "project.avatar_deleted");
}

/// 主源切换只允许唯一 owner；相同值无副作用，真实变化原子创建 job、状态和审计并启动七天冷却。
#[tokio::test]
async fn primary_source_change_is_owner_only_atomic_and_cooled_down() {
    use axum::extract::{Path, State};
    use axum::Json;

    let state = audit_contract_state().await;
    let worker_registered_before: bool = sqlx::query_scalar(
        "SELECT lexical_worker_registered FROM workspace_foundation_state WHERE singleton",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE workspace_foundation_state
         SET lexical_worker_registered = TRUE, reconciled_at = now()
         WHERE singleton",
    )
    .execute(&state.db)
    .await
    .unwrap();
    let owner = audit_contract_create_user(&state.db, "primary-change-owner", None).await;
    let manager = audit_contract_create_user(&state.db, "primary-change-manager", None).await;
    let slug = format!("primary-change-{}", owner.id);
    let mut tx = state.db.begin().await.unwrap();
    let project = projects::create_with_primary_tx(
        &mut tx,
        &slug,
        "Primary source change",
        "",
        "private",
        &["en".to_string(), "ja".to_string()],
        "en",
        "zh-Hans",
        owner.id,
    )
    .await
    .unwrap();
    memberships::upsert_tx(&mut tx, project.id, owner.id, "owner")
        .await
        .unwrap();
    memberships::upsert_tx(&mut tx, project.id, manager.id, "manager")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let forbidden = projects_routes::change_primary_source(
        State(state.clone()),
        audit_contract_current_user(&manager),
        Path(project.id),
        Json(projects_routes::ChangePrimarySourceReq {
            source_langs: vec!["en".to_string(), "ja".to_string()],
            primary_source_lang: "ja".to_string(),
        }),
    )
    .await;
    assert!(forbidden.is_err());

    let same = projects_routes::change_primary_source(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(projects_routes::ChangePrimarySourceReq {
            source_langs: vec!["en".to_string(), "ja".to_string()],
            primary_source_lang: "EN".to_string(),
        }),
    )
    .await
    .expect_api("相同 canonical 主源无副作用成功")
    .0;
    assert!(same.primary_source_changed_at.is_none());
    let jobs_before: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE project_id = $1")
        .bind(project.id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(jobs_before, 0);

    let changed = projects_routes::change_primary_source(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(projects_routes::ChangePrimarySourceReq {
            source_langs: vec!["en".to_string(), "ja".to_string()],
            primary_source_lang: "JA".to_string(),
        }),
    )
    .await
    .expect_api("owner 主源切换成功")
    .0;
    assert_eq!(changed.primary_source_lang.as_deref(), Some("ja"));
    assert_eq!(changed.lexical_state, "rebuilding");
    assert_eq!(changed.embedding_state, "pending");
    assert!(changed.primary_source_changed_at.is_some());
    let lexical_job_id: i64 =
        sqlx::query_scalar("SELECT lexical_job_id FROM projects WHERE id = $1")
            .bind(project.id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    let job_kind: String = sqlx::query_scalar("SELECT kind FROM jobs WHERE id = $1")
        .bind(lexical_job_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(job_kind, "primary_source_lexical_reindex");
    let audit = audit_contract_rows_for_actor(&state.db, owner.id)
        .await
        .into_iter()
        .find(|row| row.action == "project.primary_source_changed")
        .expect("主源切换审计存在");
    assert_eq!(audit.project_id, Some(project.id));
    assert_eq!(
        audit.payload,
        serde_json::json!({
            "previous_primary_source": "en",
            "new_primary_source": "ja",
            "source_language_count": 2,
            "lexical_job_id": lexical_job_id,
        })
    );

    sqlx::query("UPDATE jobs SET state = 'succeeded' WHERE id = $1")
        .bind(lexical_job_id)
        .execute(&state.db)
        .await
        .unwrap();
    let cooled_down = projects_routes::change_primary_source(
        State(state.clone()),
        audit_contract_current_user(&owner),
        Path(project.id),
        Json(projects_routes::ChangePrimarySourceReq {
            source_langs: vec!["en".to_string(), "ja".to_string()],
            primary_source_lang: "en".to_string(),
        }),
    )
    .await;
    assert!(cooled_down.is_err());

    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project.id)
        .execute(&state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(lexical_job_id)
        .execute(&state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
        .bind(owner.id)
        .bind(manager.id)
        .execute(&state.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE workspace_foundation_state
         SET lexical_worker_registered = $1 WHERE singleton",
    )
    .bind(worker_registered_before)
    .execute(&state.db)
    .await
    .unwrap();
}

/// 通知 mark-read、poke、私信发送/已读都属于成功 mutation；正文只能进入业务表，不能进 audit。
#[tokio::test]
async fn audit_contract_notifications_poke_and_messages_are_audited_without_content() {
    use axum::extract::{Path, State};
    use axum::Json;

    let state = audit_contract_state().await;
    let sender = audit_contract_create_user(&state.db, "audit-message-sender", None).await;
    let recipient = audit_contract_create_user(&state.db, "audit-message-recipient", None).await;
    let project = projects::create(
        &state.db,
        &format!("audit-message-project-{}", sender.id),
        "Audit Message Project",
        "",
        "private",
        &["en".to_string()],
        "zh-Hans",
        sender.id,
    )
    .await
    .unwrap();
    memberships::upsert(&state.db, project.id, sender.id, "owner")
        .await
        .unwrap();
    memberships::upsert(&state.db, project.id, recipient.id, "translator")
        .await
        .unwrap();

    let notification = notifications::create(
        &state.db,
        sender.id,
        "audit-contract",
        &serde_json::json!({"fixture": true}),
    )
    .await
    .unwrap();
    notifications_routes::mark_read(
        State(state.clone()),
        audit_contract_current_user(&sender),
        Json(notifications_routes::MarkReadReq {
            ids: Some(vec![notification.id]),
        }),
    )
    .await
    .expect_api("标记通知已读成功");

    let poke_marker = "FULL_POKE_TEXT_MUST_NOT_ENTER_AUDIT";
    notifications_routes::poke(
        State(state.clone()),
        audit_contract_current_user(&sender),
        Path(project.id),
        Json(notifications_routes::PokeReq {
            to_user_id: recipient.id,
            text: poke_marker.to_string(),
        }),
    )
    .await
    .expect_api("poke 发送成功");
    let poke_notification = notifications::list(&state.db, recipient.id, None, 10)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.kind == "poke")
        .expect("poke 通知存在");

    let message_marker = "FULL_PRIVATE_MESSAGE_MUST_NOT_ENTER_AUDIT";
    let Json(sent_message) = messages_routes::send(
        State(state.clone()),
        audit_contract_current_user(&sender),
        Json(messages_routes::SendReq {
            to_user_id: recipient.id,
            content: message_marker.to_string(),
        }),
    )
    .await
    .expect_api("私信发送成功");
    messages_routes::mark_read(
        State(state.clone()),
        audit_contract_current_user(&recipient),
        Path(sender.id),
    )
    .await
    .expect_api("私信标记已读成功");

    let sender_rows = audit_contract_rows_for_actor(&state.db, sender.id).await;
    audit_contract_assert_actions(
        &sender_rows,
        &["notification.marked_read", "poke.sent", "message.sent"],
    );
    audit_contract_assert_exact_targets(
        &sender_rows,
        &[
            ("notification.marked_read", sender.id.to_string()),
            ("poke.sent", poke_notification.id.to_string()),
            ("message.sent", sent_message.id.to_string()),
        ],
    );
    audit_contract_assert_payloads_are_typed_and_redacted(
        &sender_rows,
        &[poke_marker, message_marker],
    );

    let recipient_rows = audit_contract_rows_for_actor(&state.db, recipient.id).await;
    audit_contract_assert_actions(&recipient_rows, &["message.marked_read"]);
    audit_contract_assert_exact_targets(
        &recipient_rows,
        &[("message.marked_read", sender.id.to_string())],
    );
    audit_contract_assert_payloads_are_typed_and_redacted(
        &recipient_rows,
        &[poke_marker, message_marker],
    );
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

async fn upload_test_cleanup(pool: &prts_db::Db, user_id: i64, project_id: i64) {
    sqlx::query("DELETE FROM jobs WHERE project_id = $1")
        .bind(project_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM upload_batches WHERE project_id_snapshot = $1")
        .bind(project_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
}

/// 0010 一次冻结上传/history schema、显式 FK delete action、状态与 writer readiness gate。
#[tokio::test]
async fn upload_history_schema_contract_is_complete_and_gated() {
    let pool = pool().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public'
           AND table_name = ANY($1::TEXT[])
         ORDER BY table_name",
    )
    .bind(&[
        "file_change_items",
        "file_change_sets",
        "upload_batch_files",
        "upload_batches",
        "upload_file_attempts",
    ][..])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(tables.len(), 5);

    let readiness: (i32, bool, bool) = sqlx::query_as(
        "SELECT schema_revision, upload_history_schema_ready, file_history_writer_ready
         FROM workspace_foundation_state WHERE singleton",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(readiness, (10, true, false));

    let constraints: Vec<(String, String)> = sqlx::query_as(
        "SELECT info.constraint_name, pg_get_constraintdef(constraint.oid)
         FROM information_schema.table_constraints AS info
         JOIN pg_namespace AS namespace ON namespace.nspname = info.constraint_schema
         JOIN pg_class AS table_class ON table_class.relnamespace = namespace.oid
              AND table_class.relname = info.table_name
         JOIN pg_constraint AS constraint ON constraint.conname = info.constraint_name
              AND constraint.conrelid = table_class.oid
         WHERE info.table_schema = 'public'
           AND info.constraint_name = ANY($1::TEXT[])
         ORDER BY constraint_name",
    )
    .bind(&[
        "files_deletion_change_set_fk",
        "file_change_sets_project_id_fkey",
        "folders_deletion_change_set_fk",
        "upload_batch_files_processing_job_id_fkey",
        "upload_batch_files_target_file_id_fkey",
        "upload_file_attempts_target_file_id_fkey",
    ][..])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(constraints.len(), 6);
    for (name, definition) in constraints {
        if name.ends_with("deletion_change_set_fk") || name == "file_change_sets_project_id_fkey" {
            assert!(!definition.contains("ON DELETE CASCADE"));
            assert!(!definition.contains("ON DELETE SET NULL"));
        } else {
            assert!(definition.contains("ON DELETE SET NULL"), "{name}: {definition}");
        }
    }

    let attempt_state_constraint: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint
         WHERE conname = 'upload_file_attempts_state_chk'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(attempt_state_constraint.contains("receiving"));
    let cleanup_index: String = sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes
         WHERE schemaname = 'public' AND indexname = 'upload_file_attempts_cleanup_idx'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(cleanup_index.contains("cleaned_at IS NULL"));
}

/// byte-zero retry 保留旧 attempt，并在同一 logical file 上复用 processing job id。
#[tokio::test]
async fn upload_batch_retry_reuses_processing_job_and_preserves_attempt_history() {
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "upload-retry").await;
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(24);
    let first_key = format!("projects/{project_id}/uploads/test/first.json");
    let mut tx = pool.begin().await.unwrap();
    let batch = prts_db::uploads::create_batch_tx(
        &mut tx,
        project_id,
        user_id,
        &[prts_db::uploads::UploadDeclaration {
            path: "folder/file.json".to_string(),
            declared_bytes: 4,
            temp_key: first_key,
        }],
        expires_at,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let file = &batch.files[0];
    let first_attempt = &batch.attempts[0];

    let mut tx = pool.begin().await.unwrap();
    let claimed = prts_db::uploads::claim_attempt_for_receive_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        file.id,
        first_attempt.id,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(claimed.2.state, "receiving");
    assert!(prts_db::uploads::claim_attempt_for_receive_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        file.id,
        first_attempt.id,
    )
    .await
    .unwrap()
    .is_none());
    assert!(prts_db::uploads::mark_attempt_received_tx(
        &mut tx,
        file.id,
        first_attempt.id,
        4,
    )
    .await
    .unwrap());
    let jobs = prts_db::uploads::queue_batch_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        user_id,
    )
    .await
    .unwrap()
    .unwrap();
    let processing_job_id = jobs[0].id;
    tx.commit().await.unwrap();

    sqlx::query("UPDATE jobs SET state = 'failed' WHERE id = $1")
        .bind(processing_job_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE upload_batch_files SET state = 'failed' WHERE id = $1")
        .bind(file.id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE upload_file_attempts SET state = 'failed', error_code = 'processing_failed'
         WHERE id = $1",
    )
    .bind(first_attempt.id)
    .execute(&pool)
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    assert!(prts_db::uploads::retry_file_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        file.id,
        user_id + 1,
        "projects/other/retry.json",
        expires_at,
    )
    .await
    .unwrap()
    .is_none());
    let retry = prts_db::uploads::retry_file_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        file.id,
        user_id,
        &format!("projects/{project_id}/uploads/test/retry.json"),
        expires_at,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(retry.attempt_number, 2);
    assert_eq!(retry.bytes_received, 0);
    assert!(retry.error_code.is_none());
    tx.commit().await.unwrap();

    let snapshot = prts_db::uploads::find_batch(&pool, project_id, batch.batch.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.attempts.len(), 2);
    assert_eq!(snapshot.attempts[0].error_code.as_deref(), Some("processing_failed"));
    assert_eq!(snapshot.files[0].processing_job_id, Some(processing_job_id));

    let mut tx = pool.begin().await.unwrap();
    prts_db::uploads::claim_attempt_for_receive_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        file.id,
        retry.id,
    )
    .await
    .unwrap()
    .unwrap();
    prts_db::uploads::mark_attempt_received_tx(&mut tx, file.id, retry.id, 4)
        .await
        .unwrap();
    let jobs = prts_db::uploads::queue_batch_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        user_id,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(jobs[0].id, processing_job_id);
    assert_eq!(jobs[0].payload["attempt_id"], retry.id);
    assert_eq!(jobs[0].attempts, 0);
    tx.commit().await.unwrap();

    upload_test_cleanup(&pool, user_id, project_id).await;
}

/// complete 拒绝不完整字节数；cancel/expiry 保留 durable cleanup 候选并收敛状态。
#[tokio::test]
async fn upload_batch_incomplete_cancel_expiry_and_cleanup_are_durable() {
    let _upload_lifecycle_guard = UPLOAD_LIFECYCLE_TEST_LOCK.lock().await;
    let pool = pool().await;
    let (user_id, project_id, _) = audit_jobs_project(&pool, "upload-cleanup").await;
    let past = chrono::Utc::now() - chrono::Duration::minutes(1);
    let mut tx = pool.begin().await.unwrap();
    let batch = prts_db::uploads::create_batch_tx(
        &mut tx,
        project_id,
        user_id,
        &[prts_db::uploads::UploadDeclaration {
            path: "cancel/file.json".to_string(),
            declared_bytes: 9,
            temp_key: format!("projects/{project_id}/uploads/test/cancel.json"),
        }],
        past,
    )
    .await
    .unwrap();
    assert!(prts_db::uploads::queue_batch_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        user_id,
    )
    .await
    .is_err());
    tx.rollback().await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    let batch = prts_db::uploads::create_batch_tx(
        &mut tx,
        project_id,
        user_id,
        &[prts_db::uploads::UploadDeclaration {
            path: "cancel/file.json".to_string(),
            declared_bytes: 9,
            temp_key: format!("projects/{project_id}/uploads/test/cancel-2.json"),
        }],
        chrono::Utc::now() + chrono::Duration::hours(24),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    prts_db::uploads::claim_attempt_for_receive_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        batch.files[0].id,
        batch.attempts[0].id,
    )
    .await
    .unwrap()
    .unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let keys = prts_db::uploads::cancel_batch_tx(
        &mut tx,
        project_id,
        batch.batch.id,
        user_id,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(keys.len(), 1);
    tx.commit().await.unwrap();
    let cancelled = prts_db::uploads::find_batch(&pool, project_id, batch.batch.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.batch.state, "cancelled");
    assert_eq!(cancelled.attempts[0].state, "cancelled");
    assert!(prts_db::uploads::list_cleanup_candidates(&pool, 100)
        .await
        .unwrap()
        .iter()
        .any(|(id, _)| *id == batch.attempts[0].id));
    prts_db::uploads::mark_attempt_cleaned(&pool, batch.attempts[0].id)
        .await
        .unwrap();
    let cleaned_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT cleaned_at FROM upload_file_attempts WHERE id = $1",
    )
    .bind(batch.attempts[0].id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(cleaned_at.is_some());

    let mut tx = pool.begin().await.unwrap();
    let expired_batch = prts_db::uploads::create_batch_tx(
        &mut tx,
        project_id,
        user_id,
        &[prts_db::uploads::UploadDeclaration {
            path: "expired/file.json".to_string(),
            declared_bytes: 1,
            temp_key: format!("projects/{project_id}/uploads/test/expired.json"),
        }],
        past,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    prts_db::uploads::expire_due(&pool, 500).await.unwrap();
    let expired = prts_db::uploads::find_batch(&pool, project_id, expired_batch.batch.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(expired.batch.state, "expired");
    assert_eq!(expired.attempts[0].state, "expired");
    let expiry_audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log
         WHERE action = 'upload.batch_expired' AND target_id = $1",
    )
    .bind(expired_batch.batch.id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(expiry_audit, 1);

    upload_test_cleanup(&pool, user_id, project_id).await;
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
        db_jobs::claim_next_for_ids_and_kinds(&pool, "worker-a", 300, &[], &eligible, &job_ids,),
        db_jobs::claim_next_for_ids_and_kinds(&pool, "worker-b", 300, &[], &eligible, &job_ids,)
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
    let claimed = db_jobs::claim_next_for_ids_and_kinds(
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

    let claimed = db_jobs::claim_next_for_ids_and_kinds(
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
    let exhausted = db_jobs::find_by_id(&pool, exhausted_id)
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

    assert!(db_jobs::claim_next_for_ids_and_kinds(
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

    assert!(!db_jobs::renew_lease(&pool, job_id, "stale-worker", 300)
        .await
        .unwrap());
    assert!(db_jobs::renew_lease(&pool, job_id, "current-worker", 300)
        .await
        .unwrap());

    sqlx::query("UPDATE jobs SET lease_until = now() - interval '1 second' WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !db_jobs::renew_lease(&pool, job_id, "current-worker", 300)
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
    let locked = db_jobs::find_by_id_for_update_tx(&mut tx, job_id)
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
    let retried = db_jobs::manual_retry_tx(&mut tx, locked.id)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    assert!(delete_task.await.unwrap());

    assert_eq!(retried.id, job_id);
    assert_eq!(retried.attempts, 4);
    let detached = db_jobs::find_by_id(&pool, job_id).await.unwrap().unwrap();
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
    let claimed = db_jobs::claim_next_for_ids_and_kinds(
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
        db_jobs::resume_project_jobs(&pool, project_id)
            .await
            .unwrap(),
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
        db_jobs::update_progress(&pool, job_id, "progress-worker", 6, Some(10), "processing")
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
    let locked = db_jobs::find_by_id_for_update_tx(&mut tx, failed_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(locked.project_id, None);
    assert!(db_jobs::manual_retry_tx(&mut tx, failed_id)
        .await
        .unwrap()
        .is_none());
    tx.commit().await.unwrap();

    let kinds = vec![
        "upload_process".to_string(),
        "upload_cleanup".to_string(),
        "project_purge".to_string(),
    ];
    let claimed = db_jobs::claim_next_for_ids_and_kinds(
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
        i32::MAX,
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
    assert!(
        auth_sessions::complete_intent(&pool, intent_id, "dead-worker")
            .await
            .unwrap()
            .is_none(),
        "旧 worker 不得完成新持有者的 intent"
    );
    assert!(
        auth_sessions::reschedule_intent(
            &pool,
            intent_id,
            "dead-worker",
            "cache_unavailable",
            "redacted cache failure",
            1,
        )
        .await
        .unwrap()
        .is_none(),
        "旧 worker 不得重排新持有者的 intent"
    );
    assert!(
        auth_sessions::fail_intent_permanently(
            &pool,
            intent_id,
            "dead-worker",
            "invalid_auth_intent",
            "invalid intent",
        )
        .await
        .unwrap()
        .is_none(),
        "旧 worker 不得永久失败新持有者的 intent"
    );

    let rescheduled = auth_sessions::reschedule_intent(
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
    assert_eq!(rescheduled.id, intent_id);
    assert_eq!(rescheduled.state, auth_sessions::AuthIntentState::Queued);
    sqlx::query(
        "UPDATE auth_session_intents SET run_after = now() - interval '1 second' WHERE id = $1",
    )
    .bind(intent_id)
    .execute(&pool)
    .await
    .unwrap();
    let reclaimed = auth_sessions::claim_intent_for_session(&pool, "final-worker", 300, session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.id, intent_id);
    let failed = auth_sessions::fail_intent_permanently(
        &pool,
        intent_id,
        "final-worker",
        "invalid_auth_intent",
        "invalid intent",
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
    assert_eq!(retried.attempts, 4);

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

/// 旧版本遗留的已耗尽 queued intent 必须在领取扫描时主动收敛到 failed。
#[tokio::test]
async fn audit_jobs_exhausted_queued_auth_intent_is_failed_without_reclaim() {
    let pool = pool().await;
    let username = audit_jobs_unique("intent-queued-exhausted-user");
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
            session_handle: audit_jobs_unique("intent-queued-exhausted-session"),
            family_handle: audit_jobs_unique("intent-queued-exhausted-family"),
            user_id,
            refresh_token_hash: auth_sessions::RefreshTokenHash::parse(
                prts_auth::token::sha256_hex("intent-queued-exhausted-refresh"),
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
         SET state = 'queued', attempts = max_attempts,
             run_after = now() - interval '1 minute'
         WHERE id = $1",
    )
    .bind(intent.id)
    .execute(&pool)
    .await
    .unwrap();

    assert!(auth_sessions::claim_intent_for_session(
        &pool,
        "queued-exhausted-worker",
        300,
        session.id,
    )
    .await
    .unwrap()
    .is_none());
    let persisted: (String, i32, Option<String>) = sqlx::query_as(
        "SELECT state, attempts, last_error_code
         FROM auth_session_intents WHERE id = $1",
    )
    .bind(intent.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, "failed");
    assert_eq!(persisted.1, 1);
    assert_eq!(
        persisted.2.as_deref(),
        Some("auth_intent_attempts_exhausted")
    );

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
    let created = db_jobs::create_tx(
        &mut tx,
        db_jobs::NewJob {
            kind: db_jobs::JobKind::UploadCleanup,
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
    let claimed = db_jobs::claim_next_for_ids_and_kinds(
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
    let completed = db_jobs::complete(
        &pool,
        created.id,
        "typed-job-worker",
        db_jobs::JobResult::Completed,
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

    let _search_settings_guard = SEARCH_SETTINGS_TEST_LOCK.lock().await;
    let pool = pool().await;

    let previous = search_settings::get(&pool).await.unwrap();

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

    search_settings::set(&pool, previous, None).await.unwrap();
}

/// 上传限制只接受安全边界，并由 settings 表持久化完整字节值。
#[tokio::test]
async fn upload_settings_defaults_validation_and_repository_roundtrip() {
    use prts_db::upload_settings::{self, UploadConfig};

    let _upload_settings_guard = UPLOAD_SETTINGS_TEST_LOCK.lock().await;
    let pool = pool().await;
    let previous = upload_settings::get(&pool).await.unwrap();
    let config = UploadConfig {
        max_files_per_batch: 750,
        max_bytes_per_file: 128 * 1024 * 1024,
        max_bytes_per_batch: 3 * 1024 * 1024 * 1024,
        client_concurrency: 4,
        upload_batch_expiry_hours: 24,
    };
    let mut tx = pool.begin().await.unwrap();
    let change = upload_settings::set_tx(&mut tx, &config, None)
        .await
        .unwrap();
    assert_eq!(change.before, previous);
    assert_eq!(change.after, config);
    tx.commit().await.unwrap();
    assert_eq!(upload_settings::get(&pool).await.unwrap(), config);

    let mut tx = pool.begin().await.unwrap();
    let invalid = UploadConfig {
        client_concurrency: 0,
        ..config.clone()
    };
    assert!(upload_settings::set_tx(&mut tx, &invalid, None)
        .await
        .is_err());
    tx.rollback().await.unwrap();
    assert_eq!(upload_settings::get(&pool).await.unwrap(), config);

    let mut tx = pool.begin().await.unwrap();
    upload_settings::set_tx(&mut tx, &previous, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
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

    // —— 建项目 B（独立 owner，U 作为 translator 加入） ——
    let owner_b = users::create_password_user(
        &pool,
        &audit_jobs_unique("tm-owner"),
        None,
        &prts_auth::password::hash_password("password123").unwrap(),
        "active",
    )
    .await
    .unwrap();
    let proj_b = projects::create(
        &pool,
        "itest-tm-b",
        "TM Test B",
        "",
        "public",
        &["zh-Hans".to_string()],
        "en",
        owner_b.id,
    )
    .await
    .unwrap();
    memberships::upsert(&pool, proj_b.id, u.id, "translator")
        .await
        .unwrap();

    // —— 建项目 C（U 不是成员；由另一个 owner 创建，使用直接 INSERT） ——
    // 为了避免引入额外用户，直接插入项目并让 U 不加入成员表
    let owner_c = users::create_password_user(
        &pool,
        &audit_jobs_unique("tm-private-owner"),
        None,
        &prts_auth::password::hash_password("password123").unwrap(),
        "active",
    )
    .await
    .unwrap();
    let proj_c: (i64,) = sqlx::query_as(
        "INSERT INTO projects (slug, name, description, visibility, source_langs, target_lang, owner_id)
         VALUES ('itest-tm-c', 'TM Test C', '', 'public', ARRAY['zh-Hans'], 'en', $1)
         RETURNING id",
    )
    .bind(owner_c.id)
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
