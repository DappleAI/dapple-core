//! `vod analyze` — the phase-1 wrapper: pull → transcribe → detect in one call.

use nu_engine::command_prelude::*;

use crate::commands::media_gateway::{json_get_str, piped_string, run_job_raw, to_pipeline};

#[derive(Clone)]
pub struct VodAnalyzeCommand;

impl Command for VodAnalyzeCommand {
    fn name(&self) -> &str {
        "vod analyze"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod analyze")
            .optional(
                "vod",
                SyntaxShape::String,
                "VOD URL or id (else read from the pipe)",
            )
            .named(
                "min-score",
                SyntaxShape::Number,
                "Drop candidates below this normalized 0..1 score",
                None,
            )
            .named(
                "merge-gap",
                SyntaxShape::Number,
                "Bridge scoring segments up to this gap (seconds)",
                None,
            )
            .named(
                "game",
                SyntaxShape::String,
                "Only keep candidates whose chapter/game matches",
                None,
            )
            .named(
                "kind",
                SyntaxShape::String,
                "Restrict to kinds (comma-separated: educational,informative,exciting,fails,funny,frustration)",
                None,
            )
            .named(
                "energy-weight",
                SyntaxShape::Number,
                "Weight of the audio-energy spike for reaction kinds (default 2; 0 disables)",
                None,
            )
            .named(
                "neighbor-window",
                SyntaxShape::Number,
                "Surface other-kind candidates within this gap (seconds) as neighbors (default 30)",
                None,
            )
            .named(
                "scene-merge-gap",
                SyntaxShape::Number,
                "Group beats within this gap (seconds) into one scene (default 25)",
                None,
            )
            .named(
                "setup-min",
                SyntaxShape::Number,
                "Minimum sustained duration (seconds) for a window to frame as a Setup scene (default 25)",
                None,
            )
            .named(
                "preroll",
                SyntaxShape::Number,
                "Action lead-in: reach the clip start back to the play, in seconds (default 12; 0 disables)",
                None,
            )
            .named(
                "postroll",
                SyntaxShape::Number,
                "Action tail-out: hold the clip end through the play's resolution, in seconds (default 12; 0 disables)",
                None,
            )
            .named(
                "initial-prompt",
                SyntaxShape::String,
                "Whisper vocabulary bias (streamer handle, game, recurring terms) so accented/jargon speech decodes correctly",
                None,
            )
            .input_output_types(vec![(Type::Any, Type::record())])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Pull → transcribe → detect a VOD to a reviewable candidate record in one call."
    }

