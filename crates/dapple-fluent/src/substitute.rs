//! Simple variable substitution with $variable syntax
//!
//! This module provides a straightforward string interpolation that replaces
//! `$variable` patterns with values from a context map.
//!
//! Unlike full Fluent syntax, this does not support:
//! - Selectors/plurals
//! - Function calls
//! - Nested references
//!
//! This simplicity is intentional - complex logic belongs in prompt design,
//! not runtime templating.
//!
//! # Why a single left-to-right scan
//!
//! The obvious implementation - iterate the context map and `str::replace` each
//! `$key` in turn - is wrong in two ways that are silent rather than loud:
//!
//! 1. **`HashMap` iteration order is nondeterministic**, so the same template and
//!    the same context could render differently between processes.
//! 2. **`str::replace` has no word boundary**, so `$a` also matches the prefix of
//!    `$a_sections`. With `a` visited first, `$a_sections` becomes
//!    `<value-of-a>_sections` and the `a_sections` pass then finds nothing to
//!    replace - the section text is dropped from the render with no error. On a
//!    prompt that is exactly how a 130 KB block of source material silently goes
//!    missing, leaving a plausible-looking prompt that is missing its evidence.
//!
//! A single pass fixes both: each `$name` is resolved once, against the longest
//! valid identifier at that position, and inserted values are never re-scanned -
//! so a value that itself contains `$b` is emitted literally rather than being
//! substituted again by a later key.

use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// Render a context value the way a template expects to see it.
fn render_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        // Arrays and objects get JSON representation
        _ => value.to_string(),
    }
}

/// Length in bytes of the identifier starting at the beginning of `s`, or 0 if
/// `s` does not start with one. Identifiers are `[A-Za-z_][A-Za-z0-9_]*`, which
/// is what keeps `$1.2 million` from being read as a variable.
fn identifier_len(s: &str) -> usize {
    let mut len = 0;
    for (idx, c) in s.char_indices() {
        let ok = if idx == 0 {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_'
        };
        if !ok {
            break;
        }
        len = idx + c.len_utf8();
    }
    len
}

/// Substitute `$variable` patterns in a template with values from context.
///
/// Variable names must be alphanumeric with underscores. The substitution is a
/// single left-to-right pass: the result does not depend on the context map's
/// iteration order, a longer name is never shadowed by a shorter one that
/// prefixes it, and substituted values are not re-scanned for variables.
///
/// A `$` that is not followed by a valid identifier, or that names a key the
/// context does not hold, is emitted verbatim.
///
/// # Type Conversions
///
/// - `String` → used directly
/// - `Number` → converted to string
/// - `Bool` → "true" or "false"
/// - `Null` → empty string
/// - `Array`/`Object` → JSON string representation
///
/// # Examples
///
/// ```
/// use dapple_fluent::substitute;
/// use std::collections::HashMap;
/// use serde_json::Value;
///
/// let mut ctx = HashMap::new();
/// ctx.insert("name".to_string(), Value::String("Kenny".to_string()));
/// ctx.insert("count".to_string(), Value::Number(42.into()));
///
/// assert_eq!(substitute("Hello $name, you have $count items", &ctx), "Hello Kenny, you have 42 items");
/// ```
pub fn substitute(template: &str, context: &HashMap<String, Value>) -> String {
    substitute_tracking(template, context).0
}

