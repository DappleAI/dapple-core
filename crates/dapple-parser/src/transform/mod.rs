//! Transform layer: converts a `ParsedGuide` AST into an `IngestPlan`.
//!
//! The transform layer is pure -- it has no database access. It produces
//! intermediate definition types (`PlaybookDef`, `TaskDef`, etc.) that the
//! API handler then materializes into database entities within a transaction.
//!
//! Under the Fluent-native model, text content for stages, steps, brief packs,
//! and inference profiles lives in FluentMessage entities rather than inline.
//! The `IngestPlan` carries `FluentMessageSet` definitions keyed by message name.

pub mod asset;
pub mod brief;
pub mod brief_group;
pub mod brief_input;
pub mod brief_markdown;
pub mod brief_notice;
pub mod brief_output;
pub mod brief_pack;
pub mod card;
pub mod deliverable;
pub mod objective;
pub mod playbook;
pub mod scope;
pub mod task;
pub mod value_slot;

use crate::error::ParseError;
use crate::types::{ParsedBlock, ParsedGuide};
use std::collections::HashMap;

// ============================================================================
// IngestPlan -- the top-level output of the transform layer
// ============================================================================

/// The complete ingest plan produced by transforming a `ParsedGuide`.
///
/// Contains everything needed to materialize the guide and all derived
/// objects in a single database transaction.
#[derive(Clone, Debug)]
pub struct IngestPlan {
    /// The guide's title (from metadata).
    pub title: String,
    /// The guide's description (from metadata).
    pub description: Option<String>,
    // NO locale field -- guide structure is locale-independent.
    /// The guide's author (from metadata).
    pub author: Option<String>,
    /// The guide's classification (from metadata).
    pub classification: Option<String>,
    /// The guide's source path override (set by caller).
    pub source_path: Option<String>,
    /// Chapter definitions preserving the original structure.
    pub chapters: Vec<ChapterDef>,
    /// Optional promotional card data from guide.toml [card] section.
    pub card: Option<CardDef>,
    /// Asset files discovered in the guide directory.
    pub asset_files: Vec<AssetFileEntry>,
    /// Brief pack definitions extracted from :::brief_pack blocks.
    pub derived_brief_packs: Vec<BriefPackDef>,
    /// Playbook definitions extracted from :::stage/:::step hierarchy (one per chapter with stages).
    pub derived_playbooks: Vec<PlaybookDef>,
    /// Task definitions extracted from :::nushell blocks.
    pub derived_tasks: Vec<TaskDef>,
    /// Inference profile definitions extracted from :::inference_profile blocks.
    pub derived_inference_profiles: Vec<InferenceProfileDef>,
    /// Cross-reference definitions extracted from :::ref blocks.
    pub derived_refs: Vec<RefDef>,
    /// Scope definitions extracted from :::scope blocks (authoring metadata, not materialized).
    pub derived_scopes: Vec<ScopeDef>,
    /// Objective definitions extracted from :::objective blocks (authoring metadata, not materialized).
    pub derived_objectives: Vec<ObjectiveDef>,
    /// Deliverable definitions extracted from :::deliverable blocks (authoring metadata, not materialized).
    pub derived_deliverables: Vec<DeliverableDef>,
    /// Value slot definitions extracted from :::value_slot blocks.
    pub derived_value_slots: Vec<ValueSlotDef>,
    /// Brief input definitions extracted from :::brief_input blocks.
    pub derived_brief_inputs: Vec<BriefInputDef>,
    /// Brief notice definitions extracted from :::brief_notice blocks.
    pub derived_brief_notices: Vec<BriefNoticeDef>,
    /// Brief markdown definitions extracted from :::brief_markdown blocks.
    pub derived_brief_markdowns: Vec<BriefMarkdownDef>,
    /// Brief output definitions extracted from :::brief_output blocks.
    pub derived_brief_outputs: Vec<BriefOutputDef>,
    /// Brief definitions extracted from :::brief blocks.
    pub derived_briefs: Vec<BriefDef>,
    /// Brief group definitions extracted from :::brief_group blocks.
    pub derived_brief_groups: Vec<BriefGroupDef>,
    /// FluentMessage definitions per locale, keyed by message name.
    /// Populated from locales/*.ftl files in the guide directory.
    pub fluent_messages: HashMap<String, FluentMessageSet>,
    /// Deprecation level from guide.toml [deprecation] section, if present.
    pub deprecation_level: Option<String>,
    /// Deprecation reason from guide.toml [deprecation] section, if present.
    pub deprecation_reason: Option<String>,
    /// Prompt definitions discovered from the `prompts/` directory.
    pub derived_prompts: Vec<crate::prompt_files::PromptFileDef>,
    /// Resource definitions parsed from `resources.md`.
    pub resources: Vec<crate::citations::ResourceDef>,
    /// Citation definitions extracted from block `cite` attributes.
    pub citations: Vec<crate::citations::CitationDef>,
    /// Raw markdown of the guide's required CONDUITS.md — the conduit-facing
    /// guide frame, materialized onto the guide version row (never a chapter,
    /// never embedded).
    pub conduits_md: String,
    /// Validation warnings (non-fatal issues found during planning).
    pub warnings: Vec<String>,
}

// ============================================================================
// Chapter definition (mirrors ParsedChapter for the ingest plan)
// ============================================================================

/// A chapter in the ingest plan, carrying its blocks and hierarchy info.
#[derive(Clone, Debug)]
pub struct ChapterDef {
    pub title: String,
    pub subtitle: Option<String>,
    pub source_file: String,
    pub parent_index: Option<usize>,
    pub blocks: Vec<BlockDef>,
}

/// A block in the ingest plan.
///
/// Under the Fluent-native model, blocks with localized text have `content`
/// set to `None`. Blocks with inline content (nushell scripts) retain it.
#[derive(Clone, Debug)]
pub struct BlockDef {
    pub directive_type: String,
    pub name: Option<String>,
    /// Raw content for non-Fluent blocks (nushell scripts). None for
    /// Fluent-backed blocks whose text lives in .ftl files.
    pub content: Option<String>,
    pub attributes: serde_json::Value,
    pub tags: Vec<String>,
}

// ============================================================================
// Derived object definition types (pure data, not database models)
// ============================================================================

/// A playbook extracted from :::stage/:::step hierarchy within a single chapter.
///
/// Each chapter that contains :::stage blocks produces its own PlaybookDef.
/// When only one chapter has stages, the playbook name is the guide title.
/// When multiple chapters have stages, the name is "guide title — chapter title".
#[derive(Clone, Debug)]
pub struct PlaybookDef {
    /// Playbook name (guide title, or "guide title — chapter title" for multi-playbook guides).
    pub name: String,
    /// Index of the source chapter in the parsed guide (for provenance and junction linkage).
    pub chapter_index: usize,
    /// Ordered stages within the playbook.
    pub stages: Vec<StageDef>,
}

