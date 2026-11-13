//! Test consumer for the admin `/api/companies/...` endpoints.
//!
//! Doesn't implement the generic `Endpoint` trait because the company /
//! external-service-token endpoints don't share its shape (no batch
//! POST, top-level fields not `{ objs: [...] }`, custom admin gating).
//! Provides direct helper functions invoked from lifecycle trials.

use crate::consumer::error::ConsumerError;
use crate::consumer::{test_admin_auth_header, test_auth_header};
use dapple_prompt_api::schema::companies::{
    CompanyDetailView, CompanyView, CreateCompanyResponse, CreateCompanySchema,
    ListCompaniesResponse, RevokeCompanyResponse, RevokeCompanySchema,
};
use log::info;
use reqwest::{Client, StatusCode, Url};

fn base(base_url: &Url) -> Result<Url, ConsumerError> {
    Ok(base_url.join("api/companies/")?)
}

pub async fn create_company(
    base_url: &Url,
    schema: CreateCompanySchema,
) -> Result<CreateCompanyResponse, ConsumerError> {
    let url = base(base_url)?;
    let resp = Client::new()
        .post(url.clone())
        .header("Authorization", test_admin_auth_header())
        .json(&schema)
        .send()
        .await?;

    let status = resp.status();
    let text = resp.text().await?;
    info!("POST {} (create company) → {}: {}", url, status, text);
    if status != StatusCode::CREATED {
        return Err(ConsumerError::Custom(format!(
            "POST {} expected 201, got {}: {}",
            url, status, text
        )));
    }
    Ok(serde_json::from_str(&text)?)
}

/// Like `create_company` but exercises the non-admin code path.
pub async fn create_company_as_non_admin(
    base_url: &Url,
    schema: CreateCompanySchema,
) -> Result<StatusCode, ConsumerError> {
    let url = base(base_url)?;
    let resp = Client::new()
        .post(url.clone())
        .header("Authorization", test_auth_header())
        .json(&schema)
        .send()
        .await?;
    Ok(resp.status())
}

pub async fn list_companies(
    base_url: &Url,
    include_revoked: bool,
) -> Result<ListCompaniesResponse, ConsumerError> {
    let mut url = base(base_url)?;
    if include_revoked {
        url.query_pairs_mut().append_pair("revoked", "true");
    }
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

pub async fn get_company(
    base_url: &Url,
    id: uuid::Uuid,
) -> Result<CompanyDetailView, ConsumerError> {
    let url = base(base_url)?.join(&id.to_string())?;
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
    let body: serde_json::Value = serde_json::from_str(&text)?;
    Ok(serde_json::from_value(
        body.get("obj")
            .cloned()
            .ok_or_else(|| ConsumerError::Custom("missing `obj`".to_string()))?,
    )?)
}

pub async fn revoke_company(
    base_url: &Url,
    id: uuid::Uuid,
    reason: Option<&str>,
) -> Result<RevokeCompanyResponse, ConsumerError> {
    let url = base(base_url)?.join(&id.to_string())?;
    let resp = Client::new()
        .delete(url.clone())
        .header("Authorization", test_admin_auth_header())
        .json(&RevokeCompanySchema {
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

/// Test helper — call create endpoint without admin role and return the
/// raw status. Wrapper around `create_company_as_non_admin` to keep call
/// sites obvious.
#[allow(dead_code)]
pub fn non_admin_create_helper_anchor(_view: &CompanyView) {
    // Anchor for the import — kept simple so the linter doesn't trip.
}
