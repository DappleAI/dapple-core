//! Discovery and parsing of resource citations from `resources.md` and block cite attributes.
//!
//! Guides can include a `resources.md` file listing external resources (PDFs, URLs)
//! with structured metadata. Prose blocks can reference these resources via `cite`
//! and `cite_pages` attributes in their directive metadata.
//!
//! ## resources.md format
//!
//! ```markdown
//! # Resources
//!
//! ## portugal-business-plan
//! - **Title:** Starting a Business in Portugal: Complete Guide 2025
//! - **URL:** https://example.com/portugal-business-plan.pdf
//! - **Path:** assets/resources/portugal-business-plan.pdf
//! - **Retrieved:** 2026-04-05
//! ```
//!
//! ## Citation attribute on prose blocks
//!
//! ```markdown
//! :::prose { name: "reg-requirements", tags: [product], cite: "portugal-business-plan", cite_pages: "12-14" }
//! Content here...
//! :::
//! ```

use std::path::Path;

use crate::transform::{BlockDef, ChapterDef};

/// A resource entry parsed from `resources.md`.
#[derive(Clone, Debug)]
pub struct ResourceDef {
    /// Kebab-case key from the `##` heading.
    pub key: String,
    /// Title from `- **Title:** ...` line.
    pub title: Option<String>,
    /// URL from `- **URL:** ...` line.
    pub url: Option<String>,
    /// Local path within the guide directory from `- **Path:** ...` line.
    pub path: Option<String>,
    /// Retrieval date from `- **Retrieved:** ...` line.
    pub retrieved: Option<String>,
}

/// A citation linking a block to a resource.
#[derive(Clone, Debug)]
pub struct CitationDef {
    /// Key matching a `ResourceDef.key`.
    pub resource_key: String,
    /// Optional page range (e.g., "12-14").
    pub pages: Option<String>,
    /// The name attribute of the citing block.
    pub block_name: String,
    /// Index of the chapter containing the citing block.
    pub chapter_index: usize,
}

/// A block carrying a keyed `cite` attribute but no `name`, whose citation was dropped.
///
/// [`extract_citations_from_blocks`] requires a `name` because a citation row is joined
/// back to its materialized block by name; an anonymous block has nothing to attach to.
/// Skipping it silently means an authored citation never reaches the database, and the
/// resource-level "not cited by any block" warning only catches the degenerate case where
/// EVERY citation for that resource was dropped. A guide that got 28 of 29 right would
/// ship a silently-missing citation with a clean bill of health.
///
/// Only **keyed** cites are reported. A cite value that matches no `resources.md` key is a
/// free-text citation (a licensed or web-only authority such as `ASC 606-10-25`), which is
/// deliberately not machine-tracked and has nothing to drop.
#[derive(Clone, Debug)]
pub struct DroppedCitation {
    /// The `cite` value whose citation was dropped.
    pub resource_key: String,
    /// `source_file` of the chapter containing the block.
    pub chapter_source: String,
    /// Directive type of the block (e.g. `prose`).
    pub directive_type: String,
    /// Leading excerpt of the block body, so the author can locate an unnamed block.
    pub excerpt: String,
}

/// Characters of block body carried in a dropped-citation warning.
const EXCERPT_CHARS: usize = 60;

/// A short, single-line excerpt of a block's body for use in diagnostics.
///
/// An unnamed block has no stable identifier, so the body's opening words are the only
/// handle an author can search for.
fn block_excerpt(block: &BlockDef) -> String {
    let flat = block
        .content
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if flat.is_empty() {
        return String::from("(no inline body)");
    }
    if flat.chars().count() <= EXCERPT_CHARS {
        return flat;
    }
    let head: String = flat.chars().take(EXCERPT_CHARS).collect();
    format!("{head}...")
}

/// Find blocks whose keyed citation was dropped for lack of a `name` attribute.
///
/// Returns empty when the guide declares no resources, since every cite is then free text.
pub fn find_dropped_citations(
    chapters: &[ChapterDef],
    resources: &[ResourceDef],
) -> Vec<DroppedCitation> {
    if resources.is_empty() {
        return Vec::new();
    }

    let mut dropped = Vec::new();

    for chapter in chapters {
        for block in &chapter.blocks {
            let Some(resource_key) = block.attributes.get("cite").and_then(|v| v.as_str()) else {
                continue;
            };
            // A named block's citation is extracted normally; an unresolvable key is a
            // free-text cite and was never storable.
            if block.name.is_some() || !resources.iter().any(|r| r.key == resource_key) {
                continue;
            }

            dropped.push(DroppedCitation {
                resource_key: resource_key.to_string(),
                chapter_source: chapter.source_file.clone(),
                directive_type: block.directive_type.clone(),
                excerpt: block_excerpt(block),
            });
        }
    }

    dropped
}

