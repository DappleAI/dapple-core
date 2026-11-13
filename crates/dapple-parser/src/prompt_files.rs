//! Discovery and parsing of prompt files from the `prompts/` directory.
//!
//! Prompts live in `prompts/<prompt-name>/` directories within a guide.
//! Each file follows the naming convention `<locale>[.<lineage>[.<tier>]].md`:
//!
//! - `en.md` — default English localized body
//! - `en.claude-4-6.md` — Claude 4.6 family variant
//! - `en.claude-4-6.sonnet.md` — Claude 4.6 Sonnet specific
//! - `pt-BR.md` — Portuguese localized body
//!
//! Provider is NOT part of the filename — it's an infrastructure concern.
//! Resolution fallback: `en.claude-4-6.sonnet.md` → `en.claude-4-6.md` → `en.md`

use std::collections::HashMap;
use std::path::Path;

/// A prompt discovered from the `prompts/` directory.
#[derive(Clone, Debug)]
pub struct PromptFileDef {
    /// Prompt name (directory name, kebab-case).
    pub name: String,
    /// Localized bodies keyed by locale tag (e.g., "en", "pt-BR").
    pub localized_bodies: HashMap<String, String>,
    /// Engine-specific body variants.
    pub engine_bodies: Vec<EngineBodyFileDef>,
}

/// An engine-specific prompt body variant.
#[derive(Clone, Debug)]
pub struct EngineBodyFileDef {
    /// Locale tag (e.g., "en").
    pub locale: String,
    /// Model lineage key (e.g., "claude-4-6", "mistral").
    pub lineage: String,
    /// Optional tier within the lineage (e.g., "sonnet", "opus").
    pub tier: Option<String>,
    /// The prompt body text.
    pub body: String,
}

/// Parsed components of a prompt filename.
#[derive(Debug)]
struct ParsedFilename {
    locale: String,
    lineage: Option<String>,
    tier: Option<String>,
}

/// Parse a prompt filename into its components.
///
/// Pattern: `<locale>[.<lineage>[.<tier>]].md`
///
/// Examples:
/// - `en.md` → locale="en", lineage=None, tier=None
/// - `en.claude-4-6.md` → locale="en", lineage="claude-4-6", tier=None
/// - `en.claude-4-6.sonnet.md` → locale="en", lineage="claude-4-6", tier="sonnet"
/// - `pt-BR.md` → locale="pt-BR", lineage=None, tier=None
fn parse_prompt_filename(filename: &str) -> Option<ParsedFilename> {
    let stem = filename.strip_suffix(".md")?;
    let parts: Vec<&str> = stem.splitn(3, '.').collect();

    match parts.len() {
        1 => Some(ParsedFilename {
            locale: parts[0].to_string(),
            lineage: None,
            tier: None,
        }),
        2 => Some(ParsedFilename {
            locale: parts[0].to_string(),
            lineage: Some(parts[1].to_string()),
            tier: None,
        }),
        3 => Some(ParsedFilename {
            locale: parts[0].to_string(),
            lineage: Some(parts[1].to_string()),
            tier: Some(parts[2].to_string()),
        }),
        _ => None,
    }
}

