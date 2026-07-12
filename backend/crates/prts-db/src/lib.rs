//! `prts-db` —— 数据访问基础设施。
//!
//! 提供 PostgreSQL 连接池、Redis 连接管理器、迁移执行与健康探测，
//! 以及账号 / API Key / 设置等数据访问（[`models`] + 各仓储模块）。

pub mod api_keys;
pub mod audit;
pub mod auth_sessions;
pub mod entries;
pub mod files;
pub mod foundation;
pub mod jobs;
pub mod language_resolution;
pub mod memberships;
pub mod messages;
pub mod models;
pub mod notifications;
pub mod projects;
pub mod search;
pub mod search_settings;
pub mod settings;
pub mod stats;
pub mod upload_settings;
pub mod uploads;
pub mod users;

/// 数据库错误别名，便于上层映射而无需直接依赖 sqlx。
pub use sqlx::Error as DbError;

use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use sqlx::{FromRow, PgConnection, PgPool};

/// PostgreSQL 连接池别名。
pub type Db = PgPool;
/// Redis 连接管理器别名（内部多路复用、自动重连、可廉价 clone）。
pub type Cache = ConnectionManager;

/// 创建 PostgreSQL 连接池。
pub async fn connect_postgres(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(url)
        .await
}

/// 创建 Redis 连接管理器。
pub async fn connect_redis(url: &str) -> Result<ConnectionManager, redis::RedisError> {
    let client = redis::Client::open(url)?;
    ConnectionManager::new(client).await
}

/// 在独立 owner 连接上运行嵌入式迁移，并把 runtime role 安全传给迁移。
///
/// `set_config` 与 migrator 必须使用同一连接；迁移 SQL 只通过
/// `current_setting('prts.runtime_role')` + `format('%I', ...)` 引用标识符。
pub async fn run_migrations(
    conn: &mut PgConnection,
    runtime_role: &str,
) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::query("SELECT set_config('prts.runtime_role', $1, false)")
        .bind(runtime_role)
        .execute(&mut *conn)
        .await
        .map_err(sqlx::migrate::MigrateError::Execute)?;
    sqlx::migrate!("../../migrations").run(&mut *conn).await
}

/// 验证应用池确实使用无 owner 权限的预期 runtime role。
///
/// 缺表、角色不匹配、superuser、持有 public 对象，或 audit ACL 过宽都会 fail closed。
#[derive(Debug, Clone, FromRow)]
struct RuntimeRoleChecks {
    current_role: String,
    matches_expected: bool,
    not_superuser: bool,
    not_createdb: bool,
    not_createrole: bool,
    not_replication: bool,
    not_bypassrls: bool,
    owns_no_public_objects: bool,
    cannot_create_in_public: bool,
    audit_select: bool,
    audit_insert: bool,
    audit_no_update: bool,
    audit_no_delete: bool,
}

impl RuntimeRoleChecks {
    fn is_safe(&self) -> bool {
        self.matches_expected
            && self.not_superuser
            && self.not_createdb
            && self.not_createrole
            && self.not_replication
            && self.not_bypassrls
            && self.owns_no_public_objects
            && self.cannot_create_in_public
            && self.audit_select
            && self.audit_insert
            && self.audit_no_update
            && self.audit_no_delete
    }
}

pub async fn verify_runtime_role(pool: &PgPool, expected_role: &str) -> Result<(), sqlx::Error> {
    let checks: RuntimeRoleChecks = sqlx::query_as(
        "SELECT current_user::TEXT AS current_role,
                current_user = $1 AS matches_expected,
                NOT role.rolsuper AS not_superuser,
                NOT role.rolcreatedb AS not_createdb,
                NOT role.rolcreaterole AS not_createrole,
                NOT role.rolreplication AS not_replication,
                NOT role.rolbypassrls AS not_bypassrls,
                NOT EXISTS (
                    SELECT 1
                    FROM pg_class AS class
                    JOIN pg_namespace AS namespace ON namespace.oid = class.relnamespace
                    WHERE namespace.nspname = 'public'
                      AND class.relowner = role.oid
                ) AS owns_no_public_objects,
                NOT has_schema_privilege(current_user, 'public', 'CREATE')
                    AS cannot_create_in_public,
                has_table_privilege(current_user, 'audit_log', 'SELECT') AS audit_select,
                has_table_privilege(current_user, 'audit_log', 'INSERT') AS audit_insert,
                NOT has_table_privilege(current_user, 'audit_log', 'UPDATE') AS audit_no_update,
                NOT has_table_privilege(current_user, 'audit_log', 'DELETE') AS audit_no_delete
         FROM pg_roles AS role
         WHERE role.rolname = current_user",
    )
    .bind(expected_role)
    .fetch_one(pool)
    .await?;
    if checks.is_safe() {
        Ok(())
    } else {
        Err(sqlx::Error::Protocol(format!(
            "database role separation check failed for runtime role {}",
            checks.current_role
        )))
    }
}

/// 探测 PostgreSQL 可用性（就绪检查）。
pub async fn ping_postgres(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

/// 探测 Redis 可用性（就绪检查）。
pub async fn ping_redis(cache: &ConnectionManager) -> Result<(), redis::RedisError> {
    let mut conn = cache.clone();
    let _: String = redis::cmd("PING").query_async(&mut conn).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_role_checks_fail_closed_for_every_privileged_attribute() {
        let safe = RuntimeRoleChecks {
            current_role: "prts_runtime".to_string(),
            matches_expected: true,
            not_superuser: true,
            not_createdb: true,
            not_createrole: true,
            not_replication: true,
            not_bypassrls: true,
            owns_no_public_objects: true,
            cannot_create_in_public: true,
            audit_select: true,
            audit_insert: true,
            audit_no_update: true,
            audit_no_delete: true,
        };
        assert!(safe.is_safe());

        for unsafe_checks in [
            RuntimeRoleChecks {
                not_createdb: false,
                ..safe.clone()
            },
            RuntimeRoleChecks {
                not_createrole: false,
                ..safe.clone()
            },
            RuntimeRoleChecks {
                not_replication: false,
                ..safe.clone()
            },
            RuntimeRoleChecks {
                not_bypassrls: false,
                ..safe.clone()
            },
            RuntimeRoleChecks {
                cannot_create_in_public: false,
                ..safe.clone()
            },
        ] {
            assert!(!unsafe_checks.is_safe());
        }
    }
}
