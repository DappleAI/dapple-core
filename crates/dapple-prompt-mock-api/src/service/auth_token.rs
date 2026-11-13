//! Auth token verification service for Ethereum-signed tokens.
//!
//! This module verifies auth tokens signed with Ethereum private keys (EIP-191 personal_sign)
//! and recovers the signer address for authorization checks.
//!
//! **Note:** This is a copy of the production implementation from `dapple-prompt-api`
//! to ensure identical behavior in the mock server.

use alloy_primitives::Address;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use sha3::{Digest, Keccak256};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Expected token version byte
pub const TOKEN_VERSION: u8 = 0x01;

/// Maximum allowed duration between valid_from and valid_until (30 minutes)
pub const MAX_DURATION_SECS: u64 = 1800;

/// Expected decoded token length: 65 (signature) + 64 (message) = 129 bytes
pub const EXPECTED_TOKEN_LEN: usize = 129;

/// Signature length in bytes (r: 32, s: 32, v: 1)
const SIGNATURE_LEN: usize = 65;

/// Message length in bytes
const MESSAGE_LEN: usize = 64;

/// Errors that can occur during auth token verification
#[derive(Debug, Error)]
pub enum AuthTokenError {
    #[error("MALFORMED_TOKEN: {0}")]
    MalformedToken(String),

    #[error("INVALID_SIGNATURE: failed to recover signer from signature")]
    InvalidSignature,

    #[error("INVALID_VERSION: expected version {expected}, got {actual}")]
    InvalidVersion { expected: u8, actual: u8 },

    #[error("DURATION_EXCEEDS_MAX: token duration {duration_secs}s exceeds maximum {max_secs}s")]
    DurationExceedsMax { duration_secs: u64, max_secs: u64 },

    #[error("NOT_YET_VALID: token becomes valid at {valid_from}, current time is {current}")]
    NotYetValid { valid_from: u64, current: u64 },

    #[error("EXPIRED: token expired at {valid_until}, current time is {current}")]
    Expired { valid_until: u64, current: u64 },

    #[error("SIGNER_NOT_AUTHORIZED: address {signer} is not in the allowlist")]
    SignerNotAuthorized { signer: Address },
}

/// Result of successful token verification
#[derive(Debug)]
pub struct VerifiedToken {
    /// The recovered Ethereum address of the signer
    pub signer: Address,
    /// When the token became valid (Unix timestamp)
    pub valid_from: u64,
    /// When the token expires (Unix timestamp)
    pub valid_until: u64,
}

/// Verify an auth token and recover the signer address.
///
/// # Arguments
/// * `token` - Base64-encoded 129-byte signed token
/// * `allowed_signers` - Optional list of allowed signer addresses. If None, any signer is allowed.
///
/// # Returns
/// * `Ok(VerifiedToken)` - Token is valid, contains the recovered signer address
/// * `Err(AuthTokenError)` - Token verification failed
pub fn verify_auth_token(
    token: &str,
    allowed_signers: Option<&[Address]>,
) -> Result<VerifiedToken, AuthTokenError> {
    // 1. Decode base64
    let decoded = BASE64
        .decode(token)
        .map_err(|e| AuthTokenError::MalformedToken(format!("invalid base64: {}", e)))?;

    // 2. Validate length
    if decoded.len() != EXPECTED_TOKEN_LEN {
        return Err(AuthTokenError::MalformedToken(format!(
            "expected {} bytes, got {}",
            EXPECTED_TOKEN_LEN,
            decoded.len()
        )));
    }

    // 3. Split into signature and message
    let (signature_bytes, message_bytes) = decoded.split_at(SIGNATURE_LEN);

    // 4. Parse message: version(1) + valid_from(8) + valid_until(8) + padding(47)
    let version = message_bytes[0];
    let valid_from = u64::from_be_bytes(message_bytes[1..9].try_into().unwrap());
    let valid_until = u64::from_be_bytes(message_bytes[9..17].try_into().unwrap());

    // 5. Validate version
    if version != TOKEN_VERSION {
        return Err(AuthTokenError::InvalidVersion {
            expected: TOKEN_VERSION,
            actual: version,
        });
    }

    // 6. Validate duration
    let duration_secs = valid_until.saturating_sub(valid_from);
    if duration_secs > MAX_DURATION_SECS {
        return Err(AuthTokenError::DurationExceedsMax {
            duration_secs,
            max_secs: MAX_DURATION_SECS,
        });
    }

    // 7. Check time window
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before Unix epoch")
        .as_secs();

    if current_time < valid_from {
        return Err(AuthTokenError::NotYetValid {
            valid_from,
            current: current_time,
        });
    }

    if current_time > valid_until {
        return Err(AuthTokenError::Expired {
            valid_until,
            current: current_time,
        });
    }

    // 8. Recover signer using EIP-191 personal_sign
    let signer = recover_signer(signature_bytes, message_bytes)?;

    // 9. Check allowlist if provided
    if let Some(allowed) = allowed_signers
        && !allowed.contains(&signer)
    {
        return Err(AuthTokenError::SignerNotAuthorized { signer });
    }

    Ok(VerifiedToken {
        signer,
        valid_from,
        valid_until,
    })
}

