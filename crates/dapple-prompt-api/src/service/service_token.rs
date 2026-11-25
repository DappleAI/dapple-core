//! Service token verification for Ed25519-signed tokens.
//!
//! Token format: 128 bytes base64 encoded
//! - Signature: 64 bytes (Ed25519)
//! - Message: 64 bytes
//!   - version: 1 byte (0x01)
//!   - valid_from: 8 bytes (big-endian unix timestamp)
//!   - valid_until: 8 bytes (big-endian unix timestamp)
//!   - service_id_len: 1 byte
//!   - service_id: up to 32 bytes (UTF-8)
//!   - padding: remaining bytes

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Token prefix that identifies dapple service tokens
pub const TOKEN_PREFIX: &str = "dps_";

/// Expected token version byte
pub const TOKEN_VERSION: u8 = 0x01;

/// Total token size in bytes (before base64 encoding)
const TOKEN_BYTES: usize = 128;

/// Ed25519 signature size
const SIGNATURE_BYTES: usize = 64;

/// Message size
const MESSAGE_BYTES: usize = 64;

/// Maximum allowed duration between valid_from and valid_until (5 minutes)
pub const MAX_DURATION_SECS: u64 = 300;

/// Maximum service_id length
const MAX_SERVICE_ID_LEN: usize = 32;

/// Errors that can occur during service token verification
#[derive(Debug, Error)]
pub enum ServiceTokenError {
    #[error("MALFORMED_TOKEN: {0}")]
    MalformedToken(String),

    #[error("INVALID_SIGNATURE: failed to verify Ed25519 signature")]
    InvalidSignature,

    #[error("INVALID_VERSION: expected version {expected:#x}, got {actual:#x}")]
    InvalidVersion { expected: u8, actual: u8 },

    #[error("DURATION_EXCEEDS_MAX: token duration {duration_secs}s exceeds maximum {max_secs}s")]
    DurationExceedsMax { duration_secs: u64, max_secs: u64 },

    #[error("NOT_YET_VALID: token becomes valid at {valid_from}, current time is {current}")]
    NotYetValid { valid_from: u64, current: u64 },

    #[error("EXPIRED: token expired at {valid_until}, current time is {current}")]
    Expired { valid_until: u64, current: u64 },

    #[error("SERVICE_ID_MISMATCH: expected {expected}, got {got}")]
    ServiceIdMismatch { expected: String, got: String },

    #[error("INVALID_PUBLIC_KEY: {0}")]
    InvalidPublicKey(String),
}

/// Result of successful token verification
#[derive(Debug)]
pub struct VerifiedServiceToken {
    /// The service identifier from the token
    pub service_id: String,
    /// When the token became valid (Unix timestamp)
    pub valid_from: u64,
    /// When the token expires (Unix timestamp)
    pub valid_until: u64,
}

