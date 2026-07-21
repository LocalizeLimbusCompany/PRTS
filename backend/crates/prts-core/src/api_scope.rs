//! Stable API-key scope vocabulary and normalization rules.

/// Dynamic access to every business permission the account currently holds.
pub const ALL: &str = "all";
pub const PROFILE_READ: &str = "profile:read";
pub const PROFILE_WRITE: &str = "profile:write";
pub const PROJECT_READ: &str = "project:read";
pub const ENTRY_WRITE: &str = "entry:write";
pub const PROJECT_WRITE: &str = "project:write";
pub const PROJECT_MANAGE: &str = "project:manage";
pub const AI_USE: &str = "ai:use";
pub const MESSAGE_READ: &str = "message:read";
pub const MESSAGE_WRITE: &str = "message:write";
pub const PLATFORM_MANAGE: &str = "platform:manage";

/// Every accepted wire value, in UI display order.
pub const VALUES: [&str; 11] = [
    ALL,
    PROFILE_READ,
    PROFILE_WRITE,
    PROJECT_READ,
    ENTRY_WRITE,
    PROJECT_WRITE,
    PROJECT_MANAGE,
    AI_USE,
    MESSAGE_READ,
    MESSAGE_WRITE,
    PLATFORM_MANAGE,
];

/// Validate, de-duplicate and sort a requested scope set.
pub fn normalize(scopes: &[String]) -> Result<Vec<String>, &'static str> {
    if scopes.is_empty() {
        return Err("API_KEY_SCOPES_REQUIRED");
    }
    let mut normalized = scopes.to_vec();
    normalized.sort();
    normalized.dedup();
    if normalized
        .iter()
        .any(|scope| !VALUES.contains(&scope.as_str()))
    {
        return Err("API_KEY_SCOPE_INVALID");
    }
    if normalized.iter().any(|scope| scope == ALL) && normalized.len() != 1 {
        return Err("API_KEY_ALL_SCOPE_EXCLUSIVE");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_is_explicit_and_exclusive() {
        assert_eq!(normalize(&[ALL.to_string()]).unwrap(), [ALL]);
        assert_eq!(
            normalize(&[ALL.to_string(), PROFILE_READ.to_string()]),
            Err("API_KEY_ALL_SCOPE_EXCLUSIVE")
        );
    }

    #[test]
    fn concrete_scopes_are_deduplicated_and_sorted() {
        assert_eq!(
            normalize(&[
                PROJECT_READ.to_string(),
                PROFILE_READ.to_string(),
                PROJECT_READ.to_string(),
            ])
            .unwrap(),
            [PROFILE_READ, PROJECT_READ]
        );
    }
}
