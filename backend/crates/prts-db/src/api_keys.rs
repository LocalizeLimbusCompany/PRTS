//! API Key 数据访问。库中仅存哈希。

use sqlx::PgPool;

use crate::models::{ApiKeyRecord, User};

/// 创建一条 API Key 记录。
pub async fn create(
    pool: &PgPool,
    user_id: i64,
    name: &str,
    key_hash: &str,
    prefix: &str,
) -> Result<ApiKeyRecord, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "INSERT INTO api_keys (user_id, name, key_hash, prefix)
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(user_id)
    .bind(name)
    .bind(key_hash)
    .bind(prefix)
    .fetch_one(pool)
    .await
}

/// 列出某用户的全部 API Key。
pub async fn list_by_user(pool: &PgPool, user_id: i64) -> Result<Vec<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "SELECT * FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// 按 Key 哈希查找其所属用户（用于 API-Key 鉴权）。
pub async fn find_user_by_key_hash(
    pool: &PgPool,
    key_hash: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT u.* FROM users u
         JOIN api_keys k ON k.user_id = u.id
         WHERE k.key_hash = $1",
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await
}

/// 更新 Key 的最近使用时间（鉴权成功后调用，best-effort）。
pub async fn touch_last_used(pool: &PgPool, key_hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE key_hash = $1")
        .bind(key_hash)
        .execute(pool)
        .await
        .map(|_| ())
}

/// 吊销（删除）某用户的一条 Key。返回是否确有删除。
pub async fn revoke(pool: &PgPool, user_id: i64, id: i64) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM api_keys WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