/// Like [`substitute`], but also reports which context keys the template
/// actually consumed.
///
/// Callers use this to avoid shipping the same data twice. The conduit, for
/// example, appends the raw context as a JSON message for prompts that do not
/// interpolate it; knowing which keys a template already inlined lets it send
/// only the remainder, so every key reaches the model exactly once.
///
/// The returned set is a `BTreeSet` so the result is deterministic.
pub fn substitute_tracking(
    template: &str,
    context: &HashMap<String, Value>,
) -> (String, BTreeSet<String>) {
    let mut out = String::with_capacity(template.len());
    let mut consumed = BTreeSet::new();
    let mut rest = template;

    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let name_len = identifier_len(after);

        if name_len > 0 {
            let name = &after[..name_len];
            if let Some(value) = context.get(name) {
                out.push_str(&render_value(value));
                consumed.insert(name.to_string());
                // Continue AFTER the name: the inserted value is never rescanned.
                rest = &after[name_len..];
                continue;
            }
        }

        // Not a variable, or a name the context does not hold: keep it verbatim.
        out.push('$');
        rest = after;
    }

    out.push_str(rest);
    (out, consumed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_substitute_string() {
        let mut ctx = HashMap::new();
        ctx.insert("name".to_string(), Value::String("World".to_string()));

        assert_eq!(substitute("Hello $name!", &ctx), "Hello World!");
    }

    #[test]
    fn test_substitute_number() {
        let mut ctx = HashMap::new();
        ctx.insert("count".to_string(), Value::Number(42.into()));

        assert_eq!(
            substitute("You have $count items", &ctx),
            "You have 42 items"
        );
    }

    #[test]
    fn test_substitute_bool() {
        let mut ctx = HashMap::new();
        ctx.insert("enabled".to_string(), Value::Bool(true));

        assert_eq!(
            substitute("Feature enabled: $enabled", &ctx),
            "Feature enabled: true"
        );
    }

    #[test]
    fn test_substitute_null() {
        let mut ctx = HashMap::new();
        ctx.insert("optional".to_string(), Value::Null);

        assert_eq!(substitute("Value: $optional end", &ctx), "Value:  end");
    }

    #[test]
    fn test_substitute_multiple() {
        let mut ctx = HashMap::new();
        ctx.insert("first".to_string(), Value::String("Hello".to_string()));
        ctx.insert("second".to_string(), Value::String("World".to_string()));

        assert_eq!(substitute("$first $second!", &ctx), "Hello World!");
    }

    #[test]
    fn test_substitute_missing_variable() {
        let ctx = HashMap::new();

        // Missing variables remain as-is
        assert_eq!(substitute("Hello $name!", &ctx), "Hello $name!");
    }

    #[test]
    fn test_substitute_empty_context() {
        let ctx = HashMap::new();

        assert_eq!(substitute("No variables here", &ctx), "No variables here");
    }

    #[test]
    fn test_substitute_array() {
        let mut ctx = HashMap::new();
        ctx.insert("items".to_string(), json!(["a", "b", "c"]));

        assert_eq!(substitute("Items: $items", &ctx), r#"Items: ["a","b","c"]"#);
    }

    #[test]
    fn test_substitute_object() {
        let mut ctx = HashMap::new();
        ctx.insert("data".to_string(), json!({"key": "value"}));

        assert_eq!(substitute("Data: $data", &ctx), r#"Data: {"key":"value"}"#);
    }

    #[test]
    fn test_substitute_same_variable_twice() {
        let mut ctx = HashMap::new();
        ctx.insert("word".to_string(), Value::String("test".to_string()));

        assert_eq!(substitute("$word and $word", &ctx), "test and test");
    }

    #[test]
    fn test_substitute_adjacent_variables() {
        let mut ctx = HashMap::new();
        ctx.insert("a".to_string(), Value::String("1".to_string()));
        ctx.insert("b".to_string(), Value::String("2".to_string()));

        assert_eq!(substitute("$a$b", &ctx), "12");
    }

    #[test]
    fn test_substitute_float() {
        let mut ctx = HashMap::new();
        ctx.insert(
            "price".to_string(),
            Value::Number(serde_json::Number::from_f64(19.99).unwrap()),
        );

        assert_eq!(substitute("Price: $price", &ctx), "Price: 19.99");
    }

    // --- regression tests for the prompt-render defect -------------------------

    /// The exact shape that broke `weigh-the-m-and-a-risks-generate`: a short key
    /// that prefixes a longer one. Under the old per-key `str::replace` loop this
    /// rendered differently depending on `HashMap` order - and in the bad order
    /// the section text vanished entirely.
    #[test]
    fn test_short_key_does_not_eat_longer_key() {
        let mut ctx = HashMap::new();
        ctx.insert("a".to_string(), Value::String("ZIP".to_string()));
        ctx.insert(
            "a_sections".to_string(),
            Value::String("ITEM 1A RISK FACTORS".to_string()),
        );

        assert_eq!(
            substitute("who is `$a`\n\n$a_sections", &ctx),
            "who is `ZIP`\n\nITEM 1A RISK FACTORS"
        );
    }

    /// Same context, built in the opposite insertion order. A correct
    /// implementation cannot tell the difference; the old one could.
    #[test]
    fn test_result_is_independent_of_insertion_order() {
        let template = "$a | $b | $a_sections | $b_sections";

        let mut forward = HashMap::new();
        forward.insert("a".to_string(), Value::String("ZIP".to_string()));
        forward.insert("b".to_string(), Value::String("NOW".to_string()));
        forward.insert("a_sections".to_string(), Value::String("AAA".to_string()));
        forward.insert("b_sections".to_string(), Value::String("BBB".to_string()));

        let mut reverse = HashMap::new();
        reverse.insert("b_sections".to_string(), Value::String("BBB".to_string()));
        reverse.insert("a_sections".to_string(), Value::String("AAA".to_string()));
        reverse.insert("b".to_string(), Value::String("NOW".to_string()));
        reverse.insert("a".to_string(), Value::String("ZIP".to_string()));

        assert_eq!(substitute(template, &forward), "ZIP | NOW | AAA | BBB");
        assert_eq!(substitute(template, &reverse), "ZIP | NOW | AAA | BBB");
    }

    /// Filings are full of dollar amounts. A digit cannot start an identifier,
    /// so they pass through untouched.
    #[test]
    fn test_dollar_amounts_are_not_variables() {
        let mut ctx = HashMap::new();
        ctx.insert("a".to_string(), Value::String("ZIP".to_string()));

        assert_eq!(
            substitute("revenue of $1.2 million and $2,500", &ctx),
            "revenue of $1.2 million and $2,500"
        );
    }

    /// A lone `$`, and a `$` followed by punctuation, are literal text.
    #[test]
    fn test_bare_dollar_is_literal() {
        let ctx = HashMap::new();

        assert_eq!(substitute("cost in $ terms", &ctx), "cost in $ terms");
        assert_eq!(substitute("100$", &ctx), "100$");
        assert_eq!(substitute("$-lead", &ctx), "$-lead");
    }

    /// The module doc has always claimed substituted values are not re-scanned.
    /// The per-key loop made that false; this makes it true.
    #[test]
    fn test_inserted_value_is_not_rescanned() {
        let mut ctx = HashMap::new();
        ctx.insert(
            "a".to_string(),
            Value::String("literal $b here".to_string()),
        );
        ctx.insert(
            "b".to_string(),
            Value::String("SHOULD-NOT-APPEAR".to_string()),
        );

        assert_eq!(substitute("$a", &ctx), "literal $b here");
    }

    #[test]
    fn test_tracking_reports_consumed_keys() {
        let mut ctx = HashMap::new();
        ctx.insert("a".to_string(), Value::String("ZIP".to_string()));
        ctx.insert("b".to_string(), Value::String("NOW".to_string()));
        ctx.insert("unused".to_string(), Value::String("x".to_string()));

        let (rendered, consumed) = substitute_tracking("$a and $a and $b and $nope", &ctx);

        assert_eq!(rendered, "ZIP and ZIP and NOW and $nope");
        assert_eq!(
            consumed,
            ["a".to_string(), "b".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn test_tracking_empty_when_nothing_interpolated() {
        let mut ctx = HashMap::new();
        ctx.insert("a".to_string(), Value::String("ZIP".to_string()));

        let (rendered, consumed) = substitute_tracking("no variables at all", &ctx);

        assert_eq!(rendered, "no variables at all");
        assert!(consumed.is_empty());
    }

    /// Non-ASCII immediately after `$` is not an identifier start.
    #[test]
    fn test_non_ascii_after_dollar_is_literal() {
        let ctx = HashMap::new();

        assert_eq!(substitute("$é and $日本", &ctx), "$é and $日本");
    }
}
