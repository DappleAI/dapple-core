//! `vod detect` — score a transcript into a ranked candidate table.

use nu_engine::command_prelude::*;

use crate::commands::media_gateway::{missing_field, run_job_raw, to_pipeline};
use crate::engine::nu_value_to_json;

#[derive(Clone)]
pub struct VodDetectCommand;

impl Command for VodDetectCommand {
    fn name(&self) -> &str {
        "vod detect"
    }

    fn signature(&self) -> Signature {
        Signature::build("vod detect")
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
                "extra-beats",
                SyntaxShape::Any,
                "Caller-mined beats to merge before ranking/scene grouping: a list of records { start, end, label?, kind?, score?, snippet? }",
                None,
            )
            .switch(
                "full",
                "Return the full detect record { candidates, scenes, ... } instead of just the candidate list",
                None,
            )
            .switch("async", "Return a job_id immediately", None)
            .input_output_types(vec![
                (Type::record(), Type::List(Box::new(Type::Any))),
                (Type::record(), Type::record()),
            ])
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Detect labeled highlight candidates from a transcript (returns a filterable table)."
    }

    fn extra_description(&self) -> &str {
        r#"Pipe in a record carrying `transcript_asset_id` (and optionally `chapters`),
e.g. a `twitch pull` result merged with a `vod transcribe` result. In ONE pass it
labels four kinds — `educational` (teaching), `informative` (context), `exciting`
(hype) and `fails` (blunders) — returning a table of
{ kind, start, end, score, snippet, cues, game, neighbors } ranked by score.
Filter it with `where` (e.g. `where kind == "exciting"`) and pipe the approved rows
into `vod cut`. To weave a teaching moment with the hype/fail around it, approve the
row AND its `neighbors` entry — the cut stage merges adjacent approved ranges
(bump `--merge-gap` on `vod cut` for a wider weave).

`--extra-beats` merges caller-mined timestamped spans (things the lexicons can't
hear, e.g. commentary judged relevant against an operator primer) into the
candidates before ranking and scene grouping; each record is
{ start, end, label?, kind?, score?, snippet? }. `--full` returns the whole detect
record — { candidates, scenes, candidate_count, scene_count, ... } — instead of
just the candidate list (use it when you need the regrouped scenes back).
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
        let is_async = call.has_flag(engine_state, stack, "async")?;
        let min_score = call.get_flag::<f64>(engine_state, stack, "min-score")?;
        let merge_gap = call.get_flag::<f64>(engine_state, stack, "merge-gap")?;
        let game = call.get_flag::<String>(engine_state, stack, "game")?;
        let kind = call.get_flag::<String>(engine_state, stack, "kind")?;
        let energy_weight = call.get_flag::<f64>(engine_state, stack, "energy-weight")?;
        let neighbor_window = call.get_flag::<f64>(engine_state, stack, "neighbor-window")?;
        let preroll = call.get_flag::<f64>(engine_state, stack, "preroll")?;
        let postroll = call.get_flag::<f64>(engine_state, stack, "postroll")?;
        let scene_merge_gap = call.get_flag::<f64>(engine_state, stack, "scene-merge-gap")?;
        let setup_min = call.get_flag::<f64>(engine_state, stack, "setup-min")?;
        let extra_beats = call.get_flag::<Value>(engine_state, stack, "extra-beats")?;
        let full = call.has_flag(engine_state, stack, "full")?;

        let value = input.into_value(span)?;
        let record = value
            .as_record()
            .map_err(|_| missing_field("transcript_asset_id", span))?;

        let transcript_asset_id = record
            .get("transcript_asset_id")
            .and_then(|v| v.as_str().ok())
            .ok_or_else(|| missing_field("transcript_asset_id", span))?;
        let chapters = record
            .get("chapters")
            .map_or_else(|| serde_json::json!([]), nu_value_to_json);

        let mut body = serde_json::json!({
            "transcript_asset_id": transcript_asset_id,
            "chapters": chapters,
        });
        if let Some(ms) = min_score {
            body["min_score"] = serde_json::json!(ms);
        }
        if let Some(mg) = merge_gap {
            body["merge_gap_secs"] = serde_json::json!(mg);
        }
        if let Some(g) = game {
            body["game"] = serde_json::json!(g);
        }
        if let Some(k) = kind {
            let kinds: Vec<String> = k
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !kinds.is_empty() {
                body["kinds"] = serde_json::json!(kinds);
            }
        }
        if let Some(ew) = energy_weight {
            body["energy_weight"] = serde_json::json!(ew);
        }
        if let Some(nw) = neighbor_window {
            body["neighbor_window_secs"] = serde_json::json!(nw);
        }
        if let Some(pr) = preroll {
            body["preroll_max_secs"] = serde_json::json!(pr);
        }
        if let Some(po) = postroll {
            body["postroll_max_secs"] = serde_json::json!(po);
        }
        if let Some(smg) = scene_merge_gap {
            body["scene_merge_gap_secs"] = serde_json::json!(smg);
        }
        if let Some(sm) = setup_min {
            body["setup_min_secs"] = serde_json::json!(sm);
        }
        if let Some(xb) = extra_beats {
            let beats = nu_value_to_json(&xb);
            if beats.as_array().is_some_and(|a| !a.is_empty()) {
                body["extra_beats"] = beats;
            }
        }

        let result = run_job_raw("/vod/detect", body, is_async, span)?;
        if is_async || full {
            return Ok(to_pipeline(&result));
        }
        let candidates = result
            .get("candidates")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        Ok(to_pipeline(&candidates))
    }
}
