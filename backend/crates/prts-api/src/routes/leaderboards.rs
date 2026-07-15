//! 项目与平台贡献排行榜。

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_common::Error;
use prts_core::contribution::LeaderboardPeriod;

use crate::auth::{project as paccess, MaybeUser};
use crate::db_err;
use crate::error::{ApiError, ErrorResponse};
use crate::state::AppState;

const LEADERBOARD_LIMIT: i64 = 100;

/// 排行榜用户行；CP 使用 exact tenths，前端负责显示格式。
#[derive(Debug, Serialize, ToSchema)]
pub struct LeaderboardEntryDto {
    pub rank: i64,
    pub user_id: i64,
    pub username: String,
    pub avatar_url: Option<String>,
    pub cp_tenths: i64,
}

/// 排行榜固定响应；周期榜明确返回 UTC `[period_start, period_end)`。
#[derive(Debug, Serialize, ToSchema)]
pub struct LeaderboardResponse {
    pub period: String,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub items: Vec<LeaderboardEntryDto>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PlatformLeaderboardQuery {
    /// all | month | week；month/week 均使用 UTC 自然周期。
    pub period: Option<String>,
}

/// 项目当前成员的累计贡献榜。
#[utoipa::path(
    get,
    path = "/projects/{id}/leaderboard",
    tag = "leaderboard",
    summary = "项目累计贡献榜",
    description = "按当前项目成员的累计 exact-tenths CP 降序返回前 100 名，同分按用户 ID 升序。公开项目允许游客读取；私有项目仅成员或平台管理可见。已移除成员不显示，重新加入会从贡献事件恢复项目累计值。",
    params(("id" = i64, Path, description = "项目 ID")),
    responses(
        (status = 200, body = LeaderboardResponse),
        (status = 404, body = ErrorResponse, description = "项目不存在或不可见")
    )
)]
pub async fn project_leaderboard(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path(project_id): Path<i64>,
) -> Result<Json<LeaderboardResponse>, ApiError> {
    let access = paccess::load(&state, user.as_ref(), project_id).await?;
    access.require_view()?;
    let rows =
        prts_db::contributions::project_leaderboard(&state.db, project_id, LEADERBOARD_LIMIT)
            .await
            .map_err(db_err)?;
    Ok(Json(response("all", None, rows)))
}

/// 平台总榜、UTC 自然月榜与 UTC 周一开始的自然周榜。
#[utoipa::path(
    get,
    path = "/leaderboards/platform",
    tag = "leaderboard",
    summary = "平台贡献排行榜",
    description = "公开返回平台累计总榜或当前 UTC 自然月/自然周前 100 名。period=all 读取用户累计 exact-tenths CP；month/week 从只追加贡献事件按明确 UTC 边界聚合，同分按用户 ID 升序。",
    params(PlatformLeaderboardQuery),
    responses(
        (status = 200, body = LeaderboardResponse),
        (status = 400, body = ErrorResponse, description = "period 不是 all/month/week")
    )
)]
pub async fn platform_leaderboard(
    State(state): State<AppState>,
    Query(query): Query<PlatformLeaderboardQuery>,
) -> Result<Json<LeaderboardResponse>, ApiError> {
    let raw_period = query.period.as_deref().unwrap_or("all");
    let period = LeaderboardPeriod::parse(raw_period)
        .ok_or_else(|| Error::validation("LEADERBOARD_PERIOD_INVALID"))?;
    let bounds = period.bounds(Utc::now());
    let (start, end) = bounds
        .map(|(start, end)| (Some(start), Some(end)))
        .unwrap_or((None, None));
    let rows = prts_db::contributions::platform_leaderboard(
        &state.db,
        period,
        start,
        end,
        LEADERBOARD_LIMIT,
    )
    .await
    .map_err(db_err)?;
    Ok(Json(response(period.as_str(), bounds, rows)))
}

fn response(
    period: &str,
    bounds: Option<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)>,
    rows: Vec<prts_db::contributions::LeaderboardRow>,
) -> LeaderboardResponse {
    LeaderboardResponse {
        period: period.to_string(),
        period_start: bounds.map(|(start, _)| start.to_rfc3339()),
        period_end: bounds.map(|(_, end)| end.to_rfc3339()),
        items: rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| LeaderboardEntryDto {
                rank: i64::try_from(index.saturating_add(1)).unwrap_or(i64::MAX),
                user_id: row.user_id,
                username: row.username,
                avatar_url: row.avatar_url,
                cp_tenths: row.cp_tenths,
            })
            .collect(),
    }
}
