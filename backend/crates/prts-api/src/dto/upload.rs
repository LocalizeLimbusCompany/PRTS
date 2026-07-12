//! 上传设置的共享 OpenAPI DTO；普通 meta 与管理端使用同一响应结构。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use prts_db::upload_settings::UploadConfig;

/// 上传客户端运行时限制；字节单位避免 MB/GB 歧义。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UploadConfigDto {
    pub max_files_per_batch: i32,
    pub max_bytes_per_file: i64,
    pub max_bytes_per_batch: i64,
    pub client_concurrency: i32,
}

impl From<UploadConfig> for UploadConfigDto {
    fn from(config: UploadConfig) -> Self {
        Self {
            max_files_per_batch: config.max_files_per_batch,
            max_bytes_per_file: config.max_bytes_per_file,
            max_bytes_per_batch: config.max_bytes_per_batch,
            client_concurrency: config.client_concurrency,
        }
    }
}
