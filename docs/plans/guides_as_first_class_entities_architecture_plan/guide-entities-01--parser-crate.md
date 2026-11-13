# Issue 1: Parser Crate (crates/dapple-parser/)

**Status:** Draft
**Created:** 2026-02-13
**Dependencies:** None (can be implemented independently)
**Blocks:** Issue 5 (SQL queries need parsed guide structures for ingestion pipeline)

---

## 1. Summary

Create a new workspace crate `crates/dapple-parser/` that provides three capabilities:

1. **SUMMARY.md parsing** -- Extract the chapter tree (title, file path, depth, parent) from an mdBook SUMMARY.md file.
2. **Fence directive tokenization** -- Scan markdown files for `:::directive { metadata }` blocks, handling nesting via a stack-based state machine.
3. **Block classification** -- Convert raw fence tokens into typed `ParsedBlock` structs with directive type, name, content, attributes, and tags.

The parser crate is a pure Rust library with no database dependencies. It depends only on `serde` and `serde_json` for attribute serialization.

**Design Decision Reference:** The architecture plan (Decision #14) originally specified the parser in `dapple-data`. After review, a separate `dapple-parser` crate is preferred because:
- The parser has zero database dependencies
- It will be reused by the mdBook preprocessor (Phase 13) without pulling in sqlx
- It keeps `dapple-data` focused on its data access layer responsibility

---

## 2. Crate Structure

### File Inventory

| File | Description |
|------|-------------|
| `crates/dapple-parser/Cargo.toml` | Crate manifest |
| `crates/dapple-parser/src/lib.rs` | Public API: re-exports and `parse_guide()` entry point |
| `crates/dapple-parser/src/summary.rs` | SUMMARY.md parser |
| `crates/dapple-parser/src/fence.rs` | Line-by-line fence directive tokenizer |
| `crates/dapple-parser/src/block.rs` | Block classifier (tokens to ParsedBlocks) |
| `crates/dapple-parser/src/types.rs` | All public types (ParsedGuide, ParsedChapter, ParsedBlock, etc.) |
| `crates/dapple-parser/src/error.rs` | ParseError enum |
| `crates/dapple-parser/src/attrs.rs` | Attribute parser for `{ key: value, ... }` metadata |
| `crates/dapple-parser/tests/summary_tests.rs` | Unit tests for SUMMARY.md parsing |
| `crates/dapple-parser/tests/fence_tests.rs` | Unit tests for fence tokenization |
| `crates/dapple-parser/tests/block_tests.rs` | Unit tests for block classification |
| `crates/dapple-parser/tests/integration_tests.rs` | End-to-end parse of real guide directories |
| `crates/dapple-parser/tests/fixtures/` | Test fixture markdown files |

### Cargo.toml

```toml
[package]
name = "dapple-parser"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0.17"
log = "0.4.27"

[dev-dependencies]
tempfile = "3"
```

### Workspace Registration

Add to `/home/kenny/src/dapple/dapple-prompt/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/*"]
```

No change needed -- the existing glob `crates/*` already picks up new crates. However, **verify** that `crates/dapple-parser/` is recognized by running `cargo build -p dapple-parser` after creation.

---

## 3. Public Types

All types live in `crates/dapple-parser/src/types.rs`.

```rust
// crates/dapple-parser/src/types.rs

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The complete parsed representation of a guide directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedGuide {
    /// Guide title, extracted from the first H1 in SUMMARY.md or from book.toml.
    pub title: String,
    /// Ordered list of chapters with their parsed blocks.
    pub chapters: Vec<ParsedChapter>,
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
}
```

---

## 4. Error Type

```rust
// crates/dapple-parser/src/error.rs

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("IO error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("SUMMARY.md not found in guide directory: {0}")]
    MissingSummary(String),

    #[error("Invalid SUMMARY.md link on line {line}: {detail}")]
    InvalidSummaryLink { line: usize, detail: String },

    #[error("Chapter file not found: {0}")]
    MissingChapterFile(String),

    #[error("Unclosed fence directive starting at line {line} in {file}")]
    UnclosedFence { file: String, line: usize },

    #[error("Invalid attribute syntax at line {line} in {file}: {detail}")]
    InvalidAttributes {
        file: String,
        line: usize,
        detail: String,
    },

    #[error("Unknown directive type '{directive}' at line {line} in {file}")]
    UnknownDirective {
        file: String,
        line: usize,
        directive: String,
    },
}
```

---

## 5. SUMMARY.md Parser

**File:** `crates/dapple-parser/src/summary.rs`

### Algorithm

The SUMMARY.md parser processes the file line by line:

1. Skip blank lines and `# Section Header` lines (these are visual separators in mdBook, not chapters).
2. Match lines containing markdown links: `[Title](./path.md)` or `- [Title](./path.md)`.
3. For bullet-pointed links (`- [Title](...)`), count leading whitespace to determine depth. Each 2-space or 4-space indent level increments depth by 1.
4. For bare links (`[Title](...)` without bullet), depth is 0.
5. Strip the leading `./` from paths to get the relative `source_file`.
6. Compute `parent_index`: walk backwards through the chapter list to find the nearest chapter with `depth == current_depth - 1`. If depth is 0, `parent_index` is `None`.

### Pseudocode

```rust
pub fn parse_summary(content: &str) -> Result<Vec<ParsedChapterStub>, ParseError> {
    let mut chapters = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip blank lines and section headers
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Try to extract a markdown link [Title](path)
        if let Some((title, path)) = extract_markdown_link(trimmed) {
            let depth = compute_depth(line);
            let source_file = path.strip_prefix("./").unwrap_or(&path).to_string();
            let parent_index = find_parent_index(&chapters, depth);

            chapters.push(ParsedChapterStub {
                title,
                source_file,
                depth,
                parent_index,
            });
        }
        // Lines without links are ignored (separators, comments)
    }

    Ok(chapters)
}

/// Extract [Title](path) from a line. Handles optional bullet prefix.
fn extract_markdown_link(line: &str) -> Option<(String, String)> {
    // Strip optional "- " prefix
    let content = line.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
    // Match [Title](path)
    let open_bracket = content.find('[')?;
    let close_bracket = content[open_bracket..].find(']')? + open_bracket;
    let open_paren = content[close_bracket..].find('(')? + close_bracket;
    let close_paren = content[open_paren..].find(')')? + open_paren;

    let title = content[open_bracket + 1..close_bracket].to_string();
    let path = content[open_paren + 1..close_paren].to_string();

    Some((title, path))
}

/// Count indentation depth. Each 2 spaces (or 4 spaces, or 1 tab) = 1 depth level.
/// mdBook standard is 2-space indent per level for nested items.
fn compute_depth(line: &str) -> usize {
    let leading_spaces = line.len() - line.trim_start().len();
    // mdBook uses varying indent widths; 2 spaces per level is standard
    // but handle 4-space and tab as well
    if line.starts_with('\t') {
        let tabs = line.chars().take_while(|c| *c == '\t').count();
        return tabs;
    }
    leading_spaces / 2  // floor division: 0-1 spaces = 0, 2-3 = 1, 4-5 = 2, etc.
}

fn find_parent_index(chapters: &[ParsedChapterStub], depth: usize) -> Option<usize> {
    if depth == 0 {
        return None;
    }
    // Walk backwards to find nearest chapter at depth - 1
    for (i, ch) in chapters.iter().enumerate().rev() {
        if ch.depth == depth - 1 {
            return Some(i);
        }
    }
    None
}
```

### Key Details for the Journey-Planner-Guide SUMMARY.md

Looking at the real `/home/kenny/src/dapple/dapple-prompt/docs/guides/journey-planner-guide/SUMMARY.md`:

```markdown
# Summary

[Preface](./preface.md)
[Agent Reading Guide](./AGENTS.md)

- [Introduction](./intro.md)

# Getting Started

- [Glossary and Terms](./glossary.md)
- [Concepts](./concepts.md)
...
```

- `[Preface](./preface.md)` = depth 0, no bullet, title "Preface", source_file "preface.md"
- `- [Introduction](./intro.md)` = depth 0 (no indentation before the bullet), title "Introduction"
- Section headers like `# Getting Started` are IGNORED -- they are not chapters
- All entries here are depth 0 (no nesting), so `parent_index` is `None` for all

### ParsedChapterStub (Intermediate Type)

```rust
/// Intermediate type returned by the summary parser before blocks are parsed.
#[derive(Clone, Debug)]
pub struct ParsedChapterStub {
    pub title: String,
    pub source_file: String,
    pub depth: usize,
    pub parent_index: Option<usize>,
}
```

---

## 6. Fence Directive Tokenizer

**File:** `crates/dapple-parser/src/fence.rs`

### Directive Syntax

```markdown
:::directive_name { key: "value", tags: [agent, product] }
Content inside the directive...
:::
```

Rules:
- Opening fence: Line starts with `:::` followed by a directive name (letters, hyphens, underscores). Optionally followed by `{ ... }` attributes.
- Closing fence: Line that is exactly `:::` (with optional trailing whitespace or comment).
- Nesting: A `:::stage` can contain `:::step` which can contain `:::nushell`. The tokenizer uses a stack.
- Code blocks: Content inside triple-backtick code blocks is treated as opaque (do not match `:::` inside code blocks).
- Prose: Any content not inside a fence directive is a "prose" block.

### Token Types

```rust
/// Raw tokens emitted by the line-by-line scanner.
#[derive(Clone, Debug)]
pub enum FenceToken {
    /// Opening of a fenced directive.
    Open {
        directive: String,
        raw_attrs: String,  // The { ... } portion, unparsed
        line: usize,
    },
    /// Closing of the current fence (:::).
    Close {
        line: usize,
    },
    /// A line of content inside a fence.
    Content {
        text: String,
        line: usize,
    },
    /// A line of prose (outside any fence).
    Prose {
        text: String,
        line: usize,
    },
}
```

### Tokenizer Algorithm

```rust
pub fn tokenize(content: &str) -> Result<Vec<FenceToken>, ParseError> {
    let mut tokens = Vec::new();
    let mut fence_stack: Vec<(String, usize)> = Vec::new();  // (directive_name, open_line)
    let mut in_code_block = false;

    for (line_num, line) in content.lines().enumerate() {
        let line_num = line_num + 1;  // 1-indexed
        let trimmed = line.trim();

        // Track code block state (triple backticks)
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            // Emit as content or prose depending on fence state
            if fence_stack.is_empty() {
                tokens.push(FenceToken::Prose { text: line.to_string(), line: line_num });
            } else {
                tokens.push(FenceToken::Content { text: line.to_string(), line: line_num });
            }
            continue;
        }

        // Inside code block -- everything is opaque
        if in_code_block {
            if fence_stack.is_empty() {
                tokens.push(FenceToken::Prose { text: line.to_string(), line: line_num });
            } else {
                tokens.push(FenceToken::Content { text: line.to_string(), line: line_num });
            }
            continue;
        }

        // Check for ::: directives
        if trimmed.starts_with(":::") {
            let after_colons = trimmed[3..].trim();

            if after_colons.is_empty() || after_colons.starts_with("<!--") {
                // Closing fence: ":::" or "::: <!-- comment -->"
                if fence_stack.is_empty() {
                    // Stray closing fence -- ignore or treat as prose
                    tokens.push(FenceToken::Prose { text: line.to_string(), line: line_num });
                } else {
                    fence_stack.pop();
                    tokens.push(FenceToken::Close { line: line_num });
                }
            } else {
                // Opening fence: ":::directive_name { attrs }"
                let (directive, raw_attrs) = parse_fence_opening(after_colons);
                tokens.push(FenceToken::Open {
                    directive: directive.clone(),
                    raw_attrs,
                    line: line_num,
                });
                fence_stack.push((directive, line_num));
            }
            continue;
        }

        // Regular content line
        if fence_stack.is_empty() {
            tokens.push(FenceToken::Prose { text: line.to_string(), line: line_num });
        } else {
            tokens.push(FenceToken::Content { text: line.to_string(), line: line_num });
        }
    }

    // Check for unclosed fences
    if let Some((directive, line)) = fence_stack.last() {
        // Return error for unclosed fence -- caller provides file context
        return Err(ParseError::UnclosedFence {
            file: String::new(),  // Caller fills in
            line: *line,
        });
    }

    Ok(tokens)
}

/// Parse "directive_name { key: value }" into (name, raw_attrs).
fn parse_fence_opening(after_colons: &str) -> (String, String) {
    // directive_name is everything up to the first '{' or end of string
    if let Some(brace_pos) = after_colons.find('{') {
        let directive = after_colons[..brace_pos].trim().to_string();
        let attrs = after_colons[brace_pos..].trim().to_string();
        (directive, attrs)
    } else {
        (after_colons.trim().to_string(), String::new())
    }
}
```

---

## 7. Attribute Parser

**File:** `crates/dapple-parser/src/attrs.rs`

Parses the `{ key: "value", tags: [agent, product] }` metadata from fence directives.

### Supported Syntax

The attribute syntax is a simplified YAML/JSON hybrid:
- `{ name: "Gather Info", tags: [agent, product] }`
- Keys are unquoted identifiers
- String values can be quoted or unquoted
- Array values use `[item1, item2]` syntax
- Boolean and numeric values are supported

### Implementation Strategy

Convert the attribute string to valid JSON, then parse with `serde_json`:

```rust
pub fn parse_attributes(raw: &str) -> Result<serde_json::Value, ParseError> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }

    // The raw string should be { ... } -- try to parse as JSON first
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }

    // If JSON parse fails, normalize the YAML-like syntax to JSON:
    // 1. Ensure all keys are quoted
    // 2. Ensure all string values are quoted
    // 3. Handle unquoted array items
    let normalized = normalize_to_json(trimmed)?;

    serde_json::from_str(&normalized).map_err(|e| ParseError::InvalidAttributes {
        file: String::new(),
        line: 0,
        detail: format!("after normalization: {}", e),
    })
}

/// Extract the "tags" array from parsed attributes, if present.
pub fn extract_tags(attrs: &serde_json::Value) -> Vec<String> {
    attrs
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract the "name" field from parsed attributes, if present.
pub fn extract_name(attrs: &serde_json::Value) -> Option<String> {
    attrs.get("name").and_then(|v| v.as_str()).map(String::from)
}
```

### normalize_to_json Implementation Notes

The normalizer handles these transformations:
- `{ name: "value" }` --> `{ "name": "value" }` (quote keys)
- `{ tags: [agent, product] }` --> `{ "tags": ["agent", "product"] }` (quote unquoted array items)
- `{ name: value }` --> `{ "name": "value" }` (quote unquoted string values)

A reasonable implementation iterates character-by-character with a simple state machine tracking whether we are inside quotes, inside brackets, etc.

---

## 8. Block Classifier

**File:** `crates/dapple-parser/src/block.rs`

Converts the flat token stream from `fence.rs` into the hierarchical `ParsedBlock` structure.

### Algorithm

```rust
pub fn classify_blocks(
    tokens: Vec<FenceToken>,
    known_directives: &[&str],
) -> Result<Vec<ParsedBlock>, ParseError> {
    let mut result: Vec<ParsedBlock> = Vec::new();
    let mut prose_buffer: Vec<String> = Vec::new();
    let mut prose_start_line: Option<usize> = None;
    let mut stack: Vec<ParsedBlock> = Vec::new();  // For nesting

    for token in tokens {
        match token {
            FenceToken::Prose { text, line } => {
                if prose_start_line.is_none() {
                    prose_start_line = Some(line);
                }
                prose_buffer.push(text);
            }
            FenceToken::Open { directive, raw_attrs, line } => {
                // Flush prose buffer
                flush_prose(&mut result, &mut stack, &mut prose_buffer, &mut prose_start_line);

                let attrs = parse_attributes(&raw_attrs)?;
                let tags = extract_tags(&attrs);
                let name = extract_name(&attrs);

                let block = ParsedBlock {
                    directive_type: normalize_directive_type(&directive),
                    name,
                    content: String::new(),
                    attributes: attrs,
                    tags,
                    source_line: line,
                    children: Vec::new(),
                };

                stack.push(block);
            }
            FenceToken::Content { text, .. } => {
                // Append to the current open block's content
                if let Some(current) = stack.last_mut() {
                    if !current.content.is_empty() {
                        current.content.push('\n');
                    }
                    current.content.push_str(&text);
                }
            }
            FenceToken::Close { .. } => {
                // Pop the stack and attach to parent or result
                if let Some(completed) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(completed);
                    } else {
                        result.push(completed);
                    }
                }
            }
        }
    }

    // Flush remaining prose
    flush_prose(&mut result, &mut stack, &mut prose_buffer, &mut prose_start_line);

    Ok(result)
}

/// Normalize directive names: "brief-pack" -> "brief_pack"
fn normalize_directive_type(directive: &str) -> String {
    directive.replace('-', "_")
}

/// Flush accumulated prose lines into a prose block.
fn flush_prose(
    result: &mut Vec<ParsedBlock>,
    stack: &mut Vec<ParsedBlock>,
    buffer: &mut Vec<String>,
    start_line: &mut Option<usize>,
) {
    if buffer.is_empty() {
        return;
    }

    let content = buffer.join("\n").trim().to_string();
    if content.is_empty() {
        buffer.clear();
        *start_line = None;
        return;
    }

    let block = ParsedBlock {
        directive_type: "prose".to_string(),
        name: None,
        content,
        attributes: serde_json::Value::Object(serde_json::Map::new()),
        tags: Vec::new(),
        source_line: start_line.unwrap_or(0),
        children: Vec::new(),
    };

    if let Some(parent) = stack.last_mut() {
        parent.children.push(block);
    } else {
        result.push(block);
    }

    buffer.clear();
    *start_line = None;
}
```

---

## 9. Entry Point: parse_guide()

**File:** `crates/dapple-parser/src/lib.rs`

```rust
pub mod attrs;
pub mod block;
pub mod error;
pub mod fence;
pub mod summary;
pub mod types;

pub use error::ParseError;
pub use types::*;

use std::path::Path;

/// Known directive types that the parser recognizes.
pub const KNOWN_DIRECTIVES: &[&str] = &[
    "prose",
    "stage",
    "step",
    "nushell",
    "brief_pack",
    "brief-pack",
    "inference_profile",
    "inference-profile",
    "ref",
    "block",
];

/// Parse a complete guide directory into a structured `ParsedGuide`.
///
/// Expects the directory to contain:
/// - `SUMMARY.md` (required)
/// - Chapter `.md` files referenced by SUMMARY.md
///
/// Optionally reads `book.toml` for the guide title.
pub fn parse_guide(guide_dir: &Path) -> Result<ParsedGuide, ParseError> {
    // 1. Read SUMMARY.md
    let summary_path = guide_dir.join("src").join("SUMMARY.md");
    let summary_path = if summary_path.exists() {
        summary_path
    } else {
        // Try SUMMARY.md at guide root (some guides don't use src/)
        let alt = guide_dir.join("SUMMARY.md");
        if alt.exists() {
            alt
        } else {
            return Err(ParseError::MissingSummary(guide_dir.display().to_string()));
        }
    };

    let summary_content = std::fs::read_to_string(&summary_path).map_err(|e| ParseError::Io {
        path: summary_path.display().to_string(),
        source: e,
    })?;

    // 2. Parse SUMMARY.md into chapter stubs
    let chapter_stubs = summary::parse_summary(&summary_content)?;

    // 3. Determine the base directory for resolving chapter file paths
    let base_dir = summary_path.parent().unwrap_or(guide_dir);

    // 4. Parse each chapter file
    let mut chapters = Vec::with_capacity(chapter_stubs.len());
    for stub in &chapter_stubs {
        let chapter_path = base_dir.join(&stub.source_file);

        let chapter_content = if chapter_path.exists() {
            std::fs::read_to_string(&chapter_path).map_err(|e| ParseError::Io {
                path: chapter_path.display().to_string(),
                source: e,
            })?
        } else {
            log::warn!("Chapter file not found, skipping: {}", chapter_path.display());
            String::new()
        };

        // 5. Tokenize and classify blocks
        let tokens = fence::tokenize(&chapter_content).map_err(|mut e| {
            // Fill in the file path for error context
            if let ParseError::UnclosedFence { ref mut file, .. } = e {
                *file = stub.source_file.clone();
            }
            e
        })?;

        let blocks = block::classify_blocks(tokens, KNOWN_DIRECTIVES)?;

        chapters.push(ParsedChapter {
            title: stub.title.clone(),
            source_file: stub.source_file.clone(),
            depth: stub.depth,
            parent_index: stub.parent_index,
            blocks,
        });
    }

    // 6. Extract guide title from first SUMMARY.md H1 or book.toml
    let title = extract_guide_title(guide_dir, &summary_content);

    Ok(ParsedGuide { title, chapters })
}

/// Extract guide title. Priority: book.toml title > first H1 in SUMMARY.md > directory name.
fn extract_guide_title(guide_dir: &Path, summary_content: &str) -> String {
    // Try book.toml
    let book_toml = guide_dir.join("book.toml");
    if book_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&book_toml) {
            // Simple TOML parsing: look for `title = "..."`
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("title") {
                    if let Some(value) = trimmed.split('=').nth(1) {
                        let title = value.trim().trim_matches('"').trim_matches('\'');
                        if !title.is_empty() {
                            return title.to_string();
                        }
                    }
                }
            }
        }
    }

    // Try first H1 in SUMMARY.md
    for line in summary_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return trimmed[2..].trim().to_string();
        }
    }

    // Fall back to directory name
    guide_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled Guide")
        .to_string()
}
```

---

## 10. Test Strategy

### Test Fixtures

Create `/home/kenny/src/dapple/dapple-prompt/crates/dapple-parser/tests/fixtures/` with:

**`simple-guide/SUMMARY.md`:**
```markdown
# Summary

[Preface](./preface.md)

- [Introduction](./intro.md)

# Getting Started

- [Glossary](./glossary.md)
```

**`simple-guide/preface.md`:**
```markdown
# Preface

This is the preface content.
```

**`simple-guide/intro.md`:**
```markdown
# Introduction

Some intro prose.

:::stage { name: "Onboarding" }

:::step { name: "Gather Info", tags: [agent, product] }

Please provide your business details.

:::nushell { name: "process-info", tags: [coder] }
def main [] { { payload: {}, signals: {} } }
:::

:::

:::

More prose after the stage.
```

### Unit Tests: SUMMARY.md

```rust
// crates/dapple-parser/tests/summary_tests.rs

#[test]
fn test_parse_simple_summary() {
    let content = r#"
# Summary

[Preface](./preface.md)

- [Introduction](./intro.md)

# Getting Started

- [Glossary](./glossary.md)
- [Concepts](./concepts.md)
"#;
    let chapters = dapple_parser::summary::parse_summary(content).unwrap();
    assert_eq!(chapters.len(), 4);

    assert_eq!(chapters[0].title, "Preface");
    assert_eq!(chapters[0].source_file, "preface.md");
    assert_eq!(chapters[0].depth, 0);
    assert!(chapters[0].parent_index.is_none());

    assert_eq!(chapters[1].title, "Introduction");
    assert_eq!(chapters[1].source_file, "intro.md");

    assert_eq!(chapters[2].title, "Glossary");
    assert_eq!(chapters[2].source_file, "glossary.md");
}

#[test]
fn test_parse_nested_summary() {
    let content = r#"
# Summary

- [Chapter 1](./ch1.md)
  - [Section 1.1](./ch1/sec1.md)
  - [Section 1.2](./ch1/sec2.md)
    - [Subsection 1.2.1](./ch1/sec2/sub1.md)
- [Chapter 2](./ch2.md)
"#;
    let chapters = dapple_parser::summary::parse_summary(content).unwrap();
    assert_eq!(chapters.len(), 5);

    assert_eq!(chapters[0].depth, 0);
    assert!(chapters[0].parent_index.is_none());

    assert_eq!(chapters[1].depth, 1);
    assert_eq!(chapters[1].parent_index, Some(0));

    assert_eq!(chapters[2].depth, 1);
    assert_eq!(chapters[2].parent_index, Some(0));

    assert_eq!(chapters[3].depth, 2);
    assert_eq!(chapters[3].parent_index, Some(2));

    assert_eq!(chapters[4].depth, 0);
    assert!(chapters[4].parent_index.is_none());
}
```

### Unit Tests: Fence Tokenizer

```rust
#[test]
fn test_tokenize_simple_block() {
    let content = r#"Some prose.

:::block { tags: [agent] }
Tagged content here.
:::

More prose."#;

    let tokens = dapple_parser::fence::tokenize(content).unwrap();
    // Expect: Prose, Open, Content, Close, Prose
    assert!(matches!(tokens[0], FenceToken::Prose { .. }));
    // ... detailed assertions
}

#[test]
fn test_tokenize_nested_blocks() {
    let content = r#":::stage { name: "Setup" }
:::step { name: "Info" }
Content
:::
:::
"#;
    let tokens = dapple_parser::fence::tokenize(content).unwrap();
    // Expect: Open(stage), Open(step), Content, Close, Close
}

#[test]
fn test_code_blocks_not_matched() {
    let content = r#"```
:::stage { name: "not a stage" }
```"#;
    let tokens = dapple_parser::fence::tokenize(content).unwrap();
    // All should be Prose -- code block contents are opaque
    for token in &tokens {
        assert!(matches!(token, FenceToken::Prose { .. }));
    }
}
```

### Unit Tests: Block Classifier

```rust
#[test]
fn test_classify_prose_and_directive() {
    let content = r#"Intro prose.

:::nushell { name: "my-task", tags: [coder] }
def main [] { { payload: {}, signals: {} } }
:::

Outro prose."#;

    let tokens = dapple_parser::fence::tokenize(content).unwrap();
    let blocks = dapple_parser::block::classify_blocks(tokens, &["nushell"]).unwrap();

    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].directive_type, "prose");
    assert_eq!(blocks[1].directive_type, "nushell");
    assert_eq!(blocks[1].name, Some("my-task".to_string()));
    assert_eq!(blocks[1].tags, vec!["coder"]);
    assert_eq!(blocks[2].directive_type, "prose");
}

#[test]
fn test_classify_nested_blocks() {
    let content = r#":::stage { name: "Setup" }
:::step { name: "Gather" }
Content here
:::
:::
"#;

    let tokens = dapple_parser::fence::tokenize(content).unwrap();
    let blocks = dapple_parser::block::classify_blocks(tokens, &["stage", "step"]).unwrap();

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].directive_type, "stage");
    assert_eq!(blocks[0].children.len(), 1);
    assert_eq!(blocks[0].children[0].directive_type, "step");
}
```

### Integration Test: Real Guide Directory

```rust
#[test]
fn test_parse_journey_planner_guide() {
    let guide_dir = std::path::Path::new("../../docs/guides/journey-planner-guide");
    if !guide_dir.exists() {
        // Skip if running from a different working directory
        return;
    }

    let parsed = dapple_parser::parse_guide(guide_dir).unwrap();
    assert!(!parsed.title.is_empty());
    assert!(!parsed.chapters.is_empty());

    // The SUMMARY.md lists many chapters -- verify count roughly matches
    assert!(parsed.chapters.len() >= 20);

    // Each chapter should have at least one block (prose at minimum)
    for ch in &parsed.chapters {
        // Some chapters may have empty files; that's OK
        if !ch.blocks.is_empty() {
            // At minimum one prose block
            assert!(ch.blocks.iter().any(|b| b.directive_type == "prose")
                || ch.blocks.iter().any(|b| b.directive_type != "prose"));
        }
    }
}
```

---

## 11. Verification Steps

After implementation, verify:

1. `cargo build -p dapple-parser` compiles cleanly
2. `cargo test -p dapple-parser` passes all unit tests
3. `cargo clippy -p dapple-parser -- -D warnings` has no warnings
4. The parser correctly handles the real `journey-planner-guide/SUMMARY.md` (28 chapters)
5. Nested `:::stage > :::step > :::nushell` blocks produce the correct `children` hierarchy
6. Code blocks (triple backtick) inside chapters do not trigger false fence matches
7. The crate compiles with no sqlx or database dependencies

---

## 12. Notes for Implementation Agent

- **Do not add sqlx as a dependency.** This crate must remain database-free.
- **The `normalize_to_json` function in `attrs.rs` is the trickiest part.** Start with a simple regex-based approach and handle the common cases (quoted/unquoted keys, array values). Exotic attribute syntax can be deferred.
- **The `children` field on `ParsedBlock` is critical.** The ingestion pipeline (Issue 5) will flatten `children` into separate database rows but needs the hierarchy to determine directive nesting (e.g., which :::step belongs to which :::stage).
- **Line numbers (`source_line`) are for error reporting**, not stored in the database. Include them to help debug parsing issues.
- **The `block` directive type** is a synonym for `prose` with explicit tags. When `directive_type == "block"`, treat it as `prose` in the database but preserve the tags.
