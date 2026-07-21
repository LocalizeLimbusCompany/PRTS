//! Tenant-scoped web-search provider contracts and response normalization.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// User-selected web-search behavior.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchMode {
    #[default]
    Disabled,
    Adapter,
    Native,
    Auto,
}

impl WebSearchMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Adapter => "adapter",
            Self::Native => "native",
            Self::Auto => "auto",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "adapter" => Self::Adapter,
            "native" => Self::Native,
            "auto" => Self::Auto,
            _ => Self::Disabled,
        }
    }
}

/// Search execution status returned with every AI explanation.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchStatus {
    #[default]
    Disabled,
    Succeeded,
    Failed,
    Empty,
    Unsupported,
}

/// Safe, normalized source metadata. Provider response bodies and credentials are never retained.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
pub struct WebSearchCitation {
    pub number: usize,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub published_at: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct WebSearchRequest {
    pub query: String,
    pub max_results: usize,
}

#[derive(Debug, Clone, Default)]
pub struct WebSearchResponse {
    pub citations: Vec<WebSearchCitation>,
}

#[derive(Debug, thiserror::Error)]
pub enum WebSearchError {
    #[error("search transport failed")]
    Transport,
    #[error("search provider returned HTTP {0}")]
    Provider(u16),
    #[error("search provider returned an invalid response")]
    InvalidResponse,
}

/// Server-side search adapter. The boxed future keeps the trait object-safe without a macro crate.
pub trait WebSearchProvider: Send + Sync {
    fn id(&self) -> &str;
    fn search<'a>(
        &'a self,
        request: &'a WebSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<WebSearchResponse, WebSearchError>> + Send + 'a>>;
}

/// Tavily's JSON API is the first built-in server-side adapter.
pub struct TavilyWebSearchProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
}

impl TavilyWebSearchProvider {
    pub fn new(client: reqwest::Client, endpoint: String, api_key: String) -> Self {
        Self {
            client,
            endpoint,
            api_key,
        }
    }
}

impl WebSearchProvider for TavilyWebSearchProvider {
    fn id(&self) -> &str {
        "tavily"
    }

    fn search<'a>(
        &'a self,
        request: &'a WebSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<WebSearchResponse, WebSearchError>> + Send + 'a>> {
        Box::pin(async move {
            let response = self
                .client
                .post(&self.endpoint)
                .json(&serde_json::json!({
                    "api_key": self.api_key,
                    "query": request.query,
                    "search_depth": "basic",
                    "max_results": request.max_results,
                    "include_answer": false,
                    "include_raw_content": false
                }))
                .send()
                .await
                .map_err(|_| WebSearchError::Transport)?;
            if !response.status().is_success() {
                return Err(WebSearchError::Provider(response.status().as_u16()));
            }
            let value = response
                .json::<serde_json::Value>()
                .await
                .map_err(|_| WebSearchError::InvalidResponse)?;
            parse_tavily_response(&value, request.max_results)
        })
    }
}

/// Normalize, clean, and de-duplicate citations before they enter prompts or API responses.
pub fn normalize_citations(
    candidates: impl IntoIterator<Item = (String, String, String, Option<String>, String)>,
    max_results: usize,
) -> Vec<WebSearchCitation> {
    let mut seen = std::collections::HashSet::new();
    candidates
        .into_iter()
        .filter_map(|(title, raw_url, snippet, published_at, source)| {
            let url = url::Url::parse(raw_url.trim()).ok()?;
            let host = url.host_str()?.to_ascii_lowercase();
            if !matches!(url.scheme(), "https" | "http")
                || !url.username().is_empty()
                || url.password().is_some()
                || host == "localhost"
                || host.ends_with(".localhost")
                || host.parse::<std::net::IpAddr>().is_ok_and(is_private_ip)
            {
                return None;
            }
            let mut url = url;
            url.set_fragment(None);
            let canonical = url.as_str().trim_end_matches('/').to_ascii_lowercase();
            if !seen.insert(canonical) {
                return None;
            }
            Some((title, url.to_string(), snippet, published_at, source))
        })
        .take(max_results)
        .enumerate()
        .map(
            |(index, (title, url, snippet, published_at, source))| WebSearchCitation {
                number: index + 1,
                title: truncate_text(title.trim(), 300),
                url,
                snippet: truncate_text(snippet.trim(), 1_200),
                published_at: published_at.map(|value| truncate_text(value.trim(), 80)),
                source: truncate_text(source.trim(), 80),
            },
        )
        .collect()
}

pub fn parse_tavily_response(
    value: &serde_json::Value,
    max_results: usize,
) -> Result<WebSearchResponse, WebSearchError> {
    let results = value
        .get("results")
        .and_then(serde_json::Value::as_array)
        .ok_or(WebSearchError::InvalidResponse)?;
    let candidates = results.iter().filter_map(|item| {
        Some((
            item.get("title")?.as_str()?.to_string(),
            item.get("url")?.as_str()?.to_string(),
            item.get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            item.get("published_date")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            "tavily".to_string(),
        ))
    });
    Ok(WebSearchResponse {
        citations: normalize_citations(candidates, max_results),
    })
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
        }
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tavily_parser_cleans_deduplicates_and_limits_citations() {
        let parsed = parse_tavily_response(
            &serde_json::json!({"results": [
                {"title":"One", "url":"https://example.com/a#fragment", "content":"first"},
                {"title":"Duplicate", "url":"https://example.com/a", "content":"second"},
                {"title":"Unsafe", "url":"javascript:alert(1)", "content":"bad"},
                {"title":"Private", "url":"http://127.0.0.1/admin", "content":"bad"},
                {"title":"Two", "url":"https://example.org/b", "content":"third"}
            ]}),
            2,
        )
        .unwrap();
        assert_eq!(parsed.citations.len(), 2);
        assert_eq!(parsed.citations[0].number, 1);
        assert!(!parsed.citations[0].url.contains('#'));
        assert_eq!(parsed.citations[1].title, "Two");
    }
}
