//! Response envelope types for the Dapple API.
//!
//! The API wraps all responses in an envelope:
//! - Index/POST: `{ status, results, objs }`
//! - GET single: `{ status, results, obj }`
//! - Error: `{ status, message }`

use serde::Deserialize;

/// Envelope for index/list and batch POST responses.
#[derive(Debug, Deserialize)]
pub(crate) struct IndexEnvelope<T> {
    #[allow(dead_code)]
    pub status: String,
    #[allow(dead_code)]
    pub results: usize,
    pub objs: Vec<T>,
}

/// Envelope for single-object GET responses.
#[derive(Debug, Deserialize)]
pub(crate) struct GetEnvelope<T> {
    #[allow(dead_code)]
    pub status: String,
    #[allow(dead_code)]
    pub results: usize,
    pub obj: T,
}

/// Error envelope returned by the API.
///
/// Tolerant of every error-body shape the platform emits so the clean
/// `message` is always extracted:
/// - internal legacy: `{ "status": "fail", "message": ... }`
/// - unified / external: `{ "error": "<CODE>", "message": ... }`
///
/// `status` and `error` are accepted but never read (the SDK keys cache
/// invalidation off the HTTP status code, not the body); only `message` is
/// surfaced to callers.
#[derive(Debug, Deserialize)]
pub(crate) struct ErrorEnvelope {
    #[allow(dead_code)]
    #[serde(default)]
    pub status: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    pub error: Option<String>,
    pub message: String,
}