/// Parse a `resources.md` file from the guide directory.
///
/// Reads `{guide_dir}/resources.md` if it exists. Parses `##` headings as
/// resource keys and extracts metadata from `- **Key:** Value` lines beneath
/// each heading.
///
/// Returns an empty vec if the file does not exist.
pub fn parse_resources_file(guide_dir: &Path) -> Vec<ResourceDef> {
    let resources_path = guide_dir.join("resources.md");
    if !resources_path.exists() {
        return Vec::new();
    }

    let content = match std::fs::read_to_string(&resources_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    parse_resources_content(&content)
}

/// Parse the content of a resources.md file into `ResourceDef` entries.
fn parse_resources_content(content: &str) -> Vec<ResourceDef> {
    let mut resources = Vec::new();
    let mut current: Option<ResourceDef> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect ## heading — starts a new resource entry
        if let Some(heading) = trimmed.strip_prefix("## ") {
            // Push any in-progress resource
            if let Some(res) = current.take() {
                resources.push(res);
            }
            current = Some(ResourceDef {
                key: heading.trim().to_string(),
                title: None,
                url: None,
                path: None,
                retrieved: None,
            });
            continue;
        }

        // Parse metadata lines under the current heading
        if let Some(ref mut res) = current {
            if let Some(value) = extract_metadata_value(trimmed, "Title") {
                res.title = Some(value);
            } else if let Some(value) = extract_metadata_value(trimmed, "URL") {
                res.url = Some(value);
            } else if let Some(value) = extract_metadata_value(trimmed, "Path") {
                res.path = Some(value);
            } else if let Some(value) = extract_metadata_value(trimmed, "Retrieved") {
                res.retrieved = Some(value);
            }
        }
    }

    // Push the last resource if any
    if let Some(res) = current {
        resources.push(res);
    }

    resources
}

/// Extract a metadata value from a line matching `- **Key:** Value`.
fn extract_metadata_value(line: &str, key: &str) -> Option<String> {
    let prefix = format!("- **{key}:**");
    if let Some(rest) = line.strip_prefix(&prefix) {
        let value = rest.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    } else {
        None
    }
}

