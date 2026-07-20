//! 词条评论数据访问；列表使用倒序键集分页，删除保留占位和审计关联。

use sqlx::{PgConnection, PgPool};

use crate::models::EntryComment;

/// 按词条倒序列出评论；删除项仍返回，由 API 层输出删除占位。
pub async fn list(
    pool: &PgPool,
    project_id: i64,
    entry_id: i64,
    after: Option<i64>,
    limit: i64,
) -> Result<Vec<EntryComment>, sqlx::Error> {
    sqlx::query_as::<_, EntryComment>(
        "SELECT * FROM entry_comments
         WHERE project_id = $1 AND entry_id = $2
           AND ($3::BIGINT IS NULL OR id < $3)
         ORDER BY id DESC LIMIT $4",
    )
    .bind(project_id)
    .bind(entry_id)
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 在调用方事务内新增评论并快照作者展示信息。
pub async fn create_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
    author_id: i64,
    author_name: &str,
    author_avatar_url: Option<&str>,
    content: &str,
) -> Result<EntryComment, sqlx::Error> {
    sqlx::query_as::<_, EntryComment>(
        "INSERT INTO entry_comments (
             project_id, entry_id, author_id, author_name, author_avatar_url, content
         ) VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(project_id)
    .bind(entry_id)
    .bind(author_id)
    .bind(author_name)
    .bind(author_avatar_url)
    .bind(content)
    .fetch_one(conn)
    .await
}

/// 锁定 URL project/entry 绑定的评论。
pub async fn find_for_update_tx(
    conn: &mut PgConnection,
    project_id: i64,
    entry_id: i64,
    comment_id: i64,
) -> Result<Option<EntryComment>, sqlx::Error> {
    sqlx::query_as::<_, EntryComment>(
        "SELECT * FROM entry_comments
         WHERE project_id = $1 AND entry_id = $2 AND id = $3 FOR UPDATE",
    )
    .bind(project_id)
    .bind(entry_id)
    .bind(comment_id)
    .fetch_optional(conn)
    .await
}

/// 作者更新未删除评论正文。
pub async fn update_tx(
    conn: &mut PgConnection,
    comment_id: i64,
    content: &str,
) -> Result<EntryComment, sqlx::Error> {
    sqlx::query_as::<_, EntryComment>(
        "UPDATE entry_comments SET content = $2
         WHERE id = $1 AND deleted_at IS NULL RETURNING *",
    )
    .bind(comment_id)
    .bind(content)
    .fetch_one(conn)
    .await
}

/// 软删除评论；正文清空，避免删除后继续下发内容。
pub async fn delete_tx(
    conn: &mut PgConnection,
    comment_id: i64,
    actor_id: i64,
) -> Result<EntryComment, sqlx::Error> {
    sqlx::query_as::<_, EntryComment>(
        "UPDATE entry_comments
         SET content = '', deleted_at = now(), deleted_by = $2
         WHERE id = $1 AND deleted_at IS NULL RETURNING *",
    )
    .bind(comment_id)
    .bind(actor_id)
    .fetch_one(conn)
    .await
}
