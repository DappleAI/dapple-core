//! Escaping for generated Typst source.
//!
//! The markdown body is arbitrary (model-generated) text; anything that Typst
//! could read as markup must be neutralized. The emitters minimize markup
//! surface by preferring function forms (`#strong[…]`, `#raw("…")`,
//! `#link("…")[…]`), which leaves three contexts to escape for: markup
//! content, string literals, and raw fences.

/// Escape text for Typst MARKUP context.
///
/// Backslash-escapes every character that is (or can begin) Typst syntax:
/// markup operators anywhere (`\ # $ [ ] { } * _ ` @ < > ~`), list/heading
/// markers and term syntax at line start (`- + = / :`), and shorthand or
/// smart-punctuation introducers mid-line (`--`/`---`, `...`, `'`, `"`).
/// Escaping unconditionally is cheaper and safer than tracking position, and
/// keeps output bytes verbatim (no smart quotes, no ellipsis substitution —
/// a CLI flag like `--toc` must survive untouched).
///
/// Newlines are emitted as spaces: paragraph structure is the emitter's job,
/// never the text run's.
pub fn escape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        match c {
            '\\' | '#' | '$' | '[' | ']' | '{' | '}' | '*' | '_' | '`' | '@' | '<' | '>' | '~'
            | '-' | '+' | '=' | '/' | '\'' | '"' | '.' | ':' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Escape text for a Typst string literal (`"…"`).
pub fn escape_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Fence delimiters for a Typst raw block whose content may itself contain
/// backticks: the fence must be strictly longer than the longest backtick run
/// in the content (minimum 3).
pub fn raw_fence(content: &str) -> String {
    let mut longest = 0usize;
    let mut current = 0usize;
    for c in content.chars() {
        if c == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_operators_are_neutralized() {
        let adversarial = r##"#let x = [1] $y$ *bold* _em_ `raw` @ref <tag> ~x {b}"##;
        let escaped = escape_markup(adversarial);
        for op in [
            '#', '$', '[', ']', '*', '_', '`', '@', '<', '>', '~', '{', '}',
        ] {
            assert!(
                !escaped.contains(&format!("{op}")) || escaped.contains(&format!("\\{op}")),
                "unescaped {op} in {escaped}"
            );
        }
    }

    #[test]
    fn shorthand_introducers_survive_verbatim() {
        // `--toc` must not become an en dash; `...` must not become an ellipsis.
        assert_eq!(escape_markup("--toc"), "\\-\\-toc");
        assert_eq!(escape_markup("wait..."), "wait\\.\\.\\.");
        assert_eq!(escape_markup("a/b"), "a\\/b");
        assert_eq!(escape_markup("k: v"), "k\\: v");
    }

    #[test]
    fn newlines_become_spaces() {
        assert_eq!(escape_markup("a\nb\r\nc"), "a b  c");
    }

    #[test]
    fn string_literal_escaping() {
        assert_eq!(escape_string(r#"say "hi"\now"#), r#"say \"hi\"\\now"#);
        assert_eq!(escape_string("line\nbreak"), "line\\nbreak");
    }

    #[test]
    fn raw_fence_outruns_content_backticks() {
        assert_eq!(raw_fence("no ticks"), "```");
        assert_eq!(raw_fence("has ``` fence"), "````");
        assert_eq!(raw_fence("`````"), "``````");
    }
}
