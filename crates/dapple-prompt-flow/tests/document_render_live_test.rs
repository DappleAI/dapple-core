//! Live end-to-end test for the `document render` command.
//!
//! `#[ignore]`d — needs a running conduit with the `/render-document` endpoint
//! (and its database). Run manually against the sandbox:
//!
//! ```sh
//! DAPPLE_PROMPT_CONDUIT_URL="http://dapple-prompt-conduit.kube.dappleai.dev" \
//!   cargo test -p dapple-prompt-flow --test document_render_live_test -- --ignored --nocapture
//! ```

use dapple_prompt_flow::engine::StepEngine;

/// A guide task script calling `document render` runs end-to-end through the
/// StepEngine: registration, pipeline-input coercion, the block_in_place bridge,
/// the conduit render + asset persist, and record conversion — returning an
/// asset_id the computed-output asset arm can bind.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live test; requires a running conduit at DAPPLE_PROMPT_CONDUIT_URL"]
async fn document_render_runs_inside_step_engine() {
    assert!(
        std::env::var("DAPPLE_PROMPT_CONDUIT_URL").is_ok_and(|v| !v.trim().is_empty()),
        "set DAPPLE_PROMPT_CONDUIT_URL to run the live document render test"
    );
    let mut engine = StepEngine::new();
    let script = r##"
        let body = "# Year over Year\n\n| Item | 2024 | 2025 |\n|------|------|------|\n| Revenue | 10 | 12 |\n\nClosing paragraph."
        let doc = ($body | document render "StepEngine Smoke Report" --author "dapple" --branding technical)
        {
            payload: { report_asset_id: $doc.asset_id, mime: $doc.mime_type, bytes: $doc.content_length },
            signals: { rendered: true }
        }
    "##;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("document render must run inside the StepEngine");

    let asset_id = output.payload["report_asset_id"]
        .as_str()
        .expect("payload must carry the asset id");
    uuid::Uuid::parse_str(asset_id).expect("asset_id must be a UUID");
    assert_eq!(output.payload["mime"], serde_json::json!("application/pdf"));
    assert!(
        output.payload["bytes"].as_i64().unwrap_or(0) > 1000,
        "PDF should not be trivially small"
    );
    assert_eq!(output.signals["rendered"], serde_json::json!(true));
    println!("rendered document asset: {asset_id}");
}

/// An empty piped body fails with the shaped error before any network call —
/// deterministic and offline, so not `#[ignore]`d.
#[tokio::test(flavor = "multi_thread")]
async fn document_render_empty_body_errors_cleanly() {
    let mut engine = StepEngine::new();
    let script = r#""   " | document render "Report""#;
    let err = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect_err("document render must reject an empty body");
    let msg = err.to_string();
    assert!(
        msg.contains("Empty document body") || msg.contains("empty"),
        "the error must explain the empty body, got: {msg}"
    );
}
