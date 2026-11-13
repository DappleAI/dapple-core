use serde::{Deserialize, Serialize};

/// A usage event to send to Lago
#[derive(Debug, Clone, Serialize)]
pub struct LagoUsageEvent {
    pub transaction_id: String,
    pub external_subscription_id: String,
    pub code: String,
    pub timestamp: i64,
    pub properties: serde_json::Value,
}

/// Wrapper for the Lago events API request body
#[derive(Debug, Serialize)]
pub(crate) struct LagoEventRequest {
    pub event: LagoUsageEvent,
}

/// Response from Lago events API
#[derive(Debug, Deserialize)]
pub struct LagoEventResponse {
    #[serde(default)]
    pub status: Option<String>,
}

/// Customer creation request
#[derive(Debug, Serialize)]
pub struct CreateCustomerRequest {
    pub customer: CreateCustomerBody,
}

#[derive(Debug, Serialize)]
pub struct CreateCustomerBody {
    pub external_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Customer response from Lago
#[derive(Debug, Deserialize)]
pub struct LagoCustomerResponse {
    pub customer: LagoCustomer,
}

#[derive(Debug, Deserialize)]
pub struct LagoCustomer {
    pub lago_id: String,
    pub external_id: String,
    pub name: Option<String>,
}

/// Current usage response
#[derive(Debug, Deserialize)]
pub struct CurrentUsageResponse {
    pub customer_usage: CustomerUsage,
}

#[derive(Debug, Deserialize)]
pub struct CustomerUsage {
    pub from_datetime: String,
    pub to_datetime: String,
    pub total_amount_cents: i64,
    pub charges_usage: Vec<ChargeUsage>,
}

#[derive(Debug, Deserialize)]
pub struct ChargeUsage {
    pub billable_metric: BillableMetricRef,
    pub units: String,
    pub amount_cents: i64,
}

#[derive(Debug, Deserialize)]
pub struct BillableMetricRef {
    pub code: String,
    pub name: Option<String>,
}
