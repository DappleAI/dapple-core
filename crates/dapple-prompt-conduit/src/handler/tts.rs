//! Text-to-speech generation handler.
//!
//! `handle_tts_prompt` (SSE) is the audio analog of `handle_image_prompt` /
//! `handle_video_prompt`: routed from `handle_prompt` when a prompt's
//! `maker_keys` (lineage keys) contain `TTS_MAKER_KEY`. It resolves config from
//! the prompt's `engine_body`, synthesizes via the TTS service (OpenAI through
//! rath), persists the audio as an `audio` asset, and emits
//! `PromptEvent::asset`. Generation is the only model call; there is no edit
//! path.
//!
//! This replaces the removed VITS/ONNX TTS path: TTS now routes at phase 0 by its
//! maker/lineage key (like image/video) rather than at phase 2 by a resolved
//! `model.lineage_key`, so it never needs generic model resolution.

use axum::response::sse::{Event, Sse};
use dapple_data::query::prompts::select_prompt_engine_body_for_maker;
use futures::stream::BoxStream;
use serde::Deserialize;
use serde_json::Value;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::error::ConduitError;
use crate::event::PromptEvent;
use crate::handler::image::persist_media_asset;
use crate::handler::prompt::PromptRequest;
use crate::service::tts::{TtsGenRequest, TtsGenResult};
use crate::service::AppState;

/// Type alias for the SSE stream used by prompt handlers (matches `prompt.rs`).
type PromptStream = BoxStream<'static, Result<Event, Infallible>>;

/// Lineage/maker key that routes a prompt to the TTS generator. A guide's
/// `:::inference-profile` declares `.lineage = "tts"`; the model id (an OpenAI
/// slug, e.g. `gpt-4o-mini-tts`), voice, format, and provider live in the
/// engine_body JSON. (Maker key == lineage key — see `select_prompt_maker_keys`.)
pub const TTS_MAKER_KEY: &str = "tts";

/// Config parsed from a TTS prompt's `engine_body` (JSON).
#[derive(Debug, Default, Deserialize)]
struct TtsPromptConfig {
    /// Which hosted engine to route to (`openai`). Selects the backend like the
    /// LLM/image/video `provider`.
    provider: Option<String>,
    /// Provider model id (e.g. `gpt-4o-mini-tts`, `tts-1`); falls back to the TTS
    /// service default.
    model: Option<String>,
    /// Voice name; falls back to the service default.
    voice: Option<String>,
    /// Output audio format (`mp3`/`opus`/`aac`/`flac`/`wav`/`pcm`); falls back to
    /// the service default.
    format: Option<String>,
    /// Optional speech-rate multiplier.
    speed: Option<f32>,
    /// Raw provider `provider_config` passthrough (e.g. gpt-4o-mini-tts
    /// `instructions`). Forwarded verbatim to the rath backend.
    provider_config: Option<Value>,
}

