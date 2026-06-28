//! 分层配置加载。
//!
//! 优先级（后者覆盖前者）：内置默认值 → `config/default.toml` → 环境变量（前缀 `PRTS__`）。
//! 所有字段均带默认值，因此在缺少配置文件 / 环境变量时（如单元测试）也能成功构建。

use config::{Config, Environment, File};
use serde::Deserialize;

/// 顶层配置。
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub database: DatabaseSettings,
    #[serde(default)]
    pub redis: RedisSettings,
}

/// HTTP 服务配置。
#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

/// PostgreSQL 配置。`url` 在生产环境务必经环境变量覆盖。
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseSettings {
    pub url: String,
    pub max_connections: u32,
}

/// Redis 配置。
#[derive(Debug, Clone, Deserialize)]
pub struct RedisSettings {
    pub url: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
        }
    }
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        Self {
            url: "postgres://prts:prts@localhost:5432/prts".to_string(),
            max_connections: 10,
        }
    }
}

impl Default for RedisSettings {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
        }
    }
}

impl ServerSettings {
    /// 监听地址，形如 `0.0.0.0:3000`。
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Settings {
    /// 按优先级加载配置。`config_path` 为配置文件基名（默认 `config/default`）。
    pub fn load() -> Result<Self, config::ConfigError> {
        Self::load_from("config/default")
    }

    /// 指定配置文件基名加载，便于测试。
    pub fn load_from(config_path: &str) -> Result<Self, config::ConfigError> {
        Config::builder()
            // 配置文件可选：不存在时仅使用默认值 + 环境变量。
            .add_source(File::with_name(config_path).required(false))
            // 环境变量覆盖，例：PRTS__SERVER__PORT、PRTS__DATABASE__URL。
            .add_source(Environment::with_prefix("PRTS").separator("__"))
            .build()?
            .try_deserialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_built_in_defaults_without_file_or_env() {
        // 指向一个不存在的基名，确保只用默认值（不依赖 CWD 下的配置文件）。
        let s = Settings::load_from("__definitely_missing_config__").expect("load defaults");
        assert_eq!(s.server.port, 3000);
        assert_eq!(s.server.host, "0.0.0.0");
        assert_eq!(s.server.addr(), "0.0.0.0:3000");
        assert_eq!(s.database.max_connections, 10);
        assert!(s.redis.url.starts_with("redis://"));
    }
}
