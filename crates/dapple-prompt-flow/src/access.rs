//! Pre-flight access control for dispatch execution.
//!
//! Checks whether a user has billing entitlements before allowing
//! a dispatch to proceed. Gracefully degrades when Lago is unavailable.

use dapple_lago::client::LagoClient;
use dapple_lago::service::access::AccessDecision;

use dapple_data::DbPool;
use uuid::Uuid;

/// Check whether the user associated with a dispatch has access to execute it.
///
/// Resolution chain: dispatch → journey → user → user_billable_entity → billable_entity → Lago
///
/// Graceful degradation:
/// - No Lago client configured → returns `Allowed` (feature disabled)
/// - Billable entity has no Lago IDs → returns `Allowed` (free tier)
/// - Lago API unreachable → returns `Error` (caller should treat as non-fatal)
pub async fn check_dispatch_access(
    db: &DbPool,
    dispatch_id: Uuid,
    lago_client: &LagoClient,
) -> AccessDecision {
    // Resolve the billable entity for this dispatch via the journey → user chain.
    // Phase 4 will add select_billable_entity_for_dispatch() to dapple-data.
    // For now, this is a placeholder that always allows access.
    let _dispatch_id = dispatch_id;
    let _db = db;
    let _lago_client = lago_client;

    // TODO(Phase 4): Wire up actual billable entity resolution:
    //
    // let be = match select_billable_entity_for_dispatch(db, dispatch_id).await {
    //     Ok(be) => be,
    //     Err(e) => return AccessDecision::Error {
    //         message: format!("Failed to resolve billable entity: {}", e),
    //     },
    // };
    //
    // // No Lago IDs = free tier, allow
    // let (customer_id, subscription_id) = match (
    //     &be.lago_customer_external_id,
    //     &be.lago_subscription_external_id,
    // ) {
    //     (Some(c), Some(s)) => (c.clone(), s.clone()),
    //     _ => return AccessDecision::Allowed,
    // };
    //
    // lago_client.check_access(&customer_id, &subscription_id).await

    AccessDecision::Allowed
}
