//! API Key 数据访问。库中仅存哈希。

use sqlx::{PgConnection, PgPool};

use crate::models::{ApiKeyRecord, User};

/// 创建一条 API Key 记录。
pub async fn create(
    pool: &PgPool,
    user_id: i64,
    name: &str,
    key_hash: &str,
    prefix: &str,
    scopes: &[String],
) -> Result<ApiKeyRecord, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    create_tx(&mut connection, user_id, name, key_hash, prefix, scopes).await
}

/// 在调用方事务内创建 API Key 记录。
pub async fn create_tx(
    conn: &mut PgConnection,
    user_id: i64,
    name: &str,
    key_hash: &str,
    prefix: &str,
    scopes: &[String],
) -> Result<ApiKeyRecord, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "INSERT INTO api_keys (user_id, name, key_hash, prefix, scopes)
         VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(user_id)
    .bind(name)
    .bind(key_hash)
    .bind(prefix)
    .bind(scopes)
    .fetch_one(conn)
    .await
}

/// Update a key's display name and scopes without rotating its secret.
pub async fn update_tx(
    conn: &mut PgConnection,
    user_id: i64,
    id: i64,
    name: &str,
    scopes: &[String],
) -> Result<Option<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "UPDATE api_keys SET name = $3, scopes = $4
         WHERE id = $1 AND user_id = $2 RETURNING *",
    )
    .bind(id)
    .bind(user_id)
    .bind(name)
    .bind(scopes)
    .fetch_optional(conn)
    .await
}

/// Return the scope groups the account can meaningfully grant to a key right now.
pub async fn available_scopes(pool: &PgPool, user_id: i64) -> Result<Vec<String>, sqlx::Error> {
    let (has_membership, can_write_project, can_manage_project, platform_role): (
        bool,
        bool,
        bool,
        Option<String>,
    ) =
        sqlx::query_as(
            "SELECT
                 EXISTS(SELECT 1 FROM memberships WHERE user_id = $1),
                 EXISTS(SELECT 1 FROM memberships WHERE user_id = $1 AND role IN ('owner', 'manager', 'reviewer')),
                 EXISTS(SELECT 1 FROM memberships WHERE user_id = $1 AND role IN ('owner', 'manager')),
                 (SELECT platform_role FROM users WHERE id = $1)",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    let mut scopes = vec![
        prts_core::api_scope::ALL.to_string(),
        prts_core::api_scope::PROFILE_READ.to_string(),
        prts_core::api_scope::PROFILE_WRITE.to_string(),
        prts_core::api_scope::PROJECT_READ.to_string(),
        prts_core::api_scope::MESSAGE_READ.to_string(),
        prts_core::api_scope::MESSAGE_WRITE.to_string(),
    ];
    if has_membership {
        scopes.push(prts_core::api_scope::ENTRY_WRITE.to_string());
        scopes.push(prts_core::api_scope::AI_USE.to_string());
    }
    if can_write_project || platform_role.is_some() {
        scopes.push(prts_core::api_scope::PROJECT_WRITE.to_string());
    }
    if can_manage_project || matches!(platform_role.as_deref(), Some("super_admin" | "admin")) {
        scopes.push(prts_core::api_scope::PROJECT_MANAGE.to_string());
    }
    if matches!(platform_role.as_deref(), Some("super_admin" | "admin")) {
        scopes.push(prts_core::api_scope::PLATFORM_MANAGE.to_string());
    }
    Ok(scopes)
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

/// Lock one owned key for an audited update.
pub async fn find_owned_for_update_tx(
    conn: &mut PgConnection,
    user_id: i64,
    id: i64,
) -> Result<Option<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "SELECT * FROM api_keys WHERE id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(conn)
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

/// 按 hash 读取 API Key 记录（不返回明文）。
pub async fn find_by_key_hash(
    pool: &PgPool,
    key_hash: &str,
) -> Result<Option<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>("SELECT * FROM api_keys WHERE key_hash = $1")
        .bind(key_hash)
        .fetch_optional(pool)
        .await
}

/// 更新 Key 的最近使用时间（鉴权成功后调用，best-effort）。
pub async fn touch_last_used(pool: &PgPool, key_hash: &str) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    touch_last_used_tx(&mut connection, key_hash)
        .await
        .map(|_| ())
}

/// 在调用方事务内更新最近使用时间，并返回被更新的安全记录。
pub async fn touch_last_used_tx(
    conn: &mut PgConnection,
    key_hash: &str,
) -> Result<Option<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "UPDATE api_keys SET last_used_at = now() WHERE key_hash = $1 RETURNING *",
    )
    .bind(key_hash)
    .fetch_optional(conn)
    .await
}

/// 吊销（删除）某用户的一条 Key。返回是否确有删除。
pub async fn revoke(pool: &PgPool, user_id: i64, id: i64) -> Result<bool, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    revoke_tx(&mut connection, user_id, id)
        .await
        .map(|record| record.is_some())
}

/// 在调用方事务内吊销 API Key，并返回仅含 hash/prefix 的数据库记录供安全审计取 prefix。
pub async fn revoke_tx(
    conn: &mut PgConnection,
    user_id: i64,
    id: i64,
) -> Result<Option<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "DELETE FROM api_keys WHERE id = $1 AND user_id = $2 RETURNING *",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(conn)
    .await
}
