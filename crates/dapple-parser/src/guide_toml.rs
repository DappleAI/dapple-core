use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Parsed representation of guide.toml (Fluent-native).
///
/// The guide structure is locale-independent. All human-readable text
/// lives in Fluent (.ftl) files under locales/.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideToml {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    // NO locale field -- guide structure is locale-independent.
    // If present in the TOML, serde will ignore it (no field to bind to).
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub classification: Option<String>,
    /// Logical source path stamped onto `rev.guides.source_path` at ingest.
    /// Authors set this to namespace their guides (e.g.
    /// `"public/dapple/essay-writing"` or `"acme/menu-planning"`).
    /// The internal API's `?source_path=...` query param overrides this if
    /// present.
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub card: Option<GuideCardToml>,
    #[serde(default)]
    pub deprecation: Option<GuideDeprecationToml>,
    /// Per-chapter metadata keyed by source file name (e.g., "introduction.md").
    #[serde(default)]
    pub chapters: HashMap<String, ChapterMetadataToml>,
}

/// Per-chapter metadata from [chapters."filename.md"] sections in guide.toml.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChapterMetadataToml {
    #[serde(default)]
    pub subtitle: Option<String>,
}

/// The [deprecation] section of guide.toml.
///
/// Declares that the version being ingested is deprecated. Absence of this
/// section means the version is active. `level = "active"` is invalid —
/// omit the section instead.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideDeprecationToml {
    pub level: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// The [card] section of guide.toml (Fluent-native).
///
/// Subtitle and category are now Fluent attributes on the brief-pack
/// message, not TOML fields. Only locale-independent visual properties
/// (image path, color, accent) remain here.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuideCardToml {
    #[serde(default)]
    pub image: Option<String>, // Relative path to image file
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub accent: Option<String>,
}

/// Valid deprecation levels that can be declared in guide.toml.
const VALID_DEPRECATION_LEVELS: &[&str] = &["deprecated", "retired", "quarantined"];

