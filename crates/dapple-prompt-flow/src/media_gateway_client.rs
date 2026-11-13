//! HTTP client for the conduit-media-gateway service (Twitch VOD → learning
//! highlights).
//!
//! The media gateway uses an async job model: a `POST` kicks off a job and
//! returns a `job_id`; `GET /jobs/{id}` polls its status/result. [`run_job`]
//! block-and-polls to completion — the flow command that calls it does so via the
//! usual `block_in_place` + `block_on` bridge, so the operator sees a simple
//! "run → get result" call even though the server works asynchronously.

use std::time::Duration;

use reqwest::Client;
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum MediaGatewayError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("media gateway job failed: {0}")]
    Job(String),

    #[error(
        "{0} — the media gateway restarted mid-job (job state is process-local), \
         so the job was lost; re-dispatch to re-run it"
    )]
    JobLost(String),

    #[error("unexpected media gateway response: {0}")]
    Protocol(String),
}

/// Consecutive failed polls tolerated before giving up. A gateway pod restart
/// (e.g. an OOM kill mid-render) refuses connections for a window; riding it out
/// lets the next successful poll report the real state (usually a lost job → a
/// clear [`MediaGatewayError::JobLost`]) instead of an opaque transport error.
const MAX_CONSECUTIVE_POLL_FAILURES: u32 = 10;

/// Client for the conduit-media-gateway service.
pub struct MediaGatewayClient {
    client: Client,
    base_url: String,
    internal_key: Option<String>,
    poll_interval: Duration,
    max_polls: u32,
}

impl MediaGatewayClient {
    /// Construct against `base_url`, reading the shared `X-Internal-Key` from the
    /// environment (as the conduit client does). Poll cadence ~3s, up to ~3h.
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            internal_key: std::env::var("CONDUIT_INTERNAL_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            poll_interval: Duration::from_secs(3),
            max_polls: 3600,
        }
    }

    fn with_internal_key(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.internal_key {
            Some(key) => rb.header("X-Internal-Key", key),
            None => rb,
        }
    }

    /// `POST path` with `body`, returning the created `job_id`.
    pub async fn start_job(&self, path: &str, body: Value) -> Result<Uuid, MediaGatewayError> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .with_internal_key(self.client.post(&url).json(&body))
            .send()
            .await?;
        let status = response.status();
        let value: Value = response.json().await?;
        if !status.is_success() {
            return Err(MediaGatewayError::Job(error_message(&value)));
        }
        let job_id = value
            .get("job_id")
            .and_then(Value::as_str)
            .ok_or_else(|| MediaGatewayError::Protocol("response missing job_id".to_string()))?;
        Uuid::parse_str(job_id)
            .map_err(|e| MediaGatewayError::Protocol(format!("invalid job_id '{job_id}': {e}")))
    }

    /// Fetch a job record snapshot. A 404 is a lost job — the registry is
    /// process-local, so an id the gateway doesn't know means it restarted since
    /// the job was created.
    pub async fn get_job(&self, job_id: Uuid) -> Result<Value, MediaGatewayError> {
        let url = format!("{}/jobs/{job_id}", self.base_url);
        let response = self.with_internal_key(self.client.get(&url)).send().await?;
        let status = response.status();
        let value: Value = response.json().await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(MediaGatewayError::JobLost(error_message(&value)));
        }
        if !status.is_success() {
            return Err(MediaGatewayError::Job(error_message(&value)));
        }
        Ok(value)
    }

    /// Block-and-poll: start the job, poll to a terminal state, return its result
    /// payload (or the job's error). Transient poll transport errors (a restarting
    /// pod refusing connections) are retried up to
    /// [`MAX_CONSECUTIVE_POLL_FAILURES`] before failing the job.
    pub async fn run_job(&self, path: &str, body: Value) -> Result<Value, MediaGatewayError> {
        let job_id = self.start_job(path, body).await?;
        let mut consecutive_failures = 0u32;
        for _ in 0..self.max_polls {
            let record = match self.get_job(job_id).await {
                Ok(record) => {
                    consecutive_failures = 0;
                    record
                }
                Err(MediaGatewayError::Http(e)) => {
                    consecutive_failures += 1;
                    if consecutive_failures >= MAX_CONSECUTIVE_POLL_FAILURES {
                        return Err(MediaGatewayError::Http(e));
                    }
                    log::warn!(
                        "media gateway poll {consecutive_failures}/{MAX_CONSECUTIVE_POLL_FAILURES} \
                         for job {job_id} failed (transient?): {e}"
                    );
                    tokio::time::sleep(self.poll_interval).await;
                    continue;
                }
                Err(other) => return Err(other),
            };
            match record.get("status").and_then(Value::as_str) {
                Some("done") => {
                    return Ok(record.get("result").cloned().unwrap_or(Value::Null));
                }
                Some("failed") => {
                    let message = record
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("job failed")
                        .to_string();
                    return Err(MediaGatewayError::Job(message));
                }
                _ => tokio::time::sleep(self.poll_interval).await,
            }
        }
        Err(MediaGatewayError::Job(format!(
            "job {job_id} did not finish within the poll window"
        )))
    }
}

/// Extract an operator-facing message from an error/response body.
fn error_message(value: &Value) -> String {
    value
        .get("error")
        .and_then(Value::as_str)
        .map_or_else(|| value.to_string(), str::to_string)
}
