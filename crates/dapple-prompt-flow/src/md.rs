//! Markdown emission shared across guides — tables and admonition callouts.
//!
//! # Why this is Rust and not a nushell helper
//!
//! **Guide scripts have no import mechanism.** There are exactly two places
//! shared logic can live: a registered command in [`crate::commands`], or
//! copy-paste. Everything here was copy-pasted between guides, and measured
//! across the three shipped financial guides on 2026-08-01 the copying had
//! already gone wrong in the way copying always goes wrong — **it drops the
//! half that had no local pressure**, silently, while the guide still
//! validates, still renders and still passes its probes:
//!
//! | helper | copies | how they diverged |
//! |---|---|---|
//! | `md_table` | 3 | one arrived **without `md_cell`** — unscrubbed headers, embedded newlines, no trim, no empty-header guard |
//! | `admonish` | 3 | two emit a bare quoted title, one emits `title="…"`; one guards an empty body, two do not |
//!
//! # Why `md` and not `report`
//!
//! Measured across all 17 guides, tables and admonitions appear in exactly the
//! four financial guides and nowhere else — but that is because nothing else
//! assembles a long document *yet*, not because a table belongs to finance. The
//! nushell namespace is flat and global, so a name has to say what the thing
//! **produces**. It also has two destinations, not one: every synthesize pass
//! sends the same string to `document render` *and* to a payload key the UI
//! renders as a markdown pane, so naming it for either deliverable names half of
//! it.
//!
//! # What this module deliberately does NOT contain
//!
//! **No `slug_of`.** With 35 copies it is by far the largest duplication, and it
//! must stay duplicated. `slug_of` is an id **minter** — the guides mint explicit
//! `{#tl-<id>}` anchors which `dapple-render`'s `GfmSlugger` registers verbatim —
//! so it is not the anchor contract and must not be "fixed" to match
//! `gfm_slug_base`. It differs in five ways on purpose (collapses runs, drops
//! `ü`/`é`, maps `_`→`-`, dashes punctuation, caps at 41 chars) and uniqueness
//! rides on a trailing enumerate index. A shared minter would invite exactly the
//! convergence that breaks it.
//!
//! **No `band`.** Three definitions carry that name and only two are the same
//! thing: two guides emit a four-backtick admonish wrapper, while the third's
//! `band` emits an `#### label` heading with sorted rows under it — a section
//! grouper that shares nothing but a name. The wrapper half is [`admonish`] with
//! `fence_len = 4`; the grouper stays where it is.

/// Scrub a value for a markdown table cell.
///
/// A GFM cell can hold neither `|` (it would open a new column) nor a newline
/// (it would end the row), so both are replaced rather than escaped: a reader
/// meeting `a/b` understands it, where a reader meeting a table that has come
/// apart does not. Trimming is what keeps a column aligned when some values
/// arrive padded.
pub fn md_cell(value: &str) -> String {
    value
        .replace('|', "/")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_string()
}

