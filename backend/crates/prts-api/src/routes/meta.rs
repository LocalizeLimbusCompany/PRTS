//! 服务元信息端点。

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use crate::db_err;
use crate::dto::upload::UploadConfigDto;
use crate::error::ApiError;
use crate::state::AppState;

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

/// 返回当前上传客户端限制，不包含清理保留期或内部存储路径。
#[utoipa::path(
    get,
    path = "/meta/upload-config",
    tag = "meta",
    description = "返回新流式上传客户端的文件数、单文件字节数、批次字节数和浏览器并发限制；不返回内部清理周期、临时卷路径或任何密钥。",
    responses((status = 200, description = "上传客户端运行时限制", body = UploadConfigDto))
)]
pub async fn upload_config(
    State(state): State<AppState>,
) -> Result<Json<UploadConfigDto>, ApiError> {
    let config = prts_db::upload_settings::get(&state.db)
        .await
        .map_err(db_err)?;
    Ok(Json(config.into()))
}
