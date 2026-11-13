//! Test consumer for `/api/companies/{id}/external-service-tokens/` and
//! `/api/external-service-tokens/{id}` endpoints.

use crate::consumer::error::ConsumerError;
use crate::consumer::{test_admin_auth_header, test_auth_header};
use dapple_prompt_api::schema::external_service_tokens::{
    CreateExternalServiceTokenResponse, CreateExternalServiceTokenSchema,
    ListExternalServiceTokensResponse, RevokeExternalServiceTokenResponse,
    RevokeExternalServiceTokenSchema,
};
use log::info;
use reqwest::{Client, StatusCode, Url};
use uuid::Uuid;

fn company_collection(base_url: &Url, company_id: Uuid) -> Result<Url, ConsumerError> {
    Ok(base_url
        .join("api/companies/")?
        .join(&format!("{}/external-service-tokens/", company_id))?)
}

fn token_url(base_url: &Url, token_id: Uuid) -> Result<Url, ConsumerError> {
    Ok(base_url
        .join("api/external-service-tokens/")?
        .join(&token_id.to_string())?)
}

/// Sample Ed25519 PEM (generated once with `openssl genpkey -algorithm
/// ed25519 | openssl pkey -pubout` so tests don't depend on the openssl
/// CLI being present at test time). 32-byte raw key under a SPKI wrapper.
pub const SAMPLE_PUBLIC_KEY_PEM_A: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAzbZKwGvSLspClRmp5vn2XHHr9YEG3gPbOLZkINTrkH8=\n-----END PUBLIC KEY-----\n";

/// Second Ed25519 PEM for tests that need two distinct keys.
pub const SAMPLE_PUBLIC_KEY_PEM_B: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA5depqOz3HczG0mQtE1qqeRztXaupPbJphKaoIVmPsx8=\n-----END PUBLIC KEY-----\n";

pub async fn create_token(
    base_url: &Url,
    company_id: Uuid,
    schema: CreateExternalServiceTokenSchema,
) -> Result<CreateExternalServiceTokenResponse, ConsumerError> {
    let url = company_collection(base_url, company_id)?;
    let resp = Client::new()
        .post(url.clone())
        .header("Authorization", test_admin_auth_header())
        .json(&schema)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    info!("POST {} (create token) → {}: {}", url, status, text);
    if status != StatusCode::CREATED {
        return Err(ConsumerError::Custom(format!(
            "POST {} expected 201, got {}: {}",
            url, status, text
        )));
    }
    Ok(serde_json::from_str(&text)?)
}

/// Variant returning the raw status, used to assert error responses.
pub async fn create_token_status(
    base_url: &Url,
    company_id: Uuid,
    schema: CreateExternalServiceTokenSchema,
    use_admin: bool,
) -> Result<(StatusCode, String), ConsumerError> {
    let url = company_collection(base_url, company_id)?;
    let header = if use_admin {
        test_admin_auth_header()
    } else {
        test_auth_header()
    };
    let resp = Client::new()
        .post(url.clone())
        .header("Authorization", header)
        .json(&schema)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    Ok((status, text))
}

pub async fn list_tokens(
    base_url: &Url,
    company_id: Uuid,
) -> Result<ListExternalServiceTokensResponse, ConsumerError> {
    let url = company_collection(base_url, company_id)?;
    let resp = Client::new()
        .get(url.clone())
        .header("Authorization", test_admin_auth_header())
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(ConsumerError::Custom(format!(
            "GET {} expected 200, got {}: {}",
            url, status, text
        )));
    }
    Ok(serde_json::from_str(&text)?)
}

pub async fn revoke_token(
    base_url: &Url,
    token_id: Uuid,
    reason: Option<&str>,
) -> Result<RevokeExternalServiceTokenResponse, ConsumerError> {
    let url = token_url(base_url, token_id)?;
    let resp = Client::new()
        .delete(url.clone())
        .header("Authorization", test_admin_auth_header())
        .json(&RevokeExternalServiceTokenSchema {
            reason: reason.map(String::from),
        })
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(ConsumerError::Custom(format!(
            "DELETE {} expected 200, got {}: {}",
            url, status, text
        )));
    }
    Ok(serde_json::from_str(&text)?)
}

pub async fn revoke_token_status(
    base_url: &Url,
    token_id: Uuid,
    use_admin: bool,
) -> Result<StatusCode, ConsumerError> {
    let url = token_url(base_url, token_id)?;
    let header = if use_admin {
        test_admin_auth_header()
    } else {
        test_auth_header()
    };
    let resp = Client::new()
        .delete(url.clone())
        .header("Authorization", header)
        .json(&RevokeExternalServiceTokenSchema { reason: None })
        .send()
        .await?;
    Ok(resp.status())
}
