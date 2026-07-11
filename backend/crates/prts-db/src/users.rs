//! 用户与外部账号的数据访问。

use sqlx::{PgConnection, PgPool};

use crate::models::{ExternalAccount, User};

/// 按 id 查找用户。
pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 在调用方事务内锁定用户并返回一致快照。
pub async fn find_by_id_for_update_tx(
    conn: &mut PgConnection,
    id: i64,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// 按用户名查找。
pub async fn find_by_username(pool: &PgPool, username: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
}

/// 在调用方事务内按用户名锁定用户，供启动期角色变更与审计复用同一快照。
pub async fn find_by_username_for_update_tx(
    conn: &mut PgConnection,
    username: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1 FOR UPDATE")
        .bind(username)
        .fetch_optional(conn)
        .await
}

/// 按邮箱查找。
pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await
}

/// 用户名是否已存在。
pub async fn username_exists(pool: &PgPool, username: &str) -> Result<bool, sqlx::Error> {
    let (exists,): (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
            .bind(username)
            .fetch_one(pool)
            .await?;
    Ok(exists)
}

/// 创建账号密码用户。`status` 取 `active` 或 `pending`（待邮箱验证）。
pub async fn create_password_user(
    pool: &PgPool,
    username: &str,
    email: Option<&str>,
    password_hash: &str,
    status: &str,
) -> Result<User, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    create_password_user_tx(&mut connection, username, email, password_hash, status).await
}

/// 在调用方事务内创建账号密码用户。
pub async fn create_password_user_tx(
    conn: &mut PgConnection,
    username: &str,
    email: Option<&str>,
    password_hash: &str,
    status: &str,
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "INSERT INTO users (username, email, password_hash, status)
         VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(username)
    .bind(email)
    .bind(password_hash)
    .bind(status)
    .fetch_one(conn)
    .await
}

/// 更新个人资料（描述 / 头像 / 翻译语言偏好）。
pub async fn update_profile(
    pool: &PgPool,
    id: i64,
    description: &str,
    avatar_url: Option<&str>,
    translation_langs: &[String],
) -> Result<User, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    update_profile_tx(
        &mut connection,
        id,
        description,
        avatar_url,
        translation_langs,
    )
    .await
}

/// 在调用方事务内更新个人资料。
pub async fn update_profile_tx(
    conn: &mut PgConnection,
    id: i64,
    description: &str,
    avatar_url: Option<&str>,
    translation_langs: &[String],
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "UPDATE users SET description = $2, avatar_url = $3, translation_langs = $4
         WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(description)
    .bind(avatar_url)
    .bind(translation_langs)
    .fetch_one(conn)
    .await
}

/// 设置 / 清除平台角色（`None` 表示降为普通用户）。
pub async fn set_platform_role(
    pool: &PgPool,
    id: i64,
    role: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    set_platform_role_tx(&mut connection, id, role).await
}

/// 在调用方事务内设置或清除平台角色。
pub async fn set_platform_role_tx(
    conn: &mut PgConnection,
    id: i64,
    role: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET platform_role = $2 WHERE id = $1")
        .bind(id)
        .bind(role)
        .execute(conn)
        .await
        .map(|_| ())
}

/// 标记邮箱已验证并激活账号。
pub async fn mark_email_verified(pool: &PgPool, id: i64) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    mark_email_verified_tx(&mut connection, id).await
}

/// 在调用方事务内验证邮箱并激活账号。
pub async fn mark_email_verified_tx(conn: &mut PgConnection, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET email_verified = TRUE, status = 'active' WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await
        .map(|_| ())
}

/// 按外部身份查找已关联的用户。
pub async fn find_by_external(
    pool: &PgPool,
    provider: &str,
    external_id: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT u.* FROM users u
         JOIN external_accounts e ON e.user_id = u.id
         WHERE e.provider = $1 AND e.external_id = $2",
    )
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await
}

/// 列出某用户的外部关联账号。
pub async fn list_external_accounts(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<ExternalAccount>, sqlx::Error> {
    sqlx::query_as::<_, ExternalAccount>(
        "SELECT * FROM external_accounts WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// 创建纯 OAuth 用户并同时建立外部账号关联（事务）。
pub async fn create_oauth_user(
    pool: &PgPool,
    username: &str,
    avatar_url: Option<&str>,
    provider: &str,
    external_id: &str,
    raw: &serde_json::Value,
) -> Result<User, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let user =
        create_oauth_user_tx(&mut tx, username, avatar_url, provider, external_id, raw).await?;
    tx.commit().await?;
    Ok(user)
}

/// 在调用方事务内创建 OAuth 用户与外部账号关联。
pub async fn create_oauth_user_tx(
    conn: &mut PgConnection,
    username: &str,
    avatar_url: Option<&str>,
    provider: &str,
    external_id: &str,
    raw: &serde_json::Value,
) -> Result<User, sqlx::Error> {
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (username, avatar_url, status, email_verified)
         VALUES ($1, $2, 'active', TRUE) RETURNING *",
    )
    .bind(username)
    .bind(avatar_url)
    .fetch_one(&mut *conn)
    .await?;

    sqlx::query(
        "INSERT INTO external_accounts (user_id, provider, external_id, raw)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user.id)
    .bind(provider)
    .bind(external_id)
    .bind(raw)
    .execute(conn)
    .await?;
    Ok(user)
}
