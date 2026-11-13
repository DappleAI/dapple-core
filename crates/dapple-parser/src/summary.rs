use crate::error::ParseError;
use crate::types::ParsedChapterStub;

/// Parse the content of a SUMMARY.md file into a list of chapter stubs.
///
/// Processes the file line by line:
/// - Skips blank lines and `# Section Header` lines (visual separators, not chapters).
/// - Matches lines containing markdown links: `[Title](./path.md)` or `- [Title](./path.md)`.
/// - Computes depth from leading whitespace (2 spaces per indent level).
/// - Computes parent_index by walking backwards through the chapter list.
pub fn parse_summary(content: &str) -> Result<Vec<ParsedChapterStub>, ParseError> {
    let mut chapters = Vec::new();

    for line in content.lines() {
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

/// Extract `[Title](path)` from a line. Handles optional bullet prefix.
fn extract_markdown_link(line: &str) -> Option<(String, String)> {
    // Strip optional "- " prefix and any leading whitespace
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

/// Count indentation depth. Each 2 spaces = 1 depth level.
/// mdBook standard is 2-space indent per level for nested items.
/// Bare links without bullets (no leading `-`) are always depth 0.
fn compute_depth(line: &str) -> usize {
    let leading_spaces = line.len() - line.trim_start().len();
    // Handle tabs
    if line.starts_with('\t') {
        let tabs = line.chars().take_while(|c| *c == '\t').count();
        return tabs;
    }
    // 2 spaces per level: 0-1 spaces = 0, 2-3 = 1, 4-5 = 2, etc.
    leading_spaces / 2
}

/// Walk backwards through the chapter list to find the nearest chapter at depth - 1.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_markdown_link_bare() {
        let result = extract_markdown_link("[Preface](./preface.md)");
        assert_eq!(
            result,
            Some(("Preface".to_string(), "./preface.md".to_string()))
        );
    }

    #[test]
    fn test_extract_markdown_link_bulleted() {
        let result = extract_markdown_link("- [Introduction](./intro.md)");
        assert_eq!(
            result,
            Some(("Introduction".to_string(), "./intro.md".to_string()))
        );
    }

    #[test]
    fn test_compute_depth_no_indent() {
        assert_eq!(compute_depth("- [Foo](./foo.md)"), 0);
    }

    #[test]
    fn test_compute_depth_two_spaces() {
        assert_eq!(compute_depth("  - [Foo](./foo.md)"), 1);
    }

    #[test]
    fn test_compute_depth_four_spaces() {
        assert_eq!(compute_depth("    - [Foo](./foo.md)"), 2);
    }
}
