//! In-memory async-job registry.
//!
//! Long media work (VOD download, transcription, clip cutting) runs in a spawned
//! task; the HTTP handler returns a `job_id` immediately and the caller polls
//! `GET /jobs/{id}`. State is process-local (lost on restart) — this is a
//! single-replica, opt-in dev service, so that is acceptable: a re-kick re-runs
//! the job, and finished derivatives already live in the DB as assets.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Lifecycle of a media-gateway job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
}

/// A single job's state, serialized by `GET /jobs/{id}`.
#[derive(Clone, Debug, Serialize)]
pub struct JobRecord {
    pub id: Uuid,
    /// Job kind, e.g. `"twitch_pull"`, `"transcribe"`, `"detect"`, `"cut"`.
    pub kind: String,
    pub status: JobStatus,
    /// Present when `status == Done`: the job's result payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Present when `status == Failed`: an operator-facing error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Process-local job registry (clonable handle over shared state).
#[derive(Clone, Default)]
pub struct JobRegistry {
    inner: Arc<RwLock<HashMap<Uuid, JobRecord>>>,
}

impl JobRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new `Queued` job and return its id (UUIDv7 — time-ordered).
    pub async fn create(&self, kind: &str) -> Uuid {
        let id = Uuid::now_v7();
        let record = JobRecord {
            id,
            kind: kind.to_string(),
            status: JobStatus::Queued,
            result: None,
            error: None,
        };
        self.inner.write().await.insert(id, record);
        id
    }

    /// Transition a job to `Running`.
    pub async fn mark_running(&self, id: Uuid) {
        if let Some(rec) = self.inner.write().await.get_mut(&id) {
            rec.status = JobStatus::Running;
        }
    }

    /// Transition a job to `Done` with a result payload.
    pub async fn mark_done(&self, id: Uuid, result: serde_json::Value) {
        if let Some(rec) = self.inner.write().await.get_mut(&id) {
            rec.status = JobStatus::Done;
            rec.result = Some(result);
        }
    }

    /// Transition a job to `Failed` with an operator-facing error message.
    pub async fn mark_failed(&self, id: Uuid, error: String) {
        if let Some(rec) = self.inner.write().await.get_mut(&id) {
            rec.status = JobStatus::Failed;
            rec.error = Some(error);
        }
    }

    /// Fetch a snapshot of a job's state.
    pub async fn get(&self, id: Uuid) -> Option<JobRecord> {
        self.inner.read().await.get(&id).cloned()
    }
}
