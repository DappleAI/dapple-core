use crate::error::ParseError;

/// Raw tokens emitted by the line-by-line scanner.
#[derive(Clone, Debug)]
pub enum FenceToken {
    /// Opening of a fenced directive.
    Open {
        directive: String,
        raw_attrs: String, // The { ... } portion, unparsed
        line: usize,
    },
    /// Closing of the current fence (:::).
    Close { line: usize },
    /// A line of content inside a fence.
    Content { text: String, line: usize },
    /// A line of prose (outside any fence).
    Prose { text: String, line: usize },
}

/// Tokenize a markdown file into fence tokens.
///
/// Scans line-by-line for `:::directive { metadata }` blocks, handling:
/// - Nesting via a stack-based state machine
/// - Code block passthrough (triple backtick content is opaque)
/// - Stray closing fences are treated as prose
pub fn tokenize(content: &str) -> Result<Vec<FenceToken>, ParseError> {
    let mut tokens = Vec::new();
    let mut fence_stack: Vec<(String, usize)> = Vec::new(); // (directive_name, open_line)
    let mut in_code_block = false;

    for (line_num, line) in content.lines().enumerate() {
        let line_num = line_num + 1; // 1-indexed
        let trimmed = line.trim();

        // Track code block state (triple backticks)
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            // Emit as content or prose depending on fence state
            if fence_stack.is_empty() {
                tokens.push(FenceToken::Prose {
                    text: line.to_string(),
                    line: line_num,
                });
            } else {
                tokens.push(FenceToken::Content {
                    text: line.to_string(),
                    line: line_num,
                });
            }
            continue;
        }

        // Inside code block -- everything is opaque
        if in_code_block {
            if fence_stack.is_empty() {
                tokens.push(FenceToken::Prose {
                    text: line.to_string(),
                    line: line_num,
                });
            } else {
                tokens.push(FenceToken::Content {
                    text: line.to_string(),
                    line: line_num,
                });
            }
            continue;
        }

        // Check for ::: directives
        if let Some(rest) = trimmed.strip_prefix(":::") {
            let after_colons = rest.trim();

            if after_colons.is_empty() || after_colons.starts_with("<!--") {
                // Closing fence: ":::" or "::: <!-- comment -->"
                if fence_stack.is_empty() {
                    // Stray closing fence -- treat as prose
                    tokens.push(FenceToken::Prose {
                        text: line.to_string(),
                        line: line_num,
                    });
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
            tokens.push(FenceToken::Prose {
                text: line.to_string(),
                line: line_num,
            });
        } else {
            tokens.push(FenceToken::Content {
                text: line.to_string(),
                line: line_num,
            });
        }
    }

    // Check for unclosed fences
    if let Some((_directive, line)) = fence_stack.last() {
        return Err(ParseError::UnclosedFence {
            file: String::new(), // Caller fills in
            line: *line,
        });
    }

    Ok(tokens)
}

/// Parse "directive_name { key: value }" into (name, raw_attrs).
fn parse_fence_opening(after_colons: &str) -> (String, String) {
    if let Some(brace_pos) = after_colons.find('{') {
        let directive = after_colons[..brace_pos].trim().to_string();
        let attrs = after_colons[brace_pos..].trim().to_string();
        (directive, attrs)
    } else {
        (after_colons.trim().to_string(), String::new())
    }
}
