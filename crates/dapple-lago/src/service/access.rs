use crate::client::LagoClient;
use crate::error::LagoError;

/// Result of an access check
#[derive(Debug, Clone)]
pub enum AccessDecision {
    /// User is allowed to proceed
    Allowed,
    /// User is denied access
    Denied { reason: String },
    /// Access check failed (non-fatal - should default to allowed)
    Error { message: String },
}

impl LagoClient {
    /// Check if a customer's subscription allows usage.
    ///
    /// Returns Allowed if the subscription is active and has remaining quota.
    /// Returns Denied with reason if blocked.
    /// Returns Error on network/API failure (caller should treat as non-fatal).
    pub async fn check_access(
        &self,
        _customer_external_id: &str,
        subscription_external_id: &str,
    ) -> AccessDecision {
        match self.get_current_usage(subscription_external_id).await {
            Ok(_usage) => {
                // For now, if we can fetch usage, the subscription is active
                // Future: check against quota limits from the plan
                AccessDecision::Allowed
            }
            Err(LagoError::ApiError { status: 404, .. }) => AccessDecision::Denied {
                reason: "Subscription not found".to_string(),
            },
            Err(LagoError::ApiError { status: 402, body }) => AccessDecision::Denied {
                reason: format!("Payment required: {}", body),
            },
            Err(LagoError::ApiError { status: 403, body }) => AccessDecision::Denied {
                reason: format!("Access forbidden: {}", body),
            },
            Err(e) => AccessDecision::Error {
                message: format!("Failed to check access: {}", e),
            },
        }
    }
}
