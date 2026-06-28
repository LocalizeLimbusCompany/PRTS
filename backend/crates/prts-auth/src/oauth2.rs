//! 通用 OAuth2（Authorization Code + PKCE/S256）provider。
//!
//! ZOOT 即本 provider 的一个**配置实例**（见 docs/external/oauth_integration.md），
//! 无需为其单独写实现：差异仅在配置（端点 / client / scope）与 userinfo 字段映射。
//!
//! 会话态（state、code_verifier）由调用方暂存（如 Redis），本模块不依赖存储。

use serde::Deserialize;

use crate::{token, NormalizedIdentity};

/// OAuth2 provider 配置（敏感项来自环境变量）。
#[derive(Debug, Clone)]
pub struct OAuth2Config {
    /// provider 标识，如 `"zoot"`。
    pub provider_id: String,
    /// 客户端 id。
    pub client_id: String,
    /// 客户端密钥（仅服务端）。
    pub client_secret: String,
    /// 授权端点。
    pub authorize_url: String,
    /// 令牌端点。
    pub token_url: String,
    /// 用户信息端点。
    pub userinfo_url: String,
    /// 回调地址（须与注册值完全一致）。
    pub redirect_uri: String,
    /// 申请的 scope。
    pub scopes: Vec<String>,
}

/// 发起授权所需的数据。调用方需把 `state → code_verifier` 暂存，回调时取回。
#[derive(Debug, Clone)]
pub struct AuthorizationStart {
    /// 引导浏览器跳转的完整授权 URL。
    pub authorize_url: String,
    /// 防 CSRF 的随机 state，回调须原样校验。
    pub state: String,
    /// PKCE code_verifier，换取令牌时使用。
    pub code_verifier: String,
}

/// OAuth2 错误。
#[derive(Debug, thiserror::Error)]
pub enum OAuth2Error {
    /// HTTP / 网络 / 反序列化错误。
    #[error("oauth http error: {0}")]
    Http(String),
    /// userinfo 缺少必要字段。
    #[error("missing userinfo field: {0}")]
    MissingField(&'static str),
    /// 配置中的 URL 非法。
    #[error("invalid authorize url")]
    BadUrl,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// 通用 OAuth2 provider。
pub struct OAuth2Provider {
    config: OAuth2Config,
    http: reqwest::Client,
}

impl OAuth2Provider {
    /// 用配置构造。
    pub fn new(config: OAuth2Config) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// provider 配置。
    pub fn config(&self) -> &OAuth2Config {
        &self.config
    }

    /// 构造授权跳转 URL 并生成 PKCE。`state` 由调用方生成（随后需校验回调一致）。
    pub fn begin(&self, state: String) -> Result<AuthorizationStart, OAuth2Error> {
        let code_verifier = token::random_token(96);
        let challenge = crate::pkce::challenge_s256(&code_verifier);

        let mut url =
            url::Url::parse(&self.config.authorize_url).map_err(|_| OAuth2Error::BadUrl)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("scope", &self.config.scopes.join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");

        Ok(AuthorizationStart {
            authorize_url: url.to_string(),
            state,
            code_verifier,
        })
    }

    /// 用授权码 + code_verifier 换取令牌并拉取用户信息，映射为归一化身份。
    pub async fn complete(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<NormalizedIdentity, OAuth2Error> {
        // 1) 授权码换 access_token（application/x-www-form-urlencoded）
        let token: TokenResponse = self
            .http
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("code_verifier", code_verifier),
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| OAuth2Error::Http(e.to_string()))?
            .error_for_status()
            .map_err(|e| OAuth2Error::Http(e.to_string()))?
            .json()
            .await
            .map_err(|e| OAuth2Error::Http(e.to_string()))?;

        // 2) 用 access_token 拉取 userinfo
        let info: serde_json::Value = self
            .http
            .get(&self.config.userinfo_url)
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(|e| OAuth2Error::Http(e.to_string()))?
            .error_for_status()
            .map_err(|e| OAuth2Error::Http(e.to_string()))?
            .json()
            .await
            .map_err(|e| OAuth2Error::Http(e.to_string()))?;

        // 3) 映射为归一化身份
        map_identity(&self.config.provider_id, info)
    }
}

/// 把 userinfo JSON 映射为平台身份。
///
/// 取 `sub`（必需）、`username`、`picture`（头像）；其余字段（如 ZOOT 的
/// `work_scope`/`work_content` 翻译类别、`github_id`）整体保留在 `extra`，供上层建立关联账号。
fn map_identity(
    provider: &str,
    info: serde_json::Value,
) -> Result<NormalizedIdentity, OAuth2Error> {
    let external_id = match info.get("sub") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => return Err(OAuth2Error::MissingField("sub")),
    };
    let username = info
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let avatar_url = info
        .get("picture")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(NormalizedIdentity {
        provider: provider.to_string(),
        external_id,
        username,
        avatar_url,
        extra: info,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> OAuth2Config {
        OAuth2Config {
            provider_id: "zoot".to_string(),
            client_id: "cid".to_string(),
            client_secret: "secret".to_string(),
            authorize_url: "https://zoot.example.com/oauth/authorize".to_string(),
            token_url: "https://zoot.example.com/oauth/token".to_string(),
            userinfo_url: "https://zoot.example.com/oauth/userinfo".to_string(),
            redirect_uri: "https://prts.example.com/callback".to_string(),
            scopes: vec!["profile".to_string(), "work".to_string()],
        }
    }

    #[test]
    fn begin_builds_authorize_url_with_pkce() {
        let p = OAuth2Provider::new(cfg());
        let start = p.begin("xyz-state".to_string()).unwrap();
        let u = url::Url::parse(&start.authorize_url).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q.get("response_type").unwrap(), "code");
        assert_eq!(q.get("client_id").unwrap(), "cid");
        assert_eq!(q.get("code_challenge_method").unwrap(), "S256");
        assert_eq!(q.get("scope").unwrap(), "profile work");
        assert_eq!(q.get("state").unwrap(), "xyz-state");
        assert!(q.contains_key("code_challenge"));
        // verifier 与 challenge 对应
        assert_eq!(
            crate::pkce::challenge_s256(&start.code_verifier),
            *q.get("code_challenge").unwrap()
        );
    }

    #[test]
    fn map_identity_extracts_zoot_fields() {
        let info = serde_json::json!({
            "sub": "42",
            "username": "alice",
            "picture": "https://zoot.example.com/a.png",
            "work_scope": "英翻",
            "github_id": 123
        });
        let id = map_identity("zoot", info).unwrap();
        assert_eq!(id.external_id, "42");
        assert_eq!(id.username, "alice");
        assert_eq!(
            id.avatar_url.as_deref(),
            Some("https://zoot.example.com/a.png")
        );
        assert_eq!(id.extra.get("work_scope").unwrap(), "英翻");
    }

    #[test]
    fn map_identity_accepts_numeric_sub_and_requires_it() {
        let id = map_identity("zoot", serde_json::json!({ "sub": 7 })).unwrap();
        assert_eq!(id.external_id, "7");
        assert!(map_identity("zoot", serde_json::json!({ "username": "x" })).is_err());
    }
}
