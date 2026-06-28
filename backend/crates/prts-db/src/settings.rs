//! 平台设置数据访问（key → JSONB）。

use sqlx::PgPool;

use crate::models::Setting;

/// 读取某项设置的值。
pub async fn get(pool: &PgPool, key: &str) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v))
}

/// 写入 / 更新某项设置（upsert）。
pub async fn set(
    pool: &PgPool,
    key: &str,
    value: &serde_json::Value,
    updated_by: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_by)
         VALUES ($1, $2, $3)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value,
                                          updated_by = EXCLUDED.updated_by,
                                          updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .bind(updated_by)
    .execute(pool)
    .await
    .map(|_| ())
}

/// 列出全部设置。
pub async fn list_all(pool: &PgPool) -> Result<Vec<Setting>, sqlx::Error> {
    sqlx::query_as::<_, Setting>("SELECT * FROM settings ORDER BY key")
        .fetch_all(pool)
        .await
}
