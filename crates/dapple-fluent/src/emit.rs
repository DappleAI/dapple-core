//! Deterministic emission of guide-shaped Fluent (`locales/en.ftl`) documents.
//!
//! This module exists to replace the LLM-driven `generate-fluent` step in the
//! guide-builder pipeline. The build-time data flowing into that step is
//! already structured (lists of profiles, stages, steps, tasks), and Fluent's
//! syntax is a closed grammar — there is no semantic reason an LLM should be
//! in the format-emission path. Putting the formatter here means:
//!
//! - It's a pure function over typed data; no I/O, no token cost, no flake.
//! - Round-trip tests verify every emission parses cleanly with
//!   `fluent-syntax`, which is the same library `dapple-parser` uses
//!   server-side. If a unit test passes, the server can ingest the output.
//! - The `fluent emit-guide` nushell command in `dapple-prompt-flow` is a
//!   thin shell over [`emit_guide`]: deserialize the input, call this, return
//!   the string. All the Fluent-specific knowledge lives here.
//!
//! ## Section ordering and shape
//!
//! [`emit_guide`] always emits sections in this order, with one blank line
//! between sections and two between message groups:
//!
//! 1. Brief pack (with `.card-subtitle` / `.card-category` attributes)
//! 2. Inference profiles (one Fluent message per profile, with `.description`,
//!    `.system-prompt`, `.lineage`, `.tier`, `.release`, `.temperature`,
//!    `.max-tokens` attributes)
//! 3. Stages (one message per stage, optional `.description`)
//! 4. Steps (one message per step, optional `.description`)
//! 5. Tasks (one message per task, body is the inline content)
//!
//! Brief inputs / notices / outputs / value slots are NOT emitted; they're
//! synthesized server-side from structured brief data during ingest.
//!
//! ## Indentation rules
//!
//! Fluent's continuation rules are unforgiving: every line after the first
//! line of a multi-line value must be indented at least 1 space; attribute
//! continuation lines must be indented past the attribute marker. This module
//! uses a fixed convention:
//!
//! - 4 spaces for message-body continuation lines and attribute leader lines.
//! - 8 spaces for multi-line attribute body continuation lines.
//! - Blank lines inside a multi-line body get the same indentation as the
//!   surrounding lines (4 or 8 spaces) to avoid the parser collapsing them
//!   into the surrounding block.

use serde::Deserialize;

/// Top-level structured representation of a guide's `locales/en.ftl`.
///
/// All fields are optional in the sense that empty collections are valid —
/// a guide with no inference profiles, for example, just emits zero profile
/// messages and the rest of the document remains well-formed.
#[derive(Debug, Clone, Deserialize)]
pub struct GuideFluent {
    pub brief_pack: BriefPack,
    #[serde(default)]
    pub profiles: Vec<InferenceProfile>,
    #[serde(default)]
    pub stages: Vec<NamedMessage>,
    #[serde(default)]
    pub steps: Vec<NamedMessage>,
    #[serde(default)]
    pub tasks: Vec<TaskMessage>,
}

/// The `pack-name = ...` Fluent message at the top of the document.
#[derive(Debug, Clone, Deserialize)]
pub struct BriefPack {
    pub key: String,
    pub display_name: String,
    #[serde(default)]
    pub card_subtitle: Option<String>,
    #[serde(default)]
    pub card_category: Option<String>,
}

