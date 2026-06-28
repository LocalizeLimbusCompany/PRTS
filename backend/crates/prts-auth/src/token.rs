//! 不透明令牌与 API Key 的生成与哈希。
//!
//! - 刷新令牌 / OAuth state：高熵随机串。
//! - API Key：随机串，库中仅存其 SHA-256（高熵随机，无需慢哈希）。

use rand::distributions::Alphanumeric;
use rand::Rng;
use sha2::{Digest, Sha256};

/// API Key 明文前缀，便于识别与展示。
pub const API_KEY_PREFIX: &str = "prts_";

/// 生成 `len` 长度的随机字母数字串（URL/请求头安全）。
pub fn random_token(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// 计算字符串的 SHA-256 十六进制摘要（用于 API Key / 刷新令牌的库内存储）。
pub fn sha256_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 新建的 API Key 三元组。
pub struct NewApiKey {
    /// 明文 Key（仅创建时返回一次，形如 `prts_xxxxx…`）。
    pub plaintext: String,
    /// 展示用前缀（明文前 12 字符）。
    pub display_prefix: String,
    /// 库内存储的 SHA-256 十六进制摘要。
    pub hash: String,
}

/// 生成一个新的 API Key。
pub fn generate_api_key() -> NewApiKey {
    let plaintext = format!("{API_KEY_PREFIX}{}", random_token(40));
    let display_prefix: String = plaintext.chars().take(12).collect();
    let hash = sha256_hex(&plaintext);
    NewApiKey {
        plaintext,
        display_prefix,
        hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_token_has_requested_length_and_varies() {
        let a = random_token(40);
        let b = random_token(40);
        assert_eq!(a.len(), 40);
        assert_eq!(b.len(), 40);
        assert_ne!(a, b);
    }

    #[test]
    fn sha256_hex_is_stable_and_64_chars() {
        // 已知向量：sha256("abc")
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_hex("anything").len(), 64);
    }

    #[test]
    fn api_key_shape() {
        let k = generate_api_key();
        assert!(k.plaintext.starts_with("prts_"));
        assert_eq!(k.plaintext.len(), 5 + 40);
        assert_eq!(k.display_prefix, &k.plaintext[..12]);
        assert_eq!(k.hash, sha256_hex(&k.plaintext));
        assert_eq!(k.hash.len(), 64);
    }
}
