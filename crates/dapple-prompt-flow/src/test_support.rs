use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

/// Build a test playbook JSON-like structure using human-friendly names only.
/// Do NOT generate UUIDs/object_id here — dapple-data will assign those on insert.
pub fn build_named_playbook(prefix: &str) -> Value {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let make_name = |kind: &str, idx: u8| format!("{}-{}-{}-{}", prefix, kind, idx, ts);

    json!({
        "name": format!("{}-dispatch-{}", prefix, ts),
        "description": "Test-generated playbook",
        "priority": 0,
        "stages": [
            {
                "name": make_name("stage", 1),
                "description": "Stage 1",
                "steps": [
                    {
                        "name": make_name("step", 1),
                        "description": "Step 1",
                        "tasks": [
                            { "name": make_name("task", 1), "description": "Task 1" },
                            { "name": make_name("task", 2), "description": "Task 2" }
                        ]
                    },
                    {
                        "name": make_name("step", 2),
                        "description": "Step 2",
                        "tasks": [
                            { "name": make_name("task", 3), "description": "Task 3" },
                            { "name": make_name("task", 4), "description": "Task 4" }
                        ]
                    }
                ]
            }
        ]
    })
}
