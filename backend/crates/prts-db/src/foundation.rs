//! 项目工作区 foundation readiness 查询。

use sqlx::PgPool;

/// 只有 schema/search revision、reconciliation marker 与 lexical worker 同时就绪才开放主源切换。
pub async fn primary_source_release_ready(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE((
             SELECT schema_revision = 8
                AND primary_search_revision = 9
                AND reconciled_at IS NOT NULL
                AND lexical_worker_registered
             FROM workspace_foundation_state
             WHERE singleton
         ), FALSE)",
    )
    .fetch_one(pool)
    .await
}