/// Build a GFM table, dropping any column that is empty in every row.
///
/// Returns `""` — never a header with no body — when there is nothing to draw:
/// no rows, no headers, or every column blank. **A prop table must never cost
/// the report**, so the caller tests the result and omits its own heading with
/// it. That is the same empty-band doctrine every renderer here follows.
///
/// Dropping an all-empty column is what makes optional columns free: a caller
/// passes "Your note" and "Source" unconditionally and they simply do not appear
/// where nothing carries them, instead of printing a column of blanks. Two
/// adjacent tables can therefore differ in width, which is deliberate — each
/// carries its own header row, and a table showing only what is there beats one
/// padded to a fixed shape. The rule is "empty in EVERY row", not "mostly
/// empty": one filled cell is a fact and dropping its column would lose it.
///
/// A row shorter than `headers` is padded with blanks rather than rejected: a
/// caller assembling rows from optional fields should not have to count.
pub fn md_table(headers: &[String], rows: &[Vec<String>]) -> String {
    if headers.is_empty() || rows.is_empty() {
        return String::new();
    }

    // A named fn rather than a closure: the borrow it returns outlives the call,
    // and an inferred closure lifetime cannot express that.
    fn cell(row: &[String], i: usize) -> &str {
        row.get(i).map(String::as_str).unwrap_or("")
    }

    let keep: Vec<usize> = (0..headers.len())
        .filter(|&i| rows.iter().any(|r| !cell(r, i).trim().is_empty()))
        .collect();
    if keep.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("| ");
    out.push_str(
        &keep
            .iter()
            .map(|&i| md_cell(&headers[i]))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    out.push_str(" |\n|");
    for _ in &keep {
        out.push_str("---|");
    }
    for row in rows {
        out.push_str("\n| ");
        out.push_str(
            &keep
                .iter()
                .map(|&i| md_cell(cell(row, i)))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        out.push_str(" |");
    }
    out
}

/// An mdbook-admonish callout: coloured rail plus tinted title in both the
/// Typst PDF and the HTML page, and a contained code block anywhere that does
/// not know the syntax. Always an aside, never a section.
///
/// Returns `""` on an empty body, so a caller drops its heading with it.
///
/// # The title form is the documented one, not the one that happens to work
///
/// `dapple-render`'s grammar is `admonish [directive] ["Custom title"]
/// [key=value…]` — a **bare quoted title**. The `title="…"` spelling one guide
/// emits parses only because `extract_quoted` takes the first quoted span
/// *anywhere on the info string*, including inside a `key=value` flag. Both
/// render identically today, so tightening that parser would silently retitle
/// every band in that guide. This emits the documented form; the fix belongs in
/// the emitters, never in the parser.
///
/// A `"` in the title becomes `'`. The renderer does honour `\"`, but a title is
/// display copy and a straight quote inside one is nearly always a typo for an
/// apostrophe. An empty title is omitted entirely rather than emitted as `""`,
/// which lets the renderer supply the directive's own title-cased name.
///
/// # `fence_len`
///
/// 3 for an ordinary callout. **4 for a band** — a band body carries
/// three-backtick admonish and chart fences of its own, and the outer fence must
/// be longer than anything inside it or the band swallows its own content. The
/// renderer keys on the info string, not on the backtick count.
pub fn admonish(kind: &str, title: &str, body: &str, fence_len: usize) -> String {
    if body.trim().is_empty() {
        return String::new();
    }
    let fence = "`".repeat(fence_len.max(3));
    let title = title.replace('"', "'");
    let title = title.trim();
    let kind = kind.trim();
    let mut out = String::with_capacity(body.len() + title.len() + kind.len() + 32);
    out.push_str(&fence);
    out.push_str("admonish ");
    out.push_str(kind);
    if !title.is_empty() {
        out.push_str(" \"");
        out.push_str(title);
        out.push('"');
    }
    out.push('\n');
    out.push_str(body.trim_end());
    out.push('\n');
    out.push_str(&fence);
    out
}

// ---------------------------------------------------------------------------
// Provenance markup — read by `dapple_render::provenance`, invisible in the
// rendered PDF and HTML. The grammar lives there; these only emit it.
// ---------------------------------------------------------------------------

/// A source id is `[A-Za-z0-9_.:-]+` — mirrors
/// `dapple_render::provenance::is_valid_source_id` (this crate does not depend
/// on the renderer, and the grammar is one character class).
pub fn is_valid_source_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// An exact decimal (`103213000000`, `-0.129`): no exponent, no grouping, no
/// leading zeros — mirrors `dapple_render::provenance::is_decimal_string`.
pub fn is_decimal_string(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let int_ok = int == "0"
        || (!int.is_empty() && !int.starts_with('0') && int.chars().all(|c| c.is_ascii_digit()));
    let frac_ok = frac.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()));
    int_ok && frac_ok
}

/// `YYYY-MM-DD`.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// A unit or concept token: `USD`, `USD/shares`, `us-gaap:Revenues`.
pub fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-' | '/'))
}

/// `[text](src:id)` — one sourced value. Brackets and backslashes in the text
/// are escaped so the span cannot end early, and whitespace flattens (link
/// text cannot cross a paragraph break). Empty text renders as "".
pub fn src_span(id: &str, text: &str) -> String {
    src_span_with(id, text, &[])
}

