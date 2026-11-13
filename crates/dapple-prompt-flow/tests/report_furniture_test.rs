//! End-to-end tests for the shared `md` and `fmt` command families.
//!
//! The unit tests in `src/md.rs` and `src/fmt.rs` prove the pure functions are
//! correct. These prove the **commands** are reachable: that they parse, that
//! the pipeline plumbing converts values the way a guide script expects, and
//! that the allowlist entry exists.
//!
//! That last one needs its own test because the default security policy is
//! PERMISSIVE. A missing allowlist entry therefore costs nothing until some
//! deployment turns the allowlist on, at which point the command parses fine and
//! is refused at run time. Nothing else in the suite would notice.

use dapple_prompt_flow::engine::StepEngine;
use dapple_prompt_flow::engine::security::SecurityPolicy;
use serde_json::json;

/// Run a script body and return the string it put in `payload`.
fn payload_string(body: &str) -> String {
    let mut engine = StepEngine::new();
    let script = format!("{body}\n");
    let out = engine
        .execute_script(&script, &json!({}), None)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n---\n{script}"));
    out.payload
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| out.payload.to_string())
}

#[test]
fn md_table_builds_a_table_from_records_and_drops_an_empty_column() {
    let out = payload_string(
        r#"
let rows = [
  { Year: "2024", Filing: "10-K", Note: "" }
  { Year: "2025", Filing: "10-Q", Note: "" }
]
{ payload: ($rows | md table ["Year" "Filing" "Note"]), signals: {} }
"#,
    );
    assert_eq!(
        out,
        "| Year | Filing |\n|---|---|\n| 2024 | 10-K |\n| 2025 | 10-Q |"
    );
    assert!(!out.contains("Note"), "an all-empty column must be dropped");
}

#[test]
fn md_table_reads_records_by_name_not_by_position() {
    // The whole reason to accept records: inserting a column cannot shift the
    // data under the remaining ones.
    let out = payload_string(
        r#"
let rows = [{ Filing: "10-K", Year: "2024" }]
{ payload: ($rows | md table ["Year" "Filing"]), signals: {} }
"#,
    );
    assert_eq!(out, "| Year | Filing |\n|---|---|\n| 2024 | 10-K |");
}

#[test]
fn md_table_renders_numbers_and_blanks_nulls() {
    // A null must render blank, or its column can never qualify as empty and
    // the drop-empty-column rule never fires.
    let out = payload_string(
        r#"
let rows = [{ Year: 2024, Note: null }]
{ payload: ($rows | md table ["Year" "Note"]), signals: {} }
"#,
    );
    assert_eq!(out, "| Year |\n|---|\n| 2024 |");
}

