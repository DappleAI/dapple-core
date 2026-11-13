use crate::config::LagoConfig;
use crate::error::LagoError;
use crate::models::*;

/// HTTP client for the Lago billing API
pub struct LagoClient {
    client: reqwest::Client,
    config: LagoConfig,
}

impl LagoClient {
    pub fn new(config: LagoConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    /// Send a single usage event to Lago
    pub async fn send_event(&self, event: LagoUsageEvent) -> Result<(), LagoError> {
        let url = format!("{}/api/v1/events", self.config.api_url);
        let body = LagoEventRequest { event };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Err(LagoError::ApiError { status, body })
        }
    }

    /// Create a customer in Lago
    pub async fn create_customer(
        &self,
        external_id: &str,
        name: &str,
        email: Option<&str>,
    ) -> Result<LagoCustomer, LagoError> {
        let url = format!("{}/api/v1/customers", self.config.api_url);
        let body = CreateCustomerRequest {
            customer: CreateCustomerBody {
                external_id: external_id.to_string(),
                name: name.to_string(),
                email: email.map(String::from),
            },
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await?;

        if response.status().is_success() {
            let resp: LagoCustomerResponse = response.json().await?;
            Ok(resp.customer)
        } else {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Err(LagoError::ApiError { status, body })
        }
    }

    /// Get current usage for a customer's subscription
    pub async fn get_current_usage(
        &self,
        external_subscription_id: &str,
    ) -> Result<CustomerUsage, LagoError> {
        let url = format!(
            "{}/api/v1/subscriptions/{}/current_usage",
            self.config.api_url, external_subscription_id
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await?;

        if response.status().is_success() {
            let resp: CurrentUsageResponse = response.json().await?;
            Ok(resp.customer_usage)
        } else {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Err(LagoError::ApiError { status, body })
        }
    }
}