/// One inference profile, emitted as a Fluent message with attributes.
///
/// `system_prompt` is the only field expected to contain multi-paragraph
/// content; emission handles indentation of blank lines automatically.
#[derive(Debug, Clone, Deserialize)]
pub struct InferenceProfile {
    pub key: String,
    pub display_name: String,
    pub description: String,
    pub system_prompt: String,
    pub lineage: String,
    pub tier: String,
    pub release: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

/// A simple `key = display_name` message with an optional `.description`
/// attribute. Used for both stages and steps — the shape is identical and
/// the only thing that differs is which section the message appears in.
#[derive(Debug, Clone, Deserialize)]
pub struct NamedMessage {
    pub key: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// A `task-name = body` message. `body` may be multi-paragraph; it's emitted
/// using the same continuation-line indentation as inference-profile system
/// prompts.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskMessage {
    pub key: String,
    pub body: String,
}

/// Emit the complete `locales/en.ftl` document for the given guide.
///
/// Pure function — no I/O, no failure modes. The type system already pins
/// down everything emission needs; if a caller can hand us a `GuideFluent`,
/// the result is always valid Fluent (verified by the round-trip tests in
/// this module).
pub fn emit_guide(g: &GuideFluent) -> String {
    let mut out = String::new();

    // 1. Brief pack
    out.push_str("# Brief pack\n");
    out.push_str(&emit_brief_pack(&g.brief_pack));

    // 2. Inference profiles
    if !g.profiles.is_empty() {
        out.push_str("\n\n# Inference profiles\n");
        for (i, profile) in g.profiles.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&emit_profile(profile));
        }
    }

    // 3. Stages
    if !g.stages.is_empty() {
        out.push_str("\n\n# Stages\n");
        for (i, stage) in g.stages.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&emit_named_message(stage));
        }
    }

    // 4. Steps
    if !g.steps.is_empty() {
        out.push_str("\n\n# Steps\n");
        for (i, step) in g.steps.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&emit_named_message(step));
        }
    }

    // 5. Tasks
    if !g.tasks.is_empty() {
        out.push_str("\n\n# Tasks\n");
        for (i, task) in g.tasks.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&emit_task(task));
        }
    }

    // Trailing newline for cleanliness — matches author conventions and
    // avoids `\ No newline at end of file` diffs.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn emit_brief_pack(p: &BriefPack) -> String {
    let mut out = format!("{} = {}\n", p.key, p.display_name.trim());
    // Treat empty / whitespace-only optional attributes as absent. Fluent
    // rejects attribute lines whose value is empty (it's a parse error,
    // not a warning), so emitting `    .card-subtitle = \n` would break
    // the whole document. Upstream input shapes (the guide-builder's
    // branding record) sometimes carry "" for unset fields; this guards
    // against that without requiring upstream cleanup.
    if let Some(sub) = p.card_subtitle.as_deref() {
        let sub = sub.trim();
        if !sub.is_empty() {
            out.push_str(&format!("    .card-subtitle = {}\n", sub));
        }
    }
    if let Some(cat) = p.card_category.as_deref() {
        let cat = cat.trim();
        if !cat.is_empty() {
            out.push_str(&format!("    .card-category = {}\n", cat));
        }
    }
    out
}

fn emit_profile(p: &InferenceProfile) -> String {
    let mut out = format!("{} = {}\n", p.key, p.display_name.trim());
    out.push_str(&format!("    .description = {}\n", p.description.trim()));

    // Multi-line attribute: leader line ends with `=`, body indented 8 spaces.
    out.push_str("    .system-prompt =\n");
    out.push_str(&indent_block(p.system_prompt.trim(), 8));
    out.push('\n');

    out.push_str(&format!("    .lineage = {}\n", p.lineage.trim()));
    out.push_str(&format!("    .tier = {}\n", p.tier.trim()));
    out.push_str(&format!("    .release = {}\n", p.release.trim()));
    out.push_str(&format!(
        "    .temperature = {}\n",
        format_float(p.temperature)
    ));
    out.push_str(&format!("    .max-tokens = {}\n", p.max_tokens));
    out
}

fn emit_named_message(m: &NamedMessage) -> String {
    let mut out = format!("{} = {}\n", m.key, m.display_name.trim());
    if let Some(desc) = m.description.as_deref() {
        let desc = desc.trim();
        if !desc.is_empty() {
            // Multi-line vs single-line description: a description that
            // contains a newline gets the multi-line attribute treatment;
            // otherwise it stays inline. This avoids unnecessarily long
            // multi-line attribute bodies for one-sentence descriptions.
            if desc.contains('\n') {
                out.push_str("    .description =\n");
                out.push_str(&indent_block(desc, 8));
                out.push('\n');
            } else {
                out.push_str(&format!("    .description = {}\n", desc));
            }
        }
    }
    out
}

