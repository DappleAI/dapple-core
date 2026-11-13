//! Minimal parser check for the `crosstalk` fixture.
//!
//! The fixture is intentionally tiny: one stage, one step, and one nushell task
//! that chains the conduit media commands. This test confirms the guide parses
//! and plans into that single stage/step/task and that the orchestrator's
//! `prompt send` -> `synth` -> `av mix` chain survives parsing. The real Crosstalk
//! guide (director, voices, inputs, computed audio) lives in dapple-guides and is
//! exercised at ingest/runtime, not here.

use std::path::Path;

#[test]
fn crosstalk_parses_into_one_orchestrator_step() {
    let guide_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crosstalk");

    let parsed = dapple_parser::parse_guide(&guide_dir).expect("parse crosstalk");
    assert_eq!(parsed.metadata.title, "Crosstalk");

    let plan = dapple_parser::transform::plan_guide(&parsed, &guide_dir).expect("plan crosstalk");

    // One playbook: one stage, one step, wired to the orchestrator task.
    assert_eq!(plan.derived_playbooks.len(), 1);
    let stage = &plan.derived_playbooks[0].stages[0];
    assert_eq!(plan.derived_playbooks[0].stages.len(), 1);
    assert_eq!(stage.name, "converse");
    assert_eq!(stage.steps.len(), 1);
    assert_eq!(stage.steps[0].name, "run");
    assert_eq!(stage.steps[0].task_refs, vec!["orchestrator".to_string()]);

    // The orchestrator chains the three conduit commands.
    assert_eq!(plan.derived_tasks.len(), 1);
    let task = &plan.derived_tasks[0];
    assert_eq!(task.name, "orchestrator");
    for cmd in ["prompt send", "synth", "av mix"] {
        assert!(
            task.script.contains(cmd),
            "orchestrator should call `{cmd}`"
        );
    }
}
