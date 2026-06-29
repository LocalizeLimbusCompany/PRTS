//! Qwen 向量化（DashScope OpenAI 兼容端点）。密钥仅 env；model/base_url 运行时传入。
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("http: {0}")] Http(String),
    #[error("api {0}: {1}")] Api(u16, String),
    #[error("parse: {0}")] Parse(String),
}

pub struct QwenProvider {
    http: reqwest::Client,
    api_key: String,
    dimensions: usize,
}

#[derive(Serialize)]
struct EmbedReq<'a> { model: &'a str, input: &'a [String], dimensions: usize }
#[derive(Deserialize)]
struct EmbedResp { data: Vec<EmbedDatum> }
#[derive(Deserialize)]
struct EmbedDatum { embedding: Vec<f32> }

impl QwenProvider {
    pub fn new(api_key: String, dimensions: usize) -> Self {
        Self { http: reqwest::Client::new(), api_key, dimensions }
    }
    pub fn dimensions(&self) -> usize { self.dimensions }

    /// 单批 ≤10；调用方分块。base_url/model 取自当前 settings 快照。
    pub async fn embed_batch(&self, base_url: &str, model: &str, texts: &[String])
        -> Result<Vec<Vec<f32>>, EmbedError>
    {
        let url = format!("{}/embeddings", base_url.trim_end_matches('/'));
        let resp = self.http.post(url).bearer_auth(&self.api_key)
            .json(&EmbedReq { model, input: texts, dimensions: self.dimensions })
            .send().await.map_err(|e| EmbedError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(EmbedError::Api(status.as_u16(), resp.text().await.unwrap_or_default()));
        }
        let parsed: EmbedResp = resp.json().await.map_err(|e| EmbedError::Parse(e.to_string()))?;
        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_openai_compatible_response() {
        let body = r#"{"data":[{"embedding":[0.1,0.2]},{"embedding":[0.3,0.4]}]}"#;
        let r: EmbedResp = serde_json::from_str(body).unwrap();
        assert_eq!(r.data.len(), 2);
        assert_eq!(r.data[0].embedding, vec![0.1, 0.2]);
    }
}
