use crate::client::LagoClient;
use crate::error::LagoError;
use crate::models::LagoUsageEvent;

impl LagoClient {
    /// Send a batch of events (sequentially, since Lago doesn't have a batch endpoint)
    pub async fn send_events_batch(
        &self,
        events: Vec<LagoUsageEvent>,
    ) -> Vec<Result<(), LagoError>> {
        let mut results = Vec::with_capacity(events.len());
        for event in events {
            results.push(self.send_event(event).await);
        }
        results
    }
}
