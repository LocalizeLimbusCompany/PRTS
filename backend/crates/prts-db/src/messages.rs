//! 私信仓储：一对用户间的会话消息（键集分页）、会话列表、已读标记与未读统计。
//!
//! 会话不落独立表：`list_conversation` 用正反双向条件取一对用户之间的消息；
//! `list_threads` 聚合出每个对话方的最后一条消息与我方未读数。
//! 收发双方须共享 ≥1 项目的门限校验在应用层（`prts-api`）完成，本层只管存取。

use sqlx::{PgConnection, PgPool};

use crate::models::{ConversationThread, Message};

/// 落库一条私信并返回完整行。
pub async fn create(
    pool: &PgPool,
    sender_id: i64,
    recipient_id: i64,
    content: &str,
) -> Result<Message, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    create_tx(&mut connection, sender_id, recipient_id, content).await
}

/// 在调用方事务内创建私信。
pub async fn create_tx(
    conn: &mut PgConnection,
    sender_id: i64,
    recipient_id: i64,
    content: &str,
) -> Result<Message, sqlx::Error> {
    sqlx::query_as::<_, Message>(
        "INSERT INTO messages (sender_id, recipient_id, content) VALUES ($1, $2, $3)
         RETURNING id, sender_id, recipient_id, content, read_at, created_at",
    )
    .bind(sender_id)
    .bind(recipient_id)
    .bind(content)
    .fetch_one(conn)
    .await
}

/// 一对用户（`me` ↔ `other`）之间的会话消息，按 id 降序（较新在前）。
///
/// 键集分页：`before_id` 为游标，返回比它更旧（id 更小）的消息；`limit` 夹取到 `[1, 100]`。
/// 双向条件命中 `messages_pair_idx` / `messages_pair_rev_idx`，避免大 `OFFSET`。
pub async fn list_conversation(
    pool: &PgPool,
    me: i64,
    other: i64,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<Message>, sqlx::Error> {
    let limit = limit.clamp(1, 100);
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id, sender_id, recipient_id, content, read_at, created_at FROM messages
         WHERE ((sender_id = ",
    );
    qb.push_bind(me)
        .push(" AND recipient_id = ")
        .push_bind(other)
        .push(") OR (sender_id = ")
        .push_bind(other)
        .push(" AND recipient_id = ")
        .push_bind(me)
        .push("))");
    if let Some(before) = before_id {
        qb.push(" AND id < ").push_bind(before);
    }
    qb.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    qb.build_query_as::<Message>().fetch_all(pool).await
}

/// 会话列表：每个对话方一行——对方资料 + 该会话最后一条消息 + 我方未读数。
///
/// `DISTINCT ON (other)` 取每个对话方 id 最大（最新）的一条消息；外层按最后一条消息 id 降序，
/// 使最近有来往的会话排在最前。
pub async fn list_threads(pool: &PgPool, me: i64) -> Result<Vec<ConversationThread>, sqlx::Error> {
    sqlx::query_as::<_, ConversationThread>(
        "WITH convo AS (
             SELECT CASE WHEN sender_id = $1 THEN recipient_id ELSE sender_id END AS other,
                    id, content, sender_id, created_at
             FROM messages
             WHERE sender_id = $1 OR recipient_id = $1
         ),
         last AS (
             SELECT DISTINCT ON (other) other, id, content, sender_id, created_at
             FROM convo
             ORDER BY other, id DESC
         )
         SELECT l.other AS other_user_id,
                u.username,
                u.avatar_url,
                l.content AS last_content,
                l.sender_id AS last_sender_id,
                l.created_at AS last_created_at,
                (SELECT COUNT(*) FROM messages m
                 WHERE m.recipient_id = $1 AND m.sender_id = l.other AND m.read_at IS NULL) AS unread
         FROM last l
         JOIN users u ON u.id = l.other
         ORDER BY l.id DESC",
    )
    .bind(me)
    .fetch_all(pool)
    .await
}

/// 将某会话中「对方 `other` → 我 `me`」尚未读的消息全部标记为已读。
pub async fn mark_read(pool: &PgPool, me: i64, other: i64) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    mark_read_tx(&mut connection, me, other).await.map(|_| ())
}

/// 在调用方事务内标记私信已读，并返回实际更新数供审计。
pub async fn mark_read_tx(
    conn: &mut PgConnection,
    me: i64,
    other: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE messages SET read_at = now()
         WHERE recipient_id = $1 AND sender_id = $2 AND read_at IS NULL",
    )
    .bind(me)
    .bind(other)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

/// 我方（`me`）全部未读私信总数（跨所有会话）。
pub async fn unread_count(pool: &PgPool, me: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE recipient_id = $1 AND read_at IS NULL")
        .bind(me)
        .fetch_one(pool)
        .await
}
