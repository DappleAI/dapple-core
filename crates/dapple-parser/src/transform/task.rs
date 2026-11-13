//! Nushell task extraction from :::nushell blocks.
//!
//! Each :::nushell block produces a `TaskDef`. The block's `role` attribute
//! determines whether this is a standalone task (`role: "default"` or absent)
//! or a flow-control directive attached to a step/stage (`role: "directive"`).

use super::{BlockIndex, PlaybookDef, TaskDef};
use crate::types::ParsedBlock;

/// Extract a task definition from a :::nushell block.
///
/// Script content is resolved in this order:
/// 1. If `scripts/<name>.nu` exists in the guide, use file content
/// 2. Otherwise, use inline block content (backward compatibility)
///
/// Returns `None` if the block has no name.
pub fn extract_task(
    block: &ParsedBlock,
    source: &BlockIndex,
    script_files: &std::collections::HashMap<String, String>,
) -> Option<TaskDef> {
    let name = block.name.clone()?;

    let role = block
        .attributes
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let target = block
        .attributes
        .get("for")
        .and_then(|v| v.as_str())
        .map(String::from);

    // File-based script takes priority over inline content
    let script = if let Some(file_content) = script_files.get(&name) {
        file_content.clone()
    } else {
        block.content.clone()
    };

    Some(TaskDef {
        name,
        script,
        role,
        target,
        source_block_index: source.clone(),
    })
}

/// Deterministic name for a task nested directly inside a `:::step`.
///
/// A task authored as a `:::nushell` child of a step is **step-private**: its
/// name is scoped to the enclosing step so two different steps that each nest a
/// same-named `:::nushell` don't collide (and cross-wire) in the global
/// name→task dedup. Both the task extractor (which renames the produced
/// `TaskDef`) and `extract_step` (which builds the step's `task_refs`) call this,
/// so the definition and the reference always derive the identical string. A
/// nameless enclosing step (rare; warned) falls back to the bare name on both
/// sides so linkage still holds. Scoping uses the raw step name; slugification
/// happens uniformly downstream.
pub(crate) fn step_task_ref_name(step_name: Option<&str>, task_name: &str) -> String {
    match step_name {
        Some(s) => format!("{s}/{task_name}"),
        None => task_name.to_string(),
    }
}

/// Validate that directive-role nushell blocks have matching targets in the playbooks.
///
/// This produces warnings (not errors) since the targets might be resolved later
/// or the playbooks may not exist yet.
pub fn link_directive_tasks(
    tasks: &[TaskDef],
    playbooks: &[PlaybookDef],
    warnings: &mut Vec<String>,
) {
    for task in tasks {
        if task.role != "directive" {
            continue;
        }

        let target = match &task.target {
            Some(t) => t,
            None => {
                warnings.push(format!(
                    ":::nushell '{}' has role 'directive' but no 'for' attribute",
                    task.name
                ));
                continue;
            }
        };

        // Check if the target exists in any playbook
        if playbooks.is_empty() {
            warnings.push(format!(
                ":::nushell '{}' has role 'directive' but no playbook stages were found",
                task.name
            ));
            continue;
        }

        let found = playbooks.iter().any(|pb| {
            pb.stages
                .iter()
                .any(|s| s.name == *target || s.steps.iter().any(|step| step.name == *target))
        });

        if !found {
            warnings.push(format!(
                ":::nushell '{}' targets '{}' but no matching stage or step was found",
                task.name, target
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_nushell_block(name: &str, content: &str, attrs: serde_json::Value) -> ParsedBlock {
        ParsedBlock {
            directive_type: "nushell".to_string(),
            name: Some(name.to_string()),
            content: content.to_string(),
            attributes: attrs,
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        }
    }

    #[test]
    fn test_extract_default_task() {
        let block = make_nushell_block(
            "my-task",
            "echo hello",
            serde_json::json!({ "name": "my-task" }),
        );
        let source = BlockIndex {
            chapter_index: 0,
            block_index: 0,
        };

        let task = extract_task(&block, &source, &std::collections::HashMap::new()).unwrap();
        assert_eq!(task.name, "my-task");
        assert_eq!(task.script, "echo hello");
        assert_eq!(task.role, "default");
        assert!(task.target.is_none());
    }

    #[test]
    fn test_extract_directive_task() {
        let block = make_nushell_block(
            "research-flow",
            "if $condition { skip }",
            serde_json::json!({ "name": "research-flow", "role": "directive", "for": "Gather Info" }),
        );
        let source = BlockIndex {
            chapter_index: 0,
            block_index: 1,
        };

        let task = extract_task(&block, &source, &std::collections::HashMap::new()).unwrap();
        assert_eq!(task.name, "research-flow");
        assert_eq!(task.role, "directive");
        assert_eq!(task.target, Some("Gather Info".to_string()));
    }

    #[test]
    fn test_extract_task_no_name() {
        let block = ParsedBlock {
            directive_type: "nushell".to_string(),
            name: None,
            content: "echo unnamed".to_string(),
            attributes: serde_json::json!({}),
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        };
        let source = BlockIndex {
            chapter_index: 0,
            block_index: 0,
        };

        let result = extract_task(&block, &source, &std::collections::HashMap::new());
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_task_file_overrides_inline() {
        let block = make_nushell_block(
            "my-task",
            "inline content",
            serde_json::json!({ "name": "my-task" }),
        );
        let source = BlockIndex {
            chapter_index: 0,
            block_index: 0,
        };
        let mut script_files = std::collections::HashMap::new();
        script_files.insert("my-task".to_string(), "file content".to_string());

        let task = extract_task(&block, &source, &script_files).unwrap();
        assert_eq!(task.script, "file content");
    }

    #[test]
    fn test_extract_task_falls_back_to_inline() {
        let block = make_nushell_block(
            "my-task",
            "inline content",
            serde_json::json!({ "name": "my-task" }),
        );
        let source = BlockIndex {
            chapter_index: 0,
            block_index: 0,
        };

        let task = extract_task(&block, &source, &std::collections::HashMap::new()).unwrap();
        assert_eq!(task.script, "inline content");
    }

    #[test]
    fn test_link_directive_task_missing_target() {
        use super::super::{StageDef, StepDef};

        let task = TaskDef {
            name: "flow-control".to_string(),
            script: "skip".to_string(),
            role: "directive".to_string(),
            target: Some("Nonexistent Step".to_string()),
            source_block_index: BlockIndex {
                chapter_index: 0,
                block_index: 0,
            },
        };

        let playbook = PlaybookDef {
            name: "Test Playbook".to_string(),
            chapter_index: 0,
            stages: vec![StageDef {
                name: "Stage 1".to_string(),
                kind: None,
                icon: Vec::new(),
                flow_control: None,
                expose: Vec::new(),
                mask: Vec::new(),
                steps: vec![StepDef {
                    name: "Step 1".to_string(),
                    flow_control: None,
                    expose: Vec::new(),
                    mask: Vec::new(),
                    task_refs: Vec::new(),
                    source_block_index: BlockIndex {
                        chapter_index: 0,
                        block_index: 0,
                    },
                }],
                source_block_index: BlockIndex {
                    chapter_index: 0,
                    block_index: 0,
                },
            }],
        };

        let mut warnings = Vec::new();
        link_directive_tasks(&[task], &[playbook], &mut warnings);

        assert!(
            warnings
                .iter()
                .any(|w| w.contains("no matching stage or step"))
        );
    }
}
