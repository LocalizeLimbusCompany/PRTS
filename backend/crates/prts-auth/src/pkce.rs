//! PKCE（RFC 7636，仅 S256）。用于 OAuth2 授权码模式，见 docs/external/oauth_integration.md。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::token::random_token;

/// 生成 `code_verifier`：96 字符随机串（合法范围 43–128）。
pub fn new_verifier() -> String {
    random_token(96)
}

/// 由 `code_verifier` 派生 `code_challenge` = BASE64URL(SHA256(verifier))，去除填充。
pub fn challenge_s256(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_length_in_range() {
        let v = new_verifier();
        assert!((43..=128).contains(&v.len()));
    }

    #[test]
    fn challenge_matches_rfc7636_example() {
        // RFC 7636 附录 B 示例
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_s256(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn challenge_is_deterministic() {
        let v = new_verifier();
        assert_eq!(challenge_s256(&v), challenge_s256(&v));
    }
}
