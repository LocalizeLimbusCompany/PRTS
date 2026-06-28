//! 健康探测端点。

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

/// 存活状态。
#[derive(Serialize, ToSchema)]
pub struct HealthStatus {
    /// 固定为 `"ok"`。
    pub status: String,
}

/// 存活探测（liveness）：进程存活即返回 200，不依赖外部资源。
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses((status = 200, description = "服务存活", body = HealthStatus))
)]
pub async fn liveness() -> Json<HealthStatus> {
    Json(HealthStatus {
        status: "ok".to_string(),
    })
}

/// 就绪状态：各依赖是否可用。
#[derive(Serialize, ToSchema)]
pub struct Readiness {
    /// PostgreSQL 是否可用。
    pub database: bool,
    /// Redis 是否可用。
    pub redis: bool,
}

/// 就绪探测（readiness）：检查 PostgreSQL 与 Redis。任一不可用则返回 503。
#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    responses(
        (status = 200, description = "已就绪", body = Readiness),
        (status = 503, description = "未就绪", body = Readiness),
    )
)]
pub async fn readiness(State(state): State<AppState>) -> impl IntoResponse {
    let database = prts_db::ping_postgres(&state.db).await.is_ok();
    let redis = prts_db::ping_redis(&state.cache).await.is_ok();

    let status = if database && redis {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(Readiness { database, redis }))
}
