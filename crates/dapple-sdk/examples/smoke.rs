//! Post-deploy smoke test for the internal API.
//!
//! Proves a deployed stack actually works end to end, not just that its pods
//! are up: authenticates, then calls user-scoped endpoints that hit the
//! database.
//!
//! ```text
//! # first user + token on a fresh deploy (prints the token once):
//! dapple token pat create you@example.com --admin --create-user
//! DAPPLE_SMOKE_PAT=dpl_... cargo run -p dapple-sdk --example smoke --features service-auth
//! ```
//!
//! Configuration (environment):
//! - `DAPPLE_SMOKE_API_URL` — API base URL
//!   (default `https://dapple-prompt-api.kube.dappleai.dev`)
//! - `DAPPLE_SMOKE_PAT` — a personal access token (`dpl_…`). This is the
//!   real user path and exercises the data endpoints.
//!
//! Without a PAT the example only checks the service-token exchange: it signs
//! an Ed25519 service token, exchanges it for a JWT at `/api/get-service-token`
//! and stops, because data endpoints require a user principal.
//! - `DAPPLE_SMOKE_SERVICE_ID` — a service registered in `web.service_tokens`
//!   (default `dapple_ui`, which `deploy_dapple_core.nu` registers)
//! - `<SERVICE_ID>_SAT_SIGNING_KEY` — that service's 32-byte Ed25519 private
//!   key as 64 hex chars (e.g. `DAPPLE_UI_SAT_SIGNING_KEY`). If unset, it is
//!   read from `~/.dapple/generated.env`, where the deploy writes it.
//!
//! Exits non-zero on the first failure.

use dapple_sdk::DappleClient;
use std::process::ExitCode;

fn hex_key(hex: &str) -> Result<[u8; 32], String> {
    let hex = hex.trim().trim_matches('"');
    if hex.len() != 64 {
        return Err(format!("expected 64 hex chars, got {}", hex.len()));
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)
            .map_err(|e| format!("invalid hex at byte {i}: {e}"))?;
    }
    Ok(key)
}

/// `KEY=value` lookup in the deploy's generated env file.
fn from_generated_env(var: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let text = std::fs::read_to_string(format!("{home}/.dapple/generated.env")).ok()?;
    text.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == var)
        .map(|(_, v)| v.trim().to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    let api_url = std::env::var("DAPPLE_SMOKE_API_URL")
        .unwrap_or_else(|_| "https://dapple-prompt-api.kube.dappleai.dev".into());
    println!("api:     {api_url}");

    if let Ok(pat) = std::env::var("DAPPLE_SMOKE_PAT") {
        println!("auth:    personal access token");
        return user_checks(&DappleClient::with_api_token(&api_url, &pat)).await;
    }

    let service_id =
        std::env::var("DAPPLE_SMOKE_SERVICE_ID").unwrap_or_else(|_| "dapple_ui".into());
    let key_var = format!("{}_SAT_SIGNING_KEY", service_id.to_uppercase());

    let Some(key_hex) = std::env::var(&key_var)
        .ok()
        .or_else(|| from_generated_env(&key_var))
    else {
        eprintln!("FAIL: {key_var} is not set and not found in ~/.dapple/generated.env");
        return ExitCode::FAILURE;
    };
    let key = match hex_key(&key_hex) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("FAIL: {key_var}: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("auth:    service token [{service_id}]");
    let client = DappleClient::with_service_token(&api_url, &service_id, &key);

    // A service JWT is not a user principal, so a user-scoped endpoint must
    // answer 401 "User authentication required" — reaching that answer proves
    // the signed token was accepted and exchanged. Anything else is a failure.
    match client.playbooks().list(None).await {
        Err(dapple_sdk::SdkError::Api {
            status: 401,
            message,
        }) if message.contains("User authentication required") => {
            println!("ok       service token exchanged for a JWT");
            println!("note     set DAPPLE_SMOKE_PAT to also check the data endpoints");
            ExitCode::SUCCESS
        }
        Ok(_) => {
            println!("ok       service token exchanged; playbooks.list succeeded");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAIL     service token exchange: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Authenticated, database-backed reads as a user.
async fn user_checks(client: &DappleClient) -> ExitCode {
    match client.playbooks().list(None).await {
        Ok(p) => println!("ok       playbooks.list -> {} playbook(s)", p.len()),
        Err(e) => {
            eprintln!("FAIL     playbooks.list: {e}");
            return ExitCode::FAILURE;
        }
    }
    match client.guides().list(None).await {
        Ok(g) => println!("ok       guides.list    -> {} guide(s)", g.len()),
        Err(e) => {
            eprintln!("FAIL     guides.list: {e}");
            return ExitCode::FAILURE;
        }
    }
    println!("smoke test passed");
    ExitCode::SUCCESS
}
