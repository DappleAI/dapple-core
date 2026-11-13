//! Integration tests that require real backend infrastructure (ONNX models or
//! API keys).
//!
//! These are NO LONGER `#[ignore]`d. Instead each test probes its prerequisite
//! at runtime: when the infrastructure is available it runs and asserts (so it
//! actually catches regressions); when it is missing it prints a `SKIP: ...`
//! line and passes — so a bare `cargo test` stays green without hiding the test.
//!
//! Knobs:
//! - `DAPPLE_BACKEND_TESTS_STRICT=1` — turn a missing prerequisite into a hard
//!   failure instead of a skip (for CI lanes that are supposed to have the
//!   infrastructure). Does NOT force the billable Anthropic tests below.
//! - `DAPPLE_ANTHROPIC_LIVE_TESTS=1` — opt in to the live, *billable* Anthropic
//!   tests. Without it they always skip, even if `ANTHROPIC_API_KEY` is set
//!   (your shell may carry one for unrelated reasons).
//! - T5 tests need `SUMMARIZATION_ENABLED` + the T5 ONNX model
//!   (`nu assets/nu/k8s/convert_onnx_models.nu --model t5`).

use dapple_prompt_conduit::service::llm::{AnthropicBackend, LlmBackend, StreamChunk};
use dapple_prompt_conduit::service::model::ModelConfig;
use dapple_prompt_conduit::service::summarization::{
    DialogueTurn, SummarizationConfig, SummarizationInput, SummarizationService,
};
use dapple_prompt_conduit::service::template::{PromptMessage, RenderedPrompt};
use futures::StreamExt;

/// True if `name` is set to a truthy value (`1`/`true`/`yes`).
fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// Skip a test because a required prerequisite is missing — unless
/// `DAPPLE_BACKEND_TESTS_STRICT` is set, in which case fail loudly.
fn skip_or_strict(test: &str, reason: &str) {
    if env_flag("DAPPLE_BACKEND_TESTS_STRICT") {
        panic!(
            "{test}: {reason} (DAPPLE_BACKEND_TESTS_STRICT is set — failing instead of skipping)"
        );
    }
    eprintln!("SKIP {test}: {reason}");
}

/// Skip a test because of a deliberate opt-out (never escalated by strict mode).
fn note_skip(test: &str, reason: &str) {
    eprintln!("SKIP {test}: {reason}");
}

// ---------------------------------------------------------------------------
// T5 Summarization (requires ONNX model files)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn t5_dialogue_summarization() {
    let name = "t5_dialogue_summarization";
    let Some(config) = SummarizationConfig::from_env() else {
        skip_or_strict(
            name,
            "SUMMARIZATION_ENABLED not set / T5 summarization not configured",
        );
        return;
    };
    let service = match SummarizationService::with_t5(&config).await {
        Ok(s) => s,
        Err(e) => {
            skip_or_strict(
                name,
                &format!("T5 model failed to load ({e}); run: nu assets/nu/k8s/convert_onnx_models.nu --model t5"),
            );
            return;
        }
    };

    let turns = vec![
        DialogueTurn {
            speaker: "Alice".to_string(),
            content: "Hey Bob, are you coming to the meeting today?".to_string(),
        },
        DialogueTurn {
            speaker: "Bob".to_string(),
            content: "Yes, I'll be there at 3pm. Should I bring the reports?".to_string(),
        },
        DialogueTurn {
            speaker: "Alice".to_string(),
            content: "Yes please, and can you also send me the slides beforehand?".to_string(),
        },
        DialogueTurn {
            speaker: "Bob".to_string(),
            content: "Sure, I'll email them to you in an hour.".to_string(),
        },
    ];

    let summary = service
        .summarize_dialogue(turns)
        .await
        .expect("summarize_dialogue should succeed");

    assert!(!summary.is_empty(), "Summary should not be empty");
    assert!(
        summary.len() < 500,
        "Summary should be concise, got {} chars",
        summary.len()
    );
}

#[tokio::test]
async fn t5_text_summarization() {
    let name = "t5_text_summarization";
    let Some(config) = SummarizationConfig::from_env() else {
        skip_or_strict(
            name,
            "SUMMARIZATION_ENABLED not set / T5 summarization not configured",
        );
        return;
    };
    let service = match SummarizationService::with_t5(&config).await {
        Ok(s) => s,
        Err(e) => {
            skip_or_strict(
                name,
                &format!("T5 model failed to load ({e}); run: nu assets/nu/k8s/convert_onnx_models.nu --model t5"),
            );
            return;
        }
    };

    let text = "The quick brown fox jumps over the lazy dog. \
                This sentence contains every letter of the alphabet. \
                It has been used for many years as a typing exercise. \
                The sentence is also popular for testing fonts and keyboards.";

    let summary = service
        .summarize_text(text)
        .await
        .expect("summarize_text should succeed");

    assert!(!summary.is_empty(), "Summary should not be empty");
}

