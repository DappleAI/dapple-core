//! Service layer for conduit-oximedia.
//!
//! [`audio_analysis`] turns multitrack WAV stems into an annotated song map;
//! [`av`] arranges generated A/V pieces into an editable OTIO timeline. Both are
//! pure local computation over OxiMedia; the only shared state is a DB pool used
//! by the handlers to read input assets and persist output assets.

#[cfg(feature = "oximedia")]
pub mod audio_analysis;
#[cfg(feature = "oximedia")]
pub mod av;
#[cfg(feature = "oximedia")]
pub mod wav;

use dapple_data::DbPool;
use std::sync::Arc;

/// Shared application state (just a DB pool — the analysis/compose work is
/// stateless).
#[derive(Clone)]
pub struct AppState {
    /// Database pool for reading input assets + persisting output assets.
    pub pool: Arc<DbPool>,
}

impl AppState {
    /// Construct from a connected pool.
    #[must_use]
    pub fn new(pool: DbPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }
}