/// Verify a service token and check the Ed25519 signature.
///
/// # Arguments
/// * `token` - Base64-encoded 128-byte signed token (without dps_ prefix)
/// * `expected_service_id` - The service_id to validate against
/// * `public_key_pem` - PEM-encoded Ed25519 public key
///
/// # Returns
/// * `Ok(VerifiedServiceToken)` - Token is valid
/// * `Err(ServiceTokenError)` - Token verification failed
pub fn verify_service_token(
    token: &str,
    expected_service_id: &str,
    public_key_pem: &str,
) -> Result<VerifiedServiceToken, ServiceTokenError> {
    // 1. Decode base64
    let decoded = BASE64
        .decode(token)
        .map_err(|e| ServiceTokenError::MalformedToken(format!("invalid base64: {}", e)))?;

    // 2. Validate length
    if decoded.len() != TOKEN_BYTES {
        return Err(ServiceTokenError::MalformedToken(format!(
            "expected {} bytes, got {}",
            TOKEN_BYTES,
            decoded.len()
        )));
    }

    // 3. Split into signature and message
    let (signature_bytes, message_bytes) = decoded.split_at(SIGNATURE_BYTES);

    // 4. Parse public key from PEM
    let public_key = parse_ed25519_public_key(public_key_pem)?;

    // 5. Verify signature
    let signature =
        Signature::from_slice(signature_bytes).map_err(|_| ServiceTokenError::InvalidSignature)?;

    public_key
        .verify(message_bytes, &signature)
        .map_err(|_| ServiceTokenError::InvalidSignature)?;

    // 6. Parse message: version(1) + valid_from(8) + valid_until(8) + service_id_len(1) + service_id(<=32) + padding
    let version = message_bytes[0];
    if version != TOKEN_VERSION {
        return Err(ServiceTokenError::InvalidVersion {
            expected: TOKEN_VERSION,
            actual: version,
        });
    }

    let valid_from = u64::from_be_bytes(message_bytes[1..9].try_into().unwrap());
    let valid_until = u64::from_be_bytes(message_bytes[9..17].try_into().unwrap());
    let service_id_len = message_bytes[17] as usize;

    if service_id_len > MAX_SERVICE_ID_LEN {
        return Err(ServiceTokenError::MalformedToken(format!(
            "service_id length {} exceeds maximum {}",
            service_id_len, MAX_SERVICE_ID_LEN
        )));
    }

    let service_id = String::from_utf8(message_bytes[18..18 + service_id_len].to_vec())
        .map_err(|_| ServiceTokenError::MalformedToken("service_id is not valid UTF-8".into()))?;

    // 7. Validate duration
    let duration_secs = valid_until.saturating_sub(valid_from);
    if duration_secs > MAX_DURATION_SECS {
        return Err(ServiceTokenError::DurationExceedsMax {
            duration_secs,
            max_secs: MAX_DURATION_SECS,
        });
    }

    // 8. Check time window
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before Unix epoch")
        .as_secs();

    if current_time < valid_from {
        return Err(ServiceTokenError::NotYetValid {
            valid_from,
            current: current_time,
        });
    }

    if current_time > valid_until {
        return Err(ServiceTokenError::Expired {
            valid_until,
            current: current_time,
        });
    }

    // 9. Validate service_id matches
    if service_id != expected_service_id {
        return Err(ServiceTokenError::ServiceIdMismatch {
            expected: expected_service_id.to_string(),
            got: service_id,
        });
    }

    Ok(VerifiedServiceToken {
        service_id,
        valid_from,
        valid_until,
    })
}

/// Parse an Ed25519 public key from PEM format.
///
/// Returns the underlying ed25519-dalek `VerifyingKey`. Used by the admin
/// `POST /api/companies/{id}/external-service-tokens` handler to validate
/// the customer-supplied PEM at registration time so we never persist a
/// PEM the external auth flow can't load.
pub fn parse_ed25519_public_key(pem: &str) -> Result<VerifyingKey, ServiceTokenError> {
    let pem = pem.trim();

    // Extract the base64 content between PEM headers
    let b64 = pem
        .strip_prefix("-----BEGIN PUBLIC KEY-----")
        .and_then(|s| s.strip_suffix("-----END PUBLIC KEY-----"))
        .ok_or_else(|| ServiceTokenError::InvalidPublicKey("invalid PEM format".into()))?
        .trim()
        .replace(['\n', '\r', ' '], "");

    let der = BASE64
        .decode(&b64)
        .map_err(|e| ServiceTokenError::InvalidPublicKey(format!("base64: {}", e)))?;

    // Ed25519 public key in SubjectPublicKeyInfo format (SPKI):
    // SEQUENCE {
    //   algorithm SEQUENCE {
    //     algorithm OBJECT IDENTIFIER (1.3.101.112 = Ed25519)
    //   }
    //   publicKey BIT STRING (32 bytes)
    // }
    // Total SPKI overhead is 12 bytes, so total is 44 bytes for Ed25519
    if der.len() < 44 {
        return Err(ServiceTokenError::InvalidPublicKey(format!(
            "DER too short: {} bytes, expected at least 44",
            der.len()
        )));
    }

    // The raw 32-byte Ed25519 public key is at the end of the DER structure
    let raw_key = &der[der.len() - 32..];

    VerifyingKey::from_bytes(raw_key.try_into().unwrap())
        .map_err(|e| ServiceTokenError::InvalidPublicKey(format!("invalid Ed25519 key: {}", e)))
}

