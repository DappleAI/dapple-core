//! Service layer for conduit-media-gateway.
//!
//! [`jobs`] is the process-local async-job registry (long media work runs in a
//! spawned task; the handler returns a `job_id` and the caller polls). The shared
//! [`AppState`] carries a DB pool (read input assets, persist derivatives), the
//! job registry, and the loaded [`Config`].

pub mod jobs;

#[cfg(feature = "media-gateway")]
pub mod assemble;
#[cfg(feature = "media-gateway")]
pub mod context;
#[cfg(feature = "media-gateway")]
pub mod cut;
#[cfg(feature = "media-gateway")]
pub mod detect;
#[cfg(feature = "media-gateway")]
pub mod layered;
#[cfg(feature = "media-gateway")]
pub mod media;
#[cfg(feature = "media-gateway")]
pub mod scenes;
#[cfg(feature = "media-gateway")]
pub mod svg_mask;
#[cfg(feature = "media-gateway")]
pub mod transcribe;
#[cfg(feature = "media-gateway")]
pub mod twitch;
#[cfg(feature = "media-gateway")]
pub mod vad;
#[cfg(feature = "media-gateway")]
pub mod vodid;

use std::sync::Arc;

use dapple_data::DbPool;

use crate::config::Config;
use jobs::JobRegistry;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Database pool for reading input assets + persisting output assets.
    pub pool: Arc<DbPool>,
    /// Process-local async-job registry.
    pub jobs: JobRegistry,
    /// Loaded service configuration (Twitch creds, whisper model, scratch dir).
    pub config: Arc<Config>,
}

impl AppState {
    /// Construct from a connected pool and loaded config.
    #[must_use]
    pub fn new(pool: DbPool, config: Config) -> Self {
        Self {
            pool: Arc::new(pool),
            jobs: JobRegistry::new(),
            config: Arc::new(config),
        }
    }
}
