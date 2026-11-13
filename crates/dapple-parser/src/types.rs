use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::guide_toml::ChapterMetadataToml;

/// Full metadata from guide.toml (Fluent-native).
///
/// The guide structure is locale-independent. There is no `locale` field.
/// All human-readable text lives in Fluent (.ftl) files under locales/.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideMetadata {
    pub title: String,
    pub description: Option<String>,
    // NO locale field -- guide structure is locale-independent.
    pub author: Option<String>,
    pub classification: Option<String>,
    /// Logical source path stamped onto `rev.guides.source_path` at ingest.
    /// Pulled from `guide.toml`'s top-level `source_path = "..."` field.
    /// The internal handler may override this via the `?source_path=...`
    /// query parameter.
    pub source_path: Option<String>,
    pub card: Option<ParsedCard>,
    pub deprecation_level: Option<String>,
    pub deprecation_reason: Option<String>,
    /// Per-chapter metadata from guide.toml [chapters] sections.
    #[serde(default)]
    pub chapters: HashMap<String, ChapterMetadataToml>,
}

/// Card metadata extracted from guide.toml [card] section (Fluent-native).
///
/// Only locale-independent visual properties remain here. Subtitle and
/// category are now Fluent attributes on the brief-pack message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedCard {
    pub image: Option<String>, // Relative path, not yet resolved
    pub color: Option<String>,
    pub accent: Option<String>,
}

/// The complete parsed representation of a guide directory (Fluent-native).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedGuide {
    /// Guide title (from guide.toml, or fallback chain).
    pub title: String,
    /// Full metadata from guide.toml.
    pub metadata: GuideMetadata,
    /// Ordered list of chapters with their parsed blocks.
    pub chapters: Vec<ParsedChapter>,
    /// Non-markdown files found in the guide directory (relative paths).
    /// Populated during parse if asset discovery is enabled.
    pub asset_paths: Vec<String>,
    /// Fluent message definitions extracted from locales/*.ftl files.
    /// Keyed by locale tag (e.g., "en", "pt-BR").
    pub locales: HashMap<String, Vec<FluentMessageDef>>,
    /// Nushell script files from scripts/*.nu, keyed by name (filename without .nu).
    pub script_files: HashMap<String, String>,
    /// Raw markdown of the required root-level CONDUITS.md — the conduit-facing
    /// guide frame (binding semantics, how to read retrieved prose). Sibling of
    /// the authoring-agent AGENTS.md, which is never parsed. Not localized (V1),
    /// never a chapter, never embedded — delivered whole as a system message.
    pub conduits_md: String,
}

/// A single Fluent message definition from a .ftl file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FluentMessageDef {
    /// The message key (matches directive `name` attribute).
    pub key: String,
    /// The message body text.
    pub body: String,
    /// Fluent attributes (.key = value pairs).
    pub attributes: HashMap<String, String>,
}

/// A single chapter in the guide, corresponding to one .md file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedChapter {
    /// Chapter title from the SUMMARY.md link text.
    pub title: String,
    /// Relative file path within the guide directory (e.g., "getting-started/intro.md").
    pub source_file: String,
    /// Nesting depth (0 = top-level, 1 = indented once, etc.).
    pub depth: usize,
    /// Index of parent chapter in the flat chapter list. None = top-level.
    pub parent_index: Option<usize>,
    /// Ordered list of content blocks parsed from this chapter's markdown file.
    pub blocks: Vec<ParsedBlock>,
}

/// A single content block within a chapter.
///
/// Blocks are the atomic unit of guide content. They are either:
/// - Fenced directive blocks (:::directive { attrs } ... :::)
/// - Prose blocks (markdown content between directives)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedBlock {
    /// Directive type key. One of: "prose", "stage", "step", "nushell",
    /// "brief_pack", "inference_profile", "ref".
    pub directive_type: String,
    /// Optional name from directive metadata (e.g., `:::step { name: "Gather Info" }`).
    pub name: Option<String>,
    /// Raw content within the block. For prose blocks, this is the markdown text.
    /// For fenced blocks, this is the content between the opening and closing `:::`.
    pub content: String,
    /// Parsed key-value attributes from the `{ ... }` metadata after the directive name.
    pub attributes: serde_json::Value,
    /// Tag keys extracted from `tags: [agent, product]` in the attributes.
    pub tags: Vec<String>,
    /// One-indexed line number in the source file where this block starts.
    pub source_line: usize,
    /// Child blocks (for nesting: a :::stage contains :::step blocks).
    pub children: Vec<ParsedBlock>,
    /// Optional shared content reference path (e.g., "common/validate-output").
    /// Set when the block uses a `ref` attribute instead of `name`.
    /// Mutually exclusive with `name`.
    pub ref_path: Option<String>,
}

/// Intermediate type returned by the summary parser before blocks are parsed.
#[derive(Clone, Debug)]
pub struct ParsedChapterStub {
    pub title: String,
    pub source_file: String,
    pub depth: usize,
    pub parent_index: Option<usize>,
}
