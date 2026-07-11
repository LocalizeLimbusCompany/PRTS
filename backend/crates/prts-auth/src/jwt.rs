//! 极简 JWT（仅 HS256），用 `hmac` + `sha2` 手写，避免引入 `ring` 等 C 依赖。
//!
//! 仅实现本平台自签自验所需的最小子集：固定 `alg=HS256`、`typ=JWT`，载荷为 [`Claims`]。
//! `decode` 只做**签名校验 + 解析**；过期检查交由调用方用 [`Claims::is_valid_at`]（便于测试，不依赖系统时间）。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// JWT 载荷。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// 用户 id。
    pub sub: i64,
    /// 签发时间（Unix 秒）。
    pub iat: i64,
    /// 过期时间（Unix 秒）。
    pub exp: i64,
    /// 令牌类型，固定 `"access"`（刷新令牌是不透明串，不走 JWT）。
    pub typ: String,
    /// DB-authoritative session handle；`None` 仅用于识别旧 token，认证边界必须拒绝。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
}

impl Claims {
    /// 给定当前时间（Unix 秒）判断是否仍在有效期内（含 60s 容差）。
    pub fn is_valid_at(&self, now: i64) -> bool {
        now <= self.exp + 60
    }
}

/// JWT 错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JwtError {
    /// 结构非法（段数 / base64 / JSON）。
    #[error("malformed token")]
    Malformed,
    /// 头部 alg/typ 不被支持。
    #[error("unsupported header")]
    UnsupportedHeader,
    /// 签名校验失败。
    #[error("signature mismatch")]
    SignatureMismatch,
}

#[derive(Serialize, Deserialize)]
struct Header {
    alg: String,
    typ: String,
}

/// 用 HS256 签发 JWT。
pub fn encode(claims: &Claims, secret: &[u8]) -> String {
    let header = Header {
        alg: "HS256".to_string(),
        typ: "JWT".to_string(),
    };
    let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).expect("serialize header"));
    let p = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("serialize claims"));
    let signing_input = format!("{h}.{p}");
    let sig = sign(signing_input.as_bytes(), secret);
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(sig))
}

/// 校验签名并解析 JWT。**不检查过期**（用 [`Claims::is_valid_at`]）。
pub fn decode(token: &str, secret: &[u8]) -> Result<Claims, JwtError> {
    let mut parts = token.split('.');
    let (h_b64, p_b64, s_b64) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(p), Some(s), None) => (h, p, s),
        _ => return Err(JwtError::Malformed),
    };

    // 头部：必须 alg=HS256、typ=JWT。
    let header_bytes = URL_SAFE_NO_PAD
        .decode(h_b64)
        .map_err(|_| JwtError::Malformed)?;
    let header: Header = serde_json::from_slice(&header_bytes).map_err(|_| JwtError::Malformed)?;
    if header.alg != "HS256" || header.typ != "JWT" {
        return Err(JwtError::UnsupportedHeader);
    }

    // 签名校验（常数时间）。
    let signing_input = format!("{h_b64}.{p_b64}");
    let expected_sig = URL_SAFE_NO_PAD
        .decode(s_b64)
        .map_err(|_| JwtError::Malformed)?;
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&expected_sig)
        .map_err(|_| JwtError::SignatureMismatch)?;

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(p_b64)
        .map_err(|_| JwtError::Malformed)?;
    serde_json::from_slice(&payload_bytes).map_err(|_| JwtError::Malformed)
}

fn sign(signing_input: &[u8], secret: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(signing_input);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Claims {
        Claims {
            sub: 42,
            iat: 1_000,
            exp: 2_000,
            typ: "access".to_string(),
            sid: Some("session-handle-123456".to_string()),
        }
    }

    #[test]
    fn encode_decode_roundtrip() {
        let secret = b"super-secret-key";
        let token = encode(&sample(), secret);
        let decoded = decode(&token, secret).unwrap();
        assert_eq!(decoded, sample());
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let token = encode(&sample(), b"secret-a");
        assert_eq!(
            decode(&token, b"secret-b"),
            Err(JwtError::SignatureMismatch)
        );
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let token = encode(&sample(), b"k");
        // 篡改中段
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged_payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Claims {
                sub: 999,
                ..sample()
            })
            .unwrap(),
        );
        parts[1] = &forged_payload;
        let forged = parts.join(".");
        assert_eq!(decode(&forged, b"k"), Err(JwtError::SignatureMismatch));
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        assert_eq!(decode("only.two", b"k"), Err(JwtError::Malformed));
        assert_eq!(decode("a.b.c.d", b"k"), Err(JwtError::Malformed));
    }

    #[test]
    fn expiry_check() {
        let c = sample(); // exp = 2000
        assert!(c.is_valid_at(1_500));
        assert!(c.is_valid_at(2_050)); // 容差内
        assert!(!c.is_valid_at(2_100));
    }
}
