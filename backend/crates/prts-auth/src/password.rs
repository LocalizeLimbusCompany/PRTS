//! 密码哈希（Argon2id）。

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// Password length bounds shared by registration, administrator creation, and password change.
pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 256;

/// Validate a new password without retaining or formatting its plaintext value.
pub fn validate_new_password(plaintext: &str) -> bool {
    let length = plaintext.chars().count();
    (MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&length)
}

/// 用 Argon2id 哈希明文密码，返回 PHC 字符串（含算法/参数/盐）。
pub fn hash_password(plaintext: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default().hash_password(plaintext.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// 校验明文密码是否匹配给定 PHC 哈希。哈希非法或不匹配均返回 false。
pub fn verify_password(plaintext: &str, phc_hash: &str) -> bool {
    match PasswordHash::new(phc_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(plaintext.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_roundtrip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn distinct_salts_yield_distinct_hashes() {
        let a = hash_password("same").unwrap();
        let b = hash_password("same").unwrap();
        assert_ne!(a, b, "每次哈希应使用不同的盐");
        assert!(verify_password("same", &a));
        assert!(verify_password("same", &b));
    }

    #[test]
    fn malformed_hash_is_rejected() {
        assert!(!verify_password("x", "not-a-phc-string"));
    }

    #[test]
    fn new_password_policy_has_bounded_character_length() {
        assert!(!validate_new_password("short"));
        assert!(validate_new_password("long-enough"));
        assert!(!validate_new_password(&"x".repeat(MAX_PASSWORD_CHARS + 1)));
    }
}
