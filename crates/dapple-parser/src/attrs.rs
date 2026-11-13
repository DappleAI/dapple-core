use crate::error::ParseError;

/// Parse the `{ key: "value", tags: [agent, product] }` metadata from fence directives.
///
/// Attempts to parse as JSON first; if that fails, normalizes the YAML-like syntax to JSON
/// by quoting unquoted keys and string values.
pub fn parse_attributes(raw: &str) -> Result<serde_json::Value, ParseError> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }

    // Try to parse as JSON first
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }

    // If JSON parse fails, normalize the YAML-like syntax to JSON
    let normalized = normalize_to_json(trimmed)?;

    serde_json::from_str(&normalized).map_err(|e| ParseError::InvalidAttributes {
        file: String::new(),
        line: 0,
        detail: format!("after normalization: {e}"),
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

/// Extract the "ref" field from parsed attributes, if present.
/// Used for shared content references (e.g., `:::stage { ref: "common/validate-output" }`).
pub fn extract_ref(attrs: &serde_json::Value) -> Option<String> {
    attrs.get("ref").and_then(|v| v.as_str()).map(String::from)
}

/// Normalize YAML-like attribute syntax to valid JSON.
///
/// Handles these transformations at ANY nesting depth (objects inside arrays, etc.):
/// - `{ name: "value" }` -> `{ "name": "value" }` (quote keys)
/// - `{ tags: [agent, product] }` -> `{ "tags": ["agent", "product"] }` (quote unquoted array items)
/// - `{ name: value }` -> `{ "name": "value" }` (quote unquoted string values)
/// - `{ item_fields: [{ key: a, control: b }] }` -> objects nested in arrays (quote nested keys)
///
/// A context stack tracks whether we are inside an object (`true`) or an array
/// (`false`), so a key position (after `{` or an object-level `,`) is recognized
/// at every depth — and `:` is consumed everywhere, never just at the top level
/// (the old flat machine ignored a `:` inside `[]`, which hung on nested objects).
fn normalize_to_json(input: &str) -> Result<String, ParseError> {
    let mut chars = input.chars().peekable();
    let mut output = String::with_capacity(input.len() * 2);

    let mut in_string = false;
    let mut escape_next = false;
    // Container context stack: `true` = object, `false` = array.
    let mut ctx: Vec<bool> = Vec::new();
    // In the current object, is the next bare word a key? (Meaningless in arrays.)
    let mut expecting_key = false;

    while let Some(&ch) = chars.peek() {
        if escape_next {
            output.push(ch);
            chars.next();
            escape_next = false;
            continue;
        }

        if ch == '\\' && in_string {
            output.push(ch);
            chars.next();
            escape_next = true;
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            output.push(ch);
            chars.next();
            continue;
        }

        if in_string {
            output.push(ch);
            chars.next();
            continue;
        }

        match ch {
            '{' => {
                output.push(ch);
                chars.next();
                ctx.push(true);
                expecting_key = true;
            }
            '[' => {
                output.push(ch);
                chars.next();
                ctx.push(false);
                expecting_key = false;
            }
            '}' | ']' => {
                output.push(ch);
                chars.next();
                ctx.pop();
                // A following `,` re-derives the expectation from the parent context.
                expecting_key = false;
            }
            ':' => {
                // Object member separator — at any depth we now expect a value.
                output.push(ch);
                chars.next();
                expecting_key = false;
            }
            ',' => {
                output.push(ch);
                chars.next();
                // Next token is a key inside an object, a value inside an array.
                expecting_key = matches!(ctx.last(), Some(true));
            }
            ' ' | '\t' | '\n' | '\r' => {
                output.push(ch);
                chars.next();
            }
            _ => {
                let word = collect_unquoted_word(&mut chars);
                if word.is_empty() {
                    // Defensive: an unhandled delimiter would otherwise spin forever.
                    // Consume one char to guarantee forward progress.
                    output.push(ch);
                    chars.next();
                } else if expecting_key {
                    output.push('"');
                    output.push_str(&word);
                    output.push('"');
                    expecting_key = false;
                } else {
                    // A value (object member value or array element): quote unless it
                    // is a JSON literal (true/false/null/number).
                    push_maybe_quoted(&mut output, &word);
                }
            }
        }
    }

    Ok(output)
}

/// Collect consecutive non-delimiter characters as an unquoted word.
fn collect_unquoted_word(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut word = String::new();
    while let Some(&ch) = chars.peek() {
        match ch {
            '{' | '}' | '[' | ']' | ':' | ',' | '"' | ' ' | '\t' | '\n' | '\r' => break,
            _ => {
                word.push(ch);
                chars.next();
            }
        }
    }
    word
}

/// Push a value that may need quoting. Booleans and numbers are left unquoted;
/// everything else is treated as a string and quoted.
fn push_maybe_quoted(output: &mut String, word: &str) {
    let is_json_literal =
        word == "true" || word == "false" || word == "null" || word.parse::<f64>().is_ok();

    if is_json_literal {
        output.push_str(word);
    } else {
        output.push('"');
        output.push_str(word);
        output.push('"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_attrs() {
        let result = parse_attributes("").unwrap();
        assert!(result.is_object());
        assert!(result.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_parse_json_attrs() {
        let result = parse_attributes(r#"{ "name": "test", "tags": ["a", "b"] }"#).unwrap();
        assert_eq!(result["name"], "test");
        assert_eq!(result["tags"][0], "a");
        assert_eq!(result["tags"][1], "b");
    }

    #[test]
    fn test_parse_yaml_like_attrs() {
        let result =
            parse_attributes(r#"{ name: "Gather Info", tags: [agent, product] }"#).unwrap();
        assert_eq!(result["name"], "Gather Info");
        assert_eq!(result["tags"][0], "agent");
        assert_eq!(result["tags"][1], "product");
    }

    #[test]
    fn test_parse_unquoted_value() {
        let result = parse_attributes(r#"{ name: my-task, tags: [coder] }"#).unwrap();
        assert_eq!(result["name"], "my-task");
        assert_eq!(result["tags"][0], "coder");
    }

    #[test]
    fn test_parse_boolean_and_numeric() {
        let result = parse_attributes(r#"{ enabled: true, count: 42 }"#).unwrap();
        assert_eq!(result["enabled"], true);
        assert_eq!(result["count"], 42);
    }

    #[test]
    fn test_parse_nested_objects_in_array_shorthand() {
        // The `item_fields` shape: an array of objects in YAML-like shorthand. The
        // old normalizer hung on the `:` inside `[]`; this pins that it now quotes
        // nested keys/values correctly at depth.
        let result = parse_attributes(
            r#"{ control: repeater, item_fields: [{ key: beat, control: textarea }, { key: mix, control: text }] }"#,
        )
        .unwrap();
        assert_eq!(result["control"], "repeater");
        let fields = result["item_fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["key"], "beat");
        assert_eq!(fields[0]["control"], "textarea");
        assert_eq!(fields[1]["key"], "mix");
        assert_eq!(fields[1]["control"], "text");
    }

    #[test]
    fn test_parse_item_field_with_nested_choices_and_attrs() {
        // A per-item `select` carrying static `choices` (an array of objects) and
        // a `number` carrying `attrs`, both NESTED inside the `item_fields` array
        // of objects — the crosstalk overlap/offset case. Depth: object > array >
        // object > array > object. Multi-word values (`"talks over"`) and labels
        // with spaces/parens/commas MUST be quoted.
        let result = parse_attributes(
            r#"{ item_fields: [{ key: overlap, control: select, choices: [{ value: "waits", label: "Waits (comes in after)" }, { value: "talks over", label: "Talks over (agrees, keeps going)" }] }, { key: offset_ms, control: number, attrs: { min: 0, step: 50 } }] }"#,
        )
        .unwrap();
        let fields = result["item_fields"].as_array().unwrap();
        assert_eq!(fields[0]["key"], "overlap");
        let choices = fields[0]["choices"].as_array().unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0]["value"], "waits");
        assert_eq!(choices[0]["label"], "Waits (comes in after)");
        assert_eq!(choices[1]["value"], "talks over");
        assert_eq!(choices[1]["label"], "Talks over (agrees, keeps going)");
        assert_eq!(fields[1]["attrs"]["min"], 0);
        assert_eq!(fields[1]["attrs"]["step"], 50);
    }

    #[test]
    fn test_parse_nested_objects_mixed_quoting() {
        // Quoted strings (with spaces/punctuation) alongside unquoted bare words.
        let result = parse_attributes(
            r#"{ name: "the-input", item_fields: [{ key: "beat", control: "textarea" }] }"#,
        )
        .unwrap();
        assert_eq!(result["name"], "the-input");
        assert_eq!(result["item_fields"][0]["key"], "beat");
        assert_eq!(result["item_fields"][0]["control"], "textarea");
    }

    #[test]
    fn test_extract_tags_present() {
        let attrs: serde_json::Value =
            serde_json::from_str(r#"{"tags": ["agent", "product"]}"#).unwrap();
        let tags = extract_tags(&attrs);
        assert_eq!(tags, vec!["agent", "product"]);
    }

    #[test]
    fn test_extract_tags_absent() {
        let attrs: serde_json::Value = serde_json::from_str(r#"{"name": "test"}"#).unwrap();
        let tags = extract_tags(&attrs);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_extract_name_present() {
        let attrs: serde_json::Value = serde_json::from_str(r#"{"name": "Gather Info"}"#).unwrap();
        assert_eq!(extract_name(&attrs), Some("Gather Info".to_string()));
    }

    #[test]
    fn test_extract_name_absent() {
        let attrs: serde_json::Value = serde_json::from_str(r#"{"tags": ["a"]}"#).unwrap();
        assert_eq!(extract_name(&attrs), None);
    }
}
