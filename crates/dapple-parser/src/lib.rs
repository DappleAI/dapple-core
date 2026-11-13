pub mod attrs;
pub mod block;
pub mod canonical;
pub mod citations;
pub mod error;
pub mod fence;
pub mod ftl;
pub mod guide_toml;
pub mod prompt_files;
pub mod script_files;
pub mod summary;
pub mod transform;
pub mod types;

pub use error::ParseError;
pub use types::*;

use std::collections::HashMap;
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
    "scope",
    "objective",
    "deliverable",
    "brief",
    "brief_input",
    "brief-input",
    "brief_notice",
    "brief-notice",
    "brief_markdown",
    "brief-markdown",
    "brief_output",
    "brief-output",
    "brief_group",
    "brief-group",
    "value_slot",
    "value-slot",
];

/// Parse a complete guide directory into a structured `ParsedGuide`.
///
/// Expects the directory to contain:
/// - `guide.toml` (preferred) or `book.toml` (legacy fallback) for metadata
/// - `SUMMARY.md` (required) -- looked up at guide root first, then `src/SUMMARY.md`
/// - `CONDUITS.md` (required) -- guide root only; the conduit-facing context
///   frame (delivered whole to the model, never a chapter, never embedded)
/// - Chapter `.md` files referenced by SUMMARY.md
/// - Optional `locales/` directory with `.ftl` files for Fluent-native content
///
/// Non-markdown files in the directory tree are collected as asset paths.
/// Fluent `.ftl` files in `locales/` are parsed into FluentMessageDef entries.
pub fn parse_guide(guide_dir: &Path) -> Result<ParsedGuide, ParseError> {
    // 1. Parse guide.toml (preferred) or book.toml (fallback)
    let metadata = parse_guide_metadata(guide_dir)?;

    // 2. Read SUMMARY.md -- guide root first (guide convention), then src/ (legacy)
    let summary_path = resolve_summary_path(guide_dir)?;
    let summary_content = std::fs::read_to_string(&summary_path).map_err(|e| ParseError::Io {
        path: summary_path.display().to_string(),
        source: e,
    })?;

    // 2.5. Read CONDUITS.md -- required, guide root only (no src/ fallback:
    // the convention postdates the legacy mdBook layout). Checked after
    // guide.toml/SUMMARY.md so their missing-file errors keep precedence.
    let conduits_path = guide_dir.join("CONDUITS.md");
    if !conduits_path.exists() {
        return Err(ParseError::MissingConduits(guide_dir.display().to_string()));
    }
    let conduits_md = std::fs::read_to_string(&conduits_path).map_err(|e| ParseError::Io {
        path: conduits_path.display().to_string(),
        source: e,
    })?;

    // 3. Parse chapters (unchanged logic)
    let chapter_stubs = summary::parse_summary(&summary_content)?;
    let base_dir = summary_path.parent().unwrap_or(guide_dir);
    let chapters = parse_chapter_files(base_dir, &chapter_stubs)?;

    // 4. Discover asset files (non-.md, non-.toml, non-.ftl files in the directory tree)
    let asset_paths = discover_assets(guide_dir);

    // 5. Discover and parse Fluent (.ftl) files from locales/
    let locales = ftl::discover_and_parse_ftl_files(guide_dir)?;

    // 6. Discover nushell script files from scripts/
    let script_files = script_files::discover_script_files(guide_dir);

    Ok(ParsedGuide {
        title: metadata.title.clone(),
        metadata,
        chapters,
        asset_paths,
        locales,
        script_files,
        conduits_md,
    })
}

/// Parse guide metadata from guide.toml (preferred) or book.toml (legacy fallback).
///
/// Priority:
/// 1. `guide.toml` -- parsed with the `toml` crate for full metadata
/// 2. `book.toml` -- legacy fallback with deprecation warning; only extracts title
///
/// If neither file exists, returns `MissingGuideToml` error.
fn parse_guide_metadata(guide_dir: &Path) -> Result<GuideMetadata, ParseError> {
    let guide_toml_path = guide_dir.join("guide.toml");
    if guide_toml_path.exists() {
        let content = std::fs::read_to_string(&guide_toml_path).map_err(|e| ParseError::Io {
            path: guide_toml_path.display().to_string(),
            source: e,
        })?;

        let parsed = guide_toml::parse_guide_toml(&content)?;

        let card = parsed.card.map(|c| ParsedCard {
            image: c.image,
            color: c.color,
            accent: c.accent,
        });

        let (deprecation_level, deprecation_reason) = match parsed.deprecation {
            Some(dep) => (Some(dep.level), dep.reason),
            None => (None, None),
        };

        return Ok(GuideMetadata {
            title: parsed.title,
            description: parsed.description,
            author: parsed.author,
            classification: parsed.classification,
            source_path: parsed.source_path,
            card,
            deprecation_level,
            deprecation_reason,
            chapters: parsed.chapters,
        });
    }

    // Legacy fallback: book.toml
    let book_toml_path = guide_dir.join("book.toml");
    if book_toml_path.exists() {
        log::warn!(
            "guide.toml not found, falling back to book.toml (deprecated): {}",
            book_toml_path.display()
        );
        let title = extract_title_from_book_toml(&book_toml_path);
        return Ok(GuideMetadata {
            title,
            description: None,
            author: None,
            classification: None,
            source_path: None,
            card: None,
            deprecation_level: None,
            deprecation_reason: None,
            chapters: HashMap::new(),
        });
    }

    Err(ParseError::MissingGuideToml(
        guide_dir.display().to_string(),
    ))
}