/// Parse guide.toml from file contents.
pub fn parse_guide_toml(content: &str) -> Result<GuideToml, crate::ParseError> {
    let parsed: GuideToml =
        toml::from_str(content).map_err(|e| crate::ParseError::InvalidGuideToml {
            detail: e.to_string(),
        })?;

    // Validate deprecation level if present
    if let Some(ref dep) = parsed.deprecation {
        if dep.level == "active" {
            return Err(crate::ParseError::InvalidGuideToml {
                detail: "deprecation level \"active\" is invalid; omit the [deprecation] section for active guides".to_string(),
            });
        }
        if !VALID_DEPRECATION_LEVELS.contains(&dep.level.as_str()) {
            return Err(crate::ParseError::InvalidGuideToml {
                detail: format!(
                    "unknown deprecation level \"{}\"; valid levels are: {}",
                    dep.level,
                    VALID_DEPRECATION_LEVELS.join(", ")
                ),
            });
        }
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_guide_toml_full() {
        let toml = r##"
title = "Test Guide"
description = "A test"
author = "Test Author"
classification = "workflow"

[card]
image = "assets/images/card.jpg"
color = "#2D6A4F"
accent = "#95D5B2"
"##;
        let result = parse_guide_toml(toml).unwrap();
        assert_eq!(result.title, "Test Guide");
        assert_eq!(result.description, Some("A test".to_string()));
        assert_eq!(result.author, Some("Test Author".to_string()));
        assert_eq!(result.classification, Some("workflow".to_string()));
        assert!(result.card.is_some());
        let card = result.card.unwrap();
        assert_eq!(card.image, Some("assets/images/card.jpg".to_string()));
        assert_eq!(card.color.as_deref(), Some("#2D6A4F"));
        assert_eq!(card.accent.as_deref(), Some("#95D5B2"));
    }

    #[test]
    fn test_parse_guide_toml_minimal() {
        let toml = r#"title = "Minimal Guide""#;
        let result = parse_guide_toml(toml).unwrap();
        assert_eq!(result.title, "Minimal Guide");
        assert!(result.description.is_none());
        assert!(result.author.is_none());
        assert!(result.classification.is_none());
        assert!(result.card.is_none());
    }

    #[test]
    fn test_parse_guide_toml_with_card_no_image() {
        let toml = r##"
title = "Card Test"

[card]
color = "#FF0000"
"##;
        let result = parse_guide_toml(toml).unwrap();
        let card = result.card.unwrap();
        assert!(card.image.is_none());
        assert_eq!(card.color, Some("#FF0000".to_string()));
        assert!(card.accent.is_none());
    }

    #[test]
    fn test_parse_guide_toml_missing_title() {
        let toml = r#"description = "No title""#;
        let result = parse_guide_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_guide_toml_empty_card() {
        let toml = r#"
title = "Empty Card"

[card]
"#;
        let result = parse_guide_toml(toml).unwrap();
        let card = result.card.unwrap();
        assert!(card.image.is_none());
        assert!(card.color.is_none());
        assert!(card.accent.is_none());
    }

    #[test]
    fn test_parse_guide_toml_with_deprecation() {
        let toml = r##"
title = "Old Guide"
description = "This guide is deprecated"

[deprecation]
level = "deprecated"
reason = "Replaced by newer-guide"
"##;
        let result = parse_guide_toml(toml).unwrap();
        assert!(result.deprecation.is_some());
        let dep = result.deprecation.unwrap();
        assert_eq!(dep.level, "deprecated");
        assert_eq!(dep.reason, Some("Replaced by newer-guide".to_string()));
    }

    #[test]
    fn test_parse_guide_toml_without_deprecation() {
        let toml = r#"title = "Active Guide""#;
        let result = parse_guide_toml(toml).unwrap();
        assert!(result.deprecation.is_none());
    }

    #[test]
    fn test_parse_guide_toml_deprecation_reason_optional() {
        let toml = r##"
title = "Retired Guide"

[deprecation]
level = "retired"
"##;
        let result = parse_guide_toml(toml).unwrap();
        let dep = result.deprecation.unwrap();
        assert_eq!(dep.level, "retired");
        assert!(dep.reason.is_none());
    }

    #[test]
    fn test_parse_guide_toml_deprecation_active_rejected() {
        let toml = r##"
title = "Bad Guide"

[deprecation]
level = "active"
"##;
        let result = parse_guide_toml(toml);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("active"),
            "Error should mention 'active': {}",
            err
        );
    }

    #[test]
    fn test_parse_guide_toml_deprecation_unknown_level_rejected() {
        let toml = r##"
title = "Bad Guide"

[deprecation]
level = "archived"
"##;
        let result = parse_guide_toml(toml);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("archived"),
            "Error should mention 'archived': {}",
            err
        );
    }

    #[test]
    fn test_parse_guide_toml_deprecation_quarantined() {
        let toml = r##"
title = "Quarantined Guide"

[deprecation]
level = "quarantined"
reason = "Under review for compliance"
"##;
        let result = parse_guide_toml(toml).unwrap();
        let dep = result.deprecation.unwrap();
        assert_eq!(dep.level, "quarantined");
        assert_eq!(dep.reason, Some("Under review for compliance".to_string()));
    }

    #[test]
    fn test_parse_guide_toml_ignores_unknown_fields_like_locale() {
        // If a legacy guide.toml still has locale, subtitle, or category,
        // serde should ignore them (deny_unknown_fields is not set).
        let toml = r##"
title = "Legacy Guide"
locale = "en"

[card]
subtitle = "Old subtitle"
category = "old-category"
image = "assets/images/card.jpg"
"##;
        let result = parse_guide_toml(toml).unwrap();
        assert_eq!(result.title, "Legacy Guide");
        // locale is silently ignored -- no field to bind to
        let card = result.card.unwrap();
        assert_eq!(card.image, Some("assets/images/card.jpg".to_string()));
        // subtitle and category are silently ignored
    }

    #[test]
    fn test_parse_guide_toml_with_chapter_metadata() {
        let toml = r##"
title = "Chapter Meta Guide"

[chapters."introduction.md"]
subtitle = "What can I help you with?"

[chapters."search.md"]
subtitle = "Search for guides, topics, and more..."
"##;
        let result = parse_guide_toml(toml).unwrap();
        assert_eq!(result.title, "Chapter Meta Guide");
        assert_eq!(result.chapters.len(), 2);
        assert_eq!(
            result.chapters["introduction.md"].subtitle,
            Some("What can I help you with?".to_string())
        );
        assert_eq!(
            result.chapters["search.md"].subtitle,
            Some("Search for guides, topics, and more...".to_string())
        );
    }

    #[test]
    fn test_parse_guide_toml_no_chapters_section() {
        let toml = r#"title = "No Chapters Meta""#;
        let result = parse_guide_toml(toml).unwrap();
        assert!(result.chapters.is_empty());
    }

    #[test]
    fn test_parse_guide_toml_with_source_path() {
        let toml = r##"
title = "Has Source Path"
source_path = "acme/menu-planning"
"##;
        let result = parse_guide_toml(toml).unwrap();
        assert_eq!(result.source_path, Some("acme/menu-planning".to_string()));
    }

    #[test]
    fn test_parse_guide_toml_source_path_optional() {
        let toml = r#"title = "No Source Path""#;
        let result = parse_guide_toml(toml).unwrap();
        assert!(result.source_path.is_none());
    }
}
