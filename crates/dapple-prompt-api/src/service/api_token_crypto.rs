//! Cryptographic utilities for API token generation and validation.
//!
//! Token format: `dpl_<43 char base64url>` (256 bits entropy)
//! - Prefix: "dpl_" identifies this as a dapple API token
//! - Body: 32 bytes (256 bits) of random data, base64url encoded (no padding)
//!
//! The plaintext token is shown only once at creation time.
//! We store only the SHA-256 hash of the token for validation.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Token prefix that identifies dapple API tokens
pub const TOKEN_PREFIX: &str = "dpl_";

/// Length of random bytes (256 bits)
const TOKEN_RANDOM_BYTES: usize = 32;

/// Expected length of the full token (prefix + base64url encoded random)
/// "dpl_" (4) + base64url(32 bytes) = 4 + 43 = 47 characters
pub const EXPECTED_TOKEN_LEN: usize = 47;

/// Length of the visible prefix stored for identification (e.g., "dpl_abcd")
pub const STORED_PREFIX_LEN: usize = 8;

/// Generate a new API token and its hash.
///
/// Returns a tuple of (plaintext_token, hash).
/// The plaintext token should be shown to the user once and never stored.
/// The hash is stored in the database for validation.
pub fn generate_token() -> (String, Vec<u8>) {
    let mut random_bytes = [0u8; TOKEN_RANDOM_BYTES];
    rand::thread_rng().fill_bytes(&mut random_bytes);

    let encoded = URL_SAFE_NO_PAD.encode(random_bytes);
    let plaintext = format!("{}{}", TOKEN_PREFIX, encoded);
    let hash = hash_token(&plaintext);

    (plaintext, hash)
}

/// Compute the SHA-256 hash of a token.
pub fn hash_token(token: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hasher.finalize().to_vec()
}

/// Extract the visible prefix from a token for identification.
/// Returns the first 8 characters (e.g., "dpl_abcd").
pub fn token_prefix(token: &str) -> String {
    token.chars().take(STORED_PREFIX_LEN).collect()
}

/// Check if a string looks like a valid dpl_ token format.
pub fn is_valid_token_format(token: &str) -> bool {
    if !token.starts_with(TOKEN_PREFIX) {
        return false;
    }

    if token.len() != EXPECTED_TOKEN_LEN {
        return false;
    }

    // Check that the body is valid base64url
    let body = &token[TOKEN_PREFIX.len()..];
    URL_SAFE_NO_PAD.decode(body).is_ok()
}

/// Check if a token string starts with the dpl_ prefix.
/// Used for quick detection in auth middleware.
pub fn is_api_token(token: &str) -> bool {
    token.starts_with(TOKEN_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token_format() {
        let (token, hash) = generate_token();

        assert!(token.starts_with(TOKEN_PREFIX));
        assert_eq!(token.len(), EXPECTED_TOKEN_LEN);
        assert_eq!(hash.len(), 32); // SHA-256 produces 32 bytes
    }

    #[test]
    fn test_hash_deterministic() {
        let token = "dpl_abcdefghijklmnopqrstuvwxyz0123456789ABC";
        let hash1 = hash_token(token);
        let hash2 = hash_token(token);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_token_prefix_extraction() {
        let token = "dpl_abcdefghijklmnopqrstuvwxyz0123456789ABC";
        let prefix = token_prefix(token);

        assert_eq!(prefix, "dpl_abcd");
        assert_eq!(prefix.len(), STORED_PREFIX_LEN);
    }

    #[test]
    fn test_is_valid_token_format() {
        let (valid_token, _) = generate_token();
        assert!(is_valid_token_format(&valid_token));

        // Invalid: wrong prefix
        assert!(!is_valid_token_format(
            "abc_1234567890123456789012345678901234567890123"
        ));

        // Invalid: too short
        assert!(!is_valid_token_format("dpl_short"));

        // Invalid: no prefix
        assert!(!is_valid_token_format(
            "1234567890123456789012345678901234567890123456"
        ));
    }

    #[test]
    fn test_is_api_token() {
        assert!(is_api_token("dpl_anything"));
        assert!(!is_api_token("Bearer eyJ..."));
        assert!(!is_api_token("some_other_token"));
    }

    #[test]
    fn test_generate_unique_tokens() {
        let (token1, hash1) = generate_token();
        let (token2, hash2) = generate_token();

        assert_ne!(token1, token2);
        assert_ne!(hash1, hash2);
    }
}