/// [`src_span`] carrying item attributes in the link title:
/// `[text](src:id "id=… amount=… unit=…")`. Callers validate the values
/// (they are ids, decimals, dates and tokens, so none contains a quote or
/// whitespace); the renderer re-validates and warns on anything malformed.
pub fn src_span_with(id: &str, text: &str, attrs: &[(&str, String)]) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(flat.len() + id.len() + 8);
    out.push('[');
    for c in flat.chars() {
        if matches!(c, '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push_str("](src:");
    out.push_str(id);
    if !attrs.is_empty() {
        let pairs: Vec<String> = attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push_str(" \"");
        out.push_str(&pairs.join(" "));
        out.push('"');
    }
    out.push(')');
    out
}

/// `<!-- src:id -->` … `<!-- /src -->` around a passage or figure; with an
/// `item_id`, `<!-- src:id id=item -->`. Empty body renders as "".
///
/// Unless `trusted`, provenance markup already inside the body is defused
/// first ([`defuse_provenance`]): the usual body is model output, and a model
/// must not be able to claim that a number it wrote came from a filing.
///
/// A code fence the body leaves open is closed before the close marker, so
/// the marker can never become code text (visible in the PDF/HTML, and a
/// region that never closes would claim the rest of the document).
pub fn attribute(id: &str, item_id: Option<&str>, body: &str, trusted: bool) -> String {
    if body.trim().is_empty() {
        return String::new();
    }
    let body = if trusted {
        body.to_string()
    } else {
        defuse_provenance(body)
    };
    let body = close_open_fence(body.trim_end_matches('\n'));
    let item = item_id.map(|i| format!(" id={i}")).unwrap_or_default();
    format!("<!-- src:{id}{item} -->\n{body}\n<!-- /src -->\n")
}

/// Zero-width space: splits `src:` so no link destination or comment can
/// start with the scheme, without changing what the reader sees.
const SCHEME_BREAK: &str = "src\u{200B}:";

/// Strip provenance markup from untrusted text so none of it can reach the
/// renderer's extraction:
///
/// - an inline `[text](src:id)` span keeps its text;
/// - every other `src:` (a spaced or `<…>` destination, a reference
///   definition, an autolink, anything inside an admonition body) has the
///   scheme broken with a zero-width space — the renderer only recognizes a
///   destination that starts with `src:`, so a purely textual pass covers
///   every link form at every nesting depth;
/// - every HTML comment opener is rewritten so no region marker survives;
/// - a ```provenance info string anywhere on a line (inside a blockquote, a
///   list item, an admonition) becomes `text`.
pub fn defuse_provenance(text: &str) -> String {
    let mut s = text.to_string();
    // Inline sourced spans → their bare text (a nicety; the scheme break
    // below is what makes the result safe).
    let mut from = 0;
    while let Some(rel) = s[from..].find("](src:") {
        let mid = from + rel;
        let open = s[..mid].rfind('[');
        let close = s[mid..].find(')').map(|c| mid + c);
        match (open, close) {
            (Some(open), Some(close)) if open >= from => {
                let inner = s[open + 1..mid].to_string();
                s.replace_range(open..=close, &inner);
                from = open + inner.len();
            }
            _ => from = mid + 2,
        }
    }
    let s = s.replace("src:", SCHEME_BREAK);
    // Region markers: comments are dropped by the renderer anyway, so
    // rewriting every opener changes nothing visible.
    let s = s.replace("<!--", "<!-- quoted:");
    // Registry fences become inert code blocks.
    s.split('\n')
        .map(defuse_provenance_info)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rewrite every `provenance` info string that follows a run of 3+ backticks
/// or tildes anywhere on `line` (so a `> ```provenance` or `- ~~~provenance`
/// opener is caught too) to `text`.
fn defuse_provenance_info(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find(['`', '~']) {
        let ch = rest[pos..].chars().next().unwrap_or('`');
        let run = rest[pos..].chars().take_while(|c| *c == ch).count();
        let after = &rest[pos + run..];
        out.push_str(&rest[..pos + run]);
        rest = after;
        if run >= 3 {
            let ws = after.len() - after.trim_start().len();
            if after[ws..].starts_with("provenance") {
                out.push_str(&after[..ws]);
                out.push_str("text");
                rest = &after[ws + "provenance".len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Append a closing fence when `body` ends inside a fenced code block
/// (CommonMark: an opener of 3+ backticks or tildes indented ≤3 spaces,
/// closed by a run of the same character at least as long with nothing but
/// whitespace after it). Blockquote/list-nested fences need nothing: an
/// unindented line after the body ends their container, and the fence with it.
fn close_open_fence(body: &str) -> String {
    let mut open: Option<(char, usize)> = None;
    for line in body.split('\n') {
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent > 3 {
            continue;
        }
        let t = &line[indent..];
        let Some(ch) = t.chars().next().filter(|c| *c == '`' || *c == '~') else {
            continue;
        };
        let run = t.chars().take_while(|c| *c == ch).count();
        if run < 3 {
            continue;
        }
        let after = &t[run..];
        match open {
            None if ch == '~' || !after.contains('`') => open = Some((ch, run)),
            Some((c, n)) if c == ch && run >= n && after.trim().is_empty() => open = None,
            _ => {}
        }
    }
    match open {
        Some((ch, n)) => format!("{body}\n{}", ch.to_string().repeat(n)),
        None => body.to_string(),
    }
}

/// The ```provenance registry fence. The fence is made longer than any
/// backtick run inside the JSON, so a value can never close it early.
pub fn provenance_fence(registry: &serde_json::Value) -> String {
    let json = serde_json::to_string_pretty(registry).unwrap_or_else(|_| "{}".to_string());
    let longest = json.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}provenance\n{json}\n{fence}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_span_escapes_and_flattens() {
        assert_eq!(src_span("k10", "$391.0B"), "[$391.0B](src:k10)");
        assert_eq!(src_span("k10", "a [b]\nc"), "[a \\[b\\] c](src:k10)");
        assert_eq!(src_span("k10", "  "), "");
    }

    #[test]
    fn attribute_wraps_and_defuses_untrusted_bodies() {
        let body = "Revenue hit [$9T](src:k10).\n<!-- /src -->\n```provenance\n{}\n```\n";
        let out = attribute("mda", None, body, false);
        assert!(out.starts_with("<!-- src:mda -->\n"));
        assert!(out.ends_with("\n<!-- /src -->\n"));
        assert!(out.contains("Revenue hit $9T."), "{out}");
        assert!(!out.contains("](src:"), "{out}");
        assert!(out.contains("<!-- quoted: /src -->"), "{out}");
        assert!(out.contains("```text"), "{out}");
        // Only the wrapper's own markers remain.
        assert_eq!(out.matches("<!-- src:").count(), 1);
        assert_eq!(out.matches("<!-- /src -->").count(), 1);
        // Trusted bodies pass through.
        assert!(attribute("t", None, "[x](src:k10)", true).contains("[x](src:k10)"));
        assert_eq!(attribute("mda", None, " \n", false), "");
    }

    #[test]
    fn defuse_breaks_every_src_link_form_and_nested_registry() {
        let body = "a [$1]( src:k10) b [$2](<src:k10>) c [$3][r] d <src:k10>\n\n\
                    [r]: src:k10\n\n\
                    > ```provenance\n> {}\n> ```\n\n\
                    - ~~~ provenance\n  {}\n  ~~~\n\n\
                    ```admonish note\n[$4](src:k10) and [$5]( src:k10)\n```\n";
        let out = defuse_provenance(body);
        assert!(!out.contains("src:"), "{out}");
        assert!(!out.contains("provenance"), "{out}");
        assert!(out.contains("> ```text"), "{out}");
        assert!(out.contains("- ~~~ text"), "{out}");
        // Inline spans still collapse to their text.
        assert!(out.contains("[$4] and") || out.contains("$4 and"), "{out}");
        // Untouched text is byte-identical, trailing newline included.
        assert_eq!(defuse_provenance("plain\n"), "plain\n");
        assert_eq!(defuse_provenance("plain"), "plain");
    }

    #[test]
    fn attribute_closes_a_fence_the_body_left_open() {
        let out = attribute("mda", None, "Intro.\n\n```python\nx = 1", false);
        assert!(out.ends_with("x = 1\n```\n<!-- /src -->\n"), "{out}");
        let out = attribute("mda", None, "~~~~\ncode\n~~~\n", false);
        assert!(out.ends_with("~~~\n~~~~\n<!-- /src -->\n"), "{out}");
        // A balanced body is left alone.
        let out = attribute("mda", None, "```\ncode\n```\n", false);
        assert_eq!(out, "<!-- src:mda -->\n```\ncode\n```\n<!-- /src -->\n");
    }

    #[test]
    fn provenance_fence_outgrows_inner_backticks() {
        let v = serde_json::json!({"sources": {"a": {"kind": "filing", "note": "x ``` y"}}});
        let out = provenance_fence(&v);
        assert!(out.starts_with("````provenance\n"), "{out}");
        assert!(out.ends_with("\n````\n"));
        assert!(provenance_fence(&serde_json::json!({})).starts_with("```provenance\n"));
    }

    #[test]
    fn source_ids() {
        assert!(is_valid_source_id("k10-fy2024.rev:usd_1"));
        assert!(!is_valid_source_id(""));
        assert!(!is_valid_source_id("has space"));
        assert!(!is_valid_source_id("a)b"));
    }

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|i| i.to_string()).collect()
    }

    #[test]
    fn a_cell_can_break_neither_the_row_nor_the_column() {
        assert_eq!(md_cell("  a|b  "), "a/b");
        assert_eq!(md_cell("one\ntwo"), "one two");
        assert_eq!(md_cell("one\r\ntwo"), "one  two");
    }

    #[test]
    fn a_column_empty_in_every_row_is_dropped() {
        let out = md_table(
            &s(&["Year", "Note", "Source"]),
            &[s(&["2024", "", "10-K"]), s(&["2025", "", "10-Q"])],
        );
        // "Note" is blank in both rows and does not appear at all.
        assert_eq!(
            out,
            "| Year | Source |\n|---|---|\n| 2024 | 10-K |\n| 2025 | 10-Q |"
        );
        assert!(!out.contains("Note"));
    }

    #[test]
    fn one_filled_cell_keeps_the_whole_column() {
        // The rule is "empty in EVERY row", not "mostly empty" — one filled cell
        // is a fact and dropping the column would lose it.
        let out = md_table(
            &s(&["Year", "Note"]),
            &[s(&["2024", ""]), s(&["2025", "restated"])],
        );
        assert!(out.contains("Note"));
        assert!(out.contains("restated"));
    }

    #[test]
    fn a_prop_table_never_costs_the_report() {
        // Every empty shape returns "" rather than a header with no body, so the
        // caller drops its own heading with it.
        assert_eq!(md_table(&s(&["a"]), &[]), "");
        assert_eq!(md_table(&[], &[s(&["x"])]), "");
        assert_eq!(md_table(&s(&["a", "b"]), &[s(&["", ""]), s(&["", ""])]), "");
    }

    #[test]
    fn a_short_row_is_padded_not_rejected() {
        // A caller assembling rows from optional fields should not have to count.
        let out = md_table(&s(&["a", "b", "c"]), &[s(&["1", "2"]), s(&["3", "4", "5"])]);
        assert_eq!(
            out,
            "| a | b | c |\n|---|---|---|\n| 1 | 2 |  |\n| 3 | 4 | 5 |"
        );
    }

    #[test]
    fn table_cells_are_scrubbed_including_the_header() {
        // The copy that arrived without `md_cell` scrubbed neither, so a header
        // carrying a pipe silently split the table.
        let out = md_table(&s(&["a|b"]), &[s(&["one\ntwo"])]);
        assert_eq!(out, "| a/b |\n|---|\n| one two |");
    }

    #[test]
    fn admonish_emits_the_documented_bare_quoted_title() {
        assert_eq!(
            admonish("warning", "Read this", "body", 3),
            "```admonish warning \"Read this\"\nbody\n```"
        );
        // NOT `title="Read this"` — that spelling parses only by accident.
        assert!(!admonish("warning", "T", "b", 3).contains("title="));
    }

    #[test]
    fn a_band_fences_longer_than_anything_it_wraps() {
        // A band body carries three-backtick fences of its own; a three-backtick
        // band would end at the first of them and swallow the rest.
        let inner = admonish("note", "Inner", "x", 3);
        let band = admonish("orange", "Outer", &inner, 4);
        assert!(band.starts_with("````admonish orange"));
        assert!(band.ends_with("````"));
        assert!(band.contains("```admonish note"));
    }

    #[test]
    fn a_fence_shorter_than_three_is_raised_not_honoured() {
        // A one- or two-backtick "fence" is not a fence at all; honouring the
        // caller here would emit inline code with a body hanging off it.
        assert!(admonish("note", "T", "b", 1).starts_with("```admonish"));
    }

    #[test]
    fn an_empty_body_renders_nothing() {
        assert_eq!(admonish("note", "T", "", 3), "");
        assert_eq!(admonish("note", "T", "   \n  ", 3), "");
    }

    #[test]
    fn a_straight_quote_in_a_title_becomes_an_apostrophe() {
        let out = admonish("note", "He said \"hold\"", "b", 3);
        assert_eq!(out, "```admonish note \"He said 'hold'\"\nb\n```");
    }

    #[test]
    fn an_absent_title_leaves_the_directive_to_supply_one() {
        // `dapple-render` falls back to the directive's own title-cased name,
        // which is better copy than an empty quoted span.
        assert_eq!(admonish("note", "", "b", 3), "```admonish note\nb\n```");
        assert_eq!(admonish("note", "   ", "b", 3), "```admonish note\nb\n```");
    }
}
