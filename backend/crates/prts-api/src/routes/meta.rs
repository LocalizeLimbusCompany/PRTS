//! 服务元信息端点。

use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

/// 服务版本信息。
#[derive(Serialize, ToSchema)]
pub struct VersionInfo {
    /// 服务名（crate 名）。
    pub name: String,
    /// 语义化版本（来自 Cargo 包版本）。
    pub version: String,
}

/// 返回服务名与版本。
#[utoipa::path(
    get,
    path = "/version",
    tag = "meta",
    responses((status = 200, description = "服务版本", body = VersionInfo))
)]
pub async fn version() -> Json<VersionInfo> {
    Json(VersionInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
