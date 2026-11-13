use crate::attrs::{extract_name, extract_ref, extract_tags, parse_attributes};
use crate::error::ParseError;
use crate::fence::FenceToken;
use crate::types::ParsedBlock;

/// Convert the flat token stream from the fence tokenizer into a hierarchical
/// `ParsedBlock` structure.
///
/// Handles nesting via a stack: when an `Open` token is encountered, a new block
/// is pushed onto the stack. When a `Close` token is encountered, the top block
/// is popped and attached as a child of the next block on the stack (or to the
/// top-level result if the stack is empty).
///
/// Prose tokens are accumulated and flushed as prose blocks when a directive
/// boundary is encountered.
pub fn classify_blocks(
    tokens: Vec<FenceToken>,
    _known_directives: &[&str],
) -> Result<Vec<ParsedBlock>, ParseError> {
    let mut result: Vec<ParsedBlock> = Vec::new();
    let mut prose_buffer: Vec<String> = Vec::new();
    let mut prose_start_line: Option<usize> = None;
    let mut stack: Vec<ParsedBlock> = Vec::new();

    for token in tokens {
        match token {
            FenceToken::Prose { text, line } => {
                if prose_start_line.is_none() {
                    prose_start_line = Some(line);
                }
                prose_buffer.push(text);
            }
            FenceToken::Open {
                directive,
                raw_attrs,
                line,
            } => {
                // Flush prose buffer before opening a new directive
                flush_prose(
                    &mut result,
                    &mut stack,
                    &mut prose_buffer,
                    &mut prose_start_line,
                );

                let attrs = parse_attributes(&raw_attrs)?;
                let tags = extract_tags(&attrs);
                let name = extract_name(&attrs);
                let ref_path = extract_ref(&attrs);

                // ref and name are mutually exclusive
                if name.is_some() && ref_path.is_some() {
                    return Err(ParseError::RefAndNameConflict);
                }

                let block = ParsedBlock {
                    directive_type: normalize_directive_type(&directive),
                    name,
                    content: String::new(),
                    attributes: attrs,
                    tags,
                    source_line: line,
                    children: Vec::new(),
                    ref_path,
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
    flush_prose(
        &mut result,
        &mut stack,
        &mut prose_buffer,
        &mut prose_start_line,
    );

    Ok(result)
}

/// Normalize directive names: "brief-pack" -> "brief_pack", "inference-profile" -> "inference_profile".
fn normalize_directive_type(directive: &str) -> String {
    directive.replace('-', "_")
}

/// Flush accumulated prose lines into a prose block.
///
/// If the prose buffer is empty or contains only whitespace, it is discarded.
/// Otherwise, a prose block is created and pushed to the appropriate destination:
/// - If a directive is open on the stack, the prose is added as a child of that directive.
/// - Otherwise, it is added to the top-level result.
fn flush_prose(
    result: &mut Vec<ParsedBlock>,
    stack: &mut [ParsedBlock],
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
        ref_path: None,
    };

    if let Some(parent) = stack.last_mut() {
        parent.children.push(block);
    } else {
        result.push(block);
    }

    buffer.clear();
    *start_line = None;
}
