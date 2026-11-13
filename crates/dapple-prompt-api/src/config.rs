//! Configuration helpers for environment-based settings.

use alloy_primitives::Address;
use std::env;

/// Default JWT expiry in seconds (1 hour)
const DEFAULT_JWT_EXPIRY_SECS: u64 = 3600;

/// Check if auth token endpoint is enabled.
///
/// Returns true if both DAPPLE_PROMPT_API_SIGNING_KEY and AUTH_TOKEN_ALLOWED_SIGNERS are set.
pub fn auth_token_enabled() -> bool {
    env::var("DAPPLE_PROMPT_API_SIGNING_KEY").is_ok()
        && env::var("AUTH_TOKEN_ALLOWED_SIGNERS").is_ok()
}

/// Load allowed signers from AUTH_TOKEN_ALLOWED_SIGNERS environment variable.
///
/// Expected format: comma-separated Ethereum addresses (with or without 0x prefix).
/// Example: "0x1234...5678,0xabcd...ef01"
///
/// Returns an empty Vec if the variable is not set or parsing fails.
pub fn load_allowed_signers() -> Vec<Address> {
    let Ok(signers_str) = env::var("AUTH_TOKEN_ALLOWED_SIGNERS") else {
        return Vec::new();
    };

    signers_str
        .split(',')
        .filter_map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return None;
            }
            // Parse address, handling both "0x..." and raw hex
            trimmed.parse::<Address>().ok()
        })
        .collect()
}

/// Get JWT expiry duration in seconds.
///
/// Reads from AUTH_TOKEN_JWT_EXPIRY_SECS environment variable.
/// Defaults to 3600 (1 hour) if not set or invalid.
pub fn jwt_expiry_secs() -> u64 {
    env::var("AUTH_TOKEN_JWT_EXPIRY_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_JWT_EXPIRY_SECS)
}

/// Get the JWT secret for signing tokens.
///
/// Returns None if DAPPLE_PROMPT_API_SIGNING_KEY is not set.
pub fn jwt_secret() -> Option<String> {
    env::var("DAPPLE_PROMPT_API_SIGNING_KEY").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_expiry_default() {
        // Ensure env var is unset for this test
        // SAFETY: This is only used in single-threaded tests
        unsafe {
            env::remove_var("AUTH_TOKEN_JWT_EXPIRY_SECS");
        }
        assert_eq!(jwt_expiry_secs(), DEFAULT_JWT_EXPIRY_SECS);
    }

    #[test]
    fn test_parse_addresses() {
        // Test address parsing logic directly
        let addr1 = "0x742d35Cc6634C0532925a3b844Bc9e7595f4E3B0";
        let addr2 = "742d35Cc6634C0532925a3b844Bc9e7595f4E3B0";

        let parsed1: Address = addr1.parse().expect("should parse with 0x prefix");
        let parsed2: Address = addr2.parse().expect("should parse without 0x prefix");

        assert_eq!(parsed1, parsed2);
    }
}