#[test]
fn an_empty_table_costs_the_caller_nothing() {
    let out = payload_string(r#"{ payload: ([] | md table ["Year"]), signals: {} }"#);
    assert_eq!(out, "", "no rows must render as the empty string");
}

#[test]
fn md_admonish_wraps_a_body_and_a_band_wraps_an_admonish() {
    let out = payload_string(
        r#"
let inner = ("filed late" | md admonish warning "Timing")
let band = ($inner | md admonish orange "Claimed, not shown" --fence 4)
{ payload: $band, signals: {} }
"#,
    );
    assert!(out.starts_with("````admonish orange \"Claimed, not shown\""));
    assert!(out.ends_with("````"));
    assert!(
        out.contains("```admonish warning \"Timing\""),
        "the inner three-backtick fence must survive the wrap: {out}"
    );
    assert!(
        !out.contains("title="),
        "the documented grammar is a bare quoted title"
    );
}

#[test]
fn an_empty_admonish_body_renders_nothing() {
    // The empty-band doctrine, end to end: the caller drops its heading with it.
    let out = payload_string(r#"{ payload: ("" | md admonish note "T"), signals: {} }"#);
    assert_eq!(out, "");
}

#[test]
fn fmt_scale_keeps_the_sign_through_the_pipeline() {
    // The defect this family exists to fix, asserted at the command layer where
    // a guide script would actually meet it.
    let out = payload_string(
        r#"{ payload: [(-2000000000 | fmt scale) (2000000000 | fmt scale) (1234567 | fmt scale)], signals: {} }"#,
    );
    assert!(out.contains("-2B"), "got {out}");
    assert!(out.contains("1.23M"), "got {out}");
}

#[test]
fn fmt_usd_leads_with_the_minus() {
    let out = payload_string(r#"{ payload: (-1234500000 | fmt usd), signals: {} }"#);
    assert_eq!(out, "-$1.2B");
}

#[test]
fn fmt_round_returns_a_number_so_it_composes() {
    let out = payload_string(r#"{ payload: (1.23456 | fmt round --places 3), signals: {} }"#);
    assert_eq!(out, "1.235");
}

#[test]
fn a_value_that_is_not_a_number_renders_blank_not_zero() {
    // `try { into float } catch { 0.0 }` was the shipped behaviour, and a zero
    // on the page cannot be told apart from a measured one.
    let out = payload_string(r#"{ payload: ("n/a" | fmt usd), signals: {} }"#);
    assert_eq!(out, "");
}

#[test]
fn a_filers_thousands_separators_still_read_as_a_number() {
    let out = payload_string(r#"{ payload: ("1,234,567" | fmt usd), signals: {} }"#);
    assert_eq!(out, "$1M");
}

/// Run a script body and return the whole payload as JSON.
fn payload_json(body: &str) -> serde_json::Value {
    let mut engine = StepEngine::new();
    let script = format!("{body}\n");
    engine
        .execute_script(&script, &json!({}), None)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n---\n{script}"))
        .payload
}

#[test]
fn text_window_reaches_a_passage_the_head_cannot_see() {
    // The defect this command exists to remove, end to end: a shareholder
    // letter puts the reassurance up front and the carve-out far below.
    let out = payload_json(
        r#"
let filler = (0..1999 | each {|i| "filler. " } | str join "")
let doc = ("Dear Valued Shareholder, we are pleased to inform you that the transaction is intended to be a tax-free reorganization. "
  + $filler
  + "Fractional shares will be redeemed for cash, which will be taxable.")
let w = ($doc | text window --markers ["will be taxable"] --head 800 --radius 300)
{ payload: { dropped: $w.dropped, n: ($w.windows | length), start: ($w.windows | first | get start), rendered: $w.rendered }, signals: {} }
"#,
    );
    assert_eq!(out["n"], 1);
    assert_eq!(out["dropped"], 0);
    assert!(
        out["start"].as_i64().unwrap() > 15_000,
        "the window must come from deep in the document, got {}",
        out["start"]
    );
    let rendered = out["rendered"].as_str().unwrap();
    assert!(rendered.contains("[opening] Dear Valued Shareholder"));
    assert!(rendered.contains("which will be taxable"));
    assert!(
        rendered.contains("[from character "),
        "a lifted passage must carry the offset it came from"
    );
}

#[test]
fn text_window_says_how_many_passages_it_did_not_show() {
    // No silent caps. A cap that bit quietly would rebuild the head bias in a
    // new place and read as "we covered everything".
    let out = payload_json(
        r#"
let pad = (0..999 | each {|i| "z" } | str join "")
let doc = (0..19 | each {|i| "mark" + $pad } | str join "")
let w = ($doc | text window --markers ["mark"] --head 0 --radius 100 --max-windows 5)
{ payload: { dropped: $w.dropped, n: ($w.windows | length), notice: $w.notice, rendered: $w.rendered }, signals: {} }
"#,
    );
    assert_eq!(out["n"], 5);
    assert_eq!(out["dropped"], 15);
    assert!(
        out["notice"]
            .as_str()
            .unwrap()
            .contains("15 further marked")
    );
    assert!(
        out["rendered"]
            .as_str()
            .unwrap()
            .contains("15 further marked"),
        "`rendered` must carry the notice, so the easy path is the honest one"
    );
}

#[test]
fn omit_head_drops_the_opening_but_still_skips_hits_inside_it() {
    // A caller that already prints its own opening would otherwise get the first page
    // twice. `--head` must keep its OTHER job: a hit inside the head stays skipped.
    let out = payload_json(
        r#"
let pad = (0..799 | each {|i| "z" } | str join "")
let doc = ("mark early " + $pad + " mark late")
let w = ($doc | text window --markers ["mark"] --head 500 --radius 50 --omit-head)
{ payload: { n: ($w.windows | length), head_len: ($w.head | str length), start: ($w.windows | first | get start), rendered: $w.rendered }, signals: {} }
"#,
    );
    // "mark early" sits at char 0, inside the head, so only the one at 812 windows.
    assert_eq!(out["n"], 1);
    assert_eq!(out["start"], 762, "812 minus the 50-char radius");
    assert_eq!(
        out["head_len"], 500,
        "the head is still returned as a field"
    );
    let rendered = out["rendered"].as_str().unwrap();
    assert!(
        !rendered.contains("[opening]"),
        "--omit-head must keep the opening out of `rendered`: {rendered}"
    );
    assert!(rendered.contains("[from character "));
}

#[test]
fn text_window_with_no_markers_returns_the_head_and_nothing_else() {
    let out = payload_json(
        r#"
let doc = (0..4999 | each {|i| "z" } | str join "")
let w = ($doc | text window --head 100)
{ payload: { n: ($w.windows | length), head_len: ($w.head | str length), len: $w.length }, signals: {} }
"#,
    );
    assert_eq!(out["n"], 0);
    assert_eq!(out["head_len"], 100);
    assert_eq!(out["len"], 5000);
}

#[test]
fn every_new_command_is_on_the_standard_allowlist() {
    // The default policy is PERMISSIVE, so a missing entry here is invisible
    // until a deployment turns the allowlist on. This is the only thing that
    // would catch it.
    let policy = SecurityPolicy::new_with_standard_allowlist();
    for name in [
        "md",
        "md table",
        "md admonish",
        "fmt",
        "fmt scale",
        "fmt usd",
        "fmt round",
        "text",
        "text window",
    ] {
        assert!(
            policy.is_command_allowed(name),
            "`{name}` is registered but not allowlisted"
        );
    }
}

#[test]
fn a_script_using_the_new_commands_passes_the_allowlist_validator() {
    // Membership is necessary but not sufficient: the AST walk has to accept a
    // real pipeline built from them.
    let engine = StepEngine::with_policy(SecurityPolicy::new_with_standard_allowlist());
    let script = r#"
let rows = [{ Year: "2024", Filing: "10-K" }]
let t = ($rows | md table ["Year" "Filing"])
let n = (1234567 | fmt usd)
{ payload: ($t + $n), signals: {} }
"#;
    engine
        .validate_script(script)
        .expect("the allowlist must admit a script built from the shared furniture");
}
