//! Fluent (.ftl) file discovery and parsing.
//!
//! Scans the `locales/` directory for `.ftl` files, parses them using
//! `fluent-syntax`, and extracts message keys, bodies, and attributes
//! into `FluentMessageDef` structs.

use crate::error::ParseError;
use crate::types::FluentMessageDef;
use std::collections::HashMap;
use std::path::Path;

/// Discover and parse all `.ftl` files in the `locales/` directory.
///
/// Returns a map from locale tag (derived from the file stem, e.g., "en" from
/// "en.ftl") to a vector of `FluentMessageDef` entries parsed from that file.
///
/// Returns an empty map if the `locales/` directory does not exist.
pub fn discover_and_parse_ftl_files(
    guide_dir: &Path,
) -> Result<HashMap<String, Vec<FluentMessageDef>>, ParseError> {
    let locales_dir = guide_dir.join("locales");
    if !locales_dir.exists() || !locales_dir.is_dir() {
        return Ok(HashMap::new());
    }

    let mut result: HashMap<String, Vec<FluentMessageDef>> = HashMap::new();

    let entries = std::fs::read_dir(&locales_dir).map_err(|e| ParseError::Io {
        path: locales_dir.display().to_string(),
        source: e,
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if ext != "ftl" {
            continue;
        }

        let locale = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if locale.is_empty() {
            continue;
        }

        let content = std::fs::read_to_string(&path).map_err(|e| ParseError::Io {
            path: path.display().to_string(),
            source: e,
        })?;

        let messages = parse_ftl_content(&content, &locale, &path)?;
        result.insert(locale, messages);
    }

    Ok(result)
}

/// Parse a single `.ftl` file's content into `FluentMessageDef` entries.
///
/// Uses `fluent-syntax` to parse the Fluent AST, then extracts each message's
/// key, body text, and attributes.
fn parse_ftl_content(
    content: &str,
    locale: &str,
    file_path: &Path,
) -> Result<Vec<FluentMessageDef>, ParseError> {
    let resource = fluent_syntax_unicode::parser::parse(content).map_err(|(_, errors)| {
        let details: Vec<String> = errors.iter().map(|e| format!("{:?}", e)).collect();
        ParseError::InvalidFtlFile {
            locale: locale.to_string(),
            file: file_path.display().to_string(),
            detail: details.join("; "),
        }
    })?;

    let mut messages = Vec::new();

    for entry in &resource.body {
        // Skip terms (prefixed with -), comments, and other entries.
        // Terms are resolved at parse time per the architecture plan's
        // recommendation (Section 12.19, question 1).
        if let fluent_syntax_unicode::ast::Entry::Message(msg) = entry {
            let key = msg.id.name.to_string();

            // Extract body text
            let body = msg
                .value
                .as_ref()
                .map(|pattern| pattern_to_string(pattern))
                .unwrap_or_default();

            // Extract attributes
            let mut attributes = HashMap::new();
            for attr in &msg.attributes {
                let attr_key = attr.id.name.to_string();
                let attr_value = pattern_to_string(&attr.value);
                attributes.insert(attr_key, attr_value);
            }

            messages.push(FluentMessageDef {
                key,
                body,
                attributes,
            });
        }
    }

    Ok(messages)
}

/// Convert a Fluent pattern (sequence of elements) to a plain string.
///
/// This performs a simple extraction of text elements. Placeables (variable
/// references, term references, etc.) are rendered as their source text
/// enclosed in curly braces for now. Full Fluent runtime resolution is
/// deferred to the conduit layer.
fn pattern_to_string(pattern: &fluent_syntax_unicode::ast::Pattern<&str>) -> String {
    let mut result = String::new();

    for element in &pattern.elements {
        match element {
            fluent_syntax_unicode::ast::PatternElement::TextElement { value } => {
                result.push_str(value);
            }
            fluent_syntax_unicode::ast::PatternElement::Placeable { expression } => {
                // Render placeables as { expression } for storage.
                // Runtime resolution will handle these.
                result.push_str(&format!("{{ {} }}", expression_to_string(expression)));
            }
        }
    }

    // Trim leading/trailing whitespace that Fluent syntax introduces
    result.trim().to_string()
}

/// Convert a Fluent expression to a string representation for storage.
fn expression_to_string(expr: &fluent_syntax_unicode::ast::Expression<&str>) -> String {
    match expr {
        fluent_syntax_unicode::ast::Expression::Inline(inline) => inline_to_string(inline),
        fluent_syntax_unicode::ast::Expression::Select { .. } => "select(...)".to_string(),
    }
}

/// Convert an inline expression to a string representation.
fn inline_to_string(inline: &fluent_syntax_unicode::ast::InlineExpression<&str>) -> String {
    match inline {
        fluent_syntax_unicode::ast::InlineExpression::MessageReference { id, .. } => {
            id.name.to_string()
        }
        fluent_syntax_unicode::ast::InlineExpression::TermReference { id, .. } => {
            format!("-{}", id.name)
        }
        fluent_syntax_unicode::ast::InlineExpression::VariableReference { id } => {
            format!("${}", id.name)
        }
        fluent_syntax_unicode::ast::InlineExpression::StringLiteral { value } => {
            format!("\"{}\"", value)
        }
        fluent_syntax_unicode::ast::InlineExpression::NumberLiteral { value } => value.to_string(),
        fluent_syntax_unicode::ast::InlineExpression::Placeable { expression } => {
            expression_to_string(expression)
        }
        _ => "...".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_discover_no_locales_dir() {
        let dir = TempDir::new().unwrap();
        let result = discover_and_parse_ftl_files(dir.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_discover_empty_locales_dir() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("locales")).unwrap();
        let result = discover_and_parse_ftl_files(dir.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_discover_and_parse_single_locale() {
        let dir = TempDir::new().unwrap();
        let locales_dir = dir.path().join("locales");
        fs::create_dir(&locales_dir).unwrap();

        fs::write(
            locales_dir.join("en.ftl"),
            r#"topic-selection =
    Guide the user from initial brainstorming through to a focused
    thesis statement.

brainstorm-topics =
    Generate a list of potential essay topics.
    .description = Extended description here
"#,
        )
        .unwrap();

        let result = discover_and_parse_ftl_files(dir.path()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("en"));

        let messages = &result["en"];
        assert_eq!(messages.len(), 2);

        let topic = messages
            .iter()
            .find(|m| m.key == "topic-selection")
            .unwrap();
        assert!(topic.body.contains("Guide the user"));
        assert!(topic.attributes.is_empty());

        let brainstorm = messages
            .iter()
            .find(|m| m.key == "brainstorm-topics")
            .unwrap();
        assert!(brainstorm.body.contains("Generate a list"));
        assert_eq!(
            brainstorm.attributes.get("description"),
            Some(&"Extended description here".to_string())
        );
    }

    #[test]
    fn test_discover_multiple_locales() {
        let dir = TempDir::new().unwrap();
        let locales_dir = dir.path().join("locales");
        fs::create_dir(&locales_dir).unwrap();

        fs::write(
            locales_dir.join("en.ftl"),
            "topic-selection = Select a topic.\n",
        )
        .unwrap();

        fs::write(
            locales_dir.join("pt-BR.ftl"),
            "topic-selection = Selecione um tema.\n",
        )
        .unwrap();

        let result = discover_and_parse_ftl_files(dir.path()).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("en"));
        assert!(result.contains_key("pt-BR"));
    }

    #[test]
    fn test_parse_ftl_with_attributes() {
        let dir = TempDir::new().unwrap();
        let locales_dir = dir.path().join("locales");
        fs::create_dir(&locales_dir).unwrap();

        fs::write(
            locales_dir.join("en.ftl"),
            r#"essay-helper-pack =
    The Essay Helper guide walks users through essay writing.
    .card-subtitle = Ready to write an essay?
    .card-category = researcher
"#,
        )
        .unwrap();

        let result = discover_and_parse_ftl_files(dir.path()).unwrap();
        let messages = &result["en"];
        assert_eq!(messages.len(), 1);

        let pack = &messages[0];
        assert_eq!(pack.key, "essay-helper-pack");
        assert!(pack.body.contains("Essay Helper guide"));
        assert_eq!(
            pack.attributes.get("card-subtitle"),
            Some(&"Ready to write an essay?".to_string())
        );
        assert_eq!(
            pack.attributes.get("card-category"),
            Some(&"researcher".to_string())
        );
    }

    #[test]
    fn test_parse_ftl_invalid_syntax() {
        let dir = TempDir::new().unwrap();
        let locales_dir = dir.path().join("locales");
        fs::create_dir(&locales_dir).unwrap();

        // Invalid Fluent syntax (missing value)
        fs::write(locales_dir.join("en.ftl"), "= missing key\n").unwrap();

        let result = discover_and_parse_ftl_files(dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::InvalidFtlFile { locale, .. } => {
                assert_eq!(locale, "en");
            }
            e => panic!("Expected InvalidFtlFile, got: {:?}", e),
        }
    }

    #[test]
    fn test_non_ftl_files_ignored() {
        let dir = TempDir::new().unwrap();
        let locales_dir = dir.path().join("locales");
        fs::create_dir(&locales_dir).unwrap();

        fs::write(locales_dir.join("en.ftl"), "hello = Hello\n").unwrap();
        fs::write(locales_dir.join("notes.txt"), "not a fluent file").unwrap();

        let result = discover_and_parse_ftl_files(dir.path()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("en"));
    }
}