#[tokio::test]
async fn t5_batch_summarization() {
    let name = "t5_batch_summarization";
    let Some(config) = SummarizationConfig::from_env() else {
        skip_or_strict(
            name,
            "SUMMARIZATION_ENABLED not set / T5 summarization not configured",
        );
        return;
    };
    let service = match SummarizationService::with_t5(&config).await {
        Ok(s) => s,
        Err(e) => {
            skip_or_strict(
                name,
                &format!("T5 model failed to load ({e}); run: nu assets/nu/k8s/convert_onnx_models.nu --model t5"),
            );
            return;
        }
    };

    let inputs = vec![
        SummarizationInput::text("First document about technology."),
        SummarizationInput::text("Second document about science."),
        SummarizationInput::dialogue(vec![
            DialogueTurn {
                speaker: "A".to_string(),
                content: "Hello".to_string(),
            },
            DialogueTurn {
                speaker: "B".to_string(),
                content: "Hi there".to_string(),
            },
        ]),
    ];

    let summaries = service
        .summarize(&inputs)
        .await
        .expect("batch summarize should succeed");

    assert_eq!(summaries.len(), 3, "Should have 3 summaries");
    for (i, summary) in summaries.iter().enumerate() {
        assert!(!summary.is_empty(), "Summary {} should not be empty", i);
    }
}

// ---------------------------------------------------------------------------
// Anthropic (requires ANTHROPIC_API_KEY)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_streaming() {
    let name = "anthropic_streaming";
    if !env_flag("DAPPLE_ANTHROPIC_LIVE_TESTS") {
        note_skip(
            name,
            "DAPPLE_ANTHROPIC_LIVE_TESTS not set (live, billable Anthropic API test)",
        );
        return;
    }
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            skip_or_strict(
                name,
                "DAPPLE_ANTHROPIC_LIVE_TESTS set but ANTHROPIC_API_KEY missing",
            );
            return;
        }
    };

    let backend = AnthropicBackend::new(api_key);

    let prompt = RenderedPrompt {
        prompt_name: "test-prompt".to_string(),
        messages: vec![PromptMessage {
            role: Some("user".to_string()),
            content: "Say 'hello' and nothing else.".to_string(),
        }],
    };

    let config = ModelConfig {
        lineage_key: "claude-3-5".to_string(),
        provider_key: "anthropic".to_string(),
        tier: "haiku".to_string(),
        release: None,
        variant_id: None,
        priority: 0,
        temperature: None,
        top_p: None,
        top_k: None,
        max_tokens: None,
    };

    let mut stream = backend
        .execute_streaming(&prompt, &config)
        .await
        .expect("execute_streaming should succeed");

    let mut response = String::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(StreamChunk::Token(token)) => response.push_str(&token),
            Ok(StreamChunk::Final(_)) => {}
            Err(e) => panic!("Stream error: {:?}", e),
        }
    }

    assert!(
        response.to_lowercase().contains("hello"),
        "Response should contain 'hello', got: {}",
        response
    );
}

#[tokio::test]
async fn anthropic_model_mapping() {
    let name = "anthropic_model_mapping";
    if !env_flag("DAPPLE_ANTHROPIC_LIVE_TESTS") {
        note_skip(
            name,
            "DAPPLE_ANTHROPIC_LIVE_TESTS not set (live, billable Anthropic API test)",
        );
        return;
    }
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            skip_or_strict(
                name,
                "DAPPLE_ANTHROPIC_LIVE_TESTS set but ANTHROPIC_API_KEY missing",
            );
            return;
        }
    };

    let backend = AnthropicBackend::new(api_key);

    let prompt = RenderedPrompt {
        prompt_name: "test-prompt".to_string(),
        messages: vec![PromptMessage {
            role: Some("user".to_string()),
            content: "What is 2+2? Reply with just the number.".to_string(),
        }],
    };

    let config = ModelConfig {
        lineage_key: "claude-3-5".to_string(),
        provider_key: "anthropic".to_string(),
        tier: "sonnet".to_string(),
        release: None,
        variant_id: None,
        priority: 0,
        temperature: None,
        top_p: None,
        top_k: None,
        max_tokens: None,
    };

    let mut stream = backend
        .execute_streaming(&prompt, &config)
        .await
        .expect("execute_streaming should succeed for claude-3.5-sonnet");

    let mut response = String::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(StreamChunk::Token(token)) => response.push_str(&token),
            Ok(StreamChunk::Final(_)) => {}
            Err(e) => panic!("Stream error: {:?}", e),
        }
    }

    assert!(
        response.contains("4"),
        "Response should contain '4', got: {}",
        response
    );
}
