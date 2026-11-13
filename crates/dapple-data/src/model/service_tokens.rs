//! Models for service tokens (asymmetric key authentication).

use crate::error::DataError;
use crate::{DateTime, InsertColumnArgs, Keyed};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::types::Uuid;
use time::OffsetDateTime;

/// Public DTO for service token (without the public key).
/// Used for GET /api/service-tokens responses.
#[derive(Clone, Eq, PartialEq, JsonSchema, Debug, Deserialize, Serialize)]
pub struct ServiceToken {
    pub id: Uuid,
    pub service_id: String,
    pub name: String,
    pub description: Option<String>,
    pub key_algorithm: String,
    pub created_at: DateTime,
    pub revoked_at: Option<DateTime>,
}

/// Internal model with all database fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServiceTokenModel {
    pub id: Uuid,
    pub service_id: String,
    pub name: String,
    pub description: Option<String>,
    pub public_key_pem: String,
    pub key_algorithm: String,
    pub created_at: OffsetDateTime,
    pub created_by_user_id: Option<Uuid>,
    pub revoked_at: Option<OffsetDateTime>,
}

impl Keyed for ServiceTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Flat query result from JOIN with revocations.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FlatServiceTokenModel {
    pub id: Uuid,
    pub service_id: String,
    pub name: String,
    pub description: Option<String>,
    pub public_key_pem: String,
    pub key_algorithm: String,
    pub created_at: OffsetDateTime,
    pub created_by_user_id: Option<Uuid>,
    pub revoked_at: Option<OffsetDateTime>,
}

impl FlatServiceTokenModel {
    pub fn fold_models(models: Vec<Self>) -> Result<Vec<ServiceTokenModel>, DataError> {
        Ok(models
            .into_iter()
            .map(|m| ServiceTokenModel {
                id: m.id,
                service_id: m.service_id,
                name: m.name,
                description: m.description,
                public_key_pem: m.public_key_pem,
                key_algorithm: m.key_algorithm,
                created_at: m.created_at,
                created_by_user_id: m.created_by_user_id,
                revoked_at: m.revoked_at,
            })
            .collect())
    }
}

/// Model returned from INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedServiceTokenModel {
    pub id: Uuid,
    pub service_id: String,
    pub name: String,
    pub created_at: OffsetDateTime,
}

impl Keyed for InsertedServiceTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

/// Column-wise insert arguments.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertServiceTokenArgs {
    pub service_ids: Vec<String>,
    pub names: Vec<String>,
    pub descriptions: Vec<Option<String>>,
    pub public_key_pems: Vec<String>,
    pub key_algorithms: Vec<String>,
    pub created_by_user_ids: Vec<Option<Uuid>>,
}

impl InsertColumnArgs for InsertServiceTokenArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("service_ids", self.service_ids.len()),
            ("names", self.names.len()),
            ("descriptions", self.descriptions.len()),
            ("public_key_pems", self.public_key_pems.len()),
            ("key_algorithms", self.key_algorithms.len()),
            ("created_by_user_ids", self.created_by_user_ids.len()),
        ])
    }
}

/// Model for service token lookup during auth validation.
/// Used by select_service_token_by_service_id.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServiceTokenValidationModel {
    pub id: Uuid,
    pub service_id: String,
    pub public_key_pem: String,
    pub key_algorithm: String,
    pub revoked_at: Option<OffsetDateTime>,
}

/// Revocation insert arguments.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertServiceTokenRevocationArgs {
    pub service_token_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
}

impl InsertColumnArgs for InsertServiceTokenRevocationArgs {
    fn column_counts(&self) -> Result<Vec<(&str, usize)>, DataError> {
        Ok(vec![
            ("service_token_ids", self.service_token_ids.len()),
            ("reasons", self.reasons.len()),
        ])
    }
}

/// Model returned from revocation INSERT...RETURNING.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InsertedServiceTokenRevocationModel {
    pub id: Uuid,
    pub service_token_id: Uuid,
    pub reason: Option<String>,
    pub revoked_at: OffsetDateTime,
}

// Conversions

impl From<ServiceTokenModel> for ServiceToken {
    fn from(m: ServiceTokenModel) -> Self {
        Self {
            id: m.id,
            service_id: m.service_id,
            name: m.name,
            description: m.description,
            key_algorithm: m.key_algorithm,
            created_at: m.created_at.into(),
            revoked_at: m.revoked_at.map(Into::into),
        }
    }
}

impl crate::traits::RelationalObject for ServiceTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::InsertedModel for InsertedServiceTokenModel {}
impl crate::traits::RelationalObject for InsertedServiceTokenModel {
    fn id(&self) -> &Uuid {
        &self.id
    }
}

impl crate::traits::DataTransferObject for ServiceToken {
    fn id(&self) -> &sqlx::types::Uuid {
        &self.id
    }
}