/// Recover the signer address from an EIP-191 personal_sign signature.
///
/// EIP-191 message format: "\x19Ethereum Signed Message:\n" + len(message) + message
fn recover_signer(signature_bytes: &[u8], message_bytes: &[u8]) -> Result<Address, AuthTokenError> {
    // Extract signature components (r, s, v)
    // Signature format: r (32 bytes) || s (32 bytes) || v (1 byte)
    let r_s = &signature_bytes[..64];
    let v = signature_bytes[64];

    // Convert v to recovery id (Ethereum uses 27/28, we need 0/1)
    // RecoveryId::new(is_y_odd, is_x_reduced)
    // For standard Ethereum signatures, is_x_reduced is always false
    let recovery_id = match v {
        27 | 0 => RecoveryId::new(false, false),
        28 | 1 => RecoveryId::new(true, false),
        _ => return Err(AuthTokenError::InvalidSignature),
    };

    // Create the EIP-191 prefixed message hash
    let message_hash = eip191_hash(message_bytes);

    // Parse the signature
    let signature = Signature::from_slice(r_s).map_err(|_| AuthTokenError::InvalidSignature)?;

    // Recover the public key
    let recovered_key = VerifyingKey::recover_from_prehash(&message_hash, &signature, recovery_id)
        .map_err(|_| AuthTokenError::InvalidSignature)?;

    // Convert public key to Ethereum address
    let address = public_key_to_address(&recovered_key);

    Ok(address)
}

/// Compute the EIP-191 message hash for personal_sign.
///
/// Format: keccak256("\x19Ethereum Signed Message:\n" + len(message) + message)
fn eip191_hash(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());

    let mut hasher = Keccak256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(message);

    hasher.finalize().into()
}

/// Convert an ECDSA public key to an Ethereum address.
///
/// Address = keccak256(uncompressed_public_key[1..])[12..32]
fn public_key_to_address(key: &VerifyingKey) -> Address {
    // Get uncompressed public key bytes (65 bytes: 0x04 || x || y)
    let public_key = key.as_affine();
    let encoded = public_key.to_encoded_point(false);
    let bytes = encoded.as_bytes();

    // Skip the 0x04 prefix, hash the 64-byte (x, y) coordinates
    let mut hasher = Keccak256::new();
    hasher.update(&bytes[1..]);
    let hash = hasher.finalize();

    // Take last 20 bytes as address
    Address::from_slice(&hash[12..])
}

/// Sign an auth token using the provided signing key.
///
/// This is used by the mock server to generate tokens for testing.
pub fn sign_auth_token(signing_key: &SigningKey, valid_for_secs: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before Unix epoch")
        .as_secs();

    sign_auth_token_with_times(signing_key, now, now + valid_for_secs)
}

/// Sign an auth token with explicit timestamps.
pub fn sign_auth_token_with_times(
    signing_key: &SigningKey,
    valid_from: u64,
    valid_until: u64,
) -> String {
    // Build message: version (1) + valid_from (8) + valid_until (8) + padding (47) = 64 bytes
    let mut message = vec![TOKEN_VERSION];
    message.extend_from_slice(&valid_from.to_be_bytes());
    message.extend_from_slice(&valid_until.to_be_bytes());
    message.resize(MESSAGE_LEN, 0); // Pad to 64 bytes

    // Create EIP-191 prefixed hash
    let digest = eip191_hash(&message);

    // Sign with recoverable signature
    let (signature, recovery_id) = signing_key
        .sign_prehash_recoverable(&digest)
        .expect("signing should not fail");

    // Build token: r (32) || s (32) || v (1) || message (64) = 129 bytes
    let mut token_bytes = signature.to_bytes().to_vec(); // r || s (64 bytes)
    token_bytes.push(recovery_id.to_byte() + 27); // v in Ethereum format (27 or 28)
    token_bytes.extend_from_slice(&message);

    BASE64.encode(&token_bytes)
}

/// Extract error code prefix from an AuthTokenError
pub fn extract_error_code(e: &AuthTokenError) -> &'static str {
    match e {
        AuthTokenError::MalformedToken(_) => "MALFORMED_TOKEN",
        AuthTokenError::InvalidSignature => "INVALID_SIGNATURE",
        AuthTokenError::InvalidVersion { .. } => "INVALID_VERSION",
        AuthTokenError::DurationExceedsMax { .. } => "DURATION_EXCEEDS_MAX",
        AuthTokenError::NotYetValid { .. } => "NOT_YET_VALID",
        AuthTokenError::Expired { .. } => "EXPIRED",
        AuthTokenError::SignerNotAuthorized { .. } => "SIGNER_NOT_AUTHORIZED",
    }
}
