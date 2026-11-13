// Builder helpers wired up by the worker main loop. The loop is being
// landed incrementally; until each builder gains its caller, allow the
// dead-code warning module-wide rather than annotating each fn.
#![allow(dead_code)]

use dapple_lago::models::LagoUsageEvent;
use serde_json::json;

/// Build a Lago usage event for LLM token consumption.
///
/// Maps an LLM usage record to Lago's event format with the `llm_tokens`
/// billable metric code.
pub fn build_llm_event(
    transaction_id: &str,
    subscription_external_id: &str,
    input_tokens: i32,
    output_tokens: i32,
    timestamp: i64,
) -> LagoUsageEvent {
    LagoUsageEvent {
        transaction_id: transaction_id.to_string(),
        external_subscription_id: subscription_external_id.to_string(),
        code: "llm_tokens".to_string(),
        timestamp,
        properties: json!({
            "tokens": input_tokens + output_tokens,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }),
    }
}

/// Build a Lago usage event for asset storage consumption.
pub fn build_asset_event(
    transaction_id: &str,
    subscription_external_id: &str,
    size_bytes: i64,
    asset_kind: &str,
    timestamp: i64,
) -> LagoUsageEvent {
    LagoUsageEvent {
        transaction_id: transaction_id.to_string(),
        external_subscription_id: subscription_external_id.to_string(),
        code: "asset_storage".to_string(),
        timestamp,
        properties: json!({
            "size_bytes": size_bytes,
            "asset_kind": asset_kind,
        }),
    }
}

/// Build a Lago usage event for external service calls.
pub fn build_service_call_event(
    transaction_id: &str,
    subscription_external_id: &str,
    service_key: &str,
    units: i32,
    timestamp: i64,
) -> LagoUsageEvent {
    LagoUsageEvent {
        transaction_id: transaction_id.to_string(),
        external_subscription_id: subscription_external_id.to_string(),
        code: format!("service_call_{}", service_key),
        timestamp,
        properties: json!({
            "units": units,
            "service": service_key,
        }),
    }
}
