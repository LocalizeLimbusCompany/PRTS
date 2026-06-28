//! `prts-auth` —— 认证插件框架（Trait 抽象 + 编译期注册）。
//!
//! 设计（见 plan §9、docs/architecture.md §3.1）：
//! - 每个认证方式实现 [`AuthProvider`]，在启动时按配置注册启用；
//! - 内置 `password`、通用 `oauth2`（Authorization Code + PKCE）；
//! - ZOOT 是 `oauth2` provider 的一个配置实例 + 字段映射器。
//!
//! 加密原语见子模块：[`password`]、[`jwt`]、[`token`]、[`pkce`]。

pub mod jwt;
pub mod oauth2;
pub mod password;
pub mod pkce;
pub mod token;

pub use oauth2::{AuthorizationStart, OAuth2Config, OAuth2Error, OAuth2Provider};

use serde::{Deserialize, Serialize};

/// provider 类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// 账号密码。
    Password,
    /// OAuth2（Authorization Code + PKCE）。
    OAuth2,
}

/// 各 provider 完成认证后归一化得到的平台身份。
///
/// 上层据此 upsert 用户、建立 / 更新关联账号（如 GitHub）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedIdentity {
    /// 来源 provider id（如 `"zoot"`）。
    pub provider: String,
    /// 该 provider 下的用户唯一标识。
    pub external_id: String,
    /// 用户名。
    pub username: String,
    /// 头像 URL（可空）。
    pub avatar_url: Option<String>,
    /// 翻译类别等附加信息（如 ZOOT 的 work_scope/work_content）。
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// 认证提供方插件接口。
///
/// 注：发起 / 回调换取令牌等方法为 I/O 操作，将在 P1 以 `async fn` 落地；
/// P0 先固定身份与元信息接口，避免过早引入网络依赖。
pub trait AuthProvider: Send + Sync {
    /// 稳定的 provider 标识（如 `"password"`、`"oauth2"`、`"zoot"`）。
    fn id(&self) -> &str;
    /// provider 类别。
    fn kind(&self) -> ProviderKind;
}
