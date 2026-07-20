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
    #[serde(default)]
    pub auth: AuthSettings,
    #[serde(default)]
    pub embedding: EmbeddingSettings,
    #[serde(default)]
    pub media: MediaSettings,
    #[serde(default)]
    pub ai: AiSettings,
}

/// AI credential encryption configuration. The key is supplied only via environment variables.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AiSettings {
    /// Base64-encoded 32-byte XChaCha20-Poly1305 key (`PRTS__AI__MASTER_KEY`).
    #[serde(default)]
    pub master_key: String,
}

/// 用户上传媒体与临时上传流的持久化路径；不包含业务限制或密钥。
#[derive(Debug, Clone, Deserialize)]
pub struct MediaSettings {
    #[serde(default = "default_media_dir")]
    pub directory: String,
    #[serde(default = "default_upload_temp_dir")]
    pub upload_temp_directory: String,
}

fn default_media_dir() -> String {
    "./data/media".to_string()
}

fn default_upload_temp_dir() -> String {
    "./data/upload-temp".to_string()
}

impl Default for MediaSettings {
    fn default() -> Self {
        Self {
            directory: default_media_dir(),
            upload_temp_directory: default_upload_temp_dir(),
        }
    }
}

/// 认证相关配置。密钥与 OAuth 凭证经环境变量注入（`PRTS__AUTH__*`）。
#[derive(Debug, Clone, Deserialize)]
pub struct AuthSettings {
    /// JWT 签名密钥（HS256）。生产务必经 `PRTS__AUTH__JWT_SECRET` 覆盖。
    pub jwt_secret: String,
    /// access token 有效期（秒）。
    pub access_ttl_secs: i64,
    /// refresh token 有效期（秒）。
    pub refresh_ttl_secs: i64,
    /// 启动时授予 super_admin 的用户名（可空）。
    #[serde(default)]
    pub bootstrap_admin: String,
    /// 对外基础地址，用于拼接 OAuth 回调与登录后跳转，例如 `https://prts.zeroasso.top`。
    pub public_base_url: String,
    /// ZOOT OAuth provider 配置。
    #[serde(default)]
    pub zoot: ZootSettings,
}

/// ZOOT OAuth2 provider 配置。`client_id` 为空表示未启用。
#[derive(Debug, Clone, Deserialize)]
pub struct ZootSettings {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub authorize_url: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub userinfo_url: String,
    #[serde(default = "default_zoot_scopes")]
    pub scopes: Vec<String>,
}

fn default_zoot_scopes() -> Vec<String> {
    vec![
        "profile".to_string(),
        "work".to_string(),
        "external".to_string(),
    ]
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            jwt_secret: "dev-insecure-change-me".to_string(),
            access_ttl_secs: 900,        // 15 分钟
            refresh_ttl_secs: 2_592_000, // 30 天
            bootstrap_admin: String::new(),
            public_base_url: "http://localhost:8080".to_string(),
            zoot: ZootSettings::default(),
        }
    }
}

impl Default for ZootSettings {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            authorize_url: String::new(),
            token_url: String::new(),
            userinfo_url: String::new(),
            scopes: default_zoot_scopes(),
        }
    }
}

impl ZootSettings {
    /// 是否已配置（启用 ZOOT 登录）。
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty() && !self.authorize_url.is_empty()
    }
}

/// 向量化（Embedding）配置。密钥仅经 env 注入，绝不下发前端。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbeddingSettings {
    #[serde(default)]
    pub qwen: QwenSettings,
}

/// Qwen Embedding provider 配置。
#[derive(Debug, Clone, Deserialize)]
pub struct QwenSettings {
    /// Qwen API Key（仅 env：PRTS__EMBEDDING__QWEN__API_KEY）。空 = 未配置 → 降级。
    #[serde(default)]
    pub api_key: String,
    /// 向量维度，须与迁移 0004 的 vector(N) 一致。
    #[serde(default = "default_qwen_dimensions")]
    pub dimensions: usize,
}

fn default_qwen_dimensions() -> usize {
    1024
}

impl Default for QwenSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            dimensions: default_qwen_dimensions(),
        }
    }
}

impl QwenSettings {
    /// 是否已配置 API Key（决定向量化能否启用）。
    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }
}

/// HTTP 服务配置。
#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

/// PostgreSQL 配置。连接串只经环境变量注入；runtime 与 migration owner 必须分离。
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseSettings {
    /// 应用运行时连接串。该角色不得拥有 schema/table。
    pub url: String,
    /// 独立 migration owner 连接串，仅 `prts-api migrate` 子命令读取。
    #[serde(default)]
    pub migration_url: Option<String>,
    /// 迁移授予最小业务权限的 runtime role 名称（非密钥）。
    #[serde(default = "default_runtime_role")]
    pub runtime_role: String,
    pub max_connections: u32,
}

fn default_runtime_role() -> String {
    "prts_runtime".to_string()
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
            url: "postgres://prts_runtime:prts_runtime@localhost:5432/prts".to_string(),
            migration_url: None,
            runtime_role: default_runtime_role(),
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
        assert_eq!(s.database.runtime_role, "prts_runtime");
        assert!(s.database.migration_url.is_none());
        assert!(s.redis.url.starts_with("redis://"));
        assert_eq!(s.embedding.qwen.dimensions, 1024);
        assert!(s.embedding.qwen.api_key.is_empty());
        assert!(s.ai.master_key.is_empty());
    }

    #[test]
    fn embedding_defaults_are_safe() {
        let s = QwenSettings::default();
        assert_eq!(s.dimensions, 1024);
        assert!(
            s.api_key.is_empty(),
            "key must default empty so we degrade, not crash"
        );
    }
}
