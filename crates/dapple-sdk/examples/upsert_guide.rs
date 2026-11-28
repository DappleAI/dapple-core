//! Ingest (or re-ingest) one guide directory into a deployment.
//!
//! The upsert the dapple-prompt `canonical ingest` tool performs, without the
//! release-manifest machinery: find the guide by its source-path slug, PATCH a
//! new version when it exists, POST it when it does not.
//!
//! ```text
//! # a token for the deployment's user (prints it once):
//! nu -l -c 'use assets/nu/modules/dapple; dapple token pat create you@example.com --admin --expires-days 1'
//! DAPPLE_AUTH_TOKEN=dpl_... cargo run -p dapple-sdk --example upsert_guide --features tarball -- <guide-dir> [--dry-run]
//! ```
//!
//! Configuration (environment):
//! - `DAPPLE_AUTH_TOKEN` — a personal access token (`dpl_…`). Guide ingest is a
//!   user-scoped endpoint, so a service token is not enough.
//! - `DAPPLE_PROMPT_API_URL` — API base URL, with or without a trailing `/api`
//!   (default `https://dapple-prompt-api.kube.dappleai.dev`)
//!
//! `--dry-run` parses and validates server-side and stores nothing.

use dapple_sdk::{DappleClient, IngestParams, UpsertOutcome};
use std::path::PathBuf;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let Some(dir) = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
    else {
        eprintln!("usage: upsert_guide <guide-dir> [--dry-run]");
        return ExitCode::FAILURE;
    };
    if !dir.join("guide.toml").exists() {
        eprintln!("FAIL: {} has no guide.toml", dir.display());
        return ExitCode::FAILURE;
    }
    let Ok(pat) = std::env::var("DAPPLE_AUTH_TOKEN") else {
        eprintln!("FAIL: set DAPPLE_AUTH_TOKEN to a dpl_ personal access token");
        return ExitCode::FAILURE;
    };
    // The SDK's paths already start with `api/`, so the base is the bare host.
    // The dapple environment overlay exports the URL WITH a trailing `/api`
    // (the convention its other tools expect), which would request
    // `/api/api/guides/` and 404 — accept either form.
    let api_url = std::env::var("DAPPLE_PROMPT_API_URL")
        .unwrap_or_else(|_| "https://dapple-prompt-api.kube.dappleai.dev".into());
    let api_url = api_url
        .trim_end_matches('/')
        .trim_end_matches("/api")
        .to_string();

    println!("api:     {api_url}");
    println!("guide:   {}", dir.display());
    if dry_run {
        println!("mode:    dry run (parse and validate only, nothing stored)");
    }

    let client = DappleClient::with_api_token(&api_url, &pat);
    let params = IngestParams {
        dry_run,
        ..IngestParams::default()
    };
    match client.guides().upsert_from_dir(&dir, params).await {
        Ok(UpsertOutcome::Created(r)) => {
            println!(
                "created: {}",
                serde_json::to_string_pretty(&r).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Ok(UpsertOutcome::Updated(r)) => {
            println!(
                "updated: {}",
                serde_json::to_string_pretty(&r).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            ExitCode::FAILURE
        }
    }
}
