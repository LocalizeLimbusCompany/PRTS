//! 项目成员数据访问。

use sqlx::PgPool;

use crate::models::MemberInfo;

/// 新增或更新成员角色（upsert）。
pub async fn upsert(
    pool: &PgPool,
    project_id: i64,
    user_id: i64,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO memberships (project_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (project_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await
    .map(|_| ())
}

/// 查询某用户在某项目的角色。
pub async fn find_role(
    pool: &PgPool,
    project_id: i64,
    user_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role FROM memberships WHERE project_id = $1 AND user_id = $2")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(r,)| r))
}

/// 列出项目成员（含用户名/头像）。
pub async fn list(pool: &PgPool, project_id: i64) -> Result<Vec<MemberInfo>, sqlx::Error> {
    sqlx::query_as::<_, MemberInfo>(
        "SELECT u.id AS user_id, u.username, u.avatar_url, m.role, m.created_at
         FROM memberships m JOIN users u ON u.id = m.user_id
         WHERE m.project_id = $1
         ORDER BY m.created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// 移除成员。
pub async fn remove(pool: &PgPool, project_id: i64, user_id: i64) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM memberships WHERE project_id = $1 AND user_id = $2")
        .bind(project_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// 统计项目中某角色的人数（用于防止移除最后一个 owner）。
pub async fn count_role(pool: &PgPool, project_id: i64, role: &str) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memberships WHERE project_id = $1 AND role = $2")
            .bind(project_id)
            .bind(role)
            .fetch_one(pool)
            .await?;
    Ok(count)
}
