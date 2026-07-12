//! 上传运行时限制；业务默认值、校验和 settings 持久化的唯一来源。

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};

const KEY: &str = "upload.config";
const UPLOAD_CONFIG_LOCK_KEY: i64 = 0x5052_5453_5550_4C44;
const MEBIBYTE: i64 = 1024 * 1024;
const GIBIBYTE: i64 = 1024 * MEBIBYTE;

/// 客户端可见的上传批次限制。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadConfig {
    pub max_files_per_batch: i32,
    pub max_bytes_per_file: i64,
    pub max_bytes_per_batch: i64,
    pub client_concurrency: i32,
    #[serde(default = "default_upload_batch_expiry_hours")]
    pub upload_batch_expiry_hours: i32,
}

fn default_upload_batch_expiry_hours() -> i32 {
    24
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            max_files_per_batch: 500,
            max_bytes_per_file: 100 * MEBIBYTE,
            max_bytes_per_batch: 2 * GIBIBYTE,
            client_concurrency: 3,
            upload_batch_expiry_hours: default_upload_batch_expiry_hours(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UploadConfigChange {
    pub before: UploadConfig,
    pub after: UploadConfig,
}

/// 拒绝危险或自相矛盾的配置，不静默修改管理员输入。
pub fn validate(config: &UploadConfig) -> Result<(), &'static str> {
    if !(1..=5_000).contains(&config.max_files_per_batch) {
        return Err("max_files_per_batch must be between 1 and 5000");
    }
    if !(MEBIBYTE..=GIBIBYTE).contains(&config.max_bytes_per_file) {
        return Err("max_bytes_per_file must be between 1 MiB and 1 GiB");
    }
    if config.max_bytes_per_batch < config.max_bytes_per_file
        || config.max_bytes_per_batch > 20 * GIBIBYTE
    {
        return Err("max_bytes_per_batch must include one file and not exceed 20 GiB");
    }
    if !(1..=10).contains(&config.client_concurrency) {
        return Err("client_concurrency must be between 1 and 10");
    }
    if !(1..=168).contains(&config.upload_batch_expiry_hours) {
        return Err("upload_batch_expiry_hours must be between 1 and 168");
    }
    Ok(())
}

/// 读取当前限制；缺失或旧值不可解析时回退到安全默认。
pub async fn get(pool: &PgPool) -> Result<UploadConfig, sqlx::Error> {
    match crate::settings::get(pool, KEY).await? {
        Some(value) => Ok(serde_json::from_value(value).unwrap_or_default()),
        None => Ok(UploadConfig::default()),
    }
}

/// 串行化配置 mutation 并返回事务内当前快照。
pub async fn get_for_update_tx(conn: &mut PgConnection) -> Result<UploadConfig, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(UPLOAD_CONFIG_LOCK_KEY)
        .execute(&mut *conn)
        .await?;
    match sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM settings WHERE key = $1 FOR UPDATE",
    )
    .bind(KEY)
    .fetch_optional(conn)
    .await?
    {
        Some(value) => Ok(serde_json::from_value(value).unwrap_or_default()),
        None => Ok(UploadConfig::default()),
    }
}

/// 在调用方事务内校验并持久化限制。
pub async fn set_tx(
    conn: &mut PgConnection,
    config: &UploadConfig,
    updated_by: Option<i64>,
) -> Result<UploadConfigChange, sqlx::Error> {
    let before = get_for_update_tx(&mut *conn).await?;
    set_locked_tx(&mut *conn, before, config, updated_by).await
}

/// 在已调用 [`get_for_update_tx`] 的事务内写入，避免重复取得快照。
pub async fn set_locked_tx(
    conn: &mut PgConnection,
    before: UploadConfig,
    config: &UploadConfig,
    updated_by: Option<i64>,
) -> Result<UploadConfigChange, sqlx::Error> {
    validate(config).map_err(|message| sqlx::Error::Protocol(message.to_string()))?;
    let value =
        serde_json::to_value(config).map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
    crate::settings::set_tx(conn, KEY, &value, updated_by).await?;
    Ok(UploadConfigChange {
        before,
        after: config.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_upload_contract() {
        assert_eq!(
            UploadConfig::default(),
            UploadConfig {
                max_files_per_batch: 500,
                max_bytes_per_file: 100 * MEBIBYTE,
                max_bytes_per_batch: 2 * GIBIBYTE,
                client_concurrency: 3,
                upload_batch_expiry_hours: 24,
            }
        );
    }

    #[test]
    fn validation_rejects_each_unsafe_boundary() {
        let valid = UploadConfig::default();
        for invalid in [
            UploadConfig {
                max_files_per_batch: 0,
                ..valid.clone()
            },
            UploadConfig {
                max_bytes_per_file: MEBIBYTE - 1,
                ..valid.clone()
            },
            UploadConfig {
                max_bytes_per_batch: valid.max_bytes_per_file - 1,
                ..valid.clone()
            },
            UploadConfig {
                client_concurrency: 11,
                ..valid.clone()
            },
            UploadConfig {
                upload_batch_expiry_hours: 0,
                ..valid.clone()
            },
        ] {
            assert!(validate(&invalid).is_err());
        }
        assert!(validate(&valid).is_ok());
    }
}
