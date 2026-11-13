//! Playbook extraction from :::stage/:::step block hierarchy (Fluent-native).
//!
//! Walks the parsed guide looking for :::stage blocks per chapter. Each chapter
//! that contains :::stage blocks produces its own `PlaybookDef`. Chapters without
//! stages are skipped.
//!
//! When only one chapter has stages, the playbook name is the guide title (backwards
//! compatible). When multiple chapters have stages, names use "guide title — chapter title"
//! for disambiguation.
//!
//! Under the Fluent-native model, description text for stages and steps
//! lives in FluentMessage bodies, not inline in the block content.

use super::{BlockIndex, PlaybookDef, StageDef, StepDef};
use crate::types::{ParsedBlock, ParsedGuide};

/// Read a string-array attribute (e.g. `expose: ["topic", "audience"]`) from a
/// block, returning an empty Vec when the attribute is absent or not an array.
fn string_array_attr(block: &ParsedBlock, key: &str) -> Vec<String> {
    block
        .attributes
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Read an attribute that is normally a string array but is also accepted as a
/// bare string (`icon: "radar"` ⇒ `["radar"]`), returning an empty Vec when the
/// attribute is absent or is neither shape.
///
/// The bare-string arm exists because a one-icon stage is the common case and
/// an author should not have to type brackets for it. Values are kept VERBATIM
/// and in authored order — no sorting, no dedup, no case folding — because the
/// consuming client reads the first N and a normalization here would silently
/// change *which* N. Empty strings are dropped: they name nothing and would
/// otherwise fold into the content hash as a phantom entry.
fn string_or_string_array_attr(block: &ParsedBlock, key: &str) -> Vec<String> {
    let raw = match block.attributes.get(key) {
        Some(v) => v,
        None => return Vec::new(),
    };
    let values: Vec<String> = match raw {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        // Any other shape (number, bool, object) is a malformed authoring
        // mistake; stay permissive and treat it as absent.
        _ => Vec::new(),
    };
    values
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect()
}

/// Extract playbook definitions from the parsed guide's stage/step blocks.
///
/// Each chapter that contains :::stage blocks produces its own PlaybookDef.
/// Chapters without stages are skipped.
///
/// Returns an empty Vec if no chapters contain stages.
pub fn extract_playbooks(parsed: &ParsedGuide, warnings: &mut Vec<String>) -> Vec<PlaybookDef> {
    let mut playbooks = Vec::new();

    for (ch_idx, chapter) in parsed.chapters.iter().enumerate() {
        let mut stages = Vec::new();

        for (blk_idx, block) in chapter.blocks.iter().enumerate() {
            if block.directive_type == "stage" {
                let stage = extract_stage(block, ch_idx, blk_idx, warnings);
                stages.push(stage);
            }
        }

        if stages.is_empty() {
            continue;
        }

        playbooks.push(PlaybookDef {
            name: String::new(), // Placeholder — resolved below
            chapter_index: ch_idx,
            stages,
        });
    }

    // Resolve names: if only one chapter has stages, use guide title only.
    // If multiple, use "guide title — chapter title" for disambiguation.
    let needs_suffix = playbooks.len() > 1;

    for pb in &mut playbooks {
        if needs_suffix {
            let chapter_title = &parsed.chapters[pb.chapter_index].title;
            pb.name = format!("{} \u{2014} {}", parsed.metadata.title, chapter_title);
        } else {
            pb.name = parsed.metadata.title.clone();
        }
    }

    playbooks
}

/// Extract a single stage definition from a :::stage block.
fn extract_stage(
    block: &ParsedBlock,
    chapter_index: usize,
    block_index: usize,
    warnings: &mut Vec<String>,
) -> StageDef {
    let name = block
        .name
        .clone()
        .unwrap_or_else(|| format!("Stage {}", block_index + 1));

    let flow_control = block
        .attributes
        .get("flow_control")
        .cloned()
        .or_else(|| block.attributes.get("flowControl").cloned());

    // Extract steps from children
    let mut steps = Vec::new();
    for (child_idx, child) in block.children.iter().enumerate() {
        if child.directive_type == "step" {
            let step = extract_step(child, chapter_index, child_idx, warnings);
            steps.push(step);
        }
    }

    if steps.is_empty() {
        warnings.push(format!(
            ":::stage '{}' at chapter {}, block {} has no :::step children",
            name, chapter_index, block_index
        ));
    }

    StageDef {
        name,
        kind: block
            .attributes
            .get("kind")
            .and_then(|v| v.as_str())
            .map(String::from),
        icon: string_or_string_array_attr(block, "icon"),
        flow_control,
        expose: string_array_attr(block, "expose"),
        mask: string_array_attr(block, "mask"),
        steps,
        source_block_index: BlockIndex {
            chapter_index,
            block_index,
        },
    }
}

/// Extract a single step definition from a :::step block.
fn extract_step(
    block: &ParsedBlock,
    chapter_index: usize,
    block_index: usize,
    warnings: &mut Vec<String>,
) -> StepDef {
    let name = block
        .name
        .clone()
        .unwrap_or_else(|| format!("Step {}", block_index + 1));

    let flow_control = block
        .attributes
        .get("flow_control")
        .cloned()
        .or_else(|| block.attributes.get("flowControl").cloned());

    // A step is an ordered, fail-together set of tasks. Its task list is, in
    // order:
    //   1. nested `:::nushell` children authored directly inside the step — the
    //      step's PRIVATE tasks, scoped to the step name (matching the scoped
    //      `TaskDef`s the task walk produces), in document order; then
    //   2. the explicit `tasks: [...]` list (or single `task: "x"` sugar) — used
    //      to reference SHARED/global tasks defined outside the step.
    // Empty when the step has neither (ingest rejects a task-less step).
    let mut task_refs: Vec<String> = Vec::new();
    let mut has_nested_task = false;
    for child in &block.children {
        if child.directive_type != "nushell" {
            continue;
        }
        // Directive nushell (`role: "directive"`, attached by `for`) is flow
        // control, not a step task.
        let role = child
            .attributes
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        if role == "directive" {
            continue;
        }
        if let Some(child_name) = child.name.as_deref() {
            has_nested_task = true;
            // Scope by the step's RAW name (block.name), identical to the task
            // walk — never the "Step N" display fallback.
            task_refs.push(super::task::step_task_ref_name(
                block.name.as_deref(),
                child_name,
            ));
        }
    }
    if has_nested_task && block.name.is_none() {
        warnings.push(format!(
            ":::step at chapter {}, block {} nests :::nushell tasks but has no name; \
             nested tasks are scoped by the step name — add a name attribute",
            chapter_index, block_index
        ));
    }
    let listed = string_array_attr(block, "tasks");
    if !listed.is_empty() {
        task_refs.extend(listed);
    } else if let Some(single) = block.attributes.get("task").and_then(|v| v.as_str()) {
        task_refs.push(single.to_string());
    }

    StepDef {
        name,
        flow_control,
        expose: string_array_attr(block, "expose"),
        mask: string_array_attr(block, "mask"),
        task_refs,
        source_block_index: BlockIndex {
            chapter_index,
            block_index,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GuideMetadata, ParsedChapter};
    use std::collections::HashMap;

    fn make_step_block(name: &str, content: &str) -> ParsedBlock {
        ParsedBlock {
            directive_type: "step".to_string(),
            name: Some(name.to_string()),
            content: content.to_string(),
            attributes: serde_json::json!({ "name": name }),
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        }
    }

    fn make_stage_block(name: &str, steps: Vec<ParsedBlock>) -> ParsedBlock {
        ParsedBlock {
            directive_type: "stage".to_string(),
            name: Some(name.to_string()),
            content: String::new(),
            attributes: serde_json::json!({ "name": name }),
            tags: Vec::new(),
            source_line: 1,
            children: steps,
            ref_path: None,
        }
    }

    #[test]
    fn stage_kind_attribute_is_read_raw() {
        let mut warnings = Vec::new();

        let mut block = make_stage_block("Pull the Record", vec![make_step_block("s1", "c")]);
        block.attributes["kind"] = serde_json::json!("fetch");
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.kind.as_deref(), Some("fetch"));

        // Absent attribute -> None.
        let block = make_stage_block("No Kind", vec![make_step_block("s1", "c")]);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.kind, None);

        // Non-string value -> None; the parser stays permissive (ingest warns).
        let mut block = make_stage_block("Bad Kind", vec![make_step_block("s1", "c")]);
        block.attributes["kind"] = serde_json::json!(["fetch"]);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.kind, None);
    }

    #[test]
    fn stage_icon_attribute_accepts_array_or_bare_string() {
        let mut warnings = Vec::new();

        // The documented shape: an array, kept in AUTHORED ORDER (the client
        // reads the first five, so a sort here would change which five).
        let mut block = make_stage_block("Pull the Record", vec![make_step_block("s1", "c")]);
        block.attributes["icon"] = serde_json::json!(["radar", "scan-search", "id-card"]);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.icon, vec!["radar", "scan-search", "id-card"]);

        // A bare string is the one-icon convenience shape.
        let mut block = make_stage_block("One Icon", vec![make_step_block("s1", "c")]);
        block.attributes["icon"] = serde_json::json!("radar");
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.icon, vec!["radar"]);

        // Absent -> empty (indistinguishable from `[]`, by contract).
        let block = make_stage_block("No Icon", vec![make_step_block("s1", "c")]);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert!(stage.icon.is_empty());

        // Unknown names are NOT filtered — the vocabulary is open and belongs
        // to the drawing client. This is the inverse of `kind` on purpose.
        let mut block = make_stage_block("Odd Icon", vec![make_step_block("s1", "c")]);
        block.attributes["icon"] = serde_json::json!(["not-a-real-lucide-name"]);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.icon, vec!["not-a-real-lucide-name"]);

        // Duplicates survive (the client drops them); order still verbatim.
        let mut block = make_stage_block("Dupes", vec![make_step_block("s1", "c")]);
        block.attributes["icon"] = serde_json::json!(["radar", "radar", "globe"]);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.icon, vec!["radar", "radar", "globe"]);

        // Empty / whitespace entries name nothing and are dropped, so they
        // cannot fold into the content hash as phantom entries.
        let mut block = make_stage_block("Blanks", vec![make_step_block("s1", "c")]);
        block.attributes["icon"] = serde_json::json!(["", "  ", "globe"]);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert_eq!(stage.icon, vec!["globe"]);

        // A malformed shape is treated as absent, never as a panic.
        let mut block = make_stage_block("Bad Icon", vec![make_step_block("s1", "c")]);
        block.attributes["icon"] = serde_json::json!(42);
        let stage = extract_stage(&block, 0, 0, &mut warnings);
        assert!(stage.icon.is_empty());
    }

    /// Question 1 from the dapple-ui ask, pinned as a test: every name in their
    /// vocabulary contains hyphens, and `icon: [file-text, chart-line]` is what
    /// an author types without thinking. It must round-trip through the
    /// YAML-like attribute normalizer unquoted.
    #[test]
    fn stage_icon_hyphenated_names_survive_unquoted_attributes() {
        let attrs = crate::attrs::parse_attributes(
            r#"{ name: "Pull the Record", kind: intake, icon: [file-text, chart-line, scan-search] }"#,
        )
        .expect("unquoted hyphenated icon names must parse");

        let mut warnings = Vec::new();
        let mut block = make_stage_block("Pull the Record", vec![make_step_block("s1", "c")]);
        block.attributes = attrs;
        let stage = extract_stage(&block, 0, 0, &mut warnings);

        assert_eq!(stage.kind.as_deref(), Some("intake"));
        assert_eq!(stage.icon, vec!["file-text", "chart-line", "scan-search"]);
    }

    fn make_nushell_block(name: &str, role: Option<&str>) -> ParsedBlock {
        let attributes = match role {
            Some(r) => serde_json::json!({ "name": name, "role": r }),
            None => serde_json::json!({ "name": name }),
        };
        ParsedBlock {
            directive_type: "nushell".to_string(),
            name: Some(name.to_string()),
            content: "{ payload: {}, signals: {} }".to_string(),
            attributes,
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        }
    }

    fn make_step_block_with_children(name: &str, children: Vec<ParsedBlock>) -> ParsedBlock {
        ParsedBlock {
            directive_type: "step".to_string(),
            name: Some(name.to_string()),
            content: String::new(),
            attributes: serde_json::json!({ "name": name }),
            tags: Vec::new(),
            source_line: 1,
            children,
            ref_path: None,
        }
    }

    fn guide_with_stage(stage: ParsedBlock) -> ParsedGuide {
        ParsedGuide {
            title: "Test".to_string(),
            metadata: make_metadata("Test"),
            chapters: vec![make_chapter("Ch1", "ch1.md", vec![stage])],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        }
    }

    fn make_metadata(title: &str) -> GuideMetadata {
        GuideMetadata {
            title: title.to_string(),
            description: None,
            author: None,
            classification: None,
            source_path: None,
            card: None,
            deprecation_level: None,
            deprecation_reason: None,
            chapters: HashMap::new(),
        }
    }

    fn make_chapter(title: &str, file: &str, blocks: Vec<ParsedBlock>) -> ParsedChapter {
        ParsedChapter {
            title: title.to_string(),
            source_file: file.to_string(),
            depth: 0,
            parent_index: None,
            blocks,
        }
    }

    #[test]
    fn test_extract_playbooks_no_stages() {
        let guide = ParsedGuide {
            title: "Test".to_string(),
            metadata: make_metadata("Test"),
            chapters: vec![make_chapter(
                "Ch1",
                "ch1.md",
                vec![ParsedBlock {
                    directive_type: "prose".to_string(),
                    name: None,
                    content: "Just prose.".to_string(),
                    attributes: serde_json::json!({}),
                    tags: Vec::new(),
                    source_line: 1,
                    children: Vec::new(),
                    ref_path: None,
                }],
            )],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let result = extract_playbooks(&guide, &mut warnings);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_playbooks_single_chapter_no_suffix() {
        let step1 = make_step_block("Gather Info", "");
        let step2 = make_step_block("Process Data", "");
        let stage = make_stage_block("Research", vec![step1, step2]);

        let guide = ParsedGuide {
            title: "Essay Helper".to_string(),
            metadata: make_metadata("Essay Helper"),
            chapters: vec![make_chapter(
                "Getting Started",
                "getting-started.md",
                vec![stage],
            )],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);

        assert_eq!(playbooks.len(), 1);
        assert_eq!(playbooks[0].name, "Essay Helper");
        assert_eq!(playbooks[0].chapter_index, 0);
        assert_eq!(playbooks[0].stages.len(), 1);
        assert_eq!(playbooks[0].stages[0].name, "Research");
        assert_eq!(playbooks[0].stages[0].steps.len(), 2);
        assert_eq!(playbooks[0].stages[0].steps[0].name, "Gather Info");
        assert_eq!(playbooks[0].stages[0].steps[1].name, "Process Data");
    }

    #[test]
    fn test_extract_playbooks_multi_chapter() {
        let guide = ParsedGuide {
            title: "Stuart Librarian".to_string(),
            metadata: make_metadata("Stuart Librarian"),
            chapters: vec![
                make_chapter(
                    "Search",
                    "search.md",
                    vec![make_stage_block(
                        "Execute Search",
                        vec![make_step_block("Run Query", "")],
                    )],
                ),
                make_chapter(
                    "Browse",
                    "browse.md",
                    vec![make_stage_block(
                        "Navigate",
                        vec![make_step_block("List Guides", "")],
                    )],
                ),
            ],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);

        assert_eq!(playbooks.len(), 2);
        assert_eq!(playbooks[0].name, "Stuart Librarian \u{2014} Search");
        assert_eq!(playbooks[0].chapter_index, 0);
        assert_eq!(playbooks[0].stages[0].name, "Execute Search");
        assert_eq!(playbooks[1].name, "Stuart Librarian \u{2014} Browse");
        assert_eq!(playbooks[1].chapter_index, 1);
        assert_eq!(playbooks[1].stages[0].name, "Navigate");
    }

    #[test]
    fn test_extract_playbooks_skips_chapters_without_stages() {
        let guide = ParsedGuide {
            title: "Stuart Librarian".to_string(),
            metadata: make_metadata("Stuart Librarian"),
            chapters: vec![
                // Chapter 0: prose only (introduction)
                make_chapter(
                    "Introduction",
                    "intro.md",
                    vec![ParsedBlock {
                        directive_type: "prose".to_string(),
                        name: None,
                        content: "Welcome text".to_string(),
                        attributes: serde_json::json!({}),
                        tags: Vec::new(),
                        source_line: 1,
                        children: Vec::new(),
                        ref_path: None,
                    }],
                ),
                // Chapter 1: has stages
                make_chapter(
                    "Search",
                    "search.md",
                    vec![make_stage_block(
                        "Execute",
                        vec![make_step_block("Run", "")],
                    )],
                ),
            ],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);

        // Only 1 chapter has stages — single playbook, no suffix
        assert_eq!(playbooks.len(), 1);
        assert_eq!(playbooks[0].name, "Stuart Librarian");
        assert_eq!(playbooks[0].chapter_index, 1);
    }

    #[test]
    fn test_extract_playbooks_stage_without_steps_warns() {
        let stage = make_stage_block("Empty Stage", vec![]);

        let guide = ParsedGuide {
            title: "Test".to_string(),
            metadata: make_metadata("Test"),
            chapters: vec![make_chapter("Ch1", "ch1.md", vec![stage])],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);

        assert_eq!(playbooks.len(), 1);
        assert_eq!(playbooks[0].stages[0].steps.len(), 0);
        assert!(warnings.iter().any(|w| w.contains("no :::step children")));
    }

    #[test]
    fn test_step_with_task_ref() {
        // Single `task: "x"` is sugar for a one-element ordered list.
        let mut step = make_step_block("Execute", "");
        step.attributes = serde_json::json!({ "name": "Execute", "task": "my-nushell-task" });

        let stage = make_stage_block("Processing", vec![step]);

        let guide = ParsedGuide {
            title: "Test".to_string(),
            metadata: make_metadata("Test"),
            chapters: vec![make_chapter("Ch1", "ch1.md", vec![stage])],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);
        assert_eq!(
            playbooks[0].stages[0].steps[0].task_refs,
            vec!["my-nushell-task".to_string()]
        );
    }

    #[test]
    fn test_step_with_ordered_task_list() {
        // `tasks: [...]` captures the authored order; a step is an ordered task-set.
        let mut step = make_step_block("Execute", "");
        step.attributes = serde_json::json!({
            "name": "Execute",
            "tasks": ["get", "retrieve", "mangle", "check-safety"]
        });

        let stage = make_stage_block("Processing", vec![step]);

        let guide = ParsedGuide {
            title: "Test".to_string(),
            metadata: make_metadata("Test"),
            chapters: vec![make_chapter("Ch1", "ch1.md", vec![stage])],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);
        assert_eq!(
            playbooks[0].stages[0].steps[0].task_refs,
            vec!["get", "retrieve", "mangle", "check-safety"]
        );
    }

    #[test]
    fn test_step_without_task_is_empty() {
        // No `task`/`tasks` attribute -> empty task set (not a panic / not None).
        let step = make_step_block("Pane Only", "");
        let stage = make_stage_block("Processing", vec![step]);

        let guide = ParsedGuide {
            title: "Test".to_string(),
            metadata: make_metadata("Test"),
            chapters: vec![make_chapter("Ch1", "ch1.md", vec![stage])],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);
        assert!(playbooks[0].stages[0].steps[0].task_refs.is_empty());
    }

    #[test]
    fn test_step_nested_nushell_children_are_scoped_ordered_tasks() {
        // Nested :::nushell children are the step's private tasks, scoped to the
        // step name, in document order.
        let step = make_step_block_with_children(
            "Coherent",
            vec![
                make_nushell_block("get", None),
                make_nushell_block("retrieve", None),
                make_nushell_block("mangle", None),
            ],
        );
        let guide = guide_with_stage(make_stage_block("Pipeline", vec![step]));

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);
        assert_eq!(
            playbooks[0].stages[0].steps[0].task_refs,
            vec!["Coherent/get", "Coherent/retrieve", "Coherent/mangle"]
        );
    }

    #[test]
    fn test_step_nested_then_shared_tasks_concatenate() {
        // Nested (private, scoped) come first in doc order; the `tasks: [...]`
        // attribute (shared/global refs) is appended.
        let mut step = make_step_block_with_children(
            "Coherent",
            vec![
                make_nushell_block("get", None),
                make_nushell_block("mangle", None),
            ],
        );
        step.attributes = serde_json::json!({ "name": "Coherent", "tasks": ["check-safety"] });
        let guide = guide_with_stage(make_stage_block("Pipeline", vec![step]));

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);
        assert_eq!(
            playbooks[0].stages[0].steps[0].task_refs,
            vec!["Coherent/get", "Coherent/mangle", "check-safety"]
        );
    }

    #[test]
    fn test_step_directive_nushell_child_is_not_a_task() {
        // A `role: "directive"` nushell child is flow control (attached by `for`),
        // not one of the step's tasks.
        let step = make_step_block_with_children(
            "Coherent",
            vec![
                make_nushell_block("get", None),
                make_nushell_block("guard", Some("directive")),
            ],
        );
        let guide = guide_with_stage(make_stage_block("Pipeline", vec![step]));

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);
        assert_eq!(
            playbooks[0].stages[0].steps[0].task_refs,
            vec!["Coherent/get"],
            "directive child must be excluded from the step's tasks"
        );
    }

    #[test]
    fn test_step_nested_tasks_without_name_warns_and_uses_bare() {
        // A nameless step can't scope its nested tasks; both the walk and
        // extract_step fall back to bare names (still linked), and we warn.
        let mut step =
            make_step_block_with_children("ignored", vec![make_nushell_block("get", None)]);
        step.name = None;
        step.attributes = serde_json::json!({});
        let guide = guide_with_stage(make_stage_block("Pipeline", vec![step]));

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);
        assert_eq!(playbooks[0].stages[0].steps[0].task_refs, vec!["get"]);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("nests :::nushell tasks but has no name")),
            "a nameless step with nested tasks must warn"
        );
    }

    #[test]
    fn test_extract_playbooks_multi_chapter_mixed() {
        // 3 chapters: prose-only, stages, stages — should produce 2 playbooks with suffixes
        let guide = ParsedGuide {
            title: "Multi Guide".to_string(),
            metadata: make_metadata("Multi Guide"),
            chapters: vec![
                make_chapter(
                    "Intro",
                    "intro.md",
                    vec![ParsedBlock {
                        directive_type: "prose".to_string(),
                        name: None,
                        content: "Welcome".to_string(),
                        attributes: serde_json::json!({}),
                        tags: Vec::new(),
                        source_line: 1,
                        children: Vec::new(),
                        ref_path: None,
                    }],
                ),
                make_chapter(
                    "Search",
                    "search.md",
                    vec![make_stage_block("Find", vec![make_step_block("Query", "")])],
                ),
                make_chapter(
                    "Browse",
                    "browse.md",
                    vec![
                        make_stage_block("List", vec![make_step_block("Fetch", "")]),
                        make_stage_block("Detail", vec![make_step_block("Show", "")]),
                    ],
                ),
            ],
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        };

        let mut warnings = Vec::new();
        let playbooks = extract_playbooks(&guide, &mut warnings);

        assert_eq!(playbooks.len(), 2);
        // Both get suffixed because 2 chapters have stages
        assert_eq!(playbooks[0].name, "Multi Guide \u{2014} Search");
        assert_eq!(playbooks[0].chapter_index, 1);
        assert_eq!(playbooks[0].stages.len(), 1);
        assert_eq!(playbooks[1].name, "Multi Guide \u{2014} Browse");
        assert_eq!(playbooks[1].chapter_index, 2);
        assert_eq!(playbooks[1].stages.len(), 2);
    }
}