/// Strip the dps_ prefix from a token string.
#[cfg(test)]
fn strip_prefix(token: &str) -> Option<&str> {
    token.strip_prefix(TOKEN_PREFIX)
}

/// Test utilities for generating valid service tokens.
#[cfg(feature = "test-helpers")]
pub mod test_utils {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::RngCore;

    /// A test keypair for signing service tokens.
    #[derive(Debug)]
    pub struct TestServiceKeypair {
        /// The private signing key
        pub signing_key: SigningKey,
        /// The service identifier
        pub service_id: String,
    }

    impl TestServiceKeypair {
        /// Generate a new random test keypair for a given service_id.
        pub fn new(service_id: impl Into<String>) -> Self {
            let mut csprng = rand::thread_rng();
            let mut secret_key_bytes = [0u8; 32];
            csprng.fill_bytes(&mut secret_key_bytes);
            let signing_key = SigningKey::from_bytes(&secret_key_bytes);
            Self {
                signing_key,
                service_id: service_id.into(),
            }
        }

        /// Get the public key in PEM format.
        pub fn public_key_pem(&self) -> String {
            let verifying_key = self.signing_key.verifying_key();
            let key_bytes = verifying_key.as_bytes();

            // Build SPKI DER structure for Ed25519
            // OID for Ed25519: 1.3.101.112 = 06 03 2B 65 70
            let mut der = vec![
                0x30, 0x2A, // SEQUENCE, 42 bytes
                0x30, 0x05, // SEQUENCE (algorithm), 5 bytes
                0x06, 0x03, 0x2B, 0x65, 0x70, // OID 1.3.101.112
                0x03, 0x21, 0x00, // BIT STRING, 33 bytes (1 unused bit byte + 32 key bytes)
            ];
            der.extend_from_slice(key_bytes);

            let b64 = BASE64.encode(&der);
            format!(
                "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----",
                b64
            )
        }

        /// Sign a token valid for the specified duration from now.
        pub fn sign_token(&self, valid_for_secs: u64) -> String {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before Unix epoch")
                .as_secs();

            self.sign_token_with_times(now, now + valid_for_secs)
        }

        /// Sign a token with explicit timestamps.
        pub fn sign_token_with_times(&self, valid_from: u64, valid_until: u64) -> String {
            // Build message: version(1) + valid_from(8) + valid_until(8) + service_id_len(1) + service_id(<=32) + padding
            let mut message = vec![TOKEN_VERSION];
            message.extend_from_slice(&valid_from.to_be_bytes());
            message.extend_from_slice(&valid_until.to_be_bytes());
            message.push(self.service_id.len() as u8);
            message.extend_from_slice(self.service_id.as_bytes());
            message.resize(MESSAGE_BYTES, 0); // Pad to 64 bytes

            // Sign the message
            use ed25519_dalek::Signer;
            let signature = self.signing_key.sign(&message);

            // Build token: signature (64) || message (64) = 128 bytes
            let mut token_bytes = signature.to_bytes().to_vec();
            token_bytes.extend_from_slice(&message);

            // Return with dps_ prefix
            format!("{}{}", TOKEN_PREFIX, BASE64.encode(&token_bytes))
        }
    }

