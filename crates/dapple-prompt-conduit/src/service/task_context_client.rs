//! Thin HTTP client for dapple-prompt-api's `GET /api/dispatches/{id}/task_context/{task_id}`.
//!
//! Conduit calls this before rendering an LLM prompt so it can inject dynamic
//! runtime state (bindings, assets, prior tasks, brief context) as a system
//! message alongside the existing RAG section.
//!
//! This is deliberately a minimal mirror of the `TaskContextResponse` DTO from
//! `dapple-prompt-api::schema::dispatches`. Conduit does not depend on
//! dapple-prompt-api, so we redefine the shape here with `serde(default)` /
//! `Option` fields so missing fields are tolerated across versions.

use base64::Engine as _;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use sha2::Sha256;
use std::time::Duration;
use uuid::Uuid;

/// Client for the api's task_context endpoint. If `base_url` is `None`, every
/// `fetch()` call returns `Ok(None)` so callers can treat task-context
/// injection as a no-op when no api is configured.
#[derive(Clone)]
pub struct TaskContextClient {
    base_url: Option<String>,
    signing_key: Option<String>,
    client: Client,
}

impl TaskContextClient {
    pub fn new(base_url: Option<String>, signing_key: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client for TaskContextClient");
        Self {
            base_url: base_url.map(|u| u.trim_end_matches('/').to_string()),
            signing_key,
            client,
        }
    }

    /// Mint a short-lived HS256 JWT with `token_use: "service"`.
    /// The api middleware verifies this the same way it verifies user JWTs.
    fn mint_service_jwt(&self) -> Option<String> {
        let key = self.signing_key.as_ref()?;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let header = r#"{"alg":"HS256","typ":"JWT"}"#;
        let exp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + 60;
        let payload = format!(
            r#"{{"sub":"conduit","service_id":"conduit","token_use":"service","exp":{}}}"#,
            exp
        );

        let header_b64 = b64.encode(header.as_bytes());
        let payload_b64 = b64.encode(payload.as_bytes());
        let message = format!("{}.{}", header_b64, payload_b64);

        let mut mac =
            Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key size");
        mac.update(message.as_bytes());
        let signature = mac.finalize().into_bytes();
        let sig_b64 = b64.encode(signature);

        Some(format!("{}.{}", message, sig_b64))
    }

    /// Fetch task context for a given dispatch task.
    ///
    /// Returns `Ok(None)` if no api base URL is configured or if the api
    /// returned an envelope without an `obj` field. Returns `Err(String)` for
    /// transport / HTTP / decode errors so callers can log and continue.
    pub async fn fetch(
        &self,
        dispatch_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<TaskContext>, String> {
        let Some(base) = self.base_url.as_ref() else {
            return Ok(None);
        };

        let url = format!(
            "{}/api/dispatches/{}/task_context/{}",
            base, dispatch_id, task_id
        );

        let mut req = self.client.get(&url);
        if let Some(jwt) = self.mint_service_jwt() {
            req = req.bearer_auth(jwt);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("task_context request failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "task_context returned HTTP {} for {}: {}",
                status, url, body
            ));
        }

        let envelope: ApiEnvelope<TaskContext> = resp
            .json()
            .await
            .map_err(|e| format!("failed to decode task_context response: {}", e))?;

        Ok(envelope.obj)
    }
}

/// api's `{ status, results, obj }` envelope. We only care about `obj`.
#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    #[serde(default = "Option::default")]
    obj: Option<T>,
}

// ---------------------------------------------------------------------------
// TaskContextResponse mirror (minimal, tolerant of version drift).
// Field names and shapes must match `dapple-prompt-api::schema::dispatches`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct TaskContext {
    pub dispatch_id: Uuid,
    pub task_id: Uuid,
    #[serde(default)]
    pub bindings: Vec<TaskContextBinding>,
    #[serde(default)]
    pub assets: Vec<TaskContextAsset>,
    pub dispatch_position: TaskContextDispatchPosition,
    #[serde(default)]
    pub prior_tasks: Vec<TaskContextPriorTask>,
    #[serde(default)]
    pub brief_context: Vec<TaskContextBrief>,
    /// Raw CONDUITS.md markdown of the journey's pinned guide version — the
    /// conduit's guide frame. Rendered as its own system message ahead of the
    /// dispatch context; `None`/empty means no frame is injected.
    #[serde(default)]
    pub guide_frame: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskContextBinding {
    pub slot_name: String,
    pub value_kind: String,
    #[serde(default)]
    pub value_preview: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskContextAsset {
    pub asset_id: Uuid,
    pub extraction_kind: String,
    pub status: String,
    #[serde(default)]
    pub page_count: Option<i32>,
    #[serde(default)]
    pub text_length: Option<i64>,
    pub retrieval_command: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskContextDispatchPosition {
    pub stage_index: i32,
    #[serde(default)]
    pub stage_name: Option<String>,
    pub step_index: i32,
    #[serde(default)]
    pub step_name: Option<String>,
    pub task_index: i32,
    #[serde(default)]
    pub task_name: Option<String>,
    #[serde(default)]
    pub stage_description: Option<String>,
    #[serde(default)]
    pub step_description: Option<String>,
    #[serde(default)]
    pub stage_ord: Option<i32>,
    #[serde(default)]
    pub total_stages: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskContextPriorTask {
    pub task_id: Uuid,
    #[serde(default)]
    pub task_name: Option<String>,
    pub status: String,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskContextBrief {
    pub brief_id: Uuid,
    #[serde(default)]
    pub brief_name: Option<String>,
    #[serde(default)]
    pub units: Vec<TaskContextBriefUnit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskContextBriefUnit {
    pub unit_kind: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub submitted_value: Option<serde_json::Value>,
}
