//! HTTP handlers for the conduit-media-gateway pipeline.
//!
//! Pipeline handlers are gated under `#[cfg(feature = "media-gateway")]`:
//!   - Stage B: `twitch_pull` + the `/jobs/{id}` poll endpoint
//!   - Stage C: `vod_transcribe`
//!   - Stage D: `vod_detect`
//!   - Stage E: `vod_cut`

#[cfg(feature = "media-gateway")]
pub mod jobs;
#[cfg(feature = "media-gateway")]
pub mod render_timeline;
#[cfg(feature = "media-gateway")]
pub mod snapshot;
#[cfg(feature = "media-gateway")]
pub mod twitch_pull;
#[cfg(feature = "media-gateway")]
pub mod vod_assemble;
#[cfg(feature = "media-gateway")]
pub mod vod_context;
#[cfg(feature = "media-gateway")]
pub mod vod_cut;
#[cfg(feature = "media-gateway")]
pub mod vod_detect;
#[cfg(feature = "media-gateway")]
pub mod vod_transcribe;

#[cfg(feature = "media-gateway")]
pub use jobs::handle_get_job;
#[cfg(feature = "media-gateway")]
pub use render_timeline::handle_render_timeline;
#[cfg(feature = "media-gateway")]
pub use snapshot::handle_snapshot;
#[cfg(feature = "media-gateway")]
pub use twitch_pull::handle_twitch_pull;
#[cfg(feature = "media-gateway")]
pub use vod_assemble::handle_vod_assemble;
#[cfg(feature = "media-gateway")]
pub use vod_context::handle_vod_context;
#[cfg(feature = "media-gateway")]
pub use vod_cut::handle_vod_cut;
#[cfg(feature = "media-gateway")]
pub use vod_detect::handle_vod_detect;
#[cfg(feature = "media-gateway")]
pub use vod_transcribe::handle_vod_transcribe;

/// Kick-off response shared by every async job endpoint (`POST` returns a
/// `job_id`; poll `GET /jobs/{id}`).
#[cfg(feature = "media-gateway")]
#[derive(Debug, serde::Serialize)]
pub struct JobKickoff {
    pub job_id: uuid::Uuid,
}
