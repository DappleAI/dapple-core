//! Shared state for the mock API server.

use crate::store::InMemoryStore;
use alloy_primitives::Address;
use k256::ecdsa::SigningKey;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use rand::rngs::OsRng;
use sha3::{Digest, Keccak256};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for optional authentication
#[derive(Clone, Default)]
pub struct MockAuthConfig {
    /// Whether authentication is enabled
    pub enabled: bool,
    /// JWT secret for signing tokens
    pub jwt_secret: Option<String>,
    /// Allowed signer addresses (if empty and enabled, only mock's address is allowed)
    pub allowed_signers: Vec<Address>,
}

/// Ethereum keypair for the mock server
pub struct MockKeypair {
    pub signing_key: SigningKey,
    pub address: Address,
}

impl MockKeypair {
    /// Generate a new random keypair
    pub fn random() -> Self {
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let address = public_key_to_address(verifying_key);

        Self {
            signing_key,
            address,
        }
    }

    /// Get the address as a checksummed hex string (0x-prefixed)
    pub fn address_hex(&self) -> String {
        format!("{:#x}", self.address)
    }
}

/// Convert an ECDSA public key to an Ethereum address.
fn public_key_to_address(key: &k256::ecdsa::VerifyingKey) -> Address {
    let public_key = key.as_affine();
    let encoded = public_key.to_encoded_point(false);
    let bytes = encoded.as_bytes();

    let mut hasher = Keccak256::new();
    hasher.update(&bytes[1..]);
    let hash = hasher.finalize();

    Address::from_slice(&hash[12..])
}

/// Shared state for the mock API server
pub struct MockState {
    /// The in-memory data store holding all mock entities
    pub store: Arc<RwLock<InMemoryStore>>,
    /// Optional auth configuration (disabled by default)
    pub auth_config: MockAuthConfig,
    /// The mock server's Ethereum keypair (always generated)
    pub keypair: MockKeypair,
}

impl MockState {
    pub fn new(store: InMemoryStore) -> Self {
        let keypair = MockKeypair::random();
        tracing::info!(
            "Mock server keypair generated. Address: {}",
            keypair.address_hex()
        );

        Self {
            store: Arc::new(RwLock::new(store)),
            auth_config: MockAuthConfig::default(),
            keypair,
        }
    }

    /// Enable authentication with the given JWT secret.
    ///
    /// The mock server's own address is automatically added to allowed_signers.
    pub fn with_auth(mut self, jwt_secret: impl Into<String>) -> Self {
        let allowed_signers = vec![self.keypair.address];
        self.auth_config = MockAuthConfig {
            enabled: true,
            jwt_secret: Some(jwt_secret.into()),
            allowed_signers,
        };
        self
    }

    /// Enable authentication with custom allowed signers.
    ///
    /// The mock server's own address is automatically added to allowed_signers.
    pub fn with_auth_and_signers(
        mut self,
        jwt_secret: impl Into<String>,
        additional_signers: Vec<Address>,
    ) -> Self {
        let mut allowed_signers = vec![self.keypair.address];
        allowed_signers.extend(additional_signers);

        self.auth_config = MockAuthConfig {
            enabled: true,
            jwt_secret: Some(jwt_secret.into()),
            allowed_signers,
        };
        self
    }

    /// Check if authentication is enabled
    pub fn is_auth_enabled(&self) -> bool {
        self.auth_config.enabled
    }

    /// Get the mock server's Ethereum address
    pub fn mock_address(&self) -> Address {
        self.keypair.address
    }

    /// Get the mock server's Ethereum address as hex string
    pub fn mock_address_hex(&self) -> String {
        self.keypair.address_hex()
    }
}
