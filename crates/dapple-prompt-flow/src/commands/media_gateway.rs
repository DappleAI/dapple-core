//! Shared helpers for the `twitch`/`vod` media-gateway commands.
//!
//! Each command builds a JSON body, kicks off (or block-and-polls) a media-gateway
//! job via the usual `block_in_place` + `block_on` bridge, and converts the JSON
//! result into a nushell value.

use nu_engine::command_prelude::*;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::commands::prompt_send::get_media_gateway_url;
use crate::engine::json_to_nu_value;
use crate::media_gateway_client::{MediaGatewayClient, MediaGatewayError};

/// Kick off (`is_async`) or block-and-poll a media-gateway job, returning the raw
/// JSON result — the job's result payload, or `{ job_id }` when `is_async`.
#[allow(clippy::result_large_err)]
pub fn run_job_raw(
    path: &str,
    body: JsonValue,
    is_async: bool,
    span: Span,
) -> Result<JsonValue, ShellError> {
    let url = get_media_gateway_url().map_err(|msg| not_configured(msg, span))?;
    let client = MediaGatewayClient::new(&url);
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            if is_async {
                client
                    .start_job(path, body)
                    .await
                    .map(|id| serde_json::json!({ "job_id": id.to_string() }))
            } else {
                client.run_job(path, body).await
            }
        })
    });
    result.map_err(|e| job_error(path, &e, span))
}

/// Poll a single job's current record (for `vod job`).
#[allow(clippy::result_large_err)]
pub fn poll_job(job_id: Uuid, span: Span) -> Result<JsonValue, ShellError> {
    let url = get_media_gateway_url().map_err(|msg| not_configured(msg, span))?;
    let client = MediaGatewayClient::new(&url);
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async { client.get_job(job_id).await })
    });
    result.map_err(|e| job_error("/jobs", &e, span))
}

/// Convert a JSON value into a nushell pipeline value.
pub fn to_pipeline(value: &JsonValue) -> PipelineData {
    PipelineData::Value(json_to_nu_value(value), None)
}

/// Read a required string (a VOD url/id) from the pipe.
#[allow(clippy::result_large_err)]
pub fn piped_string(input: PipelineData, what: &str, span: Span) -> Result<String, ShellError> {
    let value = input.into_value(span)?;
    let text = value
        .as_str()
        .map_err(|_| missing_input(what, span))?
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(missing_input(what, span));
    }
    Ok(text)
}

pub fn not_configured(msg: String, span: Span) -> ShellError {
    ShellError::GenericError {
        error: "Media gateway not configured".into(),
        msg,
        span: Some(span),
        help: Some("Set DAPPLE_MEDIA_GATEWAY_URL to the conduit-media-gateway service URL".into()),
        inner: vec![],
    }
}

/// Extract a required string field from a JSON job result.
#[allow(clippy::result_large_err)]
pub fn json_get_str(value: &JsonValue, field: &str, span: Span) -> Result<String, ShellError> {
    value
        .get(field)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .ok_or_else(|| missing_field(field, span))
}

pub fn missing_field(field: &str, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Missing field `{field}`"),
        msg: format!("The piped record needs `{field}`"),
        span: Some(span),
        help: None,
        inner: vec![],
    }
}

pub fn missing_input(what: &str, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Expected {what}"),
        msg: format!("Provide {what} as an argument or via the pipe"),
        span: Some(span),
        help: None,
        inner: vec![],
    }
}

fn job_error(path: &str, error: &MediaGatewayError, span: Span) -> ShellError {
    ShellError::GenericError {
        error: format!("Media gateway {path} failed: {error}"),
        msg: "The conduit-media-gateway request did not succeed".into(),
        span: Some(span),
        help: None,
        inner: vec![],
    }
}