/// Pull the text to speak from the request (preferred) or the attempt context.
/// Accepts `text` then `input` (the `synth` command pipes text in as
/// `context_data.text`).
fn extract_tts_input(req_context: &Option<Value>, attempt_context: &Value) -> Option<String> {
    let pick = |v: &Value| {
        v.get("text")
            .or_else(|| v.get("input"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
    };
    if let Some(rc) = req_context {
        if let Some(s) = pick(rc) {
            if !s.trim().is_empty() {
                return Some(s);
            }
        }
    }
    pick(attempt_context).filter(|s| !s.trim().is_empty())
}

/// Pull the per-line mood from the request context (the `synth` command pipes it
/// as `context_data.mood`). Accepts `mood` then `instructions`; trimmed,
/// non-empty. Request-only — unlike the text, there is no attempt-context
/// fallback for a transient per-call mood.
fn extract_mood(req_context: &Option<Value>) -> Option<String> {
    let rc = req_context.as_ref()?;
    let s = rc
        .get("mood")
        .or_else(|| rc.get("instructions"))
        .and_then(|x| x.as_str())?;
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Fold a per-line `mood` onto the voice's base `provider_config.instructions`,
/// so each synth call layers transient emotion on top of the engine-body timbre
/// (`base + ". " + mood`). The base timbre stays single-sourced in the engine
/// body; the caller passes only the short mood. No-ops on an empty mood; creates
/// the object/`instructions` key when absent; leaves an unexpected non-string
/// `instructions` untouched.
fn fold_mood_instructions(base: Option<Value>, mood: Option<String>) -> Option<Value> {
    let Some(mood) = mood else {
        return base;
    };
    let mut map = match base {
        Some(Value::Object(m)) => m,
        None => serde_json::Map::new(),
        Some(other) => {
            // `provider_config` is authored as an object; anything else is
            // unexpected. Start fresh rather than corrupt it.
            log::warn!(
                "tts provider_config is not an object ({other}); starting fresh for mood merge"
            );
            serde_json::Map::new()
        }
    };
    // Decide the merged value without holding a borrow into `map`.
    enum Merge {
        Append(String),
        Set,
        Skip,
    }
    let action = match map.get("instructions") {
        Some(Value::String(base_instr)) => Merge::Append(base_instr.clone()),
        None => Merge::Set,
        Some(_) => Merge::Skip,
    };
    match action {
        Merge::Append(base_instr) => {
            // The base timbre usually ends a sentence already; don't double the
            // period. `"Warm and dry." + "outraged"` -> `"Warm and dry. outraged"`.
            let trimmed = base_instr.trim_end();
            let sep = if trimmed.ends_with(['.', '!', '?']) {
                " "
            } else {
                ". "
            };
            map.insert(
                "instructions".to_string(),
                Value::String(format!("{trimmed}{sep}{mood}")),
            );
        }
        Merge::Set => {
            map.insert("instructions".to_string(), Value::String(mood));
        }
        Merge::Skip => {
            log::warn!("tts provider_config.instructions is not a string; skipping mood merge");
        }
    }
    Some(Value::Object(map))
}

/// True when a synthesized clip is a "runaway": far more audio than the text
/// warrants. `gpt-4o-mini-tts` occasionally speaks the line, then degenerates into
/// a sustained drone, yielding a clip many times longer than the input (observed:
/// a one-sentence turn returned as ~16s of speech + ~10s of drone). We flag it by
/// characters-per-second on a clip long enough to matter: natural speech is
/// ~12–18 cps, a drone drags it far below. `duration_seconds == 0.0` (unknown,
/// e.g. a non-wav format we can't measure) is never flagged, so the guard is a
/// no-op when duration is unavailable.
fn is_runaway_tts(char_count: usize, duration_seconds: f32) -> bool {
    const MIN_SECS: f32 = 8.0;
    const MIN_CPS: f32 = 6.0;
    duration_seconds > MIN_SECS && (char_count as f32 / duration_seconds.max(0.01)) < MIN_CPS
}

/// Synthesize speech from a prompt and persist it. SSE so it shares the
/// prompt-stream shape (Started → Asset → Done / Error).
pub async fn handle_tts_prompt(
    state: AppState,
    req: PromptRequest,
    request_id: Uuid,
    start: Instant,
) -> Result<Sse<PromptStream>, ConduitError> {
    log::info!(
        "Handling TTS prompt: request_id={}, prompt_id={}",
        request_id,
        req.prompt_id
    );

    let pool = Arc::clone(&state.pool);

    let stream = async_stream::stream! {
        let started = PromptEvent::started(request_id, "tts", req.prompt_id);
        yield Ok(Event::default().data(started.to_sse_data()));

        // 1. TTS service availability (off unless OPENAI_API_KEY configured)
        let tts_service = match &state.tts_service {
            Some(svc) => svc.clone(),
            None => {
                let error = PromptEvent::error("tts_unavailable", "TTS service is not configured", false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 2. Fetch attempt context (text fallback)
        let attempt_context = match state.context_service
            .fetch_context_for_attempt(req.attempt_id)
            .await
        {
            Ok(ctx) => ctx,
            Err(e) => {
                let error = PromptEvent::error("context_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 3. Resolve config from the prompt's engine_body
        let engine_body = select_prompt_engine_body_for_maker(&pool, req.prompt_id, TTS_MAKER_KEY)
            .await
            .ok()
            .flatten();
        let cfg: TtsPromptConfig = engine_body
            .as_deref()
            .and_then(|b| serde_json::from_str(b).ok())
            .unwrap_or_default();

        // 4. Extract the text to speak
        let input_text = match extract_tts_input(&req.context_data, &attempt_context.context_data) {
            Some(t) => t,
            None => {
                let error = PromptEvent::error(
                    "input_error",
                    "No text/input field in context_data for TTS",
                    false,
                );
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // Fold any per-line mood (from context_data) onto the voice's base
        // instructions, so each synth call layers transient emotion on top of the
        // engine-body timbre. Computed once; every retry reuses it.
        let provider_config = fold_mood_instructions(
            cfg.provider_config.clone(),
            extract_mood(&req.context_data),
        );

        // 5. Synthesize, guarding against runaway/degenerate returns. Retry a
        //    bounded number of times; accept the first clean take, else keep the
        //    shortest runaway as a fallback so the conversation still renders.
        const MAX_TTS_ATTEMPTS: usize = 3;
        let char_count = input_text.chars().count();
        let mut best: Option<TtsGenResult> = None;
        let mut last_err: Option<String> = None;
        for attempt in 1..=MAX_TTS_ATTEMPTS {
            let gen_request = TtsGenRequest {
                provider: cfg.provider.clone(),
                input: input_text.clone(),
                model: cfg.model.clone().unwrap_or_default(),
                voice: cfg.voice.clone(),
                format: cfg.format.clone(),
                speed: cfg.speed,
                provider_config: provider_config.clone(),
            };
            match tts_service.synthesize(gen_request).await {
                Ok(r) => {
                    if !is_runaway_tts(char_count, r.duration_seconds) {
                        best = Some(r);
                        break;
                    }
                    log::warn!(
                        "TTS runaway on attempt {}/{}: {} chars in {:.1}s ({:.1} cps); retrying",
                        attempt,
                        MAX_TTS_ATTEMPTS,
                        char_count,
                        r.duration_seconds,
                        char_count as f32 / r.duration_seconds.max(0.01),
                    );
                    // Keep the shortest runaway seen as the least-bad fallback.
                    best = Some(match best.take() {
                        Some(b) if b.duration_seconds <= r.duration_seconds => b,
                        _ => r,
                    });
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        let result = match best {
            Some(r) => r,
            None => {
                // Every attempt errored. TTS-provider failures are retryable
                // (rate limits, transient 5xx).
                let msg = last_err.unwrap_or_else(|| "tts synthesis failed".to_string());
                let error = PromptEvent::error("tts_error", msg, true);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        if result.bytes.is_empty() {
            let error = PromptEvent::error("tts_error", "tts provider returned 0 bytes (empty audio)", true);
            yield Ok(Event::default().data(error.to_sse_data()));
            return;
        }

        // 6. Persist as an audio asset
        let (asset_id, content_length) = match persist_media_asset(&pool, &result.bytes, "audio", &result.mime_type).await {
            Ok(v) => v,
            Err(e) => {
                let error = PromptEvent::error("persist_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        log::info!(
            "TTS asset created: asset_id={}, content_length={}, mime={}",
            asset_id,
            content_length,
            result.mime_type
        );

        // 7. Asset + Done events
        let asset_event = PromptEvent::asset(
            asset_id,
            "audio",
            result.mime_type.clone(),
            content_length,
            result.duration_seconds,
            result.sample_rate,
            input_text,
        );
        yield Ok(Event::default().data(asset_event.to_sse_data()));

        let latency_ms = start.elapsed().as_millis() as u64;
        let done = PromptEvent::done(0, 0, latency_ms, "asset_created");
        yield Ok(Event::default().data(done.to_sse_data()));
    };

    Ok(Sse::new(Box::pin(stream)))
}

#[cfg(test)]
mod tests {
    use super::{extract_mood, fold_mood_instructions, is_runaway_tts};
    use serde_json::json;

    #[test]
    fn extract_mood_reads_mood_then_instructions() {
        assert_eq!(
            extract_mood(&Some(json!({ "mood": "  loud  " }))),
            Some("loud".to_string())
        );
        assert_eq!(
            extract_mood(&Some(json!({ "instructions": "soft" }))),
            Some("soft".to_string())
        );
        assert_eq!(extract_mood(&Some(json!({ "mood": "   " }))), None);
        assert_eq!(extract_mood(&Some(json!({ "text": "hi" }))), None);
        assert_eq!(extract_mood(&None), None);
    }

    #[test]
    fn fold_mood_appends_to_string_instructions() {
        let base = Some(json!({ "instructions": "Warm and dry.", "voice": "onyx" }));
        let out = fold_mood_instructions(base, Some("outraged".to_string())).unwrap();
        assert_eq!(out["instructions"], json!("Warm and dry. outraged"));
        // Other provider_config keys are preserved.
        assert_eq!(out["voice"], json!("onyx"));
    }

    #[test]
    fn fold_mood_sets_when_instructions_absent() {
        let out = fold_mood_instructions(Some(json!({ "voice": "nova" })), Some("amused".into()))
            .unwrap();
        assert_eq!(out["instructions"], json!("amused"));
        assert_eq!(out["voice"], json!("nova"));
    }

    #[test]
    fn fold_mood_creates_object_when_no_provider_config() {
        let out = fold_mood_instructions(None, Some("weary".into())).unwrap();
        assert_eq!(out["instructions"], json!("weary"));
    }

    #[test]
    fn fold_mood_noop_on_empty_mood() {
        let base = Some(json!({ "instructions": "Base." }));
        assert_eq!(fold_mood_instructions(base.clone(), None), base);
    }

    #[test]
    fn fold_mood_skips_non_string_instructions() {
        // Defensive: never corrupt an unexpected instructions shape.
        let base = Some(json!({ "instructions": { "nested": true } }));
        let out = fold_mood_instructions(base.clone(), Some("smug".into())).unwrap();
        assert_eq!(out["instructions"], json!({ "nested": true }));
    }

    #[test]
    fn flags_degenerate_over_long_returns() {
        // The observed failure: a one-sentence turn returned as ~31s (3.2 cps).
        assert!(is_runaway_tts(100, 31.0));
        assert!(is_runaway_tts(60, 20.0)); // 3 cps
    }

    #[test]
    fn passes_normal_and_unmeasured_clips() {
        assert!(!is_runaway_tts(100, 6.5)); // ~15 cps, normal pace
        assert!(!is_runaway_tts(300, 20.0)); // 15 cps
        assert!(!is_runaway_tts(500, 30.0)); // 16.7 cps, legit long narration
        assert!(!is_runaway_tts(20, 7.0)); // short clip: under the min-duration gate
        assert!(!is_runaway_tts(0, 0.0)); // unknown duration (non-wav) never flags
    }
}
