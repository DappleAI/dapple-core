//! Binary entry point for the external API surface.
//!
//! Listens on port 8081 by default (overridable via
//! `DAPPLE_PROMPT_EXTERNAL_API_PORT`). Loads
//! `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY` for JWT signing/verification —
//! a missing or empty key fails fast at startup so misconfigurations don't
//! manifest as confusing 503s mid-traffic.
//!
//! V1 surface is intentionally narrow: only `POST /api/get-service-token`
//! is wired up. List-guides, journey/briefs/dispatch endpoints come in
//! Issues 4–5.
//!
//! Future: Pattern B's plan calls for a unified `dapple-prompt-server`
//! binary with a `--surface={internal,external,user}` flag. For V1 the
//! external surface lives in its own binary; consolidation is a
//! follow-up that doesn't change the auth/routing model.

use dapple_data::DbPool;
use dapple_prompt_external_api::{EXTERNAL_JWT_SIGNING_KEY_ENV, ExternalAppState, build_router};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dapple_data::load_env();
    env_logger::init();

    // Fail fast if the surface signing key is missing — minting JWTs is
    // this surface's primary job and we want to surface the error before
    // any traffic hits us.
    let signing_key = match std::env::var(EXTERNAL_JWT_SIGNING_KEY_ENV) {
        Ok(s) if !s.is_empty() => s,
        _ => {
            eprintln!(
                "FATAL: {} is not set or empty; refusing to start the external surface",
                EXTERNAL_JWT_SIGNING_KEY_ENV
            );
            std::process::exit(1);
        }
    };
    // We don't actually pass the key into AppState — the handler and
    // extractor read it from env at use site. Reading at startup is just a
    // health check.
    drop(signing_key);

    let pool = match DbPool::connect().await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("Failed to connect to database: {}", err);
            std::process::exit(1);
        }
    };

    let port = std::env::var("DAPPLE_PROMPT_EXTERNAL_API_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8081);

    let bind_addr = format!("0.0.0.0:{}", port);
    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind {}: {}", bind_addr, e);
            std::process::exit(1);
        }
    };

    // The external surface always runs dispatch execution in-process (no
    // flow-url branch): stamp its executor identity on running events and
    // sweep attempts a previous incarnation left stranded mid-task — their
    // latest event is still non-terminal, which after a restart means the
    // owning process is gone and they would report `running` forever.
    dapple_prompt_flow::dispatch_runner::set_executor_identity("external-api");
    let sweep_pool = pool.clone();
    tokio::spawn(async move {
        dapple_prompt_flow::dispatch_runner::fail_orphaned_attempts(&sweep_pool, "external-api")
            .await;
    });

    let state = Arc::new(ExternalAppState::new(pool));
    let router = build_router(state);

    println!(
        "external-api listening on {} (iss=external-api.dapple.x)",
        bind_addr
    );

    axum::serve(listener, router.into_make_service())
        .await
        .unwrap();
}
