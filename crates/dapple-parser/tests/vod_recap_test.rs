//! Generic parser check for a VOD-recap-shaped guide.
//!
//! Deliberately synthetic and in-repo: it does NOT depend on any real guide in
//! `dapple-guides`. It confirms a VOD guide parses/plans into one stage / step /
//! task that chains the media-gateway commands (`vod analyze` -> `prompt send`
//! -> `vod assemble`) and that a computed `:::brief-output` carrying the `video`
//! display hint survives the transform. The real gaming-VOD guide
//! (`learn-from-gaming-vod`, director, prompts, computed video) lives in
//! dapple-guides and is exercised at ingest/runtime, not here.

use std::path::Path;

#[test]
fn vod_recap_parses_into_one_step_with_video_output() {
    let guide_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vod-recap");

    let parsed = dapple_parser::parse_guide(&guide_dir).expect("parse vod-recap");
    assert_eq!(parsed.metadata.title, "VOD Recap");

    let plan = dapple_parser::transform::plan_guide(&parsed, &guide_dir).expect("plan vod-recap");

    // One playbook: one stage, one step, wired to the recap task.
    assert_eq!(plan.derived_playbooks.len(), 1);
    assert_eq!(plan.derived_playbooks[0].stages.len(), 1);
    let stage = &plan.derived_playbooks[0].stages[0];
    assert_eq!(stage.name, "cut");
    assert_eq!(stage.steps.len(), 1);
    assert_eq!(stage.steps[0].name, "assemble");
    assert_eq!(stage.steps[0].task_refs, vec!["recap-cut".to_string()]);

    // The recap task chains the media-gateway commands.
    assert_eq!(plan.derived_tasks.len(), 1);
    let task = &plan.derived_tasks[0];
    assert_eq!(task.name, "recap-cut");
    for cmd in ["vod analyze", "prompt send", "vod assemble"] {
        assert!(task.script.contains(cmd), "recap task should call `{cmd}`");
    }

    // The computed reel output carries the `video` display hint through the plan.
    let recap_output = plan
        .derived_brief_outputs
        .iter()
        .find(|o| o.slot.as_deref() == Some("recap"))
        .expect("recap brief-output present");
    assert_eq!(recap_output.display, "video");
    assert_eq!(recap_output.source, "computed");
}
