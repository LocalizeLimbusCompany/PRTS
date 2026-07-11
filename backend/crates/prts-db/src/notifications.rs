//! 通知仓储：收件人维度的通知增查改（键集分页）。

use sqlx::{PgConnection, PgPool};

use crate::models::Notification;

/// 为某收件人创建一条通知（`kind` 即 SQL `type` 列）。
pub async fn create(
    pool: &PgPool,
    user_id: i64,
    kind: &str,
    payload: &serde_json::Value,
) -> Result<Notification, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    create_tx(&mut connection, user_id, kind, payload).await
}

/// 在调用方事务内创建通知。
pub async fn create_tx(
    conn: &mut PgConnection,
    user_id: i64,
    kind: &str,
    payload: &serde_json::Value,
) -> Result<Notification, sqlx::Error> {
    sqlx::query_as::<_, Notification>(
        "INSERT INTO notifications (user_id, type, payload) VALUES ($1, $2, $3)
         RETURNING id, user_id, type, payload, read_at, created_at",
    )
    .bind(user_id)
    .bind(kind)
    .bind(payload)
    .fetch_one(conn)
    .await
}

/// 键集分页：按 id 降序返回；`before_id` 为游标（返回比它更旧的条目）。
///
/// `limit` 会被夹取到 `[1, 100]` 范围内。
pub async fn list(
    pool: &PgPool,
    user_id: i64,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<Notification>, sqlx::Error> {
    let limit = limit.clamp(1, 100);
    match before_id {
        Some(before) => {
            sqlx::query_as::<_, Notification>(
                "SELECT id, user_id, type, payload, read_at, created_at FROM notifications
                 WHERE user_id = $1 AND id < $2 ORDER BY id DESC LIMIT $3",
            )
            .bind(user_id)
            .bind(before)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, Notification>(
                "SELECT id, user_id, type, payload, read_at, created_at FROM notifications
                 WHERE user_id = $1 ORDER BY id DESC LIMIT $2",
            )
            .bind(user_id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
    }
}

/// 某收件人的未读通知数。
pub async fn unread_count(pool: &PgPool, user_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL")
        .bind(user_id)
        .fetch_one(pool)
        .await
}

/// 标记通知为已读；`ids` 为空表示标记该用户全部未读为已读。
pub async fn mark_read(pool: &PgPool, user_id: i64, ids: &[i64]) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    mark_read_tx(&mut connection, user_id, ids)
        .await
        .map(|_| ())
}

/// 在调用方事务内标记通知已读，并返回实际更新数供审计。
pub async fn mark_read_tx(
    conn: &mut PgConnection,
    user_id: i64,
    ids: &[i64],
) -> Result<u64, sqlx::Error> {
    let result = if ids.is_empty() {
        sqlx::query(
            "UPDATE notifications SET read_at = now() WHERE user_id = $1 AND read_at IS NULL",
        )
        .bind(user_id)
        .execute(&mut *conn)
        .await?
    } else {
        sqlx::query(
            "UPDATE notifications SET read_at = now()
             WHERE user_id = $1 AND id = ANY($2) AND read_at IS NULL",
        )
        .bind(user_id)
        .bind(ids)
        .execute(conn)
        .await?
    };
    Ok(result.rows_affected())
}