    fn extra_description(&self) -> &str {
        r#"Runs the whole phase-1 pipeline and returns a record:
    { vod_id, url, audio_asset_id, transcript_asset_id, chapters,
      candidate_count, candidates, scene_count, scenes }
`.scenes` groups the beats into frame-labeled moments (the curation unit);
`.candidates` are the raw beats. Review `.scenes`, then cut the approved rows:
    let a = ("https://twitch.tv/videos/123" | vod analyze)
    $a.candidates | where score >= 0.6 | vod cut --source $a.url --transcript $a.transcript_asset_id
"#
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let min_score = call.get_flag::<f64>(engine_state, stack, "min-score")?;
        let merge_gap = call.get_flag::<f64>(engine_state, stack, "merge-gap")?;
        let game = call.get_flag::<String>(engine_state, stack, "game")?;
        let kind = call.get_flag::<String>(engine_state, stack, "kind")?;
        let energy_weight = call.get_flag::<f64>(engine_state, stack, "energy-weight")?;
        let neighbor_window = call.get_flag::<f64>(engine_state, stack, "neighbor-window")?;
        let scene_merge_gap = call.get_flag::<f64>(engine_state, stack, "scene-merge-gap")?;
        let setup_min = call.get_flag::<f64>(engine_state, stack, "setup-min")?;
        let preroll = call.get_flag::<f64>(engine_state, stack, "preroll")?;
        let postroll = call.get_flag::<f64>(engine_state, stack, "postroll")?;
        let initial_prompt = call.get_flag::<String>(engine_state, stack, "initial-prompt")?;
        let vod = match call.opt::<String>(engine_state, stack, 0)? {
            Some(v) => v,
            None => piped_string(input, "a VOD url or id", span)?,
        };

        // 1. Pull audio + metadata + chapters.
        let pull = run_job_raw(
            "/twitch/pull",
            serde_json::json!({ "vod": vod.clone() }),
            false,
            span,
        )?;
        let audio_asset_id = json_get_str(&pull, "audio_asset_id", span)?;
        let url = pull
            .get("url")
            .and_then(|v| v.as_str())
            .map_or_else(|| vod.clone(), str::to_string);
        let chapters = pull
            .get("chapters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));

        // 2. Transcribe (optionally vocabulary-biased for accented / jargon-heavy speech).
        let mut transcribe_body =
            serde_json::json!({ "audio_asset_id": audio_asset_id, "vod": url });
        if let Some(p) = &initial_prompt {
            transcribe_body["initial_prompt"] = serde_json::json!(p);
        }
        let transcribe = run_job_raw("/vod/transcribe", transcribe_body, false, span)?;
        let transcript_asset_id = json_get_str(&transcribe, "transcript_asset_id", span)?;

        // 3. Detect.
        let mut detect_body = serde_json::json!({
            "transcript_asset_id": transcript_asset_id,
            "chapters": chapters,
        });
        if let Some(ms) = min_score {
            detect_body["min_score"] = serde_json::json!(ms);
        }
        if let Some(mg) = merge_gap {
            detect_body["merge_gap_secs"] = serde_json::json!(mg);
        }
        if let Some(g) = game {
            detect_body["game"] = serde_json::json!(g);
        }
        if let Some(k) = kind {
            let kinds: Vec<String> = k
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !kinds.is_empty() {
                detect_body["kinds"] = serde_json::json!(kinds);
            }
        }
        if let Some(ew) = energy_weight {
            detect_body["energy_weight"] = serde_json::json!(ew);
        }
        if let Some(nw) = neighbor_window {
            detect_body["neighbor_window_secs"] = serde_json::json!(nw);
        }
        if let Some(pr) = preroll {
            detect_body["preroll_max_secs"] = serde_json::json!(pr);
        }
        if let Some(po) = postroll {
            detect_body["postroll_max_secs"] = serde_json::json!(po);
        }
        if let Some(smg) = scene_merge_gap {
            detect_body["scene_merge_gap_secs"] = serde_json::json!(smg);
        }
        if let Some(sm) = setup_min {
            detect_body["setup_min_secs"] = serde_json::json!(sm);
        }
        let detect = run_job_raw("/vod/detect", detect_body, false, span)?;
        let candidates = detect
            .get("candidates")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let candidate_count = candidates.as_array().map_or(0, Vec::len);
        // The detector also groups the beats into frame-labeled scenes (the operator's
        // curation unit); thread them through so the guide binds dl_scenes.
        let scenes = detect
            .get("scenes")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let scene_count = scenes.as_array().map_or(0, Vec::len);

        // Thread the stream's own meta content (title, description, streamer) from
        // the pull through the analyze result so the guide can bind it as context.
        // The game rides on `chapters`. `description` is often empty on Twitch.
        let out = serde_json::json!({
            "vod_id": pull.get("vod_id").cloned().unwrap_or(serde_json::Value::Null),
            "url": url,
            "audio_asset_id": audio_asset_id,
            "transcript_asset_id": transcript_asset_id,
            "title": pull.get("title").cloned().unwrap_or(serde_json::Value::Null),
            "description": pull.get("description").cloned().unwrap_or(serde_json::Value::Null),
            "user_name": pull.get("user_name").cloned().unwrap_or(serde_json::Value::Null),
            "user_login": pull.get("user_login").cloned().unwrap_or(serde_json::Value::Null),
            "chapters": chapters,
            "candidate_count": candidate_count,
            "candidates": candidates,
            "scene_count": scene_count,
            "scenes": scenes,
        });
        Ok(to_pipeline(&out))
    }
}
