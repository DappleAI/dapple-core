use std::time::Duration;

use dapple_data::DbPool;
use dapple_lago::client::LagoClient;
use sqlx::postgres::PgListener;

use crate::sync;

/// Run the worker loop: LISTEN/NOTIFY + periodic sweep.
///
/// This function never returns. It processes pending sync records
/// triggered by PostgreSQL notifications and a periodic safety-net sweep.
pub async fn run_worker(pool: &DbPool, lago: &LagoClient) -> ! {
    let mut listener = PgListener::connect_with(pool.get_pool())
        .await
        .expect("Failed to connect PG LISTEN");
    listener
        .listen("lago_event_sync")
        .await
        .expect("Failed to LISTEN on lago_event_sync");

    log::info!("Listening for lago_event_sync notifications");

    // Process any backlog from before this worker started
    let processed = sync::process_pending_syncs(pool, lago).await;
    log::info!("Startup sweep: processed {} pending syncs", processed);

    // Safety net: periodic sweep every 60s alongside LISTEN
    let mut sweep_interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            notification = listener.recv() => {
                match notification {
                    Ok(n) => {
                        log::debug!("Received NOTIFY: {}", n.payload());
                        sync::process_pending_syncs(pool, lago).await;
                    }
                    Err(e) => {
                        log::error!("LISTEN error: {}, reconnecting in 5s", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
            _ = sweep_interval.tick() => {
                let processed = sync::process_pending_syncs(pool, lago).await;
                if processed > 0 {
                    log::info!("Periodic sweep: processed {} pending syncs", processed);
                }
            }
        }
    }
}
