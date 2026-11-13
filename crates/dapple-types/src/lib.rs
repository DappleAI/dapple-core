//! Public DTO types for the Dapple Prompt API.
//!
//! This crate contains lightweight, consumer-facing types that are shared
//! between `dapple-data` (server) and `dapple-sdk` (client). It depends
//! only on `serde`, `schemars`, `uuid`, `time`, `serde_json`, and
//! `dapple-hash` -- no database or async dependencies.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

pub mod model;
pub mod reference;

// Re-export ContentHash from dapple-hash.
pub use dapple_hash::ContentHash;

// ---------------------------------------------------------------------------
// DateTime
// ---------------------------------------------------------------------------

/// Custom wrapper around `time::OffsetDateTime` with RFC 3339 serde and
/// JsonSchema implementations.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DateTime(pub OffsetDateTime);

impl From<OffsetDateTime> for DateTime {
    fn from(dt: OffsetDateTime) -> Self {
        DateTime(dt)
    }
}

impl From<DateTime> for OffsetDateTime {
    fn from(dt: DateTime) -> Self {
        dt.0
    }
}

impl Serialize for DateTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = self.0.format(&Rfc3339).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let dt = OffsetDateTime::parse(&s, &Rfc3339).map_err(serde::de::Error::custom)?;
        Ok(DateTime(dt))
    }
}

impl JsonSchema for DateTime {
    fn schema_name() -> Cow<'static, str> {
        "DateTime".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::DateTime").into()
    }

    fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "format": "date-time"
        })
    }
}

// ---------------------------------------------------------------------------
// FilterOptions
// ---------------------------------------------------------------------------

/// Pagination parameters for list endpoints.
#[derive(Clone, JsonSchema, Serialize, Deserialize, Debug, Default)]
pub struct FilterOptions {
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

impl FilterOptions {
    /// Convert pagination parameters to SQL LIMIT and OFFSET values.
    ///
    /// Returns `(limit, offset)` where:
    /// - `limit` defaults to 50 if not specified
    /// - `offset` is computed from `(page - 1) * limit`, defaulting page to 1
    pub fn sql_values(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(50) as i64;
        let offset = (self.page.unwrap_or(1) - 1) as i64 * limit;

        (limit, offset)
    }
}

// ---------------------------------------------------------------------------
// JsonObject
// ---------------------------------------------------------------------------

/// Schema helper for freeform JSON object fields.
///
/// Use via `#[schemars(with = "JsonObject")]` on `serde_json::Value` fields
/// so the OpenAPI spec emits `{ "type": "object", "additionalProperties": true }`
/// instead of bare `true`.
pub struct JsonObject;

impl JsonSchema for JsonObject {
    fn schema_name() -> Cow<'static, str> {
        "JsonObject".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::JsonObject").into()
    }

    fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

// ---------------------------------------------------------------------------
// DataTransferObject
// ---------------------------------------------------------------------------

/// Data Transfer Object (DTO) contract for consumer-facing types.
///
/// Note: DTOs are consumer-facing, sanitized representations derived from internal
/// `RelationalObject` model types. DTOs expose `id: Uuid` as the stable
/// external identifier.
///
/// Recommendation: DTO types should derive `serde::Serialize` and
/// `schemars::JsonSchema` so they are suitable for API responses and OpenAPI
/// generation. Implement `From<Model> for DTO` on the DTO to enable
/// `DataTransferObject::from_model()` to use the `Into` default.
pub trait DataTransferObject: Serialize + JsonSchema + Eq + Clone {
    /// Return the DTO's UUID.
    fn id(&self) -> &Uuid;

    /// Generic helper to create a DTO from a model type that implements `Into<Self>`.
    /// This avoids exposing the concrete model type in the trait's public signature.
    fn from_model<M>(m: M) -> Self
    where
        Self: Sized,
        M: Into<Self>,
    {
        m.into()
    }
}