/// Extract citation definitions from all blocks across chapters.
///
/// Walks the chapter/block tree and looks for `cite` and `cite_pages`
/// attributes in block attributes JSON. Only blocks with both a `name`
/// and a `cite` attribute produce a `CitationDef`.
pub fn extract_citations_from_blocks(chapters: &[ChapterDef]) -> Vec<CitationDef> {
    let mut citations = Vec::new();

    for (ch_idx, chapter) in chapters.iter().enumerate() {
        for block in &chapter.blocks {
            let resource_key = block
                .attributes
                .get("cite")
                .and_then(|v| v.as_str())
                .map(String::from);

            let Some(resource_key) = resource_key else {
                continue;
            };

            let Some(ref block_name) = block.name else {
                continue;
            };

            let pages = block
                .attributes
                .get("cite_pages")
                .and_then(|v| v.as_str())
                .map(String::from);

            citations.push(CitationDef {
                resource_key,
                pages,
                block_name: block_name.clone(),
                chapter_index: ch_idx,
            });
        }
    }

    citations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::BlockDef;
    use std::fs;

    #[test]
    fn test_parse_resources_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let guide_dir = tmp.path();

        fs::write(
            guide_dir.join("resources.md"),
            r#"# Resources

## portugal-business-plan
- **Title:** Starting a Business in Portugal: Complete Guide 2025
- **URL:** https://example.com/portugal-business-plan.pdf
- **Path:** assets/resources/portugal-business-plan.pdf
- **Retrieved:** 2026-04-05

## startup-registration-guide
- **Title:** Step-by-Step Company Registration in Portugal
- **URL:** https://example.com/startup-registration.pdf
- **Path:** assets/resources/startup-registration-guide.pdf
- **Retrieved:** 2026-04-05
"#,
        )
        .unwrap();

        let resources = parse_resources_file(guide_dir);
        assert_eq!(resources.len(), 2);

        assert_eq!(resources[0].key, "portugal-business-plan");
        assert_eq!(
            resources[0].title.as_deref(),
            Some("Starting a Business in Portugal: Complete Guide 2025")
        );
        assert_eq!(
            resources[0].url.as_deref(),
            Some("https://example.com/portugal-business-plan.pdf")
        );
        assert_eq!(
            resources[0].path.as_deref(),
            Some("assets/resources/portugal-business-plan.pdf")
        );
        assert_eq!(resources[0].retrieved.as_deref(), Some("2026-04-05"));

        assert_eq!(resources[1].key, "startup-registration-guide");
        assert_eq!(
            resources[1].title.as_deref(),
            Some("Step-by-Step Company Registration in Portugal")
        );
    }

    #[test]
    fn test_parse_resources_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let resources = parse_resources_file(tmp.path());
        assert!(resources.is_empty());
    }

    #[test]
    fn test_parse_resources_partial_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let guide_dir = tmp.path();

        fs::write(
            guide_dir.join("resources.md"),
            r#"# Resources

## only-title-resource
- **Title:** A Resource With Only a Title

## only-url-resource
- **URL:** https://example.com/doc.pdf
"#,
        )
        .unwrap();

        let resources = parse_resources_file(guide_dir);
        assert_eq!(resources.len(), 2);

        assert_eq!(resources[0].key, "only-title-resource");
        assert_eq!(
            resources[0].title.as_deref(),
            Some("A Resource With Only a Title")
        );
        assert!(resources[0].url.is_none());
        assert!(resources[0].path.is_none());
        assert!(resources[0].retrieved.is_none());

        assert_eq!(resources[1].key, "only-url-resource");
        assert!(resources[1].title.is_none());
        assert_eq!(
            resources[1].url.as_deref(),
            Some("https://example.com/doc.pdf")
        );
    }

    #[test]
    fn test_extract_citations() {
        let chapters = vec![ChapterDef {
            title: "Chapter 1".to_string(),
            subtitle: None,
            source_file: "ch1.md".to_string(),
            parent_index: None,
            blocks: vec![
                BlockDef {
                    directive_type: "prose".to_string(),
                    name: Some("reg-requirements".to_string()),
                    content: Some("Content about registration.".to_string()),
                    attributes: serde_json::json!({
                        "name": "reg-requirements",
                        "tags": ["product"],
                        "cite": "portugal-business-plan",
                        "cite_pages": "12-14"
                    }),
                    tags: vec!["product".to_string()],
                },
                BlockDef {
                    directive_type: "prose".to_string(),
                    name: Some("tax-overview".to_string()),
                    content: Some("Tax information.".to_string()),
                    attributes: serde_json::json!({
                        "name": "tax-overview",
                        "cite": "startup-registration-guide"
                    }),
                    tags: Vec::new(),
                },
            ],
        }];

        let citations = extract_citations_from_blocks(&chapters);
        assert_eq!(citations.len(), 2);

        assert_eq!(citations[0].resource_key, "portugal-business-plan");
        assert_eq!(citations[0].pages.as_deref(), Some("12-14"));
        assert_eq!(citations[0].block_name, "reg-requirements");
        assert_eq!(citations[0].chapter_index, 0);

        assert_eq!(citations[1].resource_key, "startup-registration-guide");
        assert!(citations[1].pages.is_none());
        assert_eq!(citations[1].block_name, "tax-overview");
        assert_eq!(citations[1].chapter_index, 0);
    }

    #[test]
    fn test_extract_citations_no_cites() {
        let chapters = vec![ChapterDef {
            title: "Chapter 1".to_string(),
            subtitle: None,
            source_file: "ch1.md".to_string(),
            parent_index: None,
            blocks: vec![
                BlockDef {
                    directive_type: "prose".to_string(),
                    name: Some("intro".to_string()),
                    content: Some("Introduction text.".to_string()),
                    attributes: serde_json::json!({
                        "name": "intro",
                        "tags": ["product"]
                    }),
                    tags: vec!["product".to_string()],
                },
                BlockDef {
                    directive_type: "nushell".to_string(),
                    name: Some("my-task".to_string()),
                    content: Some("echo hello".to_string()),
                    attributes: serde_json::json!({ "name": "my-task" }),
                    tags: Vec::new(),
                },
            ],
        }];

        let citations = extract_citations_from_blocks(&chapters);
        assert!(citations.is_empty());
    }
}
