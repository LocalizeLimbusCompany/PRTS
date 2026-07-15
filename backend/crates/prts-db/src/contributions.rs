//! CP 贡献事件、累计分和排行榜查询。

use chrono::{DateTime, Utc};
use prts_core::contribution::{ContributionAward, LeaderboardPeriod};
use sqlx::{FromRow, PgConnection, PgPool};

/// 排行榜单行；名次由 API 在稳定顺序上生成。
#[derive(Debug, Clone, FromRow)]
pub struct LeaderboardRow {
    pub user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub cp_tenths: i64,
}

/// 在词条更新、历史和审计所在事务内追加一次正分事件并更新两级累计值。
pub async fn award_tx(
    conn: &mut PgConnection,
    user_id: i64,
    project_id: i64,
    entry_id: i64,
    entry_version: i64,
    award: ContributionAward,
) -> Result<(), sqlx::Error> {
    if award.cp_tenths == 0 {
        return Ok(());
    }
    let inserted = sqlx::query(
        "INSERT INTO contribution_events (
             user_id, project_id, entry_id, entry_version, kind, distance, cp_tenths
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(user_id)
    .bind(project_id)
    .bind(entry_id)
    .bind(entry_version)
    .bind(award.kind.as_str())
    .bind(award.distance)
    .bind(award.cp_tenths)
    .execute(&mut *conn)
    .await?;
    if inserted.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "contribution event insert affected unexpected rows".into(),
        ));
    }
    sqlx::query(
        "UPDATE memberships
         SET cp_tenths = cp_tenths + $3
         WHERE project_id = $1 AND user_id = $2",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(award.cp_tenths)
    .execute(&mut *conn)
    .await?;
    // 平台管理员可按跨项目管理能力在线编辑但不自动成为项目成员；其平台 CP 与事件仍
    // 正常记录，若之后加入项目，membership INSERT trigger 会从事件账本恢复项目累计值。
    let user = sqlx::query("UPDATE users SET cp_tenths = cp_tenths + $2 WHERE id = $1")
        .bind(user_id)
        .bind(award.cp_tenths)
        .execute(conn)
        .await?;
    if user.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol(
            "contribution actor user is missing".into(),
        ));
    }
    Ok(())
}

/// 项目累计榜只列当前成员，按 exact tenths 降序、user id 升序稳定排列。
pub async fn project_leaderboard(
    pool: &PgPool,
    project_id: i64,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT user_row.id AS user_id, user_row.username, user_row.avatar_url,
                membership.cp_tenths
         FROM memberships AS membership
         JOIN users AS user_row ON user_row.id = membership.user_id
         WHERE membership.project_id = $1 AND membership.cp_tenths > 0
         ORDER BY membership.cp_tenths DESC, user_row.id ASC
         LIMIT $2",
    )
    .bind(project_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 平台累计榜或当前 UTC 自然周期榜。
pub async fn platform_leaderboard(
    pool: &PgPool,
    period: LeaderboardPeriod,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<LeaderboardRow>, sqlx::Error> {
    match period {
        LeaderboardPeriod::All => {
            sqlx::query_as(
                "SELECT user_row.id AS user_id, user_row.username, user_row.avatar_url,
                        user_row.cp_tenths
                 FROM users AS user_row
                 WHERE user_row.status = 'active' AND user_row.cp_tenths > 0
                 ORDER BY user_row.cp_tenths DESC, user_row.id ASC
                 LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        LeaderboardPeriod::Month | LeaderboardPeriod::Week => {
            let start = start.ok_or_else(|| {
                sqlx::Error::Protocol("period leaderboard start is missing".into())
            })?;
            let end = end
                .ok_or_else(|| sqlx::Error::Protocol("period leaderboard end is missing".into()))?;
            sqlx::query_as(
                "SELECT user_row.id AS user_id, user_row.username, user_row.avatar_url,
                        sum(event.cp_tenths)::BIGINT AS cp_tenths
                 FROM contribution_events AS event
                 JOIN users AS user_row ON user_row.id = event.user_id
                 WHERE event.created_at >= $1 AND event.created_at < $2
                   AND user_row.status = 'active'
                 GROUP BY user_row.id, user_row.username, user_row.avatar_url
                 ORDER BY cp_tenths DESC, user_row.id ASC
                 LIMIT $3",
            )
            .bind(start)
            .bind(end)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
    }
}

/// 项目永久清除前删除事件账本正文；平台累计值保留已获得的 CP。
pub async fn delete_project_events_tx(
    conn: &mut PgConnection,
    project_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM contribution_events WHERE project_id = $1")
        .bind(project_id)
        .execute(conn)
        .await?;
    Ok(())
}
