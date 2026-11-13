use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use dapple_data::DbPool;
use dapple_data::integrations;
use dapple_data::query::dispatches::select_dispatch_model_by_universal_id;
use dapple_prompt_flow::dispatch_runner::{
    execute_dispatch_and_retrigger, fail_orphaned_attempts, reconcile_journey_dispatches,
    set_executor_identity,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

struct FlowState {
    db: Arc<DbPool>,
    /// Shared signing key for service-to-service auth.
    /// Must match `DAPPLE_PROMPT_API_SIGNING_KEY` on the API side.
    signing_key: String,
}

#[derive(Deserialize)]
struct ExecuteRequest {
    dispatch_id: Uuid,
}

#[derive(Deserialize)]
struct ReconcileRequest {
    journey_id: Uuid,
}

#[tokio::main]
async fn main() {
    dapple_data::load_env();
    env_logger::init();

    let signing_key = std::env::var("DAPPLE_PROMPT_API_SIGNING_KEY")
        .expect("DAPPLE_PROMPT_API_SIGNING_KEY must be set");

    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("DB connect failed: {:?}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = integrations::init_publisher_pool(pool.get_pool().clone()) {
        eprintln!("failed to initialize publisher pool: {}", e);
        std::process::exit(1);
    }

    let pool = Arc::new(pool);

    // This process hosts in-process dispatch execution: stamp its identity on
    // every running event, and sweep attempts a previous incarnation left
    // stranded mid-task (their latest event is still non-terminal — provably
    // dead once we are booting, since execution is in-process). Without the
    // sweep those attempts report `running` forever and their journeys stall.
    set_executor_identity("flow");
    let sweep_pool = pool.clone();
    tokio::spawn(async move {
        fail_orphaned_attempts(&sweep_pool, "flow").await;
    });

    // Spawn the background extraction/embedding worker loop. This is a
    // simple polling worker for the Phase B content pipeline — it claims
    // `extract_pdf` and `embed_extraction` jobs from `rev.background_jobs`
    // and runs them until the process dies.
    let worker_pool = pool.clone();
    let worker_conduit_url = std::env::var("DAPPLE_PROMPT_CONDUIT_URL").ok();
    tokio::spawn(async move {
        dapple_prompt_flow::run_extraction_workers(worker_pool, 2, worker_conduit_url).await;
    });

    let state = Arc::new(FlowState {
        db: pool,
        signing_key,
    });

    let app = Router::new()
        .route("/execute", post(execute_handler))
        .route("/reconcile", post(reconcile_handler))
        .route("/api/health_check", get(health_check))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8082").await.unwrap();
    println!("dapple-prompt-flow listening on 0.0.0.0:8082");
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "OK"
}

async fn execute_handler(
    State(state): State<Arc<FlowState>>,
    headers: HeaderMap,
    Json(body): Json<ExecuteRequest>,
) -> StatusCode {
    // Verify the caller passes the shared signing key as a Bearer token
    let authorized = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.signing_key);

    if !authorized {
        return StatusCode::UNAUTHORIZED;
    }

    let dispatch_id = body.dispatch_id;
    let state = state.clone();

    tokio::spawn(async move {
        let model = match select_dispatch_model_by_universal_id(&state.db, dispatch_id).await {
            Ok(m) => m,
            Err(e) => {
                log::error!("Failed to load dispatch {}: {}", dispatch_id, e);
                return;
            }
        };

        // `_and_retrigger` (not bare `execute_dispatch`) so the flow executor drives
        // the journey-advancement cascade: after this dispatch finishes it drains and
        // auto-dispatches any now-ready downstream no-input steps. Without this, a
        // flow-delegated journey (the `DAPPLE_PROMPT_FLOW_URL` topology) stalls at the
        // first stage — nothing else ever re-drives it. Mirrors the in-process path in
        // `dapple-prompt-api`'s dispatches handler.
        if let Err(e) = execute_dispatch_and_retrigger((*state.db).clone(), model, 4).await {
            log::error!("Dispatch execution failed for {}: {}", dispatch_id, e);
        }
    });

    StatusCode::ACCEPTED
}

/// Best-effort recovery driver, called by the API's `/journeys/{id}/stages` and
/// `/steps` poll handlers when they run in flow-delegation mode
/// (`DAPPLE_PROMPT_FLOW_URL` set). Execution belongs in this pod, so the API
/// delegates the drain here rather than running it in-process. Re-drives a
/// stalled journey whose in-flight cascade was lost (e.g. this pod restarted
/// during a long task). Idempotent; the per-journey in-flight guard inside
/// `drain_ready_dispatches` makes the continuous polling safe.
async fn reconcile_handler(
    State(state): State<Arc<FlowState>>,
    headers: HeaderMap,
    Json(body): Json<ReconcileRequest>,
) -> StatusCode {
    let authorized = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.signing_key);

    if !authorized {
        return StatusCode::UNAUTHORIZED;
    }

    let journey_id = body.journey_id;
    let state = state.clone();
    tokio::spawn(reconcile_journey_dispatches(
        (*state.db).clone(),
        journey_id,
        4,
    ));

    StatusCode::ACCEPTED
}