    impl Clone for TestServiceKeypair {
        fn clone(&self) -> Self {
            let secret_bytes = self.signing_key.to_bytes();
            let signing_key = SigningKey::from_bytes(&secret_bytes);
            Self {
                signing_key,
                service_id: self.service_id.clone(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_malformed_token_invalid_base64() {
        let result = verify_service_token(
            "not-valid-base64!!!",
            "test_service",
            "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAGb9F2CMqM/WQKzjhFTwOb3V4VXBZ9xPKL8b/hQFBgvQ=\n-----END PUBLIC KEY-----",
        );
        assert!(matches!(result, Err(ServiceTokenError::MalformedToken(_))));
    }

    #[test]
    fn test_malformed_token_wrong_length() {
        let short_token = BASE64.encode(vec![0u8; 100]);
        let result = verify_service_token(
            &short_token,
            "test_service",
            "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAGb9F2CMqM/WQKzjhFTwOb3V4VXBZ9xPKL8b/hQFBgvQ=\n-----END PUBLIC KEY-----",
        );
        assert!(matches!(result, Err(ServiceTokenError::MalformedToken(_))));
    }

    #[test]
    fn test_strip_prefix() {
        assert_eq!(strip_prefix("dps_token_body"), Some("token_body"));
        assert_eq!(strip_prefix("dpl_token_body"), None);
        assert_eq!(strip_prefix("no_prefix"), None);
    }

    #[cfg(feature = "test-helpers")]
    mod test_helpers_tests {
        use super::super::test_utils::TestServiceKeypair;
        use super::*;

        #[test]
        fn test_keypair_generates_valid_pem() {
            let keypair = TestServiceKeypair::new("test_service");
            let pem = keypair.public_key_pem();

            assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----"));
            assert!(pem.ends_with("-----END PUBLIC KEY-----"));

            // Verify we can parse it back
            let result = parse_ed25519_public_key(&pem);
            assert!(result.is_ok(), "failed to parse PEM: {:?}", result);
        }

        #[test]
        fn test_sign_and_verify_round_trip() {
            let keypair = TestServiceKeypair::new("test_service");
            let token = keypair.sign_token(60); // 1 minute validity
            let pem = keypair.public_key_pem();

            // Strip the dps_ prefix before verification
            let token_body = strip_prefix(&token).expect("should have dps_ prefix");

            let result = verify_service_token(token_body, "test_service", &pem);
            assert!(result.is_ok(), "verification failed: {:?}", result);

            let verified = result.unwrap();
            assert_eq!(verified.service_id, "test_service");
        }

        #[test]
        fn test_service_id_mismatch() {
            let keypair = TestServiceKeypair::new("test_service");
            let token = keypair.sign_token(60);
            let pem = keypair.public_key_pem();

            let token_body = strip_prefix(&token).expect("should have dps_ prefix");

            let result = verify_service_token(token_body, "wrong_service", &pem);
            assert!(matches!(
                result,
                Err(ServiceTokenError::ServiceIdMismatch { .. })
            ));
        }

        #[test]
        fn test_expired_token() {
            let keypair = TestServiceKeypair::new("test_service");
            let pem = keypair.public_key_pem();

            // Create a token that expired 5 minutes ago
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let token = keypair.sign_token_with_times(now - 360, now - 60);

            let token_body = strip_prefix(&token).expect("should have dps_ prefix");

            let result = verify_service_token(token_body, "test_service", &pem);
            assert!(matches!(result, Err(ServiceTokenError::Expired { .. })));
        }

        #[test]
        fn test_not_yet_valid_token() {
            let keypair = TestServiceKeypair::new("test_service");
            let pem = keypair.public_key_pem();

            // Create a token that becomes valid in 5 minutes
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let token = keypair.sign_token_with_times(now + 300, now + 360);

            let token_body = strip_prefix(&token).expect("should have dps_ prefix");

            let result = verify_service_token(token_body, "test_service", &pem);
            assert!(matches!(result, Err(ServiceTokenError::NotYetValid { .. })));
        }

        #[test]
        fn test_duration_exceeds_max() {
            let keypair = TestServiceKeypair::new("test_service");
            let pem = keypair.public_key_pem();

            // Create a token with duration > MAX_DURATION_SECS
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let token = keypair.sign_token_with_times(now, now + MAX_DURATION_SECS + 100);

            let token_body = strip_prefix(&token).expect("should have dps_ prefix");

            let result = verify_service_token(token_body, "test_service", &pem);
            assert!(matches!(
                result,
                Err(ServiceTokenError::DurationExceedsMax { .. })
            ));
        }
    }
}
