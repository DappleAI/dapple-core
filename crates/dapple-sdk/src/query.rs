//! Bracket-notation query string serialization.
//!
//! The Dapple API expects tagged enum queries serialized with bracket notation:
//! ```text
//! GET /api/journeys/?query=by_user&user_id=<uuid>&options[limit]=10&options[page]=1
//! ```
//!
//! This module provides `bracket_serialize` which uses `serde_qs` to produce
//! the correct bracket-notation format.

use crate::SdkError;
use serde::Serialize;

/// Serialize a value into a bracket-notation query string.
///
/// Uses `serde_qs` which produces bracket notation for nested objects:
/// `FilterOptions { limit: Some(10), page: Some(1) }` becomes
/// `options[limit]=10&options[page]=1`.
pub fn bracket_serialize<T: Serialize>(value: &T) -> Result<String, SdkError> {
    serde_qs::to_string(value).map_err(|e| SdkError::Serialization(e.to_string()))
}