fn emit_task(t: &TaskMessage) -> String {
    let body = t.body.trim();
    if body.is_empty() {
        // Empty task body: emit just the key with an empty value. Fluent
        // requires *some* value, even if it's just a placeholder space.
        return format!("{} = \n", t.key);
    }
    if body.contains('\n') {
        // Multi-paragraph body: leader line ends with `=`, body indented 4.
        let mut out = format!("{} =\n", t.key);
        out.push_str(&indent_block(body, 4));
        out.push('\n');
        out
    } else {
        format!("{} = {}\n", t.key, body)
    }
}

/// Indent every line of `text` by `spaces` spaces. Preserves blank lines —
/// they get the same indentation as content lines so Fluent's continuation
/// parser doesn't terminate the block on them.
fn indent_block(text: &str, spaces: usize) -> String {
    let pad: String = " ".repeat(spaces);
    let mut out = String::new();
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        // Strip trailing whitespace on each line; the indentation we add is
        // the only leading whitespace we want.
        let trimmed_end = line.trim_end();
        out.push_str(&pad);
        out.push_str(trimmed_end);
    }
    out
}

/// Format a float for Fluent attribute values. Fluent doesn't have a number
/// type — all attribute values are strings — but we keep a stable canonical
/// form so deterministic emissions don't drift across runs:
///
/// - Whole numbers print without a decimal: `0.0` → `0`, `1.0` → `1`
/// - Fractional values print with at most 4 decimal digits, trailing zeros
///   stripped: `0.7` → `0.7`, `0.8500` → `0.85`
fn format_float(f: f32) -> String {
    if f.fract() == 0.0 && f.is_finite() {
        return format!("{}", f as i64);
    }
    let s = format!("{:.4}", f);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse the emitted Fluent with the same library `dapple-parser` uses
    /// server-side. Asserts every emitted string is syntactically valid AND
    /// every message we emitted survives the parse.
    fn assert_round_trip(emitted: &str, expected_keys: &[&str]) {
        let resource = fluent_syntax_unicode::parser::parse(emitted).unwrap_or_else(|(_, errs)| {
            panic!(
                "emit_guide produced invalid Fluent: {:?}\n--- emitted ---\n{}",
                errs, emitted
            )
        });

        let parsed_keys: Vec<String> = resource
            .body
            .iter()
            .filter_map(|entry| {
                if let fluent_syntax_unicode::ast::Entry::Message(msg) = entry {
                    Some(msg.id.name.to_string())
                } else {
                    None
                }
            })
            .collect();

        for expected in expected_keys {
            assert!(
                parsed_keys.iter().any(|k| k == expected),
                "expected message key `{}` not found in parsed output. Got: {:?}",
                expected,
                parsed_keys
            );
        }
    }

    fn brief_pack_only() -> GuideFluent {
        GuideFluent {
            brief_pack: BriefPack {
                key: "draft-pack".into(),
                display_name: "Draft Pack".into(),
                card_subtitle: None,
                card_category: None,
            },
            profiles: vec![],
            stages: vec![],
            steps: vec![],
            tasks: vec![],
        }
    }

    #[test]
    fn empty_guide_with_only_brief_pack_round_trips() {
        let emitted = emit_guide(&brief_pack_only());
        assert_round_trip(&emitted, &["draft-pack"]);
    }

    #[test]
    fn brief_pack_with_attributes() {
        let mut g = brief_pack_only();
        g.brief_pack.card_subtitle = Some("Compelling tagline".into());
        g.brief_pack.card_category = Some("workflow".into());
        let emitted = emit_guide(&g);
        assert!(
            emitted.contains("    .card-subtitle = Compelling tagline\n"),
            "{}",
            emitted
        );
        assert!(
            emitted.contains("    .card-category = workflow\n"),
            "{}",
            emitted
        );
        assert_round_trip(&emitted, &["draft-pack"]);
    }

    #[test]
    fn brief_pack_without_attributes_emits_no_attribute_lines() {
        let emitted = emit_guide(&brief_pack_only());
        assert!(!emitted.contains(".card-subtitle"));
        assert!(!emitted.contains(".card-category"));
    }

    #[test]
    fn brief_pack_with_empty_string_attributes_does_not_emit_them() {
        // Upstream sometimes hands us `Some("")` instead of `None` for unset
        // fields. Empty / whitespace-only attribute values are a hard parse
        // error in Fluent, so the formatter MUST drop them. Round-trip the
        // result — the file shouldn't even mention `.card-category` if the
        // input value was empty.
        let mut g = brief_pack_only();
        g.brief_pack.card_subtitle = Some("".into());
        g.brief_pack.card_category = Some("   ".into());
        let emitted = emit_guide(&g);
        assert!(!emitted.contains(".card-subtitle"), "{}", emitted);
        assert!(!emitted.contains(".card-category"), "{}", emitted);
        assert_round_trip(&emitted, &["draft-pack"]);
    }

    #[test]
    fn single_inference_profile_with_multi_paragraph_system_prompt() {
        let mut g = brief_pack_only();
        g.profiles.push(InferenceProfile {
            key: "draft-pack-analyst".into(),
            display_name: "Draft Pack Analyst".into(),
            description: "Domain-savvy guide assistant".into(),
            system_prompt: "You are an expert in content drafting.\n\nYour rules:\n1. Be specific.\n2. Ground in user input.".into(),
            lineage: "claude-4-6".into(),
            tier: "sonnet".into(),
            release: "20250929".into(),
            temperature: 0.7,
            max_tokens: 4096,
        });
        let emitted = emit_guide(&g);

        // The system-prompt body is multi-line and contains a blank line —
        // that blank line MUST be indented or Fluent terminates the block.
        // We emit `        ` (8 spaces) for blank lines too.
        assert!(emitted.contains("    .system-prompt =\n"));
        assert!(
            emitted.contains("        You are an expert in content drafting.\n"),
            "{}",
            emitted
        );
        // Blank line inside the body must be indented.
        assert!(
            emitted.contains("        \n        Your rules:\n"),
            "blank line must carry indentation:\n{}",
            emitted
        );
        // The attributes after the multi-line one must continue at 4-space
        // indentation, not 8.
        assert!(
            emitted.contains("    .lineage = claude-4-6\n"),
            "{}",
            emitted
        );
        assert!(emitted.contains("    .max-tokens = 4096\n"), "{}", emitted);
        assert_round_trip(&emitted, &["draft-pack", "draft-pack-analyst"]);

        // Round-trip the system-prompt body content too: parse, find the
        // attribute, and confirm the body comes through with the blank line
        // intact (Fluent collapses leading whitespace but preserves the
        // paragraph break).
        let resource = fluent_syntax_unicode::parser::parse(emitted.as_str()).unwrap();
        let mut found_body = None;
        for entry in &resource.body {
            if let fluent_syntax_unicode::ast::Entry::Message(msg) = entry {
                if msg.id.name == "draft-pack-analyst" {
                    for attr in &msg.attributes {
                        if attr.id.name == "system-prompt" {
                            // pattern_to_string semantics: text elements only,
                            // no placeables expected here.
                            let mut text = String::new();
                            for el in &attr.value.elements {
                                if let fluent_syntax_unicode::ast::PatternElement::TextElement {
                                    value,
                                } = el
                                {
                                    text.push_str(value);
                                }
                            }
                            found_body = Some(text);
                        }
                    }
                }
            }
        }
        let body = found_body.expect("system-prompt attribute should be present");
        assert!(
            body.contains("Be specific."),
            "round-tripped body: {:?}",
            body
        );
        assert!(
            body.contains("Your rules:"),
            "round-tripped body: {:?}",
            body
        );
    }

    #[test]
    fn two_profiles_back_to_back_have_one_blank_line_between() {
        let mut g = brief_pack_only();
        for i in 0..2 {
            g.profiles.push(InferenceProfile {
                key: format!("profile-{}", i),
                display_name: format!("Profile {}", i),
                description: "desc".into(),
                system_prompt: "prompt".into(),
                lineage: "claude-4-6".into(),
                tier: "sonnet".into(),
                release: "20250929".into(),
                temperature: 0.7,
                max_tokens: 4096,
            });
        }
        let emitted = emit_guide(&g);
        assert_round_trip(&emitted, &["draft-pack", "profile-0", "profile-1"]);
        // Two messages must be syntactically distinct — Fluent uses blank
        // lines as message separators.
        let lines: Vec<&str> = emitted.lines().collect();
        let p0_idx = lines
            .iter()
            .position(|l| l.starts_with("profile-0 ="))
            .unwrap();
        let p1_idx = lines
            .iter()
            .position(|l| l.starts_with("profile-1 ="))
            .unwrap();
        // At least one blank line between the end of profile-0's last
        // attribute and the start of profile-1.
        let between: &[&str] = &lines[p0_idx + 1..p1_idx];
        assert!(
            between.iter().any(|l| l.is_empty()),
            "expected a blank line between adjacent profiles. Lines between:\n{:?}",
            between
        );
    }

    #[test]
    fn stage_with_and_without_description() {
        let mut g = brief_pack_only();
        g.stages.push(NamedMessage {
            key: "stage-one".into(),
            display_name: "Stage One".into(),
            description: Some("Build the foundation.".into()),
        });
        g.stages.push(NamedMessage {
            key: "stage-two".into(),
            display_name: "Stage Two".into(),
            description: None,
        });
        let emitted = emit_guide(&g);
        assert!(emitted.contains("stage-one = Stage One\n"));
        assert!(emitted.contains("    .description = Build the foundation.\n"));
        assert!(emitted.contains("stage-two = Stage Two\n"));
        // stage-two has no description; the line after it should NOT be a
        // .description attribute.
        let lines: Vec<&str> = emitted.lines().collect();
        let s2_idx = lines
            .iter()
            .position(|l| l.starts_with("stage-two ="))
            .unwrap();
        if let Some(next) = lines.get(s2_idx + 1) {
            assert!(
                !next.trim_start().starts_with(".description"),
                "stage-two should not have a description line, got: {}",
                next
            );
        }
        assert_round_trip(&emitted, &["draft-pack", "stage-one", "stage-two"]);
    }

    #[test]
    fn step_display_name_with_special_characters_passes_through() {
        let mut g = brief_pack_only();
        g.steps.push(NamedMessage {
            key: "tricky-step".into(),
            display_name: "It's a step (with) \"punctuation\" & symbols!".into(),
            description: None,
        });
        let emitted = emit_guide(&g);
        // Fluent doesn't require escaping for these in attribute values —
        // they should pass through verbatim.
        assert!(
            emitted.contains("tricky-step = It's a step (with) \"punctuation\" & symbols!\n"),
            "{}",
            emitted
        );
        assert_round_trip(&emitted, &["draft-pack", "tricky-step"]);
    }

    #[test]
    fn multi_paragraph_task_body_uses_continuation_indent() {
        let mut g = brief_pack_only();
        g.tasks.push(TaskMessage {
            key: "complex-task".into(),
            body:
                "First paragraph of the task.\n\nSecond paragraph here.\nWith a continuation line."
                    .into(),
        });
        let emitted = emit_guide(&g);
        // Multi-line body: leader is `key =`, body indented 4 spaces, blank
        // line carries 4 spaces.
        assert!(emitted.contains("complex-task =\n"));
        assert!(
            emitted.contains("    First paragraph of the task.\n"),
            "{}",
            emitted
        );
        assert!(
            emitted.contains("    \n    Second paragraph here.\n"),
            "blank line in task body must be indented:\n{}",
            emitted
        );
        assert_round_trip(&emitted, &["draft-pack", "complex-task"]);
    }

    #[test]
    fn float_formatting_strips_trailing_zeros() {
        assert_eq!(format_float(0.7), "0.7");
        assert_eq!(format_float(0.85), "0.85");
        assert_eq!(format_float(1.0), "1");
        assert_eq!(format_float(0.0), "0");
        // Whole-number temperatures show up as integers, not 1.0000.
        assert_eq!(format_float(2.0), "2");
    }

    #[test]
    fn full_guide_round_trips_with_all_sections() {
        let g = GuideFluent {
            brief_pack: BriefPack {
                key: "draft-presentation-slides-pack".into(),
                display_name: "Draft Presentation Slides".into(),
                card_subtitle: Some("From outline to deck in minutes".into()),
                card_category: Some("workflow".into()),
            },
            profiles: vec![InferenceProfile {
                key: "draft-presentation-slides-analyst".into(),
                display_name: "Presentation Analyst".into(),
                description: "Builds presentation context, structure, and content.".into(),
                system_prompt: "You are an expert in presentation craft.\n\nYour behavioral rules:\n\n1. Always be concrete.\n2. Ground analysis in the user's stated audience and constraints.".into(),
                lineage: "claude-4-6".into(),
                tier: "sonnet".into(),
                release: "20250929".into(),
                temperature: 0.7,
                max_tokens: 4096,
            }],
            stages: vec![
                NamedMessage {
                    key: "define-context".into(),
                    display_name: "Define Your Presentation Context".into(),
                    description: Some("Audience, message, constraints.".into()),
                },
                NamedMessage {
                    key: "build-outline".into(),
                    display_name: "Build Your Outline".into(),
                    description: None,
                },
            ],
            steps: vec![NamedMessage {
                key: "describe-context".into(),
                display_name: "Describe Your Presentation Context".into(),
                description: Some("Capture audience, setting, and core message.".into()),
            }],
            tasks: vec![TaskMessage {
                key: "describe-context-task".into(),
                body: "Run the describe-context script.".into(),
            }],
        };
        let emitted = emit_guide(&g);
        assert_round_trip(
            &emitted,
            &[
                "draft-presentation-slides-pack",
                "draft-presentation-slides-analyst",
                "define-context",
                "build-outline",
                "describe-context",
                "describe-context-task",
            ],
        );
    }

    #[test]
    fn deterministic_output_byte_identical_across_calls() {
        let g = GuideFluent {
            brief_pack: BriefPack {
                key: "p".into(),
                display_name: "P".into(),
                card_subtitle: Some("sub".into()),
                card_category: None,
            },
            profiles: vec![InferenceProfile {
                key: "a".into(),
                display_name: "A".into(),
                description: "d".into(),
                system_prompt: "s".into(),
                lineage: "l".into(),
                tier: "t".into(),
                release: "r".into(),
                temperature: 0.5,
                max_tokens: 100,
            }],
            stages: vec![],
            steps: vec![],
            tasks: vec![],
        };
        let a = emit_guide(&g);
        let b = emit_guide(&g);
        assert_eq!(a, b, "emission must be deterministic");
    }

    #[test]
    fn empty_optional_description_does_not_emit_blank_attribute() {
        let mut g = brief_pack_only();
        g.steps.push(NamedMessage {
            key: "lonely-step".into(),
            display_name: "Lonely Step".into(),
            description: Some("   ".into()), // whitespace only
        });
        let emitted = emit_guide(&g);
        assert!(
            !emitted.contains(".description ="),
            "blank-only description should not emit an attribute line:\n{}",
            emitted
        );
        assert_round_trip(&emitted, &["draft-pack", "lonely-step"]);
    }
}
