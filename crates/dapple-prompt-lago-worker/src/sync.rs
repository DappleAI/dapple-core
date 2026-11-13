use dapple_data::DbPool;
use dapple_lago::client::LagoClient;

/// Process all pending sync records.
///
/// Fetches unsynced records from `rev.lago_event_sync`, resolves the
/// associated usage data and billable entity, builds Lago events, and
/// sends them. Updates the sync record with the result.
///
/// Returns the number of records processed.
pub async fn process_pending_syncs(pool: &DbPool, _lago: &LagoClient) -> usize {
    // Phase 4 will implement the actual query functions.
    // For now, this is a placeholder that will be wired up once
    // the dapple-data models and queries for lago_event_sync exist.
    //
    // The flow will be:
    // 1. select_pending_syncs(pool, limit=100)
    // 2. For each: load usage record + resolve billable entity
    // 3. Build LagoUsageEvent via event_builder
    // 4. lago.send_event(event)
    // 5. update_lago_event_sync_result() with sent_at and response
    // 6. On failure: increment retry_count, set next_retry_at with backoff

    log::debug!("Checking for pending lago syncs (placeholder — Phase 4 wiring needed)");
    let _ = pool; // suppress unused warning until wired
    0
}

/// Calculate exponential backoff duration for retries.
/// Caps at 1 hour.
#[allow(dead_code)] // wired up by the retry path in a follow-up commit
pub fn retry_backoff(retry_count: i32) -> std::time::Duration {
    let seconds = match retry_count {
        0 => 30,
        1 => 60,
        2 => 300,
        3 => 900,
        _ => 3600,
    };
    std::time::Duration::from_secs(seconds)
}