/// Discover and parse all prompt files from the `prompts/` directory.
///
/// Returns an empty vec if no `prompts/` directory exists.
pub fn discover_prompt_files(guide_dir: &Path) -> Vec<PromptFileDef> {
    let prompts_dir = guide_dir.join("prompts");
    if !prompts_dir.is_dir() {
        return Vec::new();
    }

    let mut prompts = Vec::new();

    let entries = match std::fs::read_dir(&prompts_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let prompt_dir = entry.path();
        if !prompt_dir.is_dir() {
            continue;
        }
        let Some(name) = prompt_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Skip hidden directories
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }

        let mut localized_bodies = HashMap::new();
        let mut engine_bodies = Vec::new();

        let files = match std::fs::read_dir(&prompt_dir) {
            Ok(f) => f,
            Err(_) => continue,
        };

        for file_entry in files.flatten() {
            let file_path = file_entry.path();
            if !file_path.is_file() {
                continue;
            }
            let Some(filename) = file_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !filename.ends_with(".md") {
                continue;
            }

            let Some(parsed) = parse_prompt_filename(filename) else {
                continue;
            };

            let body = match std::fs::read_to_string(&file_path) {
                Ok(content) => content,
                Err(_) => continue,
            };

            if let Some(lineage) = parsed.lineage {
                engine_bodies.push(EngineBodyFileDef {
                    locale: parsed.locale,
                    lineage,
                    tier: parsed.tier,
                    body,
                });
            } else {
                localized_bodies.insert(parsed.locale, body);
            }
        }

        // Only include if there's at least one body
        if !localized_bodies.is_empty() || !engine_bodies.is_empty() {
            prompts.push(PromptFileDef {
                name: name.to_string(),
                localized_bodies,
                engine_bodies,
            });
        }
    }

    prompts.sort_by(|a, b| a.name.cmp(&b.name));
    prompts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_prompt_filename_locale_only() {
        let parsed = parse_prompt_filename("en.md").unwrap();
        assert_eq!(parsed.locale, "en");
        assert!(parsed.lineage.is_none());
        assert!(parsed.tier.is_none());
    }

    #[test]
    fn test_parse_prompt_filename_locale_and_lineage() {
        let parsed = parse_prompt_filename("en.claude-4-6.md").unwrap();
        assert_eq!(parsed.locale, "en");
        assert_eq!(parsed.lineage.as_deref(), Some("claude-4-6"));
        assert!(parsed.tier.is_none());
    }

    #[test]
    fn test_parse_prompt_filename_full() {
        let parsed = parse_prompt_filename("en.claude-4-6.sonnet.md").unwrap();
        assert_eq!(parsed.locale, "en");
        assert_eq!(parsed.lineage.as_deref(), Some("claude-4-6"));
        assert_eq!(parsed.tier.as_deref(), Some("sonnet"));
    }

    #[test]
    fn test_parse_prompt_filename_locale_with_region() {
        let parsed = parse_prompt_filename("pt-BR.md").unwrap();
        assert_eq!(parsed.locale, "pt-BR");
        assert!(parsed.lineage.is_none());
    }

    #[test]
    fn test_parse_prompt_filename_not_md() {
        assert!(parse_prompt_filename("en.txt").is_none());
    }

    #[test]
    fn test_discover_prompt_files_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let prompts = discover_prompt_files(tmp.path());
        assert!(prompts.is_empty());
    }

    #[test]
    fn test_discover_prompt_files_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let prompt_dir = tmp.path().join("prompts").join("my-prompt");
        fs::create_dir_all(&prompt_dir).unwrap();
        fs::write(prompt_dir.join("en.md"), "Hello world prompt").unwrap();
        fs::write(
            prompt_dir.join("en.claude-4-6.md"),
            "Claude-specific prompt",
        )
        .unwrap();

        let prompts = discover_prompt_files(tmp.path());
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "my-prompt");
        assert_eq!(
            prompts[0].localized_bodies.get("en").unwrap(),
            "Hello world prompt"
        );
        assert_eq!(prompts[0].engine_bodies.len(), 1);
        assert_eq!(prompts[0].engine_bodies[0].lineage, "claude-4-6");
        assert_eq!(prompts[0].engine_bodies[0].body, "Claude-specific prompt");
    }

    #[test]
    fn test_discover_prompt_files_multiple_prompts() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = tmp.path().join("prompts").join("alpha-prompt");
        let p2 = tmp.path().join("prompts").join("beta-prompt");
        fs::create_dir_all(&p1).unwrap();
        fs::create_dir_all(&p2).unwrap();
        fs::write(p1.join("en.md"), "Alpha").unwrap();
        fs::write(p2.join("en.md"), "Beta").unwrap();

        let prompts = discover_prompt_files(tmp.path());
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].name, "alpha-prompt");
        assert_eq!(prompts[1].name, "beta-prompt");
    }

    #[test]
    fn test_discover_prompt_files_with_tier() {
        let tmp = tempfile::tempdir().unwrap();
        let prompt_dir = tmp.path().join("prompts").join("my-prompt");
        fs::create_dir_all(&prompt_dir).unwrap();
        fs::write(prompt_dir.join("en.md"), "Default").unwrap();
        fs::write(prompt_dir.join("en.claude-4-6.sonnet.md"), "Sonnet variant").unwrap();
        fs::write(prompt_dir.join("en.mistral.md"), "Mistral variant").unwrap();

        let prompts = discover_prompt_files(tmp.path());
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].localized_bodies.len(), 1);
        assert_eq!(prompts[0].engine_bodies.len(), 2);

        let sonnet = prompts[0]
            .engine_bodies
            .iter()
            .find(|e| e.lineage == "claude-4-6")
            .unwrap();
        assert_eq!(sonnet.tier.as_deref(), Some("sonnet"));
        assert_eq!(sonnet.body, "Sonnet variant");

        let mistral = prompts[0]
            .engine_bodies
            .iter()
            .find(|e| e.lineage == "mistral")
            .unwrap();
        assert!(mistral.tier.is_none());
    }

    #[test]
    fn test_discover_prompt_files_skips_empty_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let empty_dir = tmp.path().join("prompts").join("empty-prompt");
        fs::create_dir_all(&empty_dir).unwrap();
        // No .md files

        let prompts = discover_prompt_files(tmp.path());
        assert!(prompts.is_empty());
    }
}
