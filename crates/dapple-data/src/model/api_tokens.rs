//! Models for API tokens (programmatic access).

use crate::error::DataError;
use crate::{DateTime, InsertColumnArgs, Keyed};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Public DTO for API token (without the plaintext token).
/// Used for GET /api/api-tokens responses.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct ApiToken {
    pub id: Uuid,
    pub name: String,
    pub token_prefix: String,
    pub expires_at: Option<DateTime>,
    pub created_at: DateTime,
    pub revoked_at: Option<DateTime>,
}

/// Response DTO for POST /api/api-tokens.
/// Contains the plaintext token (shown only once).
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct ApiTokenWithSecret {
    pub id: Uuid,
    pub name: String,
    pub token: String,
    pub token_prefix: String,
    pub expires_at: Option<DateTime>,
    pub created_at: DateTime,
}

/// Internal model with all database fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ApiTokenModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub token_hash: Vec<u8>,
    pub token_prefix: String,
    pub expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

impl Keyed for ApiTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Flat query result from JOIN with revocations.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatApiTokenModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub token_hash: Vec<u8>,
    pub token_prefix: String,
    pub expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

impl FlatApiTokenModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<ApiTokenModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| ApiTokenModel {
                id: m.id,
                user_id: m.user_id,
                name: m.name,
                token_hash: m.token_hash,
                token_prefix: m.token_prefix,
                expires_at: m.expires_at,
                created_at: m.created_at,
                revoked_at: m.revoked_at,
            })
            .collect())
    }
}

/// Model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedApiTokenModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub token_hash: Vec<u8>,
    pub token_prefix: String,
    pub expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

impl Keyed for InsertedApiTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Column-wise insert arguments.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertApiTokenArgs {
    pub user_ids: Vec<Uuid>,
    pub names: Vec<String>,
    pub token_hashes: Vec<Vec<u8>>,
    pub token_prefixes: Vec<String>,
    pub expires_ats: Vec<Option<OffsetDateTime>>,
}

impl InsertColumnArgs for InsertApiTokenArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("user_ids", self.user_ids.len()),
            ("names", self.names.len()),
            ("token_hashes", self.token_hashes.len()),
            ("token_prefixes", self.token_prefixes.len()),
            ("expires_ats", self.expires_ats.len()),
        ])
    }
}

/// Model for token hash lookup during auth validation.
/// Used by select_api_token_by_hash.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ApiTokenValidationModel {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub expires_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
    /// Whether this token has admin privileges (from sparse web.admin_api_tokens table)
    pub is_admin: bool,
    /// User's roles from web.user_roles (defaults to ['user'] if none assigned)
    pub user_roles: Vec<String>,
}

/// Revocation insert arguments.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertApiTokenRevocationArgs {
    pub api_token_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
}

impl InsertColumnArgs for InsertApiTokenRevocationArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("api_token_ids", self.api_token_ids.len()),
            ("reasons", self.reasons.len()),
        ])
    }
}

/// Model returned from revocation INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedApiTokenRevocationModel {
    pub id: Uuid,
    pub api_token_id: Uuid,
    pub reason: Option<String>,
    pub revoked_at: OffsetDateTime,
}

// Conversions

impl From<ApiTokenModel> for ApiToken {
    fn from(m: ApiTokenModel) -> Self {
        Self {
            id: m.id,
            name: m.name,
            token_prefix: m.token_prefix,
            expires_at: m.expires_at.map(Into::into),
            created_at: m.created_at.into(),
            revoked_at: m.revoked_at.map(Into::into),
        }
    }
}

impl crate::traits::RelationalObject for ApiTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::InsertedModel for InsertedApiTokenModel {}
impl crate::traits::RelationalObject for InsertedApiTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::DataTransferObject for ApiToken {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}

impl crate::traits::DataTransferObject for ApiTokenWithSecret {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}