/// Extract guide title from legacy book.toml using line-by-line parsing.
fn extract_title_from_book_toml(path: &Path) -> String {
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("title")
                && let Some(value) = trimmed.split('=').nth(1)
            {
                let title = value.trim().trim_matches('"').trim_matches('\'');
                if !title.is_empty() {
                    return title.to_string();
                }
            }
        }
    }
    // Fall back to parent directory name
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled Guide")
        .to_string()
}

/// Resolve the path to SUMMARY.md.
///
/// Lookup order (reversed from legacy mdBook convention):
/// 1. `{guide_dir}/SUMMARY.md` -- guide convention
/// 2. `{guide_dir}/src/SUMMARY.md` -- legacy mdBook convention
fn resolve_summary_path(guide_dir: &Path) -> Result<std::path::PathBuf, ParseError> {
    // Guide root first (new convention)
    let root_summary = guide_dir.join("SUMMARY.md");
    if root_summary.exists() {
        return Ok(root_summary);
    }

    // Legacy: src/SUMMARY.md
    let src_summary = guide_dir.join("src").join("SUMMARY.md");
    if src_summary.exists() {
        return Ok(src_summary);
    }

    Err(ParseError::MissingSummary(guide_dir.display().to_string()))
}

/// Parse chapter markdown files from their stubs.
fn parse_chapter_files(
    base_dir: &Path,
    chapter_stubs: &[types::ParsedChapterStub],
) -> Result<Vec<ParsedChapter>, ParseError> {
    let mut chapters = Vec::with_capacity(chapter_stubs.len());

    for stub in chapter_stubs {
        let chapter_path = base_dir.join(&stub.source_file);

        let chapter_content = if chapter_path.exists() {
            std::fs::read_to_string(&chapter_path).map_err(|e| ParseError::Io {
                path: chapter_path.display().to_string(),
                source: e,
            })?
        } else {
            log::warn!(
                "Chapter file not found, skipping: {}",
                chapter_path.display()
            );
            String::new()
        };

        // Tokenize and classify blocks
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

    Ok(chapters)
}

/// Discover non-markdown, non-config, non-ftl files in the guide directory tree.
///
/// Returns relative paths (from guide_dir) for files that are potential assets:
/// - Excludes `.md` files (chapter content)
/// - Excludes `.toml` files (guide/book config)
/// - Excludes `.ftl` files (Fluent locale files)
/// - Excludes `SUMMARY.md` (already processed)
/// - Excludes hidden files and directories (starting with `.`)
/// - Excludes the `locales/` directory (contains only .ftl files)
fn discover_assets(guide_dir: &Path) -> Vec<String> {
    let mut assets = Vec::new();
    discover_assets_recursive(guide_dir, guide_dir, &mut assets);
    assets.sort();
    assets
}

fn discover_assets_recursive(root: &Path, current: &Path, assets: &mut Vec<String>) {
    let entries = match std::fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden files/directories
        if name.starts_with('.') {
            continue;
        }

        // Skip the locales/ directory (contains .ftl files, not assets)
        if path.is_dir() && name == "locales" && current == root {
            continue;
        }

        if path.is_dir() {
            discover_assets_recursive(root, &path, assets);
        } else if path.is_file() {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            // Skip markdown, toml, and ftl files
            if ext == "md" || ext == "toml" || ext == "ftl" {
                continue;
            }

            // Skip SUMMARY.md explicitly (already handled by extension filter above,
            // but being defensive)
            if name == "SUMMARY.md" {
                continue;
            }

            // Compute relative path from guide root
            if let Ok(relative) = path.strip_prefix(root) {
                assets.push(relative.to_string_lossy().to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_guide_with_guide_toml() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        // Create guide.toml (Fluent-native -- no locale field)
        fs::write(
            guide_dir.join("guide.toml"),
            r##"
title = "Test Guide"
description = "A test guide"
author = "Test Author"
classification = "tutorial"

[card]
image = "assets/images/card.jpg"
color = "#2D6A4F"
"##,
        )
        .unwrap();

        // Create SUMMARY.md
        fs::write(
            guide_dir.join("SUMMARY.md"),
            "- [Introduction](./intro.md)\n",
        )
        .unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

        // Create chapter
        fs::write(guide_dir.join("intro.md"), "# Introduction\n\nHello world.").unwrap();

        let result = parse_guide(guide_dir).unwrap();
        assert_eq!(result.title, "Test Guide");
        assert_eq!(
            result.metadata.description,
            Some("A test guide".to_string())
        );
        assert_eq!(result.metadata.author, Some("Test Author".to_string()));
        assert_eq!(result.metadata.classification, Some("tutorial".to_string()));
        assert!(result.metadata.card.is_some());
        let card = result.metadata.card.unwrap();
        assert_eq!(card.image, Some("assets/images/card.jpg".to_string()));
        assert_eq!(card.color, Some("#2D6A4F".to_string()));
        assert_eq!(result.chapters.len(), 1);
        assert_eq!(result.chapters[0].title, "Introduction");
        // No locales directory, so locales map is empty
        assert!(result.locales.is_empty());
    }

    #[test]
    fn test_parse_guide_with_book_toml_fallback() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        // Create book.toml (legacy)
        fs::write(guide_dir.join("book.toml"), "title = \"Legacy Guide\"\n").unwrap();

        // Create SUMMARY.md
        fs::write(guide_dir.join("SUMMARY.md"), "- [Intro](./intro.md)\n").unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

        // Create chapter
        fs::write(guide_dir.join("intro.md"), "# Intro\n\nContent.").unwrap();

        let result = parse_guide(guide_dir).unwrap();
        assert_eq!(result.title, "Legacy Guide");
        assert!(result.metadata.description.is_none());
        assert!(result.metadata.card.is_none());
    }

    #[test]
    fn test_parse_guide_no_config_file() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        // Create SUMMARY.md but no config file
        fs::write(guide_dir.join("SUMMARY.md"), "- [Intro](./intro.md)\n").unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

        let result = parse_guide(guide_dir);
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::MissingGuideToml(_) => {} // Expected
            e => panic!("Expected MissingGuideToml, got: {:?}", e),
        }
    }

    #[test]
    fn test_parse_guide_missing_conduits() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        fs::write(guide_dir.join("guide.toml"), "title = \"No Frame\"\n").unwrap();
        fs::write(guide_dir.join("SUMMARY.md"), "- [Intro](./intro.md)\n").unwrap();
        fs::write(guide_dir.join("intro.md"), "# Intro\n\nContent.").unwrap();
        // No CONDUITS.md — the guide must be rejected.

        let result = parse_guide(guide_dir);
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::MissingConduits(_) => {} // Expected
            e => panic!("Expected MissingConduits, got: {:?}", e),
        }
    }

    #[test]
    fn test_parse_guide_conduits_content_round_trips() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        fs::write(guide_dir.join("guide.toml"), "title = \"Framed\"\n").unwrap();
        fs::write(guide_dir.join("SUMMARY.md"), "- [Intro](./intro.md)\n").unwrap();
        fs::write(guide_dir.join("intro.md"), "# Intro\n\nContent.").unwrap();
        let frame = "# Conduit Context\n\n## Bindings\n\n- `topic` (text) — the essay topic.\n";
        fs::write(guide_dir.join("CONDUITS.md"), frame).unwrap();

        let result = parse_guide(guide_dir).unwrap();
        assert_eq!(result.conduits_md, frame);
    }

    #[test]
    fn test_summary_lookup_order() {
        // Guide root SUMMARY.md should be preferred over src/SUMMARY.md
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        fs::write(
            guide_dir.join("guide.toml"),
            "title = \"Root Summary Test\"\n",
        )
        .unwrap();

        // Create both SUMMARY.md locations
        fs::write(
            guide_dir.join("SUMMARY.md"),
            "- [Root Chapter](./root-ch.md)\n",
        )
        .unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();
        fs::create_dir(guide_dir.join("src")).unwrap();
        fs::write(
            guide_dir.join("src").join("SUMMARY.md"),
            "- [Src Chapter](./src-ch.md)\n",
        )
        .unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

        // Create chapter files
        fs::write(guide_dir.join("root-ch.md"), "# Root\n\nContent.").unwrap();
        fs::write(guide_dir.join("src").join("src-ch.md"), "# Src\n\nContent.").unwrap();

        let result = parse_guide(guide_dir).unwrap();
        // Should pick root SUMMARY.md, which references root-ch.md
        assert_eq!(result.chapters[0].title, "Root Chapter");
    }

    #[test]
    fn test_discover_assets() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        fs::write(guide_dir.join("guide.toml"), "title = \"Asset Test\"\n").unwrap();
        fs::write(guide_dir.join("SUMMARY.md"), "- [Ch](./ch.md)\n").unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();
        fs::write(guide_dir.join("ch.md"), "# Chapter\n\nContent.").unwrap();

        // Create some asset files
        fs::create_dir_all(guide_dir.join("assets/images")).unwrap();
        fs::write(
            guide_dir.join("assets/images").join("card.jpg"),
            b"fake-jpg",
        )
        .unwrap();
        fs::write(
            guide_dir.join("assets/images").join("diagram.png"),
            b"fake-png",
        )
        .unwrap();
        fs::write(guide_dir.join("data.csv"), b"a,b,c").unwrap();

        let result = parse_guide(guide_dir).unwrap();
        assert_eq!(result.asset_paths.len(), 3);
        assert!(result.asset_paths.contains(&"data.csv".to_string()));
        assert!(
            result
                .asset_paths
                .contains(&"assets/images/card.jpg".to_string())
        );
        assert!(
            result
                .asset_paths
                .contains(&"assets/images/diagram.png".to_string())
        );
    }

    #[test]
    fn test_discover_assets_excludes_hidden_and_config() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        fs::write(guide_dir.join("guide.toml"), "title = \"Exclusion Test\"\n").unwrap();
        fs::write(guide_dir.join("SUMMARY.md"), "- [Ch](./ch.md)\n").unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();
        fs::write(guide_dir.join("ch.md"), "Content.").unwrap();

        // Hidden file should be excluded
        fs::write(guide_dir.join(".gitignore"), "*.bak").unwrap();
        // Config files should be excluded by extension
        fs::write(guide_dir.join("config.toml"), "key = \"value\"").unwrap();
        // Markdown files should be excluded
        fs::write(guide_dir.join("notes.md"), "# Notes").unwrap();
        // Only this should be discovered
        fs::write(guide_dir.join("photo.jpg"), b"fake").unwrap();

        let result = parse_guide(guide_dir).unwrap();
        assert_eq!(result.asset_paths.len(), 1);
        assert_eq!(result.asset_paths[0], "photo.jpg");
    }

    #[test]
    fn test_discover_assets_excludes_ftl_files_and_locales_dir() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        fs::write(guide_dir.join("guide.toml"), "title = \"FTL Test\"\n").unwrap();
        fs::write(guide_dir.join("SUMMARY.md"), "- [Ch](./ch.md)\n").unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();
        fs::write(guide_dir.join("ch.md"), "Content.").unwrap();

        // Create locales directory with .ftl files
        fs::create_dir(guide_dir.join("locales")).unwrap();
        fs::write(guide_dir.join("locales").join("en.ftl"), "hello = Hello\n").unwrap();
        fs::write(guide_dir.join("locales").join("pt-BR.ftl"), "hello = Ola\n").unwrap();

        // Create a non-ftl asset
        fs::write(guide_dir.join("image.png"), b"fake-png").unwrap();

        let result = parse_guide(guide_dir).unwrap();
        // Only the image should be discovered as an asset
        assert_eq!(result.asset_paths.len(), 1);
        assert_eq!(result.asset_paths[0], "image.png");
        // Locales should be parsed
        assert_eq!(result.locales.len(), 2);
        assert!(result.locales.contains_key("en"));
        assert!(result.locales.contains_key("pt-BR"));
    }

    #[test]
    fn test_parse_guide_with_locales() {
        let dir = TempDir::new().unwrap();
        let guide_dir = dir.path();

        fs::write(guide_dir.join("guide.toml"), "title = \"Fluent Guide\"\n").unwrap();
        fs::write(guide_dir.join("SUMMARY.md"), "- [Ch](./ch.md)\n").unwrap();
        fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();
        fs::write(guide_dir.join("ch.md"), "# Chapter\n\nSome prose.").unwrap();

        fs::create_dir(guide_dir.join("locales")).unwrap();
        fs::write(
            guide_dir.join("locales").join("en.ftl"),
            r#"topic-selection = Guide the user through topic selection.
brainstorm = Generate topic ideas.
    .description = Extended brainstorm description
"#,
        )
        .unwrap();

        let result = parse_guide(guide_dir).unwrap();
        assert_eq!(result.locales.len(), 1);

        let en_messages = &result.locales["en"];
        assert_eq!(en_messages.len(), 2);

        let topic = en_messages
            .iter()
            .find(|m| m.key == "topic-selection")
            .unwrap();
        assert!(topic.body.contains("Guide the user"));

        let brainstorm = en_messages.iter().find(|m| m.key == "brainstorm").unwrap();
        assert_eq!(
            brainstorm.attributes.get("description"),
            Some(&"Extended brainstorm description".to_string())
        );
    }
}
