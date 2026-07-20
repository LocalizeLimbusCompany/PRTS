//! 用户与外部账号的数据访问。

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

use crate::models::{ExternalAccount, User};

/// 管理员用户列表的公开排序；每种排序都以 user id 作稳定 tie-break。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminUserSort {
    UsernameAsc,
    UsernameDesc,
    CreatedAtAsc,
    CreatedAtDesc,
}

/// 已验证签名后的数据库键集边界。
#[derive(Debug, Clone)]
pub enum AdminUserAfter {
    Username { value: String, user_id: i64 },
    CreatedAt { value: DateTime<Utc>, user_id: i64 },
}

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

/// 管理员用户列表。只允许四条静态键集 SQL，不接受 OFFSET。
pub async fn list_admin_users(
    pool: &PgPool,
    q: Option<&str>,
    role: Option<&str>,
    sort: AdminUserSort,
    after: Option<&AdminUserAfter>,
    limit: i64,
) -> Result<Vec<User>, sqlx::Error> {
    let (after_username, after_created_at, after_id) = match after {
        Some(AdminUserAfter::Username { value, user_id }) => {
            (Some(value.as_str()), None, Some(*user_id))
        }
        Some(AdminUserAfter::CreatedAt { value, user_id }) => (None, Some(*value), Some(*user_id)),
        None => (None, None, None),
    };
    let common =
        "FROM users
         WHERE ($1::TEXT IS NULL
                OR lower(username) LIKE '%' || prts_escape_like_pattern(lower($1)) || '%' ESCAPE '\\')
           AND ($2::TEXT IS NULL
                OR ($2 = 'user' AND platform_role IS NULL)
                OR platform_role = $2)";
    let sql = match sort {
        AdminUserSort::UsernameAsc => format!(
            "SELECT * {common}
             AND ($3::TEXT IS NULL OR (lower(username), id) > (lower($3), $4))
             ORDER BY lower(username) ASC, id ASC LIMIT $5"
        ),
        AdminUserSort::UsernameDesc => format!(
            "SELECT * {common}
             AND ($3::TEXT IS NULL OR (lower(username), id) < (lower($3), $4))
             ORDER BY lower(username) DESC, id DESC LIMIT $5"
        ),
        AdminUserSort::CreatedAtAsc => format!(
            "SELECT * {common}
             AND ($3::TIMESTAMPTZ IS NULL OR (created_at, id) > ($3, $4))
             ORDER BY created_at ASC, id ASC LIMIT $5"
        ),
        AdminUserSort::CreatedAtDesc => format!(
            "SELECT * {common}
             AND ($3::TIMESTAMPTZ IS NULL OR (created_at, id) < ($3, $4))
             ORDER BY created_at DESC, id DESC LIMIT $5"
        ),
    };
    match sort {
        AdminUserSort::UsernameAsc | AdminUserSort::UsernameDesc => {
            sqlx::query_as::<_, User>(&sql)
                .bind(q)
                .bind(role)
                .bind(after_username)
                .bind(after_id)
                .bind(limit)
                .fetch_all(pool)
                .await
        }
        AdminUserSort::CreatedAtAsc | AdminUserSort::CreatedAtDesc => {
            sqlx::query_as::<_, User>(&sql)
                .bind(q)
                .bind(role)
                .bind(after_created_at)
                .bind(after_id)
                .bind(limit)
                .fetch_all(pool)
                .await
        }
    }
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

/// 管理员在调用方事务内创建强制改密账号；明文密码从未进入仓储层。
pub async fn create_admin_password_user_tx(
    conn: &mut PgConnection,
    username: &str,
    password_hash: &str,
    role: Option<&str>,
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "INSERT INTO users (
             username, password_hash, status, platform_role, password_change_required
         ) VALUES ($1, $2, 'active', $3, TRUE)
         RETURNING *",
    )
    .bind(username)
    .bind(password_hash)
    .bind(role)
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

/// 在调用方事务内更新词条历史差分偏好。
pub async fn update_entry_diff_mode_tx(
    conn: &mut PgConnection,
    id: i64,
    entry_diff_mode: &str,
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>("UPDATE users SET entry_diff_mode = $2 WHERE id = $1 RETURNING *")
        .bind(id)
        .bind(entry_diff_mode)
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

/// 在已锁定用户的调用方事务内替换密码哈希并持久清除改密提醒。
pub async fn update_password_tx(
    conn: &mut PgConnection,
    id: i64,
    password_hash: &str,
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "UPDATE users
         SET password_hash = $2, password_change_required = FALSE
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .bind(password_hash)
    .fetch_one(conn)
    .await
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