/// A stage within a playbook (Fluent-native).
///
/// Description text lives in the FluentMessage body, not inline.
#[derive(Clone, Debug)]
pub struct StageDef {
    /// Stage name from directive attributes.
    pub name: String,
    // description removed -- lives in FluentMessage body
    /// Authored `kind:` attribute — a closed stage-kind vocabulary key
    /// (`rev.stage_kinds`). Carried raw here; the parser stays permissive and
    /// validation (warning + stored-as-absent on unknown) happens at ingest.
    pub kind: Option<String>,
    /// Authored `icon:` attribute — an OPEN vocabulary of icon names owned by
    /// the drawing client, in authored order. Accepts an array or a bare
    /// string. Never validated against a vocabulary at any layer: an unknown
    /// name is a client-side degrade to the `kind` glyph, not an error here.
    /// Ingest warns only about the length cap the client documents.
    pub icon: Vec<String>,
    /// Optional flow control (expanded JSONLogic).
    pub flow_control: Option<serde_json::Value>,
    /// Value-slot names the stage exposes (materialized as stage overlays with
    /// visibility='expose'). These are the slots the stage's tasks need.
    pub expose: Vec<String>,
    /// Value-slot names the stage masks (visibility='mask'); overrides an
    /// expose inherited from a broader scope via the resolution cascade.
    pub mask: Vec<String>,
    /// Ordered steps within this stage.
    pub steps: Vec<StepDef>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A step within a stage (Fluent-native).
///
/// Description text lives in the FluentMessage body, not inline.
#[derive(Clone, Debug)]
pub struct StepDef {
    /// Step name from directive attributes.
    pub name: String,
    // description removed -- lives in FluentMessage body
    /// Optional flow control (expanded JSONLogic).
    pub flow_control: Option<serde_json::Value>,
    /// Value-slot names the step exposes (materialized as stage_step overlays
    /// with visibility='expose').
    pub expose: Vec<String>,
    /// Value-slot names the step masks (visibility='mask'); overrides a
    /// stage-level expose for this step's tasks.
    pub mask: Vec<String>,
    /// Names of :::nushell blocks to link as the step's tasks, in authored
    /// order. A step is an ordered, fail-together set of tasks; this is empty
    /// for a task-less step and has one element for the common single-task case.
    pub task_refs: Vec<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A nushell task extracted from a :::nushell block.
#[derive(Clone, Debug)]
pub struct TaskDef {
    /// Task name from directive attributes.
    pub name: String,
    /// The nushell script content.
    pub script: String,
    /// Role of this nushell block: "default" for standalone task,
    /// "directive" for flow control attachment.
    pub role: String,
    /// If role is "directive", the name of the step/stage this attaches to.
    pub target: Option<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A brief pack extracted from a :::brief_pack block (Fluent-native).
///
/// Content text and card subtitle/category live in FluentMessage body
/// and attributes, not inline.
#[derive(Clone, Debug)]
pub struct BriefPackDef {
    /// Brief pack name from directive attributes.
    pub name: String,
    // content removed -- lives in FluentMessage body
    // card_subtitle and card_category extracted from Fluent attributes
    /// Ordered group names referenced by this brief pack.
    pub group_names: Vec<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A value slot definition extracted from a :::value_slot block.
///
/// Defines a typed data slot that brief inputs/outputs can reference.
#[derive(Clone, Debug)]
pub struct ValueSlotDef {
    /// Value slot name from directive attributes.
    pub name: String,
    /// The value kind: text, int, number, bool, json, asset.
    pub kind: String,
    /// Optional constraints as JSON (validation rules, ranges, etc.).
    pub constraints: Option<serde_json::Value>,
    /// Selection cardinality: `"single"` (default, exactly one item) or
    /// `"multi"` (a keep-N subset of N typed items of `kind`).
    pub cardinality: String,
    /// For `multi`: minimum items the operator must keep (None = no floor;
    /// `0` allows keeping none). Always None for `single`.
    pub min_selections: Option<i32>,
    /// For `multi`: maximum items the operator may keep (None = unbounded).
    /// Always None for `single`.
    pub max_selections: Option<i32>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A brief input definition extracted from a :::brief_input block.
///
/// Defines an input control within a brief unit.
#[derive(Clone, Debug)]
pub struct BriefInputDef {
    /// Brief input name from directive attributes.
    pub name: String,
    /// Input control type. A free string validated against the control-type
    /// catalog at ingest (text, textarea, select, radio, checkbox, number, date,
    /// file, repeater, …) — the set is open, not closed here.
    pub control: String,
    /// Optional value slot name this input writes to.
    pub slot: Option<String>,
    /// Optional name of a computed value slot supplying this control's choices
    /// at runtime (the dynamic generate→curate→refine candidate source). When
    /// set, the client reads the candidate tiles from that slot's bound value
    /// instead of from authored static choices.
    pub candidate_source: Option<String>,
    /// Author-controlled opt-out from the required default for single-value
    /// controls (`text`/`textarea`/`number`/`date`). `true` ⇒ the operator may
    /// submit the field blank. Absent in the directive ⇒ `false` (required) —
    /// unchanged behavior for every existing input. Keep-N `checkbox`
    /// cardinality stays governed by the slot's `min`/`max`, not this flag.
    pub optional: bool,
    /// Per-item field schema for a `repeater` control: a raw JSON array of
    /// `{ key, control }` describing the editable fields of each list record.
    /// `None` for every non-repeater control. Ingest resolves per-field copy
    /// from the unit's Fluent attributes and bakes it into `control_spec`.
    pub item_fields: Option<serde_json::Value>,
    /// Per-item refine wiring (a `repeater` control). `refine_input` names the
    /// LOCATOR slot the dedicated refine step exposes; the client matches its
    /// `value_slot_id` in `/steps` to find the refine step, then runs an on-demand
    /// refine dispatch whose `{ record, notes }` ride in the dispatch intent (the
    /// slot itself is never bound). Ingest resolves the name to its `value_slot_id`
    /// in `control_spec`. `None` when the repeater offers no per-item refine.
    pub refine_input: Option<String>,
    /// Axis presentation config for a `timeline` control: a raw JSON object of
    /// `{ unit?, allowAbsolute? }` (e.g. `unit: "minutes"` for a talk built over
    /// x minutes; `allowAbsolute` lets an entry pin to an absolute axis position
    /// rather than sequencing). Carried opaquely and baked into `control_spec`;
    /// the axis renderer lives in dapple-ui. `None` for non-timeline controls.
    pub axis: Option<serde_json::Value>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A brief notice definition extracted from a :::brief_notice block.
///
/// Defines a notice/information unit within a brief.
#[derive(Clone, Debug)]
pub struct BriefNoticeDef {
    /// Brief notice name from directive attributes.
    pub name: String,
    /// Notice level: info, warning, success.
    pub level: String,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A brief markdown definition extracted from a :::brief_markdown block.
///
/// Defines a display-only prose unit within a brief. Content is stored inline
/// in the directive body (not via fluent messages), since brief markdown is
/// typically multi-paragraph narrative text authored alongside the brief.
#[derive(Clone, Debug)]
pub struct BriefMarkdownDef {
    /// Brief markdown name from directive attributes.
    pub name: String,
    /// Inline markdown body from the directive fence.
    pub markdown_text: String,
    /// Value-slot names this markdown body interpolates via `$slot` placeholders
    /// (the `interpolates: [...]` attribute). A non-empty list marks this unit as
    /// a *deliverable*: at brief-render time the server substitutes each named
    /// slot's bound value into the body (see `dapple_fluent::substitute`).
    /// Empty ⇒ ordinary display-only prose. Listing the slots explicitly (rather
    /// than scanning the body) keeps resolution deterministic, drives the brief
    /// content-hash fold, and bounds substitution.
    pub interpolates: Vec<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A brief output definition extracted from a :::brief_output block.
///
/// Defines an output display unit within a brief.
#[derive(Clone, Debug)]
pub struct BriefOutputDef {
    /// Brief output name from directive attributes.
    pub name: String,
    /// Optional value slot name this output reads from.
    pub slot: Option<String>,
    /// Output source kind: input, files, computed.
    pub source: String,
    /// RFC-6901 JSON pointer into the producing task's payload for `computed`
    /// outputs (e.g. `/choices`). `None` or `"."`/empty = the whole payload
    /// root. Threaded through to `rev.brief_outputs.source_path`; the runtime
    /// producer extracts the bound value from the payload at this path.
    pub source_path: Option<String>,
    /// Typed display hint (how a client renders the bound value):
    /// image/text/markdown/list/checklist/table/fields/hidden. Default `text`.
    pub display: String,
    /// Collapse the rendered block behind a "Reveal" spoiler toggle.
    pub reveal: bool,
    /// Heading label for the block (None = no heading).
    pub title: Option<String>,
    /// Sibling-format grouping token: outputs sharing a `group` are the SAME
    /// deliverable in different formats (source markdown + HTML + PDF), which
    /// lets a client render one block with a format switcher rather than
    /// unrelated sections. Opaque — any stable string the author picks, never
    /// parsed. `None` = ungrouped (the default, and every pre-grouping guide).
    pub group: Option<String>,
    /// Short label naming this output's format within its `group` (`"html"`,
    /// `"pdf"`, …). Deliberately an OPEN vocabulary, unlike `display`: it is a
    /// button label, and a closed set would reject a format an author coins
    /// (`"epub"`) — the exact failure that name-parsing grouping had.
    pub format: Option<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A brief definition extracted from a :::brief block.
///
/// Groups brief units (inputs, notices, outputs) into a single brief.
#[derive(Clone, Debug)]
pub struct BriefDef {
    /// Brief name from directive attributes.
    pub name: String,
    /// Ordered unit names referenced by this brief.
    pub unit_names: Vec<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A brief group definition extracted from a :::brief_group block.
///
/// Groups briefs into an ordered collection.
#[derive(Clone, Debug)]
pub struct BriefGroupDef {
    /// Brief group name from directive attributes.
    pub name: String,
    /// Ordered brief names referenced by this group.
    pub brief_names: Vec<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// An inference profile extracted from an :::inference_profile block (Fluent-native).
///
/// Description, system_prompt, temperature, max_tokens are extracted from
/// FluentMessage body and attributes, not from inline directive attributes.
#[derive(Clone, Debug)]
pub struct InferenceProfileDef {
    /// Profile name from directive attributes.
    pub name: String,
    // description removed -- lives in FluentMessage body
    // system_prompt, temperature, max_tokens extracted from Fluent attributes
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A cross-reference extracted from a :::ref block.
#[derive(Clone, Debug)]
pub struct RefDef {
    /// The slug being referenced.
    pub slug: String,
    /// The type of object being referenced (e.g., "guide", "playbook").
    pub ref_type: String,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A scope definition extracted from a :::scope block.
///
/// Scope blocks carry chapter-level authoring metadata (audience, goals,
/// boundaries, tone) as inline key:value content. Not materialized to
/// the database — used by Studio to build prompt context.
#[derive(Clone, Debug)]
pub struct ScopeDef {
    /// Scope name from directive attributes.
    pub name: String,
    /// Target audience description.
    pub audience: Option<String>,
    /// Learning/workflow goals.
    pub goals: Option<String>,
    /// What is explicitly out of scope.
    pub boundaries: Option<String>,
    /// Tone and depth guidance.
    pub tone: Option<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// An objective definition extracted from an :::objective block.
///
/// Objectives carry a statement and optional expectations as inline
/// structured content. Not materialized to the database — used by
/// Studio to build prompt context.
#[derive(Clone, Debug)]
pub struct ObjectiveDef {
    /// Objective name from directive attributes.
    pub name: String,
    /// The objective statement (free text before "expectations:").
    pub statement: String,
    /// Expected outcomes (list items after "expectations:").
    pub expectations: Vec<String>,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A deliverable definition extracted from a :::deliverable block.
///
/// Deliverables describe tangible outputs. Not materialized to the
/// database — used by Studio to build prompt context.
#[derive(Clone, Debug)]
pub struct DeliverableDef {
    /// Deliverable name from directive attributes.
    pub name: String,
    /// Description of the deliverable (full block body).
    pub description: String,
    /// Source location for provenance tracking.
    pub source_block_index: BlockIndex,
}

/// A file entry discovered in the guide directory for asset upload.
#[derive(Clone, Debug)]
pub struct AssetFileEntry {
    /// Relative path within the guide directory (e.g., "assets/images/card.jpg").
    pub relative_path: String,
    /// MIME type inferred from file extension.
    pub mime_type: String,
    /// Raw file bytes.
    pub data: Vec<u8>,
}

/// Card definition for the promotional card (Fluent-native).
///
/// Subtitle and category are now Fluent attributes on the brief-pack
/// message. Only locale-independent visual properties remain here.
#[derive(Clone, Debug)]
pub struct CardDef {
    /// Relative path to the card image within the guide directory.
    pub image: Option<String>,
    pub color: Option<String>,
    pub accent: Option<String>,
}

/// Location within the parsed guide for provenance tracking.
#[derive(Clone, Debug)]
pub struct BlockIndex {
    pub chapter_index: usize,
    pub block_index: usize,
}

/// A set of FluentMessage definitions for a single message key across locales.
///
/// Each guide block name maps to one FluentMessageSet. During ingest, each
/// set is materialized as a FluentMessage path + version with localized bodies.
#[derive(Clone, Debug)]
pub struct FluentMessageSet {
    /// The message key (matches directive name).
    pub key: String,
    /// Description (from the first locale's body, or explicit).
    pub description: Option<String>,
    /// Localized bodies keyed by locale tag.
    pub bodies: HashMap<String, String>,
    /// Fluent attributes keyed by locale, then by attribute name.
    pub attributes: HashMap<String, HashMap<String, String>>,
}

// ============================================================================
// plan_guide -- the main transform entry point
// ============================================================================

/// Transform a `ParsedGuide` into an `IngestPlan`.
///
/// This is the main entry point for the transform layer. It walks the parsed
/// guide's chapter/block tree and extracts all derived objects:
///
/// - :::stage/:::step blocks become a `PlaybookDef`
/// - :::nushell blocks become `TaskDef`s
/// - :::brief_pack blocks become `BriefPackDef`s
/// - :::inference_profile blocks become `InferenceProfileDef`s
/// - :::ref blocks become `RefDef`s
/// - guide.toml [card] section becomes a `CardDef`
/// - Non-markdown files become `AssetFileEntry`s
/// - locales/*.ftl files become `FluentMessageSet`s
///
/// The `guide_dir` parameter is needed to read asset file bytes.
pub fn plan_guide(
    parsed: &ParsedGuide,
    guide_dir: &std::path::Path,
) -> Result<IngestPlan, ParseError> {
    let mut warnings = Vec::new();

    // 1. Extract card metadata
    let card = card::extract_card(&parsed.metadata);

    // 2. Collect asset files from the directory
    let asset_files = asset::collect_asset_files(guide_dir, &parsed.asset_paths, &mut warnings);

    // 3. Build chapter definitions with block definitions
    let chapters = build_chapter_defs(parsed);

    // 3b. Discover prompt files from prompts/ directory
    let derived_prompts = crate::prompt_files::discover_prompt_files(guide_dir);

    // 4. Extract derived objects by walking all blocks
    let mut derived_tasks = Vec::new();
    let mut derived_brief_packs = Vec::new();
    let mut derived_inference_profiles = Vec::new();
    let mut derived_refs = Vec::new();
    let mut derived_scopes = Vec::new();
    let mut derived_objectives = Vec::new();
    let mut derived_deliverables = Vec::new();
    let mut derived_value_slots = Vec::new();
    let mut derived_brief_inputs = Vec::new();
    let mut derived_brief_notices = Vec::new();
    let mut derived_brief_markdowns = Vec::new();
    let mut derived_brief_outputs = Vec::new();
    let mut derived_briefs = Vec::new();
    let mut derived_brief_groups = Vec::new();

    for (ch_idx, chapter) in parsed.chapters.iter().enumerate() {
        for (blk_idx, block) in chapter.blocks.iter().enumerate() {
            extract_derived_from_block(
                block,
                ch_idx,
                blk_idx,
                None,
                &parsed.script_files,
                &mut derived_tasks,
                &mut derived_brief_packs,
                &mut derived_inference_profiles,
                &mut derived_refs,
                &mut derived_scopes,
                &mut derived_objectives,
                &mut derived_deliverables,
                &mut derived_value_slots,
                &mut derived_brief_inputs,
                &mut derived_brief_notices,
                &mut derived_brief_markdowns,
                &mut derived_brief_outputs,
                &mut derived_briefs,
                &mut derived_brief_groups,
                &mut warnings,
            );
        }
    }

    // 5. Extract playbooks from stage/step hierarchy (one per chapter with stages)
    let derived_playbooks = playbook::extract_playbooks(parsed, &mut warnings);

    // 6. Link directive nushell blocks to their target steps/stages
    task::link_directive_tasks(&derived_tasks, &derived_playbooks, &mut warnings);

    // 6b. Guardrail: a value-slot that a stage/step `expose`s but that NO
    //     :::brief-input collects and NO :::brief-output produces can never be
    //     bound — its stage gates on it forever (never reaches `done`) and an
    //     operator "pick" into it has no `value_slot_id` to bind against. This is
    //     the systematic generate → pick → refine authoring bug: pick slots were
    //     declared (:::value_slot) and exposed but never given a :::brief-input
    //     pin. WARN ONLY (never a hard error): the materializer's
    //     `validate_overlay_slot_references` deliberately leaves this coherence
    //     check out because a few shapes legitimately expose an uncollected slot
    //     (overlay-only authoring for agent context, or a slot bound by another
    //     surface). A warning surfaces the near-certain bug in the dry-run
    //     summary + ingest logs without breaking those edge cases.
    warn_uncollectable_exposed_slots(
        &derived_playbooks,
        &derived_brief_inputs,
        &derived_brief_outputs,
        &mut warnings,
    );

    // 6b'. Companion guardrail: a :::brief-output that parsed but is not listed
    //      in any :::brief's `units` is silently dropped at materialization (the
    //      brief→output association is by name via `units`). This is the trap
    //      behind "the assembly task succeeds but the result never surfaces".
    warn_unwired_brief_outputs(&derived_briefs, &derived_brief_outputs, &mut warnings);

    // 6c. HARD wiring validation of the pack → group → brief chain. Every
    //     runtime read walks journey → pinned pack → groups → briefs, and the
    //     materializer only warn-and-skips dangling names, so a break in the
    //     chain's middle links ships a guide that materializes green and then
    //     stalls live: the computed producer binds nothing, pickers have no
    //     candidates and no render spec, and the DONE rollup cannot see the
    //     stage (retort-the-n-port "Read the Portfolio as a Company",
    //     2026-08-02). Unlike the soft coherence warnings above, an orphaned
    //     group/brief or an unreachable collector has no legitimate authoring
    //     shape — so this one is an ERROR, not a warning.
    validate_brief_wiring(
        &derived_brief_packs,
        &derived_brief_groups,
        &derived_briefs,
        &derived_brief_inputs,
        &derived_brief_outputs,
        &derived_playbooks,
    )?;

    // 6b. Cardinality coherence: a checkbox collects a multi slot; a
    //     candidate_source names a declared slot; a multi slot is collected by
    //     a checkbox. WARN ONLY (same soft-coherence rationale as above).
    warn_cardinality_coherence(&derived_value_slots, &derived_brief_inputs, &mut warnings);

    // 7. Build FluentMessageSets from the parsed locales
    let fluent_messages = build_fluent_message_sets(&parsed.locales);

    // 8. Parse resources.md and extract citations from blocks
    let resources = crate::citations::parse_resources_file(guide_dir);
    let citations = crate::citations::extract_citations_from_blocks(&chapters);

    // 9. Carry the conduit frame; an empty file is legal but almost certainly
    //    an authoring gap — the conduit would receive an empty guide frame.
    if parsed.conduits_md.trim().is_empty() {
        warnings.push(
            "CONDUITS.md is empty — the conduit receives an empty guide frame. \
             Describe the guide's bindings and how to read its agent-facing prose."
                .to_string(),
        );
    }

    Ok(IngestPlan {
        title: parsed.metadata.title.clone(),
        description: parsed.metadata.description.clone(),
        author: parsed.metadata.author.clone(),
        classification: parsed.metadata.classification.clone(),
        source_path: parsed.metadata.source_path.clone(), // from guide.toml; handler may override via ?source_path= query param
        chapters,
        card,
        asset_files,
        derived_brief_packs,
        derived_playbooks,
        derived_tasks,
        derived_inference_profiles,
        derived_refs,
        derived_scopes,
        derived_objectives,
        derived_deliverables,
        derived_value_slots,
        derived_brief_inputs,
        derived_brief_notices,
        derived_brief_markdowns,
        derived_brief_outputs,
        derived_briefs,
        derived_brief_groups,
        derived_prompts,
        resources,
        citations,
        fluent_messages,
        deprecation_level: parsed.metadata.deprecation_level.clone(),
        deprecation_reason: parsed.metadata.deprecation_reason.clone(),
        conduits_md: parsed.conduits_md.clone(),
        warnings,
    })
}

/// Emit a warning for every value-slot that is `expose`d by a stage or step
/// overlay but is collected by NO `:::brief-input` and produced by NO
/// `:::brief-output`. Such a slot is unbindable: its stage gates on it forever
/// (it can never reach `done`) and an operator pick into it has no
/// `value_slot_id` to bind against. The canonical trigger is a generate → pick →
/// refine "pick" slot that was declared (`:::value_slot`) and exposed but never
/// given a `:::brief-input` to pin it.
///
/// Collectability is guide-wide: a slot is fine if ANY brief-input/output in the
/// guide targets it. The exposed set is the simple union of stage/step `expose`
/// lists; `mask` refinements are not modeled here, so a slot exposed and then
/// fully masked everywhere (rare) would be a benign false positive.
///
/// This is intentionally a WARNING, not a hard error. The materializer's
/// `validate_overlay_slot_references` (dapple-prompt-api) documents that a stage
/// may *legitimately* expose a slot with no brief-input (overlay-only authoring
/// for agent context, or a slot bound by another surface), so the precise
/// coherence rule is deliberately soft. The dominant real case is the
/// generate → pick → refine bug (a pick slot never given a `:::brief-input`),
/// which a warning surfaces without breaking the legitimate shapes.
fn warn_uncollectable_exposed_slots(
    playbooks: &[PlaybookDef],
    brief_inputs: &[BriefInputDef],
    brief_outputs: &[BriefOutputDef],
    warnings: &mut Vec<String>,
) {
    use std::collections::BTreeSet;

    // Slots some brief unit can supply (a brief-input collects, a brief-output
    // produces — any source kind).
    let collectable: BTreeSet<&str> = brief_inputs
        .iter()
        .filter_map(|i| i.slot.as_deref())
        .chain(brief_outputs.iter().filter_map(|o| o.slot.as_deref()))
        .collect();

    // Deduped union of every exposed slot across all stages and steps; only the
    // ones with no collector are reported (sorted, one warning each).
    let mut uncollectable: BTreeSet<&str> = BTreeSet::new();
    for pb in playbooks {
        for stage in &pb.stages {
            for slot in &stage.expose {
                if !collectable.contains(slot.as_str()) {
                    uncollectable.insert(slot.as_str());
                }
            }
            for step in &stage.steps {
                for slot in &step.expose {
                    if !collectable.contains(slot.as_str()) {
                        uncollectable.insert(slot.as_str());
                    }
                }
            }
        }
    }

    for slot in uncollectable {
        warnings.push(format!(
            "value-slot '{slot}' is exposed (stage/step overlay) but has no \
             :::brief-input or :::brief-output to collect or produce it — it can \
             never be bound, so its stage will never report done. Add a \
             :::brief-input (operator pick/answer) or :::brief-output (computed)."
        ));
    }
}

/// Emit a warning for every `:::brief-output` that parsed (it has a `name`) but
/// is not referenced by any `:::brief`'s `units` list.
///
/// The materializer associates an output to its brief *solely* by matching the
/// output's name against each brief's authored `units` list (the output is
/// inserted only while walking the owning brief's `unit_names`). An output that
/// no brief lists is therefore **silently dropped**: it never becomes a
/// `rev.brief_outputs` row, so a `computed` output authored this way never
/// produces a binding and its stage can never report `done`. This is the exact
/// trap behind "the assembly task succeeds but the result never surfaces" — the
/// `:::brief-output` was declared but never wired into the brief.
///
/// WARN-only, consistent with [`warn_uncollectable_exposed_slots`]: the
/// `:::brief_output` block with no `name` is already warned at extraction time,
/// and a hard error here would break the ingest of guides already carrying this
/// latent gap. The warning surfaces in the dry-run summary and ingest logs.
fn warn_unwired_brief_outputs(
    briefs: &[BriefDef],
    brief_outputs: &[BriefOutputDef],
    warnings: &mut Vec<String>,
) {
    use std::collections::BTreeSet;

    let wired: BTreeSet<&str> = briefs
        .iter()
        .flat_map(|b| b.unit_names.iter().map(String::as_str))
        .collect();

    for output in brief_outputs {
        if !wired.contains(output.name.as_str()) {
            warnings.push(format!(
                ":::brief-output '{name}' is not listed in any :::brief's `units`, \
                 so it will not be materialized (brief→output association is by \
                 name via the brief's `units` list). Add '{name}' to the owning \
                 brief's `units`.",
                name = output.name
            ));
        }
    }
}

/// Hard-error validation of the brief wiring chain: pack → group → brief.
///
/// Every runtime read walks journey → pinned pack → groups → briefs, so a
/// brief system authored outside that chain is INVISIBLE at runtime while
/// materializing "successfully" as dead rows: the computed-output producer
/// binds nothing, a picker has no candidates and no render spec, and the DONE
/// rollups cannot see the stage. The materializer only warn-and-skips dangling
/// names (log-only), and [`warn_uncollectable_exposed_slots`] is guide-wide
/// (it passes when a collector exists but is unreachable), so this is the only
/// loud check on the chain's middle links.
///
/// All problems are collected into ONE error so the author fixes everything in
/// a single pass:
/// 1. a `:::brief-group` not listed in any `:::brief-pack`'s `groups`
/// 2. a `:::brief` not listed in any `:::brief-group`'s `briefs`
/// 3. briefs/groups authored but no `:::brief-pack` declared at all
/// 4. dangling references — a pack naming an unknown group, a group naming an
///    unknown brief (the materializer would silently skip the association)
/// 5. an exposed slot whose collector (`:::brief-input`/`:::brief-output`)
///    exists guide-wide but none of its collectors is reachable via
///    pack → group → brief. (Collector-missing-entirely stays a WARNING —
///    see [`warn_uncollectable_exposed_slots`] for the legitimate shapes; a
///    collector that EXISTS but cannot be reached is always broken wiring.)
fn validate_brief_wiring(
    brief_packs: &[BriefPackDef],
    brief_groups: &[BriefGroupDef],
    briefs: &[BriefDef],
    brief_inputs: &[BriefInputDef],
    brief_outputs: &[BriefOutputDef],
    playbooks: &[PlaybookDef],
) -> Result<(), ParseError> {
    use std::collections::BTreeSet;

    let mut problems: Vec<String> = Vec::new();

    // 3. Brief system authored but no pack to reach it through.
    if (!briefs.is_empty() || !brief_groups.is_empty()) && brief_packs.is_empty() {
        problems.push(
            "briefs/brief-groups are authored but no :::brief-pack is declared — \
             without a pack none of them can ever surface at runtime. Declare a \
             :::brief-pack listing the groups in stage order."
                .to_string(),
        );
    }

    let packed_groups: BTreeSet<&str> = brief_packs
        .iter()
        .flat_map(|p| p.group_names.iter().map(String::as_str))
        .collect();
    let grouped_briefs: BTreeSet<&str> = brief_groups
        .iter()
        .flat_map(|g| g.brief_names.iter().map(String::as_str))
        .collect();
    let group_names: BTreeSet<&str> = brief_groups.iter().map(|g| g.name.as_str()).collect();
    let brief_names: BTreeSet<&str> = briefs.iter().map(|b| b.name.as_str()).collect();

    // 1. Orphaned groups (only when a pack exists; with no pack at all,
    //    problem 3 above already names the fix once instead of per-group).
    if !brief_packs.is_empty() {
        for group in brief_groups {
            if !packed_groups.contains(group.name.as_str()) {
                problems.push(format!(
                    ":::brief-group '{name}' is not listed in any :::brief-pack's \
                     `groups`, so its briefs are invisible to every runtime read. \
                     Add '{name}' to the pack's `groups` list in stage order.",
                    name = group.name
                ));
            }
        }
    }

    // 2. Orphaned briefs (same guard rationale as above).
    if !brief_groups.is_empty() {
        for brief in briefs {
            if !grouped_briefs.contains(brief.name.as_str()) {
                problems.push(format!(
                    ":::brief '{name}' is not listed in any :::brief-group's \
                     `briefs`, so it is invisible to every runtime read. Add \
                     '{name}' to its group's `briefs` list.",
                    name = brief.name
                ));
            }
        }
    }

    // 4. Dangling references.
    for pack in brief_packs {
        for gname in &pack.group_names {
            if !group_names.contains(gname.as_str()) {
                problems.push(format!(
                    ":::brief-pack '{pack}' lists group '{gname}' but no \
                     :::brief-group with that name exists.",
                    pack = pack.name
                ));
            }
        }
    }
    for group in brief_groups {
        for bname in &group.brief_names {
            if !brief_names.contains(bname.as_str()) {
                problems.push(format!(
                    ":::brief-group '{group}' lists brief '{bname}' but no \
                     :::brief with that name exists.",
                    group = group.name
                ));
            }
        }
    }

    // 5. Exposed slots whose collectors all sit outside the pack walk.
    if !brief_packs.is_empty() {
        let reachable_briefs: BTreeSet<&str> = brief_groups
            .iter()
            .filter(|g| packed_groups.contains(g.name.as_str()))
            .flat_map(|g| g.brief_names.iter().map(String::as_str))
            .collect();
        let reachable_units: BTreeSet<&str> = briefs
            .iter()
            .filter(|b| reachable_briefs.contains(b.name.as_str()))
            .flat_map(|b| b.unit_names.iter().map(String::as_str))
            .collect();

        let mut exposed: BTreeSet<&str> = BTreeSet::new();
        for pb in playbooks {
            for stage in &pb.stages {
                exposed.extend(stage.expose.iter().map(String::as_str));
                for step in &stage.steps {
                    exposed.extend(step.expose.iter().map(String::as_str));
                }
            }
        }

        for slot in exposed {
            let collectors: Vec<&str> = brief_inputs
                .iter()
                .filter(|i| i.slot.as_deref() == Some(slot))
                .map(|i| i.name.as_str())
                .chain(
                    brief_outputs
                        .iter()
                        .filter(|o| o.slot.as_deref() == Some(slot))
                        .map(|o| o.name.as_str()),
                )
                .collect();
            // No collector anywhere -> warn_uncollectable_exposed_slots covers
            // it (legitimate overlay-only shapes exist).
            if collectors.is_empty() {
                continue;
            }
            if !collectors.iter().any(|c| reachable_units.contains(c)) {
                problems.push(format!(
                    "value-slot '{slot}' is exposed (stage/step overlay) and has \
                     collector(s) [{collectors}], but none is reachable via \
                     pack → group → brief, so the slot can never be bound at \
                     runtime. Wire the owning brief's group into the pack (or \
                     the collector into a wired brief's `units`).",
                    collectors = collectors.join(", ")
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(ParseError::UnwiredBriefSystem {
            detail: problems.join("\n"),
        })
    }
}

/// Soft coherence checks for the multi-select (keep-N) authoring surface:
///
/// 1. A `checkbox` brief-input should write a `multi` value slot — `checkbox`
///    is the multi control, so pairing it with a `single` slot is almost
///    certainly an authoring slip (the operator could check several tiles but
///    only one would survive the single-item bind validation).
/// 2. A `multi` value slot should be collected by a `checkbox` brief-input —
///    any other control renders a single-value widget and cannot express the
///    subset.
/// 3. A `candidate_source` must name a value slot the guide declares; otherwise
///    the client has nowhere to read the runtime candidate tiles from.
///
/// All WARN-only, matching [`warn_uncollectable_exposed_slots`]: a slot may be
/// bound by another surface or produced by a computed output, so none of these
/// is a hard error.
fn warn_cardinality_coherence(
    value_slots: &[ValueSlotDef],
    brief_inputs: &[BriefInputDef],
    warnings: &mut Vec<String>,
) {
    use std::collections::BTreeMap;

    let slot_cardinality: BTreeMap<&str, &str> = value_slots
        .iter()
        .map(|vs| (vs.name.as_str(), vs.cardinality.as_str()))
        .collect();

    // Controls that render a subset/list selector — they collect MORE than one
    // item, so a `multi` (keep-N) slot needs one of them and a `single` slot bound
    // by one is the reverse mismatch. `checkbox` plus the multi-value builders
    // (`repeater`/`timeline`/`list_builder` accumulate a list; `table` is a
    // repeater rendered as a grid; `conversation` grows a turn list). Every
    // other control (text, textarea, select, radio) is single-value.
    const SUBSET_SELECTOR_CONTROLS: [&str; 6] = [
        "checkbox",
        "repeater",
        "timeline",
        "list_builder",
        "conversation",
        "table",
    ];

    for input in brief_inputs {
        let is_subset_selector = SUBSET_SELECTOR_CONTROLS
            .iter()
            .any(|c| input.control.eq_ignore_ascii_case(c));

        if let Some(slot) = input.slot.as_deref() {
            match slot_cardinality.get(slot) {
                Some(&"multi") if !is_subset_selector => warnings.push(format!(
                    ":::brief-input '{}' writes the multi (keep-N) value-slot \
                     '{slot}' with control '{}', but only a subset selector \
                     (checkbox, repeater, timeline, list_builder, conversation, \
                     table) collects more than one item — a single-value control \
                     will collapse the pick to one item.",
                    input.name, input.control
                )),
                Some(&"single") if is_subset_selector => warnings.push(format!(
                    ":::brief-input '{}' uses subset-selector control '{}' but its \
                     value-slot '{slot}' is 'single' — a subset selector collects \
                     multiple items, so the slot should declare cardinality: \"multi\".",
                    input.name, input.control
                )),
                _ => {}
            }
        }

        if let Some(cs) = input.candidate_source.as_deref()
            && !slot_cardinality.contains_key(cs)
        {
            warnings.push(format!(
                ":::brief-input '{}' references candidate_source '{cs}', but no \
                 :::value_slot named '{cs}' is declared — the client has no slot \
                 to read the runtime candidate tiles from. Declare the \
                 candidate-source value slot (a computed json slot).",
                input.name
            ));
        }
    }
}

/// Recursively flatten a `ParsedBlock` tree into `BlockDef`s in depth-first order.
///
/// Stage blocks contain step children, and step blocks may contain nushell
/// children. This flattener ensures all nested blocks appear in the chapter's
/// flat block list so they are materialized into the database.
fn flatten_block_to_defs(block: &ParsedBlock) -> Vec<BlockDef> {
    let content = match block.directive_type.as_str() {
        "nushell" | "ref" | "scope" | "objective" | "deliverable" => Some(block.content.clone()),
        // Brief structure directives are metadata-only; their structure
        // lives in attributes, not content.
        "brief_input" | "brief_notice" | "brief_output" | "brief" | "brief_group"
        | "value_slot" => None,
        _ => {
            if block.content.trim().is_empty() {
                None
            } else {
                Some(block.content.clone())
            }
        }
    };

    let mut result = vec![BlockDef {
        directive_type: block.directive_type.clone(),
        name: block.name.clone(),
        content,
        attributes: block.attributes.clone(),
        tags: block.tags.clone(),
    }];

    for child in &block.children {
        result.extend(flatten_block_to_defs(child));
    }

    result
}

/// Build chapter definitions from the parsed guide.
///
/// Under the Fluent-native model, blocks that have localizable content
/// (stage, step, prose, brief_pack, inference_profile) store `content`
/// as `None`. Blocks with inline code content (nushell) retain it.
///
/// Nested blocks (e.g. stage → step → nushell) are flattened into a
/// single depth-first list so all blocks are stored in the database.
fn build_chapter_defs(parsed: &ParsedGuide) -> Vec<ChapterDef> {
    parsed
        .chapters
        .iter()
        .map(|ch| {
            let subtitle = parsed
                .metadata
                .chapters
                .get(&ch.source_file)
                .and_then(|meta| meta.subtitle.clone());
            ChapterDef {
                title: ch.title.clone(),
                subtitle,
                source_file: ch.source_file.clone(),
                parent_index: ch.parent_index,
                blocks: ch.blocks.iter().flat_map(flatten_block_to_defs).collect(),
            }
        })
        .collect()
}

/// Build FluentMessageSets from the parsed locales.
///
/// Groups all locale message definitions by message key to produce a
/// single FluentMessageSet per unique key across all locales.
fn build_fluent_message_sets(
    locales: &HashMap<String, Vec<crate::types::FluentMessageDef>>,
) -> HashMap<String, FluentMessageSet> {
    let mut sets: HashMap<String, FluentMessageSet> = HashMap::new();

    for (locale, messages) in locales {
        for msg_def in messages {
            let set = sets
                .entry(msg_def.key.clone())
                .or_insert_with(|| FluentMessageSet {
                    key: msg_def.key.clone(),
                    description: None,
                    bodies: HashMap::new(),
                    attributes: HashMap::new(),
                });

            // Set description from the first locale's body if not yet set
            if set.description.is_none() && !msg_def.body.is_empty() {
                // Use the first ~200 chars of the body as description
                let desc = if msg_def.body.chars().count() > 200 {
                    let truncated: String = msg_def.body.chars().take(200).collect();
                    format!("{}...", truncated)
                } else {
                    msg_def.body.clone()
                };
                set.description = Some(desc);
            }

            // Store the body for this locale
            set.bodies.insert(locale.clone(), msg_def.body.clone());

            // Store the attributes for this locale
            if !msg_def.attributes.is_empty() {
                set.attributes
                    .insert(locale.clone(), msg_def.attributes.clone());
            }
        }
    }

    sets
}

/// Extract derived objects from a single block (and its children recursively).
#[allow(clippy::too_many_arguments)]
fn extract_derived_from_block(
    block: &ParsedBlock,
    chapter_index: usize,
    block_index: usize,
    // Name of the `:::step` this block is a direct child of, if any. A
    // role=default `:::nushell` under a step is step-private and gets its name
    // scoped to the step (see `task::step_task_ref_name`).
    enclosing_step: Option<&str>,
    script_files: &std::collections::HashMap<String, String>,
    tasks: &mut Vec<TaskDef>,
    brief_packs: &mut Vec<BriefPackDef>,
    inference_profiles: &mut Vec<InferenceProfileDef>,
    refs: &mut Vec<RefDef>,
    scopes: &mut Vec<ScopeDef>,
    objectives: &mut Vec<ObjectiveDef>,
    deliverables: &mut Vec<DeliverableDef>,
    value_slots: &mut Vec<ValueSlotDef>,
    brief_inputs: &mut Vec<BriefInputDef>,
    brief_notices: &mut Vec<BriefNoticeDef>,
    brief_markdowns: &mut Vec<BriefMarkdownDef>,
    brief_outputs: &mut Vec<BriefOutputDef>,
    briefs: &mut Vec<BriefDef>,
    brief_groups: &mut Vec<BriefGroupDef>,
    warnings: &mut Vec<String>,
) {
    let source = BlockIndex {
        chapter_index,
        block_index,
    };

    // Blocks with a `ref` attribute used to materialize as SharedRefDef entries
    // for shared-content lookup. Shared content is deferred to V2+; until it
    // returns with a proper redesign, we silently skip blocks that carry a
    // ref_path so they don't get mis-extracted by their directive_type's
    // normal path (e.g., a :::stage with ref="..." should not be treated as a
    // local stage definition). See docs/plans/external-api/00-architecture.md
    // §7 Out of Scope.
    if block.ref_path.is_some() {
        return;
    }

    match block.directive_type.as_str() {
        "nushell" => {
            if let Some(mut task_def) = task::extract_task(block, &source, script_files) {
                // A role=default :::nushell nested directly in a step is
                // step-private: scope its name so it matches the step's
                // `task_ref` and can't collide with another step's same-named
                // nested task. Directive nushell (attached by `for`) keep bare
                // names — they're referenced by target, not by step task_refs.
                if enclosing_step.is_some() && task_def.role == "default" {
                    task_def.name = task::step_task_ref_name(enclosing_step, &task_def.name);
                }
                tasks.push(task_def);
            } else {
                warnings.push(format!(
                    ":::nushell block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "brief_pack" => {
            if let Some(bp_def) = brief_pack::extract_brief_pack(block, &source) {
                brief_packs.push(bp_def);
            } else {
                warnings.push(format!(
                    ":::brief_pack block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "inference_profile" => {
            if let Some(ip_def) = extract_inference_profile(block, &source) {
                inference_profiles.push(ip_def);
            } else {
                warnings.push(format!(
                    ":::inference_profile block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "ref" => {
            if let Some(ref_def) = extract_ref(block, &source) {
                refs.push(ref_def);
            }
        }
        "scope" => {
            if let Some(scope_def) = scope::extract_scope(block, &source) {
                scopes.push(scope_def);
            } else {
                warnings.push(format!(
                    ":::scope block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "objective" => {
            if let Some(obj_def) = objective::extract_objective(block, &source) {
                objectives.push(obj_def);
            } else {
                warnings.push(format!(
                    ":::objective block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "deliverable" => {
            if let Some(del_def) = deliverable::extract_deliverable(block, &source) {
                deliverables.push(del_def);
            } else {
                warnings.push(format!(
                    ":::deliverable block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "value_slot" => {
            if let Some(vs_def) = value_slot::extract_value_slot(block, &source) {
                value_slots.push(vs_def);
            } else {
                warnings.push(format!(
                    ":::value_slot block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "brief_input" => {
            if let Some(bi_def) = brief_input::extract_brief_input(block, &source) {
                brief_inputs.push(bi_def);
            } else {
                warnings.push(format!(
                    ":::brief_input block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "brief_notice" => {
            if let Some(bn_def) = brief_notice::extract_brief_notice(block, &source) {
                brief_notices.push(bn_def);
            } else {
                warnings.push(format!(
                    ":::brief_notice block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "brief_markdown" => {
            if let Some(bm_def) = brief_markdown::extract_brief_markdown(block, &source) {
                brief_markdowns.push(bm_def);
            } else {
                warnings.push(format!(
                    ":::brief_markdown block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "brief_output" => {
            if let Some(bo_def) = brief_output::extract_brief_output(block, &source) {
                brief_outputs.push(bo_def);
            } else {
                warnings.push(format!(
                    ":::brief_output block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "brief" => {
            if let Some(b_def) = brief::extract_brief(block, &source) {
                briefs.push(b_def);
            } else {
                warnings.push(format!(
                    ":::brief block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        "brief_group" => {
            if let Some(bg_def) = brief_group::extract_brief_group(block, &source) {
                brief_groups.push(bg_def);
            } else {
                warnings.push(format!(
                    ":::brief_group block at chapter {}, block {} has no name attribute",
                    chapter_index, block_index
                ));
            }
        }
        _ => {
            // prose, stage, step -- handled elsewhere or stored as-is
        }
    }

    // Recurse into children. A `:::nushell` whose DIRECT parent is a `:::step`
    // is that step's private task (scoped); deeper nesting is not, so we only
    // set the enclosing step for direct children of a step.
    let child_enclosing_step = if block.directive_type == "step" {
        block.name.as_deref()
    } else {
        None
    };
    for (child_idx, child) in block.children.iter().enumerate() {
        extract_derived_from_block(
            child,
            chapter_index,
            child_idx,
            child_enclosing_step,
            script_files,
            tasks,
            brief_packs,
            inference_profiles,
            refs,
            scopes,
            objectives,
            deliverables,
            value_slots,
            brief_inputs,
            brief_notices,
            brief_markdowns,
            brief_outputs,
            briefs,
            brief_groups,
            warnings,
        );
    }
}

/// Extract an inference profile definition from an :::inference_profile block.
fn extract_inference_profile(
    block: &ParsedBlock,
    source: &BlockIndex,
) -> Option<InferenceProfileDef> {
    let name = block.name.clone()?;
    Some(InferenceProfileDef {
        name,
        source_block_index: source.clone(),
    })
}

/// Extract a reference definition from a :::ref block.
fn extract_ref(block: &ParsedBlock, source: &BlockIndex) -> Option<RefDef> {
    let slug = block
        .attributes
        .get("slug")
        .and_then(|v| v.as_str())
        .map(String::from)?;

    let ref_type = block
        .attributes
        .get("type")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "guide".to_string());

    Some(RefDef {
        slug,
        ref_type,
        source_block_index: source.clone(),
    })
}

/// Produce a dry-run validation summary from an ingest plan.
impl IngestPlan {
    /// Count total blocks across all chapters.
    pub fn total_blocks(&self) -> usize {
        self.chapters.iter().map(|ch| ch.blocks.len()).sum()
    }

    /// Count total assets.
    pub fn total_assets(&self) -> usize {
        self.asset_files.len()
    }

    /// Count derived playbook stages (total across all playbooks).
    pub fn derived_stage_count(&self) -> usize {
        self.derived_playbooks.iter().map(|p| p.stages.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GuideMetadata, ParsedCard, ParsedChapter};

    fn make_block(directive: &str, name: Option<&str>, content: &str) -> ParsedBlock {
        ParsedBlock {
            directive_type: directive.to_string(),
            name: name.map(|n| n.to_string()),
            content: content.to_string(),
            attributes: serde_json::json!({}),
            tags: Vec::new(),
            source_line: 1,
            children: Vec::new(),
            ref_path: None,
        }
    }

    fn make_guide(chapters: Vec<ParsedChapter>) -> ParsedGuide {
        ParsedGuide {
            title: "Test Guide".to_string(),
            metadata: GuideMetadata {
                title: "Test Guide".to_string(),
                description: Some("A test guide".to_string()),
                author: None,
                classification: None,
                source_path: None,
                card: None,
                deprecation_level: None,
                deprecation_reason: None,
                chapters: HashMap::new(),
            },
            chapters,
            asset_paths: Vec::new(),
            locales: HashMap::new(),
            script_files: HashMap::new(),
            conduits_md: String::new(),
        }
    }

    fn bidx() -> BlockIndex {
        BlockIndex {
            chapter_index: 0,
            block_index: 0,
        }
    }

    fn test_step(name: &str, expose: &[&str]) -> StepDef {
        StepDef {
            name: name.to_string(),
            flow_control: None,
            expose: expose.iter().map(|s| s.to_string()).collect(),
            mask: Vec::new(),
            task_refs: Vec::new(),
            source_block_index: bidx(),
        }
    }

    fn test_stage(name: &str, expose: &[&str], steps: Vec<StepDef>) -> StageDef {
        StageDef {
            name: name.to_string(),
            kind: None,
            icon: Vec::new(),
            flow_control: None,
            expose: expose.iter().map(|s| s.to_string()).collect(),
            mask: Vec::new(),
            steps,
            source_block_index: bidx(),
        }
    }

    fn test_brief_input(slot: &str) -> BriefInputDef {
        BriefInputDef {
            name: format!("{slot}-input"),
            control: "select".to_string(),
            slot: Some(slot.to_string()),
            candidate_source: None,
            optional: false,
            item_fields: None,
            refine_input: None,
            axis: None,
            source_block_index: bidx(),
        }
    }

    fn thesis_playbook() -> Vec<PlaybookDef> {
        // Mirrors essay-outline-builder's "Define Your Thesis" stage: a typed
        // input on step 1, a "pick" exposed on step 2.
        vec![PlaybookDef {
            name: "pb".to_string(),
            chapter_index: 0,
            stages: vec![test_stage(
                "Define Your Thesis",
                &[],
                vec![
                    test_step("Describe Your Topic", &["topic"]),
                    test_step("Generate Thesis Directions", &["selected_approach"]),
                ],
            )],
        }]
    }

    #[test]
    fn test_warn_uncollectable_exposed_slots_flags_unbacked_pick() {
        // Only the typed input has a :::brief-input; the pick has neither a
        // brief-input nor a brief-output, so it is unbindable and must warn.
        let brief_inputs = vec![test_brief_input("topic")];
        let mut warnings = Vec::new();
        warn_uncollectable_exposed_slots(&thesis_playbook(), &brief_inputs, &[], &mut warnings);

        assert_eq!(
            warnings.len(),
            1,
            "exactly the pick slot should warn: {warnings:?}"
        );
        assert!(warnings[0].contains("selected_approach"));
        assert!(
            !warnings.iter().any(|w| w.contains("'topic'")),
            "a collected input must not warn"
        );
    }

    #[test]
    fn test_warn_uncollectable_exposed_slots_clean_with_input_pin() {
        // Adding a :::brief-input for the pick makes every exposed slot
        // collectable -> no warning (the fix this guardrail guides authors to).
        let brief_inputs = vec![
            test_brief_input("topic"),
            test_brief_input("selected_approach"),
        ];
        let mut warnings = Vec::new();
        warn_uncollectable_exposed_slots(&thesis_playbook(), &brief_inputs, &[], &mut warnings);

        assert!(
            warnings.is_empty(),
            "all exposed slots collectable: {warnings:?}"
        );
    }

    #[test]
    fn test_warn_uncollectable_exposed_slots_clean_with_computed_output() {
        // A computed brief-output also makes a slot collectable (task produces it).
        let brief_inputs = vec![test_brief_input("topic")];
        let brief_outputs = vec![BriefOutputDef {
            name: "approach-output".to_string(),
            slot: Some("selected_approach".to_string()),
            source: "computed".to_string(),
            source_path: None,
            display: "text".to_string(),
            reveal: false,
            title: None,
            group: None,
            format: None,
            source_block_index: bidx(),
        }];
        let mut warnings = Vec::new();
        warn_uncollectable_exposed_slots(
            &thesis_playbook(),
            &brief_inputs,
            &brief_outputs,
            &mut warnings,
        );

        assert!(
            warnings.is_empty(),
            "computed output collects the slot: {warnings:?}"
        );
    }

    fn test_brief(name: &str, unit_names: &[&str]) -> BriefDef {
        BriefDef {
            name: name.to_string(),
            unit_names: unit_names.iter().map(|s| s.to_string()).collect(),
            source_block_index: bidx(),
        }
    }

    fn test_brief_output(name: &str, slot: &str) -> BriefOutputDef {
        BriefOutputDef {
            name: name.to_string(),
            slot: Some(slot.to_string()),
            source: "computed".to_string(),
            source_path: Some(".".to_string()),
            display: "text".to_string(),
            reveal: false,
            title: None,
            group: None,
            format: None,
            source_block_index: bidx(),
        }
    }

    #[test]
    fn test_warn_unwired_brief_outputs_flags_orphan() {
        // The output parsed (it has a name) but no brief lists it in `units`,
        // so the materializer would silently drop it.
        let briefs = vec![test_brief(
            "land-the-worst-pun-brief",
            &[
                "land-the-worst-pun-intro",
                "land-the-worst-pun-selected-closer-input",
            ],
        )];
        let outputs = vec![test_brief_output(
            "assembled-story-output",
            "land_the_worst_pun_assembled_story",
        )];
        let mut warnings = Vec::new();
        warn_unwired_brief_outputs(&briefs, &outputs, &mut warnings);

        assert_eq!(
            warnings.len(),
            1,
            "the orphaned output should warn: {warnings:?}"
        );
        assert!(warnings[0].contains("assembled-story-output"));
        assert!(warnings[0].contains("`units`"));
    }

    #[test]
    fn test_warn_unwired_brief_outputs_clean_when_wired() {
        // Listing the output's name in the brief's `units` clears the warning —
        // exactly what the materializer needs to associate and insert it.
        let briefs = vec![test_brief(
            "land-the-worst-pun-brief",
            &[
                "land-the-worst-pun-intro",
                "land-the-worst-pun-selected-closer-input",
                "assembled-story-output",
            ],
        )];
        let outputs = vec![test_brief_output(
            "assembled-story-output",
            "land_the_worst_pun_assembled_story",
        )];
        let mut warnings = Vec::new();
        warn_unwired_brief_outputs(&briefs, &outputs, &mut warnings);

        assert!(
            warnings.is_empty(),
            "wired output must not warn: {warnings:?}"
        );
    }

    fn test_brief_group(name: &str, brief_names: &[&str]) -> BriefGroupDef {
        BriefGroupDef {
            name: name.to_string(),
            brief_names: brief_names.iter().map(|s| s.to_string()).collect(),
            source_block_index: bidx(),
        }
    }

    fn test_brief_pack(name: &str, group_names: &[&str]) -> BriefPackDef {
        BriefPackDef {
            name: name.to_string(),
            group_names: group_names.iter().map(|s| s.to_string()).collect(),
            source_block_index: bidx(),
        }
    }

    /// The five def collections `validate_brief_wiring` takes, in parameter order.
    type BriefSystemDefs = (
        Vec<BriefPackDef>,
        Vec<BriefGroupDef>,
        Vec<BriefDef>,
        Vec<BriefInputDef>,
        Vec<BriefOutputDef>,
    );

    /// The fully wired chain the other wiring tests mutate one link of:
    /// pack -> group -> brief -> units [input collecting `topic`,
    /// output producing `selected_approach`], with both slots exposed by
    /// `thesis_playbook()`.
    fn wired_brief_system() -> BriefSystemDefs {
        (
            vec![test_brief_pack("pack", &["group"])],
            vec![test_brief_group("group", &["brief"])],
            vec![test_brief("brief", &["topic-input", "approach-output"])],
            vec![test_brief_input("topic")],
            vec![test_brief_output("approach-output", "selected_approach")],
        )
    }

    #[test]
    fn test_validate_brief_wiring_clean_chain_passes() {
        let (packs, groups, briefs, inputs, outputs) = wired_brief_system();
        let result = validate_brief_wiring(
            &packs,
            &groups,
            &briefs,
            &inputs,
            &outputs,
            &thesis_playbook(),
        );
        assert!(result.is_ok(), "fully wired chain must pass: {result:?}");
    }

    #[test]
    fn test_validate_brief_wiring_flags_orphan_group() {
        // The retort-the-n-port 2026-08-02 stall: the group and its brief are
        // authored and internally wired, but the pack's `groups` list omits
        // the group — so every runtime read misses the whole brief system.
        let (mut packs, mut groups, mut briefs, inputs, mut outputs) = wired_brief_system();
        groups.push(test_brief_group(
            "portfolio-as-company-group",
            &["portfolio-as-company-brief"],
        ));
        briefs.push(test_brief("portfolio-as-company-brief", &["book-output"]));
        outputs.push(test_brief_output("book-output", "fn_book"));
        // Not exposed by the playbook -> only the orphan-group problem fires.
        packs[0] = test_brief_pack("pack", &["group"]);

        let err = validate_brief_wiring(
            &packs,
            &groups,
            &briefs,
            &inputs,
            &outputs,
            &thesis_playbook(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("portfolio-as-company-group"),
            "must name the orphaned group: {msg}"
        );
        assert!(
            !msg.contains("':::brief 'brief'"),
            "wired brief must not be flagged: {msg}"
        );
    }

    #[test]
    fn test_validate_brief_wiring_flags_orphan_brief() {
        let (packs, groups, mut briefs, inputs, outputs) = wired_brief_system();
        briefs.push(test_brief("stray-brief", &["stray-unit"]));

        let err = validate_brief_wiring(
            &packs,
            &groups,
            &briefs,
            &inputs,
            &outputs,
            &thesis_playbook(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("stray-brief"));
    }

    #[test]
    fn test_validate_brief_wiring_flags_missing_pack() {
        // The mind-the-13f shape: 14 groups authored, no :::brief-pack at all.
        let (_, groups, briefs, inputs, outputs) = wired_brief_system();
        let err = validate_brief_wiring(&[], &groups, &briefs, &inputs, &outputs, &[]).unwrap_err();
        assert!(err.to_string().contains("no :::brief-pack is declared"));
    }

    #[test]
    fn test_validate_brief_wiring_flags_dangling_references() {
        let (mut packs, mut groups, briefs, inputs, outputs) = wired_brief_system();
        packs[0] = test_brief_pack("pack", &["group", "ghost-group"]);
        groups[0] = test_brief_group("group", &["brief", "ghost-brief"]);

        let err = validate_brief_wiring(
            &packs,
            &groups,
            &briefs,
            &inputs,
            &outputs,
            &thesis_playbook(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ghost-group"), "dangling group ref: {msg}");
        assert!(msg.contains("ghost-brief"), "dangling brief ref: {msg}");
    }

    #[test]
    fn test_validate_brief_wiring_flags_unreachable_collector() {
        // The collector EXISTS guide-wide (so warn_uncollectable_exposed_slots
        // stays silent) but its brief's group is not in the pack -> the exposed
        // slot can never be bound at runtime. This is the shape that shipped
        // retort-the-n-port's stage-3 stall through a 0-warnings validate.
        let (packs, mut groups, mut briefs, inputs, mut outputs) = wired_brief_system();
        groups.push(test_brief_group("orphan-group", &["orphan-brief"]));
        briefs.push(test_brief("orphan-brief", &["candidates-output"]));
        outputs.push(test_brief_output("candidates-output", "bk_candidates"));

        let playbooks = vec![PlaybookDef {
            name: "pb".to_string(),
            chapter_index: 0,
            stages: vec![test_stage(
                "Read the Portfolio as a Company",
                &[],
                vec![test_step("Roll Up the Portfolio", &["bk_candidates"])],
            )],
        }];

        let err = validate_brief_wiring(&packs, &groups, &briefs, &inputs, &outputs, &playbooks)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bk_candidates") && msg.contains("candidates-output"),
            "must name the slot and its unreachable collector: {msg}"
        );
    }

    #[test]
    fn test_validate_brief_wiring_uncollected_expose_stays_soft() {
        // An exposed slot with NO collector anywhere is the documented
        // overlay-only shape: warn_uncollectable_exposed_slots warns, this
        // validator must NOT error on it.
        let (packs, groups, briefs, inputs, outputs) = wired_brief_system();
        let playbooks = vec![PlaybookDef {
            name: "pb".to_string(),
            chapter_index: 0,
            stages: vec![test_stage(
                "s",
                &[],
                vec![test_step("st", &["agent_context_only"])],
            )],
        }];
        let result = validate_brief_wiring(&packs, &groups, &briefs, &inputs, &outputs, &playbooks);
        assert!(
            result.is_ok(),
            "collector-less expose is warn-territory, not an error: {result:?}"
        );
    }

    fn test_value_slot(name: &str, cardinality: &str) -> ValueSlotDef {
        ValueSlotDef {
            name: name.to_string(),
            kind: "text".to_string(),
            constraints: None,
            cardinality: cardinality.to_string(),
            min_selections: None,
            max_selections: None,
            source_block_index: bidx(),
        }
    }

    #[test]
    fn test_warn_cardinality_checkbox_on_single_warns() {
        let slots = vec![test_value_slot("kept", "single")];
        let mut input = test_brief_input("kept");
        input.control = "checkbox".to_string();
        let mut warnings = Vec::new();
        warn_cardinality_coherence(&slots, &[input], &mut warnings);
        assert_eq!(warnings.len(), 1, "checkbox over single should warn");
        assert!(warnings[0].contains("checkbox"));
    }

    #[test]
    fn test_warn_cardinality_noncheckbox_on_multi_warns() {
        let slots = vec![test_value_slot("kept", "multi")];
        let mut input = test_brief_input("kept");
        input.control = "select".to_string();
        let mut warnings = Vec::new();
        warn_cardinality_coherence(&slots, &[input], &mut warnings);
        assert_eq!(warnings.len(), 1, "non-checkbox over multi should warn");
    }

    #[test]
    fn test_warn_cardinality_checkbox_on_multi_clean() {
        let slots = vec![test_value_slot("kept", "multi")];
        let mut input = test_brief_input("kept");
        input.control = "checkbox".to_string();
        let mut warnings = Vec::new();
        warn_cardinality_coherence(&slots, &[input], &mut warnings);
        assert!(
            warnings.is_empty(),
            "checkbox over multi is clean: {warnings:?}"
        );
    }

    #[test]
    fn test_warn_cardinality_repeater_on_multi_clean() {
        // A repeater is a multi-value builder — writing a `multi` slot with it is
        // correct and must NOT warn (regression: the check used to whitelist only
        // `checkbox`, so the guide's `dl_story` repeater emitted a spurious warning).
        let slots = vec![test_value_slot("kept", "multi")];
        let mut input = test_brief_input("kept");
        input.control = "repeater".to_string();
        let mut warnings = Vec::new();
        warn_cardinality_coherence(&slots, &[input], &mut warnings);
        assert!(
            warnings.is_empty(),
            "repeater over multi is a valid subset selector: {warnings:?}"
        );
    }

    #[test]
    fn test_warn_cardinality_repeater_on_single_warns() {
        // The reverse mismatch: a subset selector over a `single` slot should warn.
        let slots = vec![test_value_slot("kept", "single")];
        let mut input = test_brief_input("kept");
        input.control = "repeater".to_string();
        let mut warnings = Vec::new();
        warn_cardinality_coherence(&slots, &[input], &mut warnings);
        assert_eq!(warnings.len(), 1, "repeater over single should warn");
        assert!(warnings[0].contains("repeater"));
    }

    #[test]
    fn test_warn_cardinality_unknown_candidate_source_warns() {
        let slots = vec![test_value_slot("kept", "multi")];
        let mut input = test_brief_input("kept");
        input.control = "checkbox".to_string();
        input.candidate_source = Some("missing_candidates".to_string());
        let mut warnings = Vec::new();
        warn_cardinality_coherence(&slots, &[input], &mut warnings);
        assert_eq!(warnings.len(), 1, "unknown candidate_source should warn");
        assert!(warnings[0].contains("candidate_source"));
    }

    #[test]
    fn test_warn_cardinality_known_candidate_source_clean() {
        let slots = vec![
            test_value_slot("kept", "multi"),
            test_value_slot("supplier_candidates", "single"),
        ];
        let mut input = test_brief_input("kept");
        input.control = "checkbox".to_string();
        input.candidate_source = Some("supplier_candidates".to_string());
        let mut warnings = Vec::new();
        warn_cardinality_coherence(&slots, &[input], &mut warnings);
        assert!(
            warnings.is_empty(),
            "declared candidate_source is clean: {warnings:?}"
        );
    }

    #[test]
    fn test_plan_guide_empty() {
        let guide = make_guide(vec![]);
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        assert_eq!(plan.title, "Test Guide");
        assert!(plan.chapters.is_empty());
        assert!(plan.derived_tasks.is_empty());
        assert!(plan.derived_playbooks.is_empty());
        assert!(plan.derived_brief_packs.is_empty());
        assert!(plan.fluent_messages.is_empty());
    }

    #[test]
    fn test_plan_guide_empty_conduits_warns() {
        // make_guide leaves conduits_md empty — plan_guide must flag it.
        let guide = make_guide(vec![]);
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("CONDUITS.md is empty")),
            "expected empty-CONDUITS warning, got: {:?}",
            plan.warnings
        );
    }

    #[test]
    fn test_plan_guide_conduits_carried_and_no_warning() {
        let mut guide = make_guide(vec![]);
        guide.conduits_md = "# Conduit Context\n\nBindings explained here.\n".to_string();
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();
        assert_eq!(plan.conduits_md, guide.conduits_md);
        assert!(
            !plan.warnings.iter().any(|w| w.contains("CONDUITS.md")),
            "no CONDUITS warning expected, got: {:?}",
            plan.warnings
        );
    }

    #[test]
    fn test_plan_guide_extracts_tasks() {
        let mut block = make_block("nushell", Some("my-task"), "echo hello");
        block.attributes = serde_json::json!({ "name": "my-task" });

        let chapter = ParsedChapter {
            title: "Chapter 1".to_string(),
            source_file: "ch1.md".to_string(),
            depth: 0,
            parent_index: None,
            blocks: vec![block],
        };

        let guide = make_guide(vec![chapter]);
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        assert_eq!(plan.derived_tasks.len(), 1);
        assert_eq!(plan.derived_tasks[0].name, "my-task");
        assert_eq!(plan.derived_tasks[0].script, "echo hello");
    }

    #[test]
    fn test_plan_guide_extracts_brief_packs() {
        let mut block = make_block("brief_pack", Some("onboarding"), "## Overview\nWelcome!");
        block.attributes = serde_json::json!({ "name": "onboarding" });

        let chapter = ParsedChapter {
            title: "Chapter 1".to_string(),
            source_file: "ch1.md".to_string(),
            depth: 0,
            parent_index: None,
            blocks: vec![block],
        };

        let guide = make_guide(vec![chapter]);
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        assert_eq!(plan.derived_brief_packs.len(), 1);
        assert_eq!(plan.derived_brief_packs[0].name, "onboarding");
    }

    #[test]
    fn test_plan_guide_extracts_card() {
        let mut guide = make_guide(vec![]);
        guide.metadata.card = Some(ParsedCard {
            image: Some("assets/images/card.jpg".to_string()),
            color: Some("#2D6A4F".to_string()),
            accent: None,
        });

        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        assert!(plan.card.is_some());
        let card = plan.card.unwrap();
        assert_eq!(card.image, Some("assets/images/card.jpg".to_string()));
        assert_eq!(card.color, Some("#2D6A4F".to_string()));
    }

    #[test]
    fn test_plan_guide_builds_fluent_message_sets() {
        let mut guide = make_guide(vec![]);

        // Add locale data
        let mut locales = HashMap::new();
        locales.insert(
            "en".to_string(),
            vec![
                crate::types::FluentMessageDef {
                    key: "topic-selection".to_string(),
                    body: "Guide the user through topic selection.".to_string(),
                    attributes: HashMap::new(),
                },
                crate::types::FluentMessageDef {
                    key: "essay-pack".to_string(),
                    body: "Essay helper pack.".to_string(),
                    attributes: {
                        let mut attrs = HashMap::new();
                        attrs.insert("card-subtitle".to_string(), "Ready to write?".to_string());
                        attrs
                    },
                },
            ],
        );
        locales.insert(
            "pt-BR".to_string(),
            vec![crate::types::FluentMessageDef {
                key: "topic-selection".to_string(),
                body: "Orientar o usuario na selecao de temas.".to_string(),
                attributes: HashMap::new(),
            }],
        );
        guide.locales = locales;

        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        assert_eq!(plan.fluent_messages.len(), 2);

        let topic = &plan.fluent_messages["topic-selection"];
        assert_eq!(topic.bodies.len(), 2);
        assert!(topic.bodies.contains_key("en"));
        assert!(topic.bodies.contains_key("pt-BR"));

        let pack = &plan.fluent_messages["essay-pack"];
        assert_eq!(pack.bodies.len(), 1);
        assert_eq!(
            pack.attributes
                .get("en")
                .and_then(|a| a.get("card-subtitle")),
            Some(&"Ready to write?".to_string())
        );
    }

    #[test]
    fn test_nushell_blocks_retain_content() {
        let block = make_block("nushell", Some("my-task"), "echo hello");
        let chapter = ParsedChapter {
            title: "Ch1".to_string(),
            source_file: "ch1.md".to_string(),
            depth: 0,
            parent_index: None,
            blocks: vec![block],
        };

        let guide = make_guide(vec![chapter]);
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        assert_eq!(
            plan.chapters[0].blocks[0].content,
            Some("echo hello".to_string())
        );
    }

    #[test]
    fn test_nested_blocks_are_flattened_depth_first() {
        // Build a stage → step → nushell tree
        let nushell_block = ParsedBlock {
            directive_type: "nushell".to_string(),
            name: Some("my-task".to_string()),
            content: "echo hello".to_string(),
            attributes: serde_json::json!({ "name": "my-task" }),
            tags: Vec::new(),
            source_line: 3,
            children: Vec::new(),
            ref_path: None,
        };

        let step_block = ParsedBlock {
            directive_type: "step".to_string(),
            name: Some("brainstorm".to_string()),
            content: "".to_string(),
            attributes: serde_json::json!({ "name": "brainstorm" }),
            tags: Vec::new(),
            source_line: 2,
            children: vec![nushell_block],
            ref_path: None,
        };

        let stage_block = ParsedBlock {
            directive_type: "stage".to_string(),
            name: Some("topic-selection".to_string()),
            content: "".to_string(),
            attributes: serde_json::json!({ "name": "topic-selection" }),
            tags: Vec::new(),
            source_line: 1,
            children: vec![step_block],
            ref_path: None,
        };

        let chapter = ParsedChapter {
            title: "Writing".to_string(),
            source_file: "writing.md".to_string(),
            depth: 0,
            parent_index: None,
            blocks: vec![stage_block],
        };

        let guide = make_guide(vec![chapter]);
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        // Should have 3 flat blocks: stage, step, nushell
        assert_eq!(plan.chapters[0].blocks.len(), 3);
        assert_eq!(plan.chapters[0].blocks[0].directive_type, "stage");
        assert_eq!(plan.chapters[0].blocks[1].directive_type, "step");
        assert_eq!(plan.chapters[0].blocks[2].directive_type, "nushell");

        // Nushell retains inline content
        assert_eq!(
            plan.chapters[0].blocks[2].content,
            Some("echo hello".to_string())
        );

        // Stage and step have empty content → None (Fluent-backed)
        assert_eq!(plan.chapters[0].blocks[0].content, None);
        assert_eq!(plan.chapters[0].blocks[1].content, None);
    }

    #[test]
    fn test_prose_blocks_with_empty_content_are_none() {
        let block = make_block("prose", Some("my-prose"), "");
        let chapter = ParsedChapter {
            title: "Ch1".to_string(),
            source_file: "ch1.md".to_string(),
            depth: 0,
            parent_index: None,
            blocks: vec![block],
        };

        let guide = make_guide(vec![chapter]);
        let dir = tempfile::TempDir::new().unwrap();
        let plan = plan_guide(&guide, dir.path()).unwrap();

        assert_eq!(plan.chapters[0].blocks[0].content, None);
    }
}
