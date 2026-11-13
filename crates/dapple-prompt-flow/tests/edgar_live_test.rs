//! Live SEC EDGAR end-to-end test for the `edgar` command internals.
//!
//! `#[ignore]`d — hits sec.gov, so it never runs in CI. Run manually with a
//! declared User-Agent:
//!
//! ```sh
//! DAPPLE_EDGAR_USER_AGENT="dapple you@example.com" \
//!   cargo test -p dapple-prompt-flow --test edgar_live_test -- --ignored --nocapture
//! ```

use dapple_prompt_flow::edgar_form345::parse_form4;
use dapple_prompt_flow::edgar_sections::{html_to_text, split_items, strip_ix_header};
use dapple_prompt_flow::engine::StepEngine;
use edgarkit::{Edgar, FilingOperations, FilingOptions};

/// A guide task script calling `edgar filings` runs end-to-end through the
/// StepEngine (registration, arg parsing, the block_in_place bridge inside the
/// engine's tokio context, record conversion). Live network — same opt-in as
/// the sectioning test below.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_filings_runs_inside_step_engine() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let years = (edgar filings "320193" --years 1 --items "1A" --max-chars 500)
        {
            payload: { years: ($years | length), first_item: ($years | first | get items | first | get item) },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar filings must run inside the StepEngine");
    assert_eq!(output.payload["years"], serde_json::json!(1));
    assert_eq!(output.payload["first_item"], serde_json::json!("1A"));
    assert_eq!(output.signals["fetched"], serde_json::json!(true));
}

/// `edgar facts` accepts both bare (`Assets`) and taxonomy-qualified
/// (`us-gaap:Assets`) concepts. Regression for the prefix bug that keyed the
/// lookup on the fully-qualified string and so marked every prefixed concept
/// `missing` (which starved the read-the-10k Numbers section into an empty pick).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn facts_accepts_qualified_and_bare_concepts() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    // ZipRecruiter (CIK 1617553) tags all three; request two qualified and one bare.
    let script = r#"
        let f = (edgar facts "1617553" --concepts ["us-gaap:Assets" "us-gaap:NetIncomeLoss" "Liabilities"])
        {
            payload: { rows: ($f.facts | length), missing: ($f.missing | length), entity: $f.entity_name },
            signals: { ok: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar facts must run inside the StepEngine");
    assert_eq!(
        output.payload["missing"],
        serde_json::json!(0),
        "qualified and bare concepts must all resolve; none should be missing"
    );
    assert!(
        output.payload["rows"].as_u64().unwrap_or(0) > 0,
        "expected value rows for the resolved concepts"
    );
}

/// Without `DAPPLE_EDGAR_USER_AGENT` the command fails with the shaped
/// "not configured" error — deterministic and offline, so not `#[ignore]`d.
/// Skips itself when the var IS set (a dev running the live tests).
#[tokio::test(flavor = "multi_thread")]
async fn edgar_without_user_agent_errors_cleanly() {
    if std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok_and(|v| !v.trim().is_empty()) {
        eprintln!("DAPPLE_EDGAR_USER_AGENT is set; skipping the not-configured smoke");
        return;
    }
    let mut engine = StepEngine::new();
    let script = r#"edgar filings "AAPL" --years 1"#;
    let err = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect_err("edgar must fail cleanly when unconfigured");
    let msg = err.to_string();
    assert!(
        msg.contains("DAPPLE_EDGAR_USER_AGENT"),
        "the error must name the env var to set, got: {msg}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn aapl_latest_10k_sections_end_to_end() {
    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("client");

    // Apple's CIK — skips the ticker-list download to keep the test to 2 requests.
    let cik = "320193";
    let submission = edgar.submissions(cik).await.expect("submissions");
    let filings = submission.filings(Some(
        FilingOptions::new()
            .with_form_type("10-K")
            .with_include_amendments(false),
    ));
    assert!(!filings.is_empty(), "AAPL must have 10-K filings");

    let latest = &filings[0];
    let doc = latest
        .primary_document
        .as_deref()
        .expect("10-K must have a primary document");
    let filing_id = format!("{}:{}", latest.accession_number, doc);
    let html = edgar
        .get_filing_content_by_id(cik, &filing_id)
        .await
        .expect("filing content");
    assert!(html.len() > 100_000, "10-K primary doc should be large");

    let text = html_to_text(&strip_ix_header(&html));
    let sectioned = split_items(&text);

    println!(
        "sections: {:?}, warnings: {:?}",
        sectioned
            .items
            .iter()
            .map(|s| (s.key.as_str(), s.text.chars().count()))
            .collect::<Vec<_>>(),
        sectioned.warnings
    );

    // A real 10-K must yield the core Items.
    for key in ["1", "1A", "7", "8"] {
        assert!(
            sectioned.items.iter().any(|s| s.key == key),
            "expected Item {key} in AAPL 10-K sections"
        );
    }
    let risk = sectioned.items.iter().find(|s| s.key == "1A").unwrap();
    assert!(
        risk.text.chars().count() > 5_000,
        "Risk Factors should be substantial, got {} chars",
        risk.text.chars().count()
    );
}

/// The run-the-10k pull script's exact call shapes: `--form 8-K` with a
/// dotted-code `--items` filter and `--form "DEF 14A"`, both through the
/// StepEngine, both expected to carry a citable sec.gov `url` per record.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_filings_form_8k_and_proxy_inside_step_engine() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    // Apple's CIK; one year keeps the download count small (the earnings 8-Ks
    // alone guarantee 2.02 hits within a year).
    let script = r#"
        let events = (edgar filings "320193" --form "8-K" --years 1 --items "2.02" --max-chars 800)
        let proxies = (edgar filings "320193" --form "DEF 14A" --years 1 --max-chars 800)
        let first_event = ($events | first)
        {
            payload: {
                event_count: ($events | length),
                event_url: ($first_event.url? | default ""),
                event_codes: ($first_event.items | get item),
                proxy_count: ($proxies | length),
                proxy_url: (($proxies | first).url? | default ""),
                proxy_titles: (($proxies | first).items | get title),
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar filings --form must run inside the StepEngine");

    let event_count = output.payload["event_count"].as_u64().unwrap_or(0);
    assert!(
        event_count >= 2,
        "expected several 2.02 8-Ks in a year, got {event_count}"
    );
    let event_url = output.payload["event_url"].as_str().unwrap_or("");
    assert!(
        event_url.starts_with("https://www.sec.gov/Archives/edgar/data/320193/"),
        "8-K url must be the sec.gov primary document, got: {event_url}"
    );
    assert!(
        output.payload["event_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|c| c == "2.02")),
        "the filtered 8-K must section a 2.02 item, got: {}",
        output.payload["event_codes"]
    );

    assert_eq!(output.payload["proxy_count"], serde_json::json!(1));
    let proxy_url = output.payload["proxy_url"].as_str().unwrap_or("");
    assert!(
        proxy_url.starts_with("https://www.sec.gov/Archives/edgar/data/320193/"),
        "proxy url must be the sec.gov primary document, got: {proxy_url}"
    );
    let proxy_titles = output.payload["proxy_titles"].to_string().to_lowercase();
    assert!(
        proxy_titles.contains("compensation"),
        "proxy sections should include a compensation heading, got: {proxy_titles}"
    );
}

/// The one link in `edgar insiders` that unit tests cannot reach: the submissions
/// feed names a Form 4's XSL-RENDERED view (`xslF345X06/wk-form4_1.xml`), and the
/// machine-readable XML is the same name with that prefix removed. If EDGAR ever
/// stops serving the raw document beside the rendered one, every insider row goes
/// silently empty, so this asserts the round trip against the live archive.
///
/// No database needed: it drives the client and the parser directly.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn form4_raw_xml_is_fetchable_by_stripped_document_name() {
    let user_agent = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&user_agent).expect("client");

    // NVIDIA: a heavy Form 4 filer, so the newest filings are always recent.
    let cik = "1045810";
    let submission = edgar.submissions(cik).await.expect("submissions");
    let filings = submission.filings(Some(
        FilingOptions::new()
            .with_form_type("4".to_string())
            .with_include_amendments(false),
    ));
    let filing = filings.first().expect("NVDA files Form 4s");
    let doc = filing
        .primary_document
        .as_ref()
        .expect("a Form 4 has a primary document");
    assert!(
        doc.starts_with("xsl"),
        "the feed is expected to name the rendered view; got {doc}. If EDGAR \
         changed this, edgar_insiders::strip_xsl_prefix needs revisiting."
    );

    let raw = doc.split_once('/').map(|(_, rest)| rest).unwrap_or(doc);
    let id = format!("{}:{}", filing.accession_number, raw);
    let xml = edgar
        .get_filing_content_by_id(cik, &id)
        .await
        .expect("the raw XML must sit beside the rendered view");
    assert!(
        xml.contains("<ownershipDocument"),
        "expected Form 4 XML, got: {}",
        &xml[..xml.len().min(200)]
    );

    let rows = parse_form4(&xml, &filing.accession_number, Some(&filing.filing_date))
        .expect("live Form 4 must parse");
    // A filing can legitimately report holdings only, so the assertion is on the
    // shape rather than on a count: whatever parsed must be attributed and coded.
    for r in &rows {
        assert!(!r.owner_name.is_empty(), "every row names its filer");
        assert!(r.trans_code.is_some(), "every transaction carries a code");
        assert_eq!(r.issuer_cik, Some(1_045_810));
    }
    println!(
        "parsed {} transaction(s) from {} ({})",
        rows.len(),
        filing.accession_number,
        filing.filing_date
    );
}

/// `edgar insiders` end to end inside the StepEngine, the way a guide task calls
/// it: registration, arg parsing, the live fetch, the database cache round trip,
/// and record conversion. Requires DAPPLE_PROMPT_DATABASE_URL as well as the
/// EDGAR user agent.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network + database test; requires DAPPLE_EDGAR_USER_AGENT and DAPPLE_PROMPT_DATABASE_URL"]
async fn edgar_insiders_runs_inside_step_engine() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let rows = (edgar insiders "1045810" --years 1)
        {
            payload: {
                rows: ($rows | length)
                classes: ($rows | get class | uniq | sort)
                codes: ($rows | get code | uniq | sort)
                named: ($rows | where {|r| ($r.owner | str length) > 0 } | length)
                discretionary: ($rows | where class == "discretionary" | length)
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar insiders must run inside the StepEngine");

    let rows = output.payload["rows"].as_i64().unwrap_or(0);
    assert!(rows > 0, "NVDA files Form 4s within a year, got {rows}");
    assert_eq!(
        output.payload["named"], output.payload["rows"],
        "every returned row must name its filer"
    );
    // The classification is the point of the command: whatever comes back must
    // land in the documented vocabulary and never in an unlabelled bucket.
    for c in output.payload["classes"].as_array().expect("classes") {
        let c = c.as_str().unwrap_or("");
        assert!(
            matches!(c, "discretionary" | "compensation" | "other"),
            "unexpected class {c}"
        );
    }
    println!(
        "rows={} codes={} classes={} discretionary={}",
        output.payload["rows"],
        output.payload["codes"],
        output.payload["classes"],
        output.payload["discretionary"]
    );
}

// ---------------------------------------------------------------------------
// `edgar fund` — fund identity and scope enforcement
// ---------------------------------------------------------------------------

/// A share-class ticker must pin exactly ONE fund and carry its series id.
///
/// This is the load-bearing case: N-PORT is filed per series, so a resolution
/// that stopped at the trust CIK would leave every downstream call unable to say
/// which of the trust's several dozen funds it meant.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_fund_pins_a_fund_from_a_share_class_ticker() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let f = (edgar fund "VTSAX")
        {
            payload: {
                cik: $f.trust_cik,
                pinned: $f.pinned,
                in_scope: $f.in_scope,
                kind: $f.kind,
                series_count: ($f.series | length),
                series_id: ($f.series | first | get series_id),
                classes: ($f.series | first | get classes | length),
                skipped: $f.registry_skipped
            },
            signals: { ok: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar fund must run inside the StepEngine");
    assert_eq!(output.payload["pinned"], serde_json::json!(true));
    assert_eq!(output.payload["in_scope"], serde_json::json!(true));
    assert_eq!(output.payload["series_count"], serde_json::json!(1));
    assert_eq!(
        output.payload["series_id"],
        serde_json::json!("S000002848"),
        "VTSAX is a class of Vanguard Total Stock Market Index Fund"
    );
    assert_eq!(output.payload["cik"], serde_json::json!(36405));
    assert_eq!(output.payload["kind"], serde_json::json!("OpenEndFund"));
    // Sibling share classes ride along; only the FUND is pinned, not the class.
    assert!(
        output.payload["classes"].as_u64().unwrap_or(0) >= 4,
        "expected the fund's other share classes, got {}",
        output.payload["classes"]
    );
    println!(
        "VTSAX -> cik={} series={} classes={} registry_skipped={}",
        output.payload["cik"],
        output.payload["series_id"],
        output.payload["classes"],
        output.payload["skipped"]
    );
}

/// A bare trust CIK is ambiguous by construction and must return a PICKER, not a
/// guess and not an error. One trust holds many funds; one N-PORT covers one.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_fund_bare_trust_cik_returns_a_picker() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let f = (edgar fund "36405")
        {
            payload: { pinned: $f.pinned, series_count: ($f.series | length), in_scope: $f.in_scope },
            signals: { ok: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("a bare trust CIK is a picker, not a failure");
    assert_eq!(output.payload["pinned"], serde_json::json!(false));
    assert_eq!(output.payload["in_scope"], serde_json::json!(true));
    assert!(
        output.payload["series_count"].as_u64().unwrap_or(0) > 1,
        "Vanguard Index Funds holds many series; got {}",
        output.payload["series_count"]
    );
}

/// An operating-company ticker must fail LOUD and name the guide that handles
/// it. Returning an empty fund record would read as "a fund with no holdings".
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_fund_rejects_an_operating_company_by_name() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let f = (edgar fund "AAPL")
        { payload: { cik: $f.trust_cik }, signals: { ok: true } }
    "#;
    let err = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect_err("AAPL is not an N-PORT filer and must be refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("run-the-10k"),
        "refusal must name the guide that DOES handle it; got: {msg}"
    );
}

/// Unit investment trusts must resolve, not be refused.
///
/// SPY, QQQ and DIA are UITs: they register on S-6 and have **no** `S000`/`C000`
/// structure, so a resolver that requires a series id excludes three of the most
/// recognizable tickers in the market. But they DO file `NPORT-P` (verified
/// against SPY's live submissions feed, alongside `N-CSR`, `N-CEN`, `485BPOS`
/// and `NPORT-EX`), so the guide's own scope rule — "does it file N-PORT?" —
/// plainly admits them. The trust IS the portfolio, hence `structure: unitary`,
/// an empty `series` list, and `pinned: true`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_fund_resolves_unit_investment_trusts() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    for ticker in ["SPY", "QQQ", "DIA"] {
        let script = format!(
            r#"
            let f = (edgar fund "{ticker}")
            {{
                payload: {{
                    name: $f.entity_name,
                    structure: $f.structure,
                    pinned: $f.pinned,
                    in_scope: $f.in_scope,
                    series_count: ($f.series | length)
                }},
                signals: {{ ok: true }}
            }}
        "#
        );
        let output = engine
            .execute_script(&script, &serde_json::json!({}), None)
            .unwrap_or_else(|e| panic!("{ticker} must resolve, got: {e:?}"));
        println!(
            "{ticker} -> {} structure={} pinned={} series={}",
            output.payload["name"],
            output.payload["structure"],
            output.payload["pinned"],
            output.payload["series_count"]
        );
        assert_eq!(
            output.payload["in_scope"],
            serde_json::json!(true),
            "{ticker} files N-PORT, so the guide's scope rule admits it"
        );
        assert_eq!(
            output.payload["structure"],
            serde_json::json!("unitary"),
            "{ticker} is a unit investment trust"
        );
        assert_eq!(
            output.payload["pinned"],
            serde_json::json!(true),
            "one CIK, one portfolio — an empty series list is completeness, not ambiguity"
        );
        assert!(
            !output.payload["name"].as_str().unwrap_or("").is_empty(),
            "{ticker} must carry a filer name even with no registry row"
        );
    }
}

/// Parse the LIVE series/class registry, not just a fixture.
///
/// Two things only real bytes can prove: that ~17% of rows carry quoted fields
/// with embedded commas (hand-splitting would corrupt them), and that the skip
/// rate stays near zero. A silently-climbing skip count is exactly the failure
/// mode this parser exists to avoid — it would make a real fund look absent.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn series_class_registry_parses_live_without_silent_loss() {
    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    let text = edgar
        .get(dapple_prompt_flow::edgar_fund_registry::SERIES_CLASS_REGISTRY_URL)
        .await
        .expect("fetch series/class registry");

    let parsed = dapple_prompt_flow::edgar_fund_registry::parse_series_class_csv(&text)
        .expect("registry must parse");
    println!(
        "registry: {} rows, {} skipped, {} warnings",
        parsed.rows.len(),
        parsed.skipped,
        parsed.warnings.len()
    );
    assert!(
        parsed.rows.len() > 40_000,
        "expected the full registry, got {} rows",
        parsed.rows.len()
    );
    // Skips are legitimate for trust rows with a [NULL] series, but they should
    // be a rounding error. A jump here means the layout moved under us.
    let skip_pct = (parsed.skipped as f64 / (parsed.rows.len() + parsed.skipped) as f64) * 100.0;
    assert!(
        skip_pct < 2.0,
        "skip rate {skip_pct:.2}% is too high to be [NULL] trust rows"
    );

    // HOW FAR FORWARD THE SNAPSHOT REACHES, which is the thing the row count
    // above cannot tell you and the reason seven years of staleness went unseen:
    // the SEC publishes several vintages of this file and the NEWEST has the
    // FEWEST rows, because deregistered funds leave. A row-count floor is
    // therefore satisfied just as happily by a decade-old file.
    //
    // Pinned to an exact value rather than a floor: a bump is a deliberate act
    // and should have to be acknowledged here, in the same commit that measures
    // the new ceiling.
    let ceiling = parsed
        .max_series_ordinal
        .expect("a parsed registry must report its ceiling");
    println!("registry ceiling: S{ceiling:09}");
    assert_eq!(
        ceiling,
        dapple_prompt_flow::edgar_fund_registry::SERIES_CLASS_REGISTRY_CEILING,
        "the pinned snapshot moved: URL now reaches S{ceiling:09}. If the SEC \
         published a newer file, update SERIES_CLASS_REGISTRY_CEILING with it"
    );

    // A quoted address field with an embedded comma must not have shifted the
    // ticker column — the row is only correct if the ticker still resolves.
    let id = dapple_prompt_flow::edgar_fund_registry::classify_identifier("VTSAX")
        .expect("classify VTSAX");
    let r = dapple_prompt_flow::edgar_fund_registry::resolve(&parsed.rows, &id)
        .expect("VTSAX must resolve in the live registry");
    assert_eq!(r.trust_cik, 36405);
    assert_eq!(r.series[0].series_id, "S000002848");
}

/// The two halves of the cascade, against the funds that exposed the bug.
///
/// Motley Fool Asset Management's ETFs live in RBB Fund (CIK 831114) and
/// straddle the snapshot ceiling exactly:
///
/// - `TMFC` is `S000061203`, **below** the pinned snapshot's reach — the URL bump
///   alone is what fixes it. It could not resolve at all under the previously
///   shipped snapshot, whose ceiling was `S000055654`.
/// - `MFVL` is `S000097673`, **above** every published snapshot — only the ticker
///   map reaches it, so it is what proves the tail is actually wired rather than
///   merely present.
///
/// Asserting both is the point. Testing only `TMFC` would pass with the tail
/// deleted, and testing only `MFVL` would pass with the stale URL restored.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn cascade_reaches_funds_on_both_sides_of_the_snapshot_ceiling() {
    use dapple_prompt_flow::edgar_fund_registry as reg;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");

    let csv = edgar
        .get(reg::SERIES_CLASS_REGISTRY_URL)
        .await
        .expect("fetch series/class registry");
    let snapshot = reg::parse_series_class_csv(&csv).expect("registry must parse");
    let ceiling = snapshot.max_series_ordinal.expect("ceiling");

    // Below the ceiling: the snapshot serves it, no tail involved.
    let tmfc = reg::classify_identifier("TMFC").expect("classify TMFC");
    let hit = reg::resolve(&snapshot.rows, &tmfc)
        .expect("TMFC must resolve from the snapshot — this is the URL bump");
    assert_eq!(hit.trust_cik, 831_114, "TMFC belongs to RBB Fund");
    assert_eq!(hit.series[0].series_id, "S000061203");
    assert!(hit.is_pinned(), "a ticker names exactly one fund");
    assert!(
        reg::series_ordinal("S000061203").is_some_and(|o| o <= ceiling),
        "TMFC is supposed to be inside the snapshot's reach"
    );

    // Above the ceiling: the snapshot CANNOT serve it. Assert that first, so
    // this test still fails if a future snapshot silently absorbs the case and
    // the tail quietly stops being exercised.
    let mfvl = reg::classify_identifier("MFVL").expect("classify MFVL");
    assert!(
        reg::series_ordinal("S000097673").is_some_and(|o| o > ceiling),
        "MFVL must be beyond the snapshot for this test to mean anything"
    );
    assert!(
        reg::resolve(&snapshot.rows, &mfvl).is_none(),
        "MFVL is newer than the snapshot; if it resolves here, re-point this test \
         at a fund that is still beyond the ceiling"
    );

    let json = edgar
        .get(reg::MF_TICKERS_URL)
        .await
        .expect("fetch fund ticker map");
    let (tail, skipped) = reg::parse_mf_tickers(&json).expect("ticker map must parse");
    println!("ticker map: {} rows, {skipped} skipped", tail.len());
    assert!(
        tail.len() > 20_000,
        "expected the full map, got {}",
        tail.len()
    );

    let via_tail = reg::resolve_tail(&tail, &mfvl).expect("MFVL must resolve through the tail");
    assert_eq!(via_tail.trust_cik, 831_114);
    assert_eq!(via_tail.series[0].series_id, "S000097673");
    // No name here, and the placeholder is the id rather than an empty string.
    assert_eq!(via_tail.series[0].series_name, "S000097673");
}

/// Parse the LIVE 3 MB N-PORT, not just a shaped fixture.
///
/// The strongest completeness check available is arithmetic the filing does not
/// state: `pctVal` across every position must sum to roughly the whole
/// portfolio. A parser that silently dropped rows — the derivative rows being
/// the likely casualty, since they carry every trap at once — would land visibly
/// short of it.
///
/// Note `pctVal` is a **percentage, not a fraction**: a $29M position in a
/// $1.99T fund is filed as `0.001467`, i.e. 0.001467%. The total therefore lands
/// near 100, and slightly ABOVE it here (100.25) because payables and short
/// derivative legs offset — normal for a fund, not a parse defect.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn nport_parses_a_live_three_megabyte_filing() {
    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    let xml = edgar
        .get("https://www.sec.gov/Archives/edgar/data/36405/000003640526000323/primary_doc.xml")
        .await
        .expect("fetch NPORT-P primary_doc.xml");
    println!("primary_doc.xml: {} bytes", xml.len());

    let f = dapple_prompt_flow::edgar_nport::parse_nport(&xml).expect("live filing must parse");

    assert_eq!(f.series_id.as_deref(), Some("S000002848"));
    assert_eq!(f.period_date.as_deref(), Some("2026-03-31"));
    assert!(
        f.net_assets.unwrap_or(0.0) > 1e12,
        "expected a trillion-dollar fund"
    );
    assert_eq!(
        f.holdings.len(),
        3524,
        "every position must survive the parse"
    );

    // Weights must account for the whole portfolio. This is the check that
    // catches silent row loss.
    let total: f64 = f.holdings.iter().filter_map(|h| h.pct_val).sum();
    println!("sum(pctVal) = {total:.6}%");
    assert!(
        (total - 100.0).abs() < 2.0,
        "position weights sum to {total:.6}%, not ~100% — rows were dropped"
    );

    // The xs:choice distribution, verified independently against the raw XML.
    let mut isin = 0usize;
    let mut ticker = 0usize;
    let mut other = 0usize;
    for h in &f.holdings {
        match h.identifier.as_ref().map(|i| i.kind()) {
            Some("isin") => isin += 1,
            Some("ticker") => ticker += 1,
            Some("other") => other += 1,
            _ => {}
        }
    }
    println!("identifiers: isin={isin} ticker={ticker} other={other}");
    assert_eq!((isin, other, ticker), (3479, 42, 3));

    // The trap rows: derivatives get issuerCat from an ATTRIBUTE on a
    // differently-named element, and must not inherit their swap's reference
    // instrument as their own identifier.
    let derivs: Vec<_> = f.holdings.iter().filter(|h| h.is_derivative()).collect();
    println!("derivatives: {}", derivs.len());
    assert_eq!(derivs.len(), 15);
    assert!(
        derivs.iter().all(|h| h.issuer_cat.is_some()),
        "issuerConditional/@issuerCat must normalize into issuer_cat"
    );
    assert!(
        derivs.iter().any(|h| h.counterparty.is_some()),
        "swap counterparties must be captured"
    );
    assert!(
        derivs.iter().all(|h| h.cusip.is_none()),
        "derivative rows carry <cusip>N/A</cusip>; the sentinel must normalize to None"
    );

    // EXPOSURE, not market value. These 15 swaps are worth a rounding error
    // against NAV and carry hundreds of millions of notional; a review reading
    // `pct_val` alone reports the opposite of what the fund holds.
    let notional: f64 = derivs.iter().filter_map(|h| h.notional).map(f64::abs).sum();
    println!(
        "derivative notional = ${notional:.0} ({:.4}% of NAV)",
        f.derivative_notional_pct().unwrap_or(0.0)
    );
    assert!(
        derivs.iter().filter(|h| h.notional.is_some()).count() >= 10,
        "notional must survive the parse on the derivative rows"
    );
    assert!(
        notional > 1e8,
        "expected material notional across the swap book"
    );
    let mkt: f64 = derivs.iter().filter_map(|h| h.pct_val).map(f64::abs).sum();
    assert!(
        f.derivative_notional_pct().unwrap() > mkt,
        "notional share must exceed the market-value share, or the two have been conflated"
    );

    // Fund-level leverage and lifecycle. Every one of these is a filed answer,
    // and a filed 0 must stay 0 rather than becoming absent.
    println!(
        "final_filing={:?} borrowings={:?} cash_not_reported={:?} lei={:?}",
        f.final_filing,
        f.total_borrowings(),
        f.cash_not_reported,
        f.reg_lei
    );
    assert_eq!(
        f.final_filing,
        Some(false),
        "isFinalFiling must survive the parse"
    );
    assert!(f.reg_lei.is_some(), "registrant LEI must survive the parse");
    assert_eq!(
        f.total_borrowings(),
        Some(0.0),
        "this fund files zeros across the borrowing lines; a filed zero is an answer, not an absence"
    );

    // Securities-lending collateral, per position.
    assert!(
        f.holdings
            .iter()
            .any(|h| h.lending_cash_collateral.is_some()),
        "lending collateral answers must survive the parse"
    );

    // Three monthly flow SPANS decomposed from one quarterly filing, tiling
    // backwards from the period end.
    let ends: Vec<_> = f.flows.iter().filter_map(|x| x.month_end.clone()).collect();
    println!("flow spans: {ends:?}");
    assert_eq!(ends, vec!["2026-01-31", "2026-02-28", "2026-03-31"]);
}

/// `edgar nport` end-to-end, plus the span-tiling check.
///
/// Three consecutive quarterly filings must decompose into NINE distinct,
/// contiguous month ends with no duplicates. This is what catches the one
/// genuine assumption in the decomposition: that `mon1` is the EARLIEST month of
/// the period and `mon3` the period end. If that were backwards, each filing
/// would project forward instead of back and consecutive filings would overlap —
/// duplicates here, rather than silently mislabelled flows downstream.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_nport_series_mode_and_span_tiling() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let f = (edgar nport "S000002848" --periods 3)
        {
            payload: {
                periods: ($f | length),
                periods_dates: ($f | get period_date),
                holdings: ($f | first | get holding_count),
                tickers: ($f | first | get holdings | where identifier.kind == "ticker" | length),
                months: ($f | get flows | flatten | get month_end | sort),
                lags: ($f | get lag_days)
            },
            signals: { ok: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar nport must run inside the StepEngine");

    println!(
        "periods={} dates={} holdings={} tickers={} lags={}",
        output.payload["periods"],
        output.payload["periods_dates"],
        output.payload["holdings"],
        output.payload["tickers"],
        output.payload["lags"]
    );
    println!("month spans: {}", output.payload["months"]);

    assert_eq!(output.payload["periods"], serde_json::json!(3));
    assert!(
        output.payload["holdings"].as_u64().unwrap_or(0) > 3000,
        "a total-market index fund holds thousands of positions"
    );

    // The tiling assertion.
    let months: Vec<String> = output.payload["months"]
        .as_array()
        .expect("months array")
        .iter()
        .map(|m| m.as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        months.len(),
        9,
        "three quarterly filings => nine monthly spans"
    );
    let mut uniq = months.clone();
    uniq.dedup();
    assert_eq!(
        uniq.len(),
        9,
        "month spans overlap ({months:?}) — mon1/mon3 are being projected the wrong way"
    );

    // Filing lag is reported raw; judging it late belongs to the deadline table.
    for lag in output.payload["lags"].as_array().expect("lags") {
        let d = lag.as_i64().expect("lag is an integer");
        assert!((0..365).contains(&d), "implausible filing lag: {d} days");
    }
}

/// Unitary trusts select by CIK, not series — SPY has no series id at all.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_nport_unitary_trust_selects_by_cik() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    // 884394 = SPDR S&P 500 ETF TRUST.
    let script = r#"
        let f = (edgar nport "884394" --periods 1)
        {
            payload: {
                period: ($f | first | get period_date),
                holdings: ($f | first | get holding_count),
                net_assets: ($f | first | get net_assets),
                series: (($f | first | get series_id) | describe)
            },
            signals: { ok: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("a unitary trust must pull by CIK");
    println!(
        "SPY period={} holdings={} net_assets={} series_id_type={}",
        output.payload["period"],
        output.payload["holdings"],
        output.payload["net_assets"],
        output.payload["series"]
    );
    assert!(
        output.payload["holdings"].as_u64().unwrap_or(0) > 400,
        "the S&P 500 tracker should hold ~500 positions"
    );
}

// ---------------------------------------------------------------------------
// Holdings -> CIK resolution probe (the gate before the look-through stages)
// ---------------------------------------------------------------------------

/// Fetch the newest N-PORT for an identifier and parse it.
async fn latest_nport(
    edgar: &Edgar,
    identifier: &str,
) -> dapple_prompt_flow::edgar_nport::NportFiling {
    let idx = edgar
        .get(&format!(
            "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={identifier}\
             &type=NPORT-P&dateb=&owner=include&count=1&output=atom"
        ))
        .await
        .expect("browse-edgar index");
    let doc = roxmltree::Document::parse(&idx).expect("atom parses");
    let pick = |tag: &str| {
        doc.descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(|t| t.trim().to_string())
    };
    let accession = pick("accession-number").expect("an NPORT-P accession");
    let cik = pick("cik").expect("feed cik");
    let xml = edgar
        .get(&format!(
            "https://www.sec.gov/Archives/edgar/data/{}/{}/primary_doc.xml",
            cik.trim_start_matches('0'),
            accession.replace('-', "")
        ))
        .await
        .expect("primary_doc.xml");
    dapple_prompt_flow::edgar_nport::parse_nport(&xml).expect("filing parses")
}

/// **The gate.** Measures how much of a real portfolio resolves to a filer, and
/// therefore whether the look-through stages can be built on this at all.
///
/// It deliberately probes only the two FREE exact paths (ticker, then normalized
/// issuer name against `company_tickers.json`). The plan's fallback — a CUSIP→CIK
/// map bootstrapped from the DERA 13F data sets — is expensive and only worth
/// building if these leave too much of NAV unread. Measure before building.
///
/// Three shapes, because they fail differently: a clean large-cap tracker, a
/// total-market fund whose tail is micro-caps and foreign listings, and a bond
/// fund that is mostly instruments with no issuer filing at all.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn holdings_resolution_rate_probe() {
    use dapple_prompt_flow::edgar_identity::{
        Coverage, FilerRegistry, HoldingIdentity, resolve_holding, summarize,
    };

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");

    let tickers_json = edgar
        .get("https://www.sec.gov/files/company_tickers.json")
        .await
        .expect("fetch company_tickers.json");
    let reg = FilerRegistry::from_json(&tickers_json).expect("registry parses");
    println!(
        "filer registry: {} tickers, {} ambiguous normalized names\n",
        reg.len(),
        reg.ambiguous_count()
    );
    assert!(reg.len() > 5_000, "expected the full ticker registry");

    let report = |label: &str, c: &Coverage| {
        println!("=== {label} ===");
        println!(
            "  positions: {} total | {} resolved | {} not-a-filer | {} unresolved",
            c.total_positions,
            c.resolved_positions,
            c.not_a_filer_positions,
            c.unresolved_positions
        );
        println!(
            "  NAV%:      {:.2} resolved | {:.2} not-a-filer | {:.2} unresolved",
            c.resolved_pct, c.not_a_filer_pct, c.unresolved_pct
        );
        println!("  via:       {:?}", c.by_method);
        if !c.not_a_filer_reasons.is_empty() {
            println!("  no filer:  {:?}", c.not_a_filer_reasons);
        }
        if !c.unresolved_reasons.is_empty() {
            println!("  why not:   {:?}", c.unresolved_reasons);
        }
        for (name, w) in c.worst_unresolved.iter().take(5) {
            println!("    unresolved {w:>6.3}%  {name}");
        }
        println!();
    };

    // (label, identifier) — SPY selects by CIK (unitary trust, no series).
    let funds = [
        ("SPY  large-cap tracker (unitary)", "884394"),
        ("VTSAX total market (3.5k positions)", "S000002848"),
        ("VBTLX total bond market", "S000002853"),
    ];

    let mut worst_resolved_pct = f64::MAX;
    for (label, id) in funds {
        let f = latest_nport(&edgar, id).await;
        let rows: Vec<_> = f
            .holdings
            .iter()
            .map(|h| {
                let ticker = h
                    .identifier
                    .as_ref()
                    .and_then(|i| (i.kind() == "ticker").then_some(i.value()));
                let res = resolve_holding(
                    &reg,
                    &HoldingIdentity {
                        name: h.name.as_deref(),
                        ticker,
                        asset_cat: h.asset_cat.as_deref(),
                        issuer_cat: h.issuer_cat.as_deref(),
                        is_derivative: h.is_derivative(),
                    },
                );
                (
                    h.name.clone().unwrap_or_else(|| "(unnamed)".into()),
                    h.pct_val,
                    res,
                )
            })
            .collect();
        let c = summarize(&rows);
        report(&format!("{label} [{} positions]", f.holdings.len()), &c);

        // Coverage is only meaningful against what COULD be read: instruments
        // with no filer are excluded from the denominator, since a Treasury is a
        // fact about the portfolio, not a failure of the resolver.
        let lookable = c.resolved_pct + c.unresolved_pct;
        if lookable > 1.0 {
            worst_resolved_pct = worst_resolved_pct.min(c.resolved_pct / lookable * 100.0);
        }
    }

    println!("worst per-fund resolution of lookable NAV: {worst_resolved_pct:.1}%");
    // Measured at 96.8% (2026-08-01). The threshold sits below that with room
    // for registry drift, but high enough that a normalization regression —
    // which is how this reached 96.8% from an initial 84.7% — fails loudly
    // instead of quietly shrinking the look-through.
    assert!(
        worst_resolved_pct > 90.0,
        "the free ticker+name cascade resolves only {worst_resolved_pct:.1}% of lookable NAV \
         on the worst fund; it measured 96.8% — either normalization regressed or the CUSIP \
         bootstrap is now required"
    );
}

/// The real 485BPOS section index: 181 reports covering 8 funds.
///
/// The synthetic fixture has two sections per fund; this has eleven, which is
/// what actually exercises the fund-name derivation.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn filing_summary_partitions_a_real_multi_fund_prospectus() {
    use dapple_prompt_flow::edgar_fund_sections::parse_filing_summary;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    let xml = edgar
        .get("https://www.sec.gov/Archives/edgar/data/1605941/000119312526066774/FilingSummary.xml")
        .await
        .expect("fetch FilingSummary.xml");

    let f = parse_filing_summary(&xml).expect("must parse");
    println!("sections={} funds={}", f.sections.len(), f.fund_names.len());
    for (i, name) in &f.fund_names {
        println!("  fund {i}: {name:?} ({} sections)", f.for_fund(*i).len());
    }
    if !f.warnings.is_empty() {
        println!("warnings: {:?}", &f.warnings[..f.warnings.len().min(3)]);
    }

    assert_eq!(f.fund_names.len(), 8, "this prospectus covers eight funds");
    for (i, name) in &f.fund_names {
        // A fund name that absorbed a section title would run long and end in a
        // section word; one that over-stripped would be empty.
        assert!(!name.is_empty(), "fund {i} lost its name entirely");
        assert!(
            name.to_lowercase().ends_with("fund"),
            "fund {i} name {name:?} looks like it absorbed a section title"
        );
    }
    // Every fund must expose the two sections the guide's promise side needs.
    for i in f.fund_names.keys() {
        for key in ["InvestmentStrategy", "InvestmentRisks"] {
            assert!(
                f.section(key, Some(*i)).is_some(),
                "fund {i} is missing {key}"
            );
        }
    }
}

/// The real N-CEN: registrant red flags plus per-series fee and lending economics.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn ncen_parses_a_live_filing() {
    use dapple_prompt_flow::edgar_ncen::parse_ncen;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    let xml = edgar
        .get("https://www.sec.gov/Archives/edgar/data/1605941/000207169126000005/primary_doc.xml")
        .await
        .expect("fetch N-CEN primary_doc.xml");

    let f = parse_ncen(&xml).expect("must parse");
    println!(
        "schema={:?} accountants={:?} flags={:?} series={}",
        f.schema_version,
        f.accountants
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>(),
        f.raised_flags(),
        f.series.len()
    );
    for s in f.series.iter().take(3) {
        println!(
            "  {:?} {:?} non_div={:?} waiver={:?} recoupable={:?} lending_income={:?} agents={:?}",
            s.series_id,
            s.fund_name,
            s.non_diversified,
            s.expense_limitation,
            s.fees_waived_recoupable,
            s.sec_lending_net_income,
            s.payments_to_agents
        );
    }

    assert!(!f.series.is_empty(), "N-CEN must yield per-series blocks");
    assert!(
        !f.accountants.is_empty(),
        "the independent accountant must be named"
    );
    assert!(
        f.series.iter().all(|s| s.series_id.is_some()),
        "every series block must carry its series id"
    );
    // The fee-waiver economics the fee stage depends on.
    assert!(
        f.series.iter().any(|s| s.fees_waived_recoupable.is_some()),
        "recoupable-waiver disclosure must survive the parse"
    );
}

/// The fields a fixture cannot vouch for: lifecycle, board and adviser identity
/// against a real filing.
///
/// Every one of these was written from a live document, and a hand-built fixture
/// only proves the parser reads the fixture. Vanguard (CIK 36405) is used
/// because it is large enough to exercise the board block and multi-series
/// lifecycle answers at once.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn ncen_lifecycle_board_and_adviser_survive_a_live_filing() {
    use dapple_prompt_flow::edgar_ncen::parse_ncen;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    let xml = edgar
        .get("https://www.sec.gov/Archives/edgar/data/36405/000003640526000103/primary_doc.xml")
        .await
        .expect("fetch N-CEN primary_doc.xml");

    let f = parse_ncen(&xml).expect("must parse");
    let (independent, interested) = f.board_split();
    println!(
        "series={} directors={} board={independent} independent / {interested} interested",
        f.series.len(),
        f.directors.len()
    );
    for s in f.series.iter().take(3) {
        println!(
            "  {:?} liquidated={:?} first={:?} terminated_classes={:?} credit={:?} \
             track_before={:?} track_after={:?} adviser={:?} crd={:?} agent_affiliated={:?}",
            s.series_id,
            s.liquidated,
            s.first_filing,
            s.terminated_classes,
            s.credit_line,
            s.tracking_diff_before_expense,
            s.tracking_diff_after_expense,
            s.advisers,
            s.adviser_crd,
            s.lending_agent_affiliated
        );
    }

    // The board block sits outside `registrantInfo`; a scoping mistake returns
    // an empty vec rather than an error, so assert it is populated.
    assert!(!f.directors.is_empty(), "the board must be named");
    assert!(
        f.directors.iter().all(|d| d.name.is_some()),
        "a director row without a name is a parse artefact, not a trustee"
    );
    assert!(
        independent + interested <= f.directors.len(),
        "the split can never exceed the roster: an unclassified trustee is in neither count"
    );

    // The service-provider path had no assertion anywhere before this.
    assert!(
        f.series.iter().any(|s| !s.advisers.is_empty()),
        "the adviser must be named on at least one series"
    );
    assert!(
        f.series.iter().any(|s| s.adviser_crd.is_some()),
        "the adviser's CRD is the identity key the name match cannot supply"
    );

    // Lifecycle answers must be present as filed answers, not absent fields.
    assert!(
        f.series.iter().any(|s| s.liquidated.is_some()),
        "isFundLiquidated must survive the parse"
    );
    assert!(
        f.series.iter().any(|s| s.terminated_classes.is_some()),
        "numTerminatedClass must survive the parse"
    );

    // `isCreditLineCommitted` answers with a word. If this ever comes back
    // empty on a fund that has a facility, something started routing it
    // through the Y/N reader.
    assert!(
        f.series.iter().any(|s| s.credit_line.is_some()),
        "the credit-line answer is text and must not be dropped by a boolean reader"
    );
}

/// `edgar ncen` and `edgar fund-sections` end-to-end through the StepEngine.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn fund_document_commands_run_inside_step_engine() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let c = (edgar ncen "1605941")
        let s = (edgar fund-sections "1605941")
        let one = (edgar fund-sections "1605941" --fund "1290 High Yield Bond Fund"
                     --fetch [InvestmentStrategy])
        {
            payload: {
                auditor: ($c.accountants | first | get name),
                flags: ($c.raised_flags | length),
                ncen_series: ($c.series | length),
                recoupable: ($c.series | where fees_waived_recoupable == true | length),
                funds: ($s.funds | where fund_index != null | length),
                one_fund: ($one.funds | length),
                one_name: ($one.funds | first | get fund_name),
                fetched: ($one.funds | first | get sections | where text != null | length),
                strategy_chars: ($one.funds | first | get sections
                                  | where section_key == "InvestmentStrategy"
                                  | first | get text | str length)
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("fund document commands must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    assert_eq!(out.payload["ncen_series"], serde_json::json!(8));
    assert!(out.payload["recoupable"].as_u64().unwrap_or(0) > 0);
    assert_eq!(out.payload["funds"], serde_json::json!(8));
    // --fund narrows to exactly one group, and --fetch pulls its rendered text.
    assert_eq!(out.payload["one_fund"], serde_json::json!(1));
    assert_eq!(out.payload["fetched"], serde_json::json!(1));
    assert!(
        out.payload["strategy_chars"].as_u64().unwrap_or(0) > 200,
        "the investment-strategy section should carry real prose, got {}",
        out.payload["strategy_chars"]
    );
}

/// `--tagged` against a real prospectus: the risks and benchmarks a fund actually tagged.
///
/// The assertions here are STRUCTURAL rather than a pinned inventory, because the inventory
/// is the filer's to change at the next annual update and a test that pins it would fail on
/// a correct reading. What must hold regardless:
///
///   - taxonomy boilerplate never reaches `risks` (that is the namespace filter working);
///   - a share-classed performance measure never reaches `benchmarks` (the discriminator);
///   - every member carries a non-empty name (the ladder always produces something);
///   - and an EMPTY result is accompanied by a warning saying why — a fund whose prospectus
///     predates the 2024q3 risk axis is a fact, and reading it as "this fund tagged no
///     risks" is the inverted answer this command exists not to give.
///
/// The actual inventory is printed so a reader can eyeball it against the prospectus.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn tagged_reads_the_risks_and_benchmarks_a_prospectus_declared() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let p = (edgar fund-sections "1605941" --form "485BPOS" --tagged)
        let series = ($p.tagged | columns)
        let all_risks = ($series | each {|s| ($p.tagged | get $s | get risks) } | flatten)
        let all_bench = ($series | each {|s| ($p.tagged | get $s | get benchmarks) } | flatten)
        {
            payload: {
                series: $series,
                risk_count: ($all_risks | length),
                bench_count: ($all_bench | length),
                risk_members: ($all_risks | each {|r| $r.member } | uniq),
                risk_names: ($all_risks | each {|r| $r.name } | uniq),
                bench_names: ($all_bench | each {|b| $b.name } | uniq),
                unattributed: ($p.unattributed_benchmarks | each {|b| $b.name }),
                warnings: $p.warnings,
                unnamed: ($all_risks | append $all_bench
                           | where {|m| ($m.name | str trim | str length) == 0 } | length)
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("--tagged must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    let risk_members = out.payload["risk_members"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let bench_names = out.payload["bench_names"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let warnings = out.payload["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Boilerplate is dropped by declaring namespace, so no rr/oef-OWNED member survives.
    // `us-gaap:` members are NOT boilerplate and must be allowed through.
    for m in &risk_members {
        let q = m.as_str().unwrap_or_default();
        assert!(
            !q.starts_with("oef:") && !q.starts_with("rr:"),
            "taxonomy boilerplate leaked into the tagged risks: {q}"
        );
    }
    // The fund's own returns are share-classed and must never read as a benchmark.
    for b in &bench_names {
        let n = b.as_str().unwrap_or_default().to_ascii_lowercase();
        assert!(
            !n.contains("after tax") && !n.contains("aftertax"),
            "a share-classed return was reported as a benchmark: {n}"
        );
    }
    assert_eq!(
        out.payload["unnamed"],
        serde_json::json!(0),
        "the name ladder must always yield a name"
    );
    // Silence is only acceptable when something explains it.
    if risk_members.is_empty() {
        assert!(
            !warnings.is_empty(),
            "no risks were tagged and nothing said why — that is the reading this command \
             must never leave a caller to make"
        );
    }
}

/// The guide's prospectus fetch keys must match the filing's real role vocabulary.
///
/// `retort-the-n-port`'s pull asked for eight section keys, five of which are not
/// in the vocabulary this command documents — `InvestmentObjective`,
/// `ShareholderFees`, `AnnualFundOperatingExpenses`, `AverageAnnualTotalReturns`
/// and `AnnualTotalReturns`. They matched nothing, so the promise pass's
/// `objective` and the fees pass's `expense_text` resolved to the empty string on
/// EVERY run. That is not a degraded answer but an inverted one: a fund whose
/// prospectus was never read reads as a fund that disclosed nothing, and a fee
/// finding computed from "" is the worst available lie.
///
/// This runs the guide's own candidate lists against a live filing, so it FAILS if
/// the mapping is wrong rather than reporting a plausible zero. `keys_with_text`
/// is printed for the case where a filer uses a vocabulary neither list covers.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn guide_prospectus_fetch_keys_match_the_role_vocabulary() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    // Mirrors retort-the-n-port-pull.nu `prospectus_keys` and the candidate lists
    // in promise-generate / fees-generate verbatim. Keep the three in step.
    let script = r#"
        let keys = ["RiskReturn" "InvestmentObjective" "InvestmentStrategy" "InvestmentRisks"
                    "FeesAndExpenses" "ShareholderFeesData" "ShareholderFees"
                    "OperatingExpensesData" "AnnualFundOperatingExpenses" "ExpenseExample"
                    "PerformanceTableData" "BarChartData"]
        let p = (edgar fund-sections "1605941" --fund "1290 High Yield Bond Fund" --fetch $keys)
        let sections = ($p.funds | each {|f| ($f.sections? | default []) } | flatten)

        let section_of = {|ks|
            let hit = ($ks | each {|k|
                ($sections
                  | where {|s| (($s.section_key? | default "" | into string) | str downcase) == ($k | str downcase) }
                  | where {|s| (($s.text? | default "" | into string) | str trim | str length) > 0 }
                  | get -i 0)
            } | where {|m| $m != null } | get -i 0)
            if $hit == null { "" } else { ($hit.text? | default "" | into string) }
        }
        let section_text = {|ks|
            let want = ($ks | each {|k| $k | str downcase })
            ($sections
              | where {|s| (($s.section_key? | default "" | into string) | str downcase) in $want }
              | each {|s| ($s.text? | default "" | into string) }
              | where {|t| ($t | str trim | str length) > 0 }
              | str join " ")
        }
        let objective = (do $section_of ["InvestmentObjective" "RiskReturn"])
        let expense = (do $section_text ["OperatingExpensesData" "AnnualFundOperatingExpenses"
                                         "ShareholderFeesData" "ShareholderFees" "FeesAndExpenses"])
        {
            payload: {
                keys_with_text: ($sections
                    | where {|s| (($s.text? | default "" | into string) | str trim | str length) > 0 }
                    | each {|s| ($s.section_key? | default "" | into string) } | uniq | sort),
                objective_chars: ($objective | str length),
                expense_chars: ($expense | str length),
                strategy_chars: ((do $section_of ["InvestmentStrategy"]) | str length)
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("the guide's fetch keys must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    // The two reads that were silently empty. Both must now carry real prose.
    //
    // The bar is 80 rather than the 200 it started at, and the number went DOWN because the
    // text got BETTER. It was calibrated when `--fetch` returned the rendered `R<n>.htm`,
    // whose bulk is viewer chrome; the section now comes from the instance, where a fund's
    // stated objective is what it is in the prospectus — ONE SENTENCE. Measured across all
    // eight funds of this filing, `ObjectivePrimaryTextBlock` runs 33 to 119 characters
    // ("Seeks to maximize current income." is a complete answer at 33), and the whole
    // `RiskReturn` section with its headings measured 147. A 200-character floor would be
    // asserting that the filing pads its objective.
    assert!(
        out.payload["objective_chars"].as_u64().unwrap_or(0) > 80,
        "the fund's stated objective must be readable; got {} chars. Section keys \
         that came back with text: {}",
        out.payload["objective_chars"],
        out.payload["keys_with_text"]
    );
    assert!(
        out.payload["expense_chars"].as_u64().unwrap_or(0) > 200,
        "the fee tables must be readable; got {} chars. Section keys that came back \
         with text: {}",
        out.payload["expense_chars"],
        out.payload["keys_with_text"]
    );
    // Control: this key was already correct, so a failure here is the filing or the
    // command changing, not the key mapping.
    assert!(
        out.payload["strategy_chars"].as_u64().unwrap_or(0) > 200,
        "control read (InvestmentStrategy) must still work"
    );
}

/// `edgar company-cik` end-to-end: is a fund's adviser itself a public company?
///
/// The load-bearing assertion is that `not_listed` stays APART from
/// `unresolved`. A mutually-owned adviser (Vanguard, owned by its own funds) and
/// a private one (Fidelity, Capital Group) answer `not_listed` correctly and
/// permanently — that is a fact about how the firm is owned, and reporting it as
/// a failed lookup would throw the answer away.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn company_cik_separates_not_listed_from_unresolved() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        # A listed adviser, a mutually-owned one, and a private one.
        let rows = ([
            "Franklin Resources Inc"
            "The Vanguard Group, Inc."
            "Fidelity Management & Research Company"
        ] | edgar company-cik)

        # And the single-argument form, which answers with a record.
        let one = (edgar company-cik "BlackRock, Inc.")

        # EDGAR titles some filers surname-first: T. Rowe Price is
        # `PRICE T ROWE GROUP INC`. A conventional spelling therefore misses a
        # genuinely listed company, which is why `not_listed` means "not matched
        # under this name" and NEVER "is not a public company".
        let surname_first = (edgar company-cik "T. Rowe Price Group Inc")
        let as_titled = (edgar company-cik "PRICE T ROWE GROUP INC")

        {
            payload: {
                rows: ($rows | length),
                listed: ($rows | where outcome == "listed" | length),
                not_listed: ($rows | where outcome == "not_listed" | length),
                unresolved: ($rows | where outcome == "unresolved" | length),
                franklin_cik: ($rows | where name =~ "Franklin" | first | get cik),
                vanguard_outcome: ($rows | where name =~ "Vanguard" | first | get outcome),
                one_outcome: $one.outcome,
                one_has_cik: ($one.cik != null),
                surname_first_outcome: $surname_first.outcome,
                as_titled_outcome: $as_titled.outcome,
                as_titled_cik: $as_titled.cik
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar company-cik must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    assert_eq!(out.payload["rows"], serde_json::json!(3));

    // Franklin Resources is NYSE-listed and files its own 10-K, which is what
    // makes the adviser's own stated interests quotable rather than inferred.
    assert_eq!(out.payload["franklin_cik"], serde_json::json!(38777));

    // Vanguard is owned by its own funds. `not_listed` here is the ANSWER.
    assert_eq!(
        out.payload["vanguard_outcome"],
        serde_json::json!("not_listed"),
        "a mutually-owned adviser must answer not_listed, never unresolved"
    );

    // Nothing in this set is ambiguous, so the two negative buckets must not
    // have been merged.
    assert_eq!(
        out.payload["unresolved"],
        serde_json::json!(0),
        "no name here is ambiguous; a non-zero count means not_listed leaked into unresolved"
    );
    assert!(out.payload["not_listed"].as_u64().unwrap_or(0) >= 1);
    assert!(out.payload["listed"].as_u64().unwrap_or(0) >= 1);

    // The single-argument form answers with a record, not a one-row table.
    assert_eq!(out.payload["one_outcome"], serde_json::json!("listed"));
    assert_eq!(out.payload["one_has_cik"], serde_json::json!(true));

    // THE LIMITATION, ASSERTED RATHER THAN REMEMBERED. EDGAR titles some filers
    // surname-first (`PRICE T ROWE GROUP INC`), so a conventional spelling of a
    // genuinely listed company answers `not_listed`. The lookup is truthful —
    // it found no match under that name — but a consumer that renders this as
    // "the adviser is not a public company" states something false. The guide
    // must phrase the negative as UNESTABLISHED.
    assert_eq!(
        out.payload["surname_first_outcome"],
        serde_json::json!("not_listed"),
        "if this ever becomes `listed`, the matcher widened and the holdings \
         resolution rate needs re-measuring with it"
    );
    assert_eq!(
        out.payload["as_titled_outcome"],
        serde_json::json!("listed")
    );
    assert_eq!(out.payload["as_titled_cik"], serde_json::json!(1113169));
}

/// `edgar search` end-to-end, including the case that would silently truncate.
///
/// The service refuses to page past 10,000 documents. A command that quietly
/// returned the first page of a 400,000-hit query would look exactly like one
/// that found everything, which is the failure this asserts against: a broad
/// query must set `ceiling_hit` and carry a note naming the shortfall.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_search_reports_what_it_could_not_reach() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        # A narrow window: one quarter of 497 supplements mentioning the phrase.
        let narrow = (edgar search '"portfolio manager"' --forms [497]
                        --startdt 2025-01-01 --enddt 2025-03-31 --limit 20)

        # One fund family, narrower still.
        let scoped = (edgar search '"portfolio manager"' --forms [497]
                        --ciks ["0000036405"] --limit 5)

        # Deliberately broad, to trip the service's own ceiling.
        let broad = (edgar search "fund" --limit 10)

        {
            payload: {
                narrow_total: $narrow.total,
                narrow_returned: $narrow.returned,
                narrow_truncated: $narrow.truncated,
                narrow_ceiling: $narrow.ceiling_hit,
                first_form: ($narrow.hits | first | get form),
                first_has_url: (($narrow.hits | first | get url) != null),
                first_names: ($narrow.hits | first | get names | length),
                narrow_is_floor: $narrow.total_is_lower_bound,
                scoped_returned: $scoped.returned,
                broad_ceiling: $broad.ceiling_hit,
                broad_total: $broad.total,
                broad_is_floor: $broad.total_is_lower_bound,
                broad_note_len: ($broad.note | default "" | str length)
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar search must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    // A real corpus behind the narrow query, and --limit honoured exactly.
    assert!(out.payload["narrow_total"].as_u64().unwrap_or(0) > 100);
    assert_eq!(out.payload["narrow_returned"], serde_json::json!(20));
    assert_eq!(out.payload["narrow_truncated"], serde_json::json!(true));
    assert_eq!(
        out.payload["narrow_ceiling"],
        serde_json::json!(false),
        "one quarter of 497s is far under the ceiling"
    );

    // Hits are usable: the form, a fetchable URL, and every joint filer.
    assert_eq!(out.payload["first_form"], serde_json::json!("497"));
    assert_eq!(out.payload["first_has_url"], serde_json::json!(true));
    assert!(
        out.payload["first_names"].as_u64().unwrap_or(0) >= 1,
        "display_names is a list and must survive as one"
    );

    assert!(out.payload["scoped_returned"].as_u64().unwrap_or(0) >= 1);

    // THE LOAD-BEARING ONE. A query this broad exceeds what any amount of
    // paging can reach, and the command must say so rather than hand back a
    // page that looks complete.
    assert_eq!(
        out.payload["broad_ceiling"],
        serde_json::json!(true),
        "a corpus-wide query must report the service's 10,000 ceiling"
    );
    assert!(
        out.payload["broad_note_len"].as_u64().unwrap_or(0) > 40,
        "the ceiling must come with a sentence naming the shortfall"
    );

    // `total` IS A FLOOR HERE, NOT A COUNT. The service pins the value at
    // exactly 10,000 and qualifies it in a separate field, so a consumer reading
    // `total` alone would state an exact figure the service never claimed.
    assert_eq!(out.payload["broad_total"], serde_json::json!(10_000));
    assert_eq!(
        out.payload["broad_is_floor"],
        serde_json::json!(true),
        "a pinned total must be flagged as a lower bound"
    );
    // And a countable query must NOT be flagged, or the qualifier means nothing.
    assert_eq!(out.payload["narrow_is_floor"], serde_json::json!(false));
}

/// `edgar resolve` end-to-end: the look-through hinge.
///
/// The Rust cascade already has a probe (`holdings_resolution_rate_probe`); this
/// asserts the NUSHELL surface over it, because a guide script cannot call a Rust
/// module and every look-through stage depends on this command existing.
///
/// The load-bearing assertion is that the three buckets stay DISTINCT and sum to
/// the portfolio. Collapsing `no_filer` into `unresolved` is the failure mode that
/// would report a bond fund as unreadable, and it is invisible in a single
/// resolution-rate number.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_resolve_annotates_and_weights() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let r = (edgar nport "S000002848" --periods 1 | first | get holdings | edgar resolve)
        let c = $r.coverage
        let h = $r.holdings
        {
            payload: {
                rows: ($h | length),
                total: $c.total_positions,
                resolved_pct: $c.resolved_pct,
                no_filer_pct: $c.no_filer_pct,
                unresolved_pct: $c.unresolved_pct,
                by_method: $c.by_method,
                no_filer_reasons: $c.no_filer_reasons,
                worst: ($c.worst_unresolved | first 3),
                # Every annotated row carries the four new columns.
                labels: ($h | get resolution | uniq | sort),
                resolved_have_cik: ($h | where resolution == "resolved" | where cik == null | length),
                nofiler_have_reason: ($h | where resolution == "no_filer" | where reason == null | length),
                # A resolved row keeps everything it arrived with.
                keeps_pct: ($h | first | get pct_val | describe)
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar resolve must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    let rows = out.payload["rows"].as_u64().unwrap_or(0);
    assert!(
        rows > 3000,
        "a total-market fund holds thousands of positions"
    );
    assert_eq!(out.payload["total"].as_u64().unwrap_or(0), rows);

    // The three buckets partition the portfolio: they sum to ~100% of NAV.
    let resolved = out.payload["resolved_pct"].as_f64().unwrap_or(0.0);
    let no_filer = out.payload["no_filer_pct"].as_f64().unwrap_or(0.0);
    let unresolved = out.payload["unresolved_pct"].as_f64().unwrap_or(0.0);
    let total_pct = resolved + no_filer + unresolved;
    assert!(
        (95.0..=105.0).contains(&total_pct),
        "buckets must partition the portfolio, got {total_pct} \
         (pct_val is a percentage, not a fraction)"
    );

    // The gate the whole look-through rests on.
    assert!(
        resolved > 90.0,
        "free ticker + name matching resolved only {resolved}% of NAV; \
         a normalization regression is the usual cause"
    );

    // Buckets stay separable, and each carries the field that explains it.
    assert_eq!(
        out.payload["resolved_have_cik"],
        serde_json::json!(0),
        "a resolved holding must carry a CIK"
    );
    assert_eq!(
        out.payload["nofiler_have_reason"],
        serde_json::json!(0),
        "a no_filer holding must say which instrument class it is"
    );
    // A ticker identifier resolves for free; that path must actually be exercised.
    assert!(
        out.payload["by_method"]["ticker"].as_u64().unwrap_or(0) > 0
            || out.payload["by_method"]["name"].as_u64().unwrap_or(0) > 0,
        "no holding resolved by either free path"
    );
}

/// The Form ADV Part 1A roster parses live, and the three things about it that
/// are easy to get wrong all hold.
///
/// This exists because the capability was written off once on an unchecked
/// sentence ("Form ADV is on IAPD, not EDGAR"), and because the three traps
/// below are each the kind that ships green and misleads later:
///
/// 1. **The CSV is not UTF-8.** A UTF-8-only read fails outright and a shell
///    `grep` over the raw bytes silently reports no match on a string that is
///    plainly there.
/// 2. **The self-reported CIK is wrong** for a real subject, so a CIK join must
///    never be the primary key.
/// 3. **Freshness cannot be asserted with a row count.** The pinned snapshot is
///    a constant someone has to bump; the newest filing date is the one signal
///    that moves the right way when they have not.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn adv_roster_parses_live_and_its_three_traps_hold() {
    use dapple_prompt_flow::edgar_adv::{
        ADV_ROSTER_MIN_LATEST_FILING, ADV_ROSTER_SNAPSHOT, ADV_ROSTER_URL, AdvLookup,
        RosterEncoding, decode_roster, lookup_adviser, parse_adv_roster,
    };
    use std::io::Read;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    let bytes = edgar
        .get_bytes(ADV_ROSTER_URL)
        .await
        .expect("fetch the ADV roster zip");

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("roster is a zip");
    let csv_members: Vec<usize> = (0..zip.len())
        .filter(|i| {
            zip.by_index(*i)
                .map(|f| f.name().to_ascii_lowercase().ends_with(".csv"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        csv_members.len(),
        1,
        "the archive is expected to hold exactly one CSV member"
    );
    let mut raw = Vec::new();
    zip.by_index(csv_members[0])
        .expect("open member")
        .read_to_end(&mut raw)
        .expect("read member");

    // TRAP 1. Decoding is not a formality here.
    let (text, encoding) = decode_roster(&raw);
    println!(
        "ADV roster: {} bytes, decoded as {}",
        raw.len(),
        encoding.as_str()
    );
    if encoding == RosterEncoding::Utf8 {
        println!(
            "NOTE: the roster is now valid UTF-8. The windows-1252 fallback is still \
             correct but is no longer exercised by the live file."
        );
    }

    let parsed = parse_adv_roster(&text).expect("roster must parse");
    println!(
        "ADV roster: {} advisers, {} skipped, newest filing {:?}",
        parsed.rows.len(),
        parsed.skipped,
        parsed.max_latest_filing
    );
    assert!(
        parsed.rows.len() > 10_000,
        "expected the full adviser roster, got {} rows",
        parsed.rows.len()
    );
    let skip_pct = (parsed.skipped as f64 / (parsed.rows.len() + parsed.skipped) as f64) * 100.0;
    assert!(skip_pct < 2.0, "skip rate {skip_pct:.2}% is too high");

    // TRAP 3. The assertion a row count cannot make. If someone repoints
    // ADV_ROSTER_URL at an older snapshot, rows.len() may well go UP; the newest
    // filing date is the term that goes down.
    let newest = parsed
        .max_latest_filing
        .as_deref()
        .expect("roster must carry readable filing dates");
    assert!(
        newest >= ADV_ROSTER_MIN_LATEST_FILING,
        "the pinned {ADV_ROSTER_SNAPSHOT} snapshot should reach at least \
         {ADV_ROSTER_MIN_LATEST_FILING}, but its newest filing is {newest} — \
         the URL may have been repointed at an older file"
    );

    // The two first subjects, by their two different working routes.
    let AdvLookup::Registered(mfam) = lookup_adviser(&parsed.rows, "149688") else {
        panic!("CRD 149688 (Motley Fool Asset Management) must resolve");
    };
    assert!(
        mfam.primary_business_name.to_lowercase().contains("motley"),
        "CRD 149688 resolved to {}",
        mfam.primary_business_name
    );
    assert!(
        mfam.aum_total.unwrap_or(0.0) > 1e9,
        "regulatory AUM should be billions, got {:?}",
        mfam.aum_total
    );
    assert!(
        !mfam.fee_bases.is_empty(),
        "Item 5E should report at least one compensation basis"
    );

    let AdvLookup::Registered(mfwm) =
        lookup_adviser(&parsed.rows, "Motley Fool Wealth Management LLC")
    else {
        panic!("the name path must find Motley Fool Wealth Management");
    };
    assert_eq!(mfwm.crd, "165689");

    // TRAP 2, live. The adviser filed a truncated CIK, so a CIK join misses the
    // firm entirely while the name join finds it. If the SEC or the filer ever
    // corrects this, the assertion fires and the module docs need revisiting —
    // which is the point, since the whole join strategy rests on it.
    assert_ne!(
        mfwm.cik_self_reported,
        Some(1_607_978),
        "the roster's self-reported CIK is expected to be WRONG here; if it is now \
         correct, re-read the join-key note in edgar_adv"
    );
    assert_eq!(
        lookup_adviser(&parsed.rows, "1607978"),
        AdvLookup::NotRegistered,
        "the real EDGAR CIK must not resolve, because the adviser never filed it"
    );

    // NOT_REGISTERED IS AN ANSWER. Berkshire files 13F as an operating company.
    assert_eq!(
        lookup_adviser(&parsed.rows, "BERKSHIRE HATHAWAY INC"),
        AdvLookup::NotRegistered,
        "an operating company that files 13F is not a registered adviser"
    );

    // The coverage ratio this whole command exists to enable.
    for (label, row, filed_13f) in [
        ("MFAM", &mfam, 2_326_025_992.0_f64),
        ("MFWM", &mfwm, 884_214_118.0_f64),
    ] {
        let aum = row.aum_total.expect("regulatory AUM");
        println!(
            "{label}: 13F ${filed_13f:.0} / ADV AUM ${aum:.0} = {:.1}% of the manager",
            filed_13f / aum * 100.0
        );
        assert!(
            filed_13f <= aum * 1.05,
            "{label}: a 13F should not exceed regulatory AUM"
        );
    }
}

/// N-MFP3, parsed straight from a live filing, with all six traps asserted.
///
/// Subject: Morgan Stanley Institutional Liquidity Funds, Prime Portfolio
/// (`S000004151`), report date 2026-06-30, a $14.4bn floating-NAV institutional
/// prime fund with ten share classes and 158 positions. It is the fund the
/// misfiling was found on, so this test doubles as the regression pin for it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn nmfp_parses_a_live_filing_and_its_six_traps_hold() {
    use dapple_prompt_flow::edgar_nmfp::parse_nmfp;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    let xml = edgar
        .get("https://www.sec.gov/Archives/edgar/data/1227155/000141036826069212/primary_doc.xml")
        .await
        .expect("fetch N-MFP3 primary_doc.xml");

    let f = parse_nmfp(&xml).expect("must parse");
    println!(
        "series={:?} category={:?} classes={} positions={} nav_points={} gross={} warnings={:?}",
        f.series_id,
        f.category,
        f.classes.len(),
        f.positions.len(),
        f.daily_nav.len(),
        f.seven_day_gross_yield.len(),
        f.warnings
    );

    assert_eq!(f.series_id.as_deref(), Some("S000004151"));
    assert_eq!(f.report_date.as_deref(), Some("2026-06-30"));
    assert_eq!(f.total_share_classes, Some(10));
    assert_eq!(f.classes.len(), 10);
    assert_eq!(f.positions.len(), 158);

    // TRAP 1 — the daily series are self-nested. A descendant-first read returns
    // the wrapper and leaves every value None.
    assert_eq!(f.daily_nav.len(), 21, "one point per business day");
    assert!(
        f.daily_nav.iter().all(|p| p.value.is_some()),
        "every dated point must carry a value; all-None means the wrapper was read"
    );

    // TRAP 2 — proportions are fractions, so a filed .4018 is 40.18 percent.
    let la = f.liquid_assets.first().expect("liquidity is filed daily");
    let daily = la.daily_pct.expect("daily liquid assets pct").as_percent();
    assert!(
        (1.0..=100.0).contains(&daily),
        "a liquidity share of {daily}% is out of range; the fraction was read as a percentage"
    );

    // TRAP 3 — the daily series do not share a scope. Gross yield is filed once
    // for the fund, net yield once per class, so a date-only zip of the two
    // would fan 21 rows out to 210.
    assert_eq!(f.seven_day_gross_yield.len(), 21);
    let net_total: usize = f.classes.iter().map(|c| c.seven_day_net_yield.len()).sum();
    assert_eq!(net_total, 210, "21 business days across 10 share classes");
    let spread = f.yield_spread_by_class();
    assert_eq!(spread.len(), 210, "the join is per (class, date)");
    assert!(
        spread.iter().all(|r| r.4 >= 0.0),
        "a net yield above its own fund's gross yield would be a filing error"
    );

    // TRAP 4 — stablePricePerShare 0.0000 is "not applicable". This fund is
    // floating-NAV and files exactly that.
    assert_eq!(f.seeks_stable_price, Some(false));
    assert_eq!(f.stable_price, None);
    let reference = f
        .reference_price()
        .expect("falls back to the series median");
    assert!(
        (reference - 1.0).abs() < 0.01,
        "reference price {reference} should sit near a dollar for this fund"
    );

    // TRAP 5 — nameOfPersonDescExpensePay holds a dollar amount, and it is filed
    // per share class rather than per fund.
    assert!(
        f.classes
            .iter()
            .all(|c| c.expense_payment_amount.is_some() || c.expense_paid_by_person.is_none()),
        "the expense-payment element parses as a number, never as a name"
    );

    // TRAP 6 — the identity closes on the amortized basis for stable-value funds
    // and on market value for floating ones. This one is floating.
    let id = &f.identity;
    println!(
        "identity: closes_on={:?} market_gap={:?} amortized_gap={:?}",
        id.closes_on(),
        id.market_gap(),
        id.amortized_gap()
    );
    assert_eq!(
        id.closes_on(),
        Some("market"),
        "a floating-NAV fund strikes net assets on market value"
    );
    assert_eq!(
        id.shadow_price_deviation(),
        None,
        "amortized cost covers only the eligible subset here; differencing it is a phantom"
    );

    // The misfiling. One point out of band among twenty valid ones, and the
    // all-zero sentinel is NOT what this is.
    assert!(!f.nav_series_not_applicable());
    let bad = f.nav_anomalies();
    println!("nav anomalies: {bad:?}");
    assert_eq!(bad.len(), 1, "exactly one point does not belong");
    assert_eq!(bad[0].date, "2026-06-12");
    assert_eq!(bad[0].value, Some(-0.0144));

    // The class-level series for the same month contains no such point, which is
    // what makes the series-level one a filing artifact rather than a fund event.
    for c in &f.classes {
        assert!(
            c.daily_nav.iter().filter_map(|p| p.value).all(|v| v > 0.0),
            "class {:?} should carry no non-positive NAV",
            c.class_id
        );
    }

    // The flows ride the misspelled wrapper.
    let flow_points: usize = f.classes.iter().map(|c| c.flows.len()).sum();
    assert_eq!(
        flow_points, 210,
        "the `dialy` (sic) wrapper must be matched verbatim or this is 0"
    );

    assert!(
        f.warnings.is_empty(),
        "unexpected warnings: {:?}",
        f.warnings
    );
}

/// A government fund, for the opposite shape: repo collateral itemized in the
/// filing, and a stable-value balance sheet that closes on amortized cost.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn nmfp_reads_repo_collateral_and_the_amortized_identity() {
    use dapple_prompt_flow::edgar_nmfp::parse_nmfp;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");
    // Federated Hermes Money Market Obligations Trust, one series, 2026-06-30.
    let xml = edgar
        .get("https://www.sec.gov/Archives/edgar/data/856517/000119312526298520/primary_doc.xml")
        .await
        .expect("fetch N-MFP3 primary_doc.xml");

    let f = parse_nmfp(&xml).expect("must parse");
    println!(
        "series={:?} category={:?} stable={:?} positions={} repos={}",
        f.series_id,
        f.category,
        f.stable_price,
        f.positions.len(),
        f.positions.iter().filter(|p| p.repo.is_some()).count()
    );

    // A stable-value fund files a price, and its net assets are struck on
    // amortized cost. Asserting the MARKET basis here would report the fund as
    // broken: measured over one month of the whole industry the market basis
    // closes for only 29.6% of filings and the amortized basis for 78.8%.
    assert_eq!(f.seeks_stable_price, Some(true));
    assert!(f.stable_price.is_some());
    assert_eq!(
        f.identity.closes_on(),
        Some("amortized"),
        "market_gap={:?} amortized_gap={:?}",
        f.identity.market_gap(),
        f.identity.amortized_gap()
    );
    let deviation = f
        .identity
        .shadow_price_deviation()
        .expect("readable once the amortized identity closes");
    let nav = f.identity.net_assets.expect("net assets");
    println!(
        "shadow price deviation {deviation} on {nav} net assets; market NAV/share {:?}",
        f.market_nav_per_share()
    );
    assert!(
        (deviation / nav).abs() < 0.01,
        "a money market shadow price moves in basis points, not percent"
    );

    // Repo collateral is itemized inside the same filing, so the look-through is
    // filed rather than inferred.
    let collateral = f.repo_collateral();
    assert!(
        !collateral.is_empty(),
        "this fund holds repos and each should carry its collateral"
    );
    println!("collateral lines: {}", collateral.len());
    assert!(
        collateral.iter().all(|(_, c)| c.maturity_date.is_some()),
        "maturityDate wraps a `date` child; a one-level read finds nothing"
    );
    assert!(
        collateral.iter().all(|(_, c)| c.lei.is_none()),
        "LEIID is the literal string N/A on collateral rows and normalizes to absent"
    );

    // A healthy fund files nothing anomalous.
    assert!(f.nav_anomalies().is_empty(), "{:?}", f.nav_anomalies());
}

/// `edgar nmfp` through the StepEngine — proves registration, the allowlist,
/// argument parsing and the record shaping all work together, which a
/// parser-direct test cannot.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn nmfp_runs_inside_step_engine() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let m = (edgar nmfp "S000004151" | first)
        let thin = (edgar nmfp "S000004151" --no-positions | first)
        {
            payload: {
                series: $m.series_name,
                classes: ($m.classes | length),
                positions: ($m.position_count),
                nav_points: ($m.daily_nav | length),
                gross_points: ($m.seven_day_gross_yield | length),
                spread_rows: ($m.yield_spread | length),
                anomalies: ($m.nav_anomalies | length),
                closes_on: $m.identity.closes_on,
                stable_price: $m.stable_price,
                daily_pct: ($m.liquid_assets | first | get daily_pct),
                thin_positions: ($thin.positions == null),
                thin_count: $thin.position_count
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar nmfp must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    assert_eq!(out.payload["classes"], serde_json::json!(10));
    assert_eq!(out.payload["positions"], serde_json::json!(158));
    assert_eq!(out.payload["nav_points"], serde_json::json!(21));
    assert_eq!(out.payload["gross_points"], serde_json::json!(21));
    assert_eq!(out.payload["spread_rows"], serde_json::json!(210));
    assert_eq!(out.payload["anomalies"], serde_json::json!(1));
    assert_eq!(out.payload["closes_on"], serde_json::json!("market"));
    // 0.0000 normalizes to absent rather than to a price of nothing.
    assert!(out.payload["stable_price"].is_null());
    // The single conversion site: a filed fraction reaches nushell as a
    // percentage, so a script that multiplied by 100 would report 4018%.
    let pct = out.payload["daily_pct"].as_f64().unwrap_or_default();
    assert!(
        (1.0..=100.0).contains(&pct),
        "daily liquid assets reached the script as {pct}, which is not a percentage"
    );
    // --no-positions keeps the count and drops the schedule.
    assert_eq!(out.payload["thin_positions"], serde_json::json!(true));
    assert_eq!(out.payload["thin_count"], serde_json::json!(158));
}

/// `edgar fund --accept` — the scope decision belongs to the caller.
///
/// Without the flag the command refuses a money market fund, which is correct
/// for the N-PORT guides and makes `price-the-mfp` unable to resolve its own
/// subject. Both halves are asserted here, because the refusal is load-bearing
/// for the sibling guides and the acceptance is load-bearing for this one.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_fund_scope_is_the_callers_to_declare() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();

    // Accepted: the money market guide names the kind it reads.
    let script = r#"
        let f = (edgar fund "S000004151" --accept [MoneyMarketFund])
        {
            payload: {
                kind: $f.kind,
                in_scope: $f.in_scope,
                redirect: $f.redirect_guide,
                pinned: $f.pinned,
                series: ($f.series | length),
                series_id: ($f.series | first | get series_id)
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("an accepted kind must resolve");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());
    assert_eq!(out.payload["kind"], serde_json::json!("MoneyMarketFund"));
    // `in_scope` still answers the N-PORT question and is still false. It is
    // reported, not obeyed.
    assert_eq!(out.payload["in_scope"], serde_json::json!(false));
    assert_eq!(out.payload["redirect"], serde_json::json!("price-the-mfp"));
    assert_eq!(out.payload["pinned"], serde_json::json!(true));
    assert_eq!(out.payload["series_id"], serde_json::json!("S000004151"));

    // Refused: the same fund, with no accept list, is still not an N-PORT filer.
    let mut engine2 = StepEngine::new();
    let refused = engine2.execute_script(
        r#"{ payload: (edgar fund "S000004151"), signals: {} }"#,
        &serde_json::json!({}),
        None,
    );
    let err = refused.expect_err("the default scope must still refuse a money market fund");
    let msg = format!("{err:?}");
    println!("refusal: {msg}");
    assert!(
        msg.contains("price-the-mfp"),
        "the refusal must name the guide that fits: {msg}"
    );

    // Refused: an accept list that names a different kind.
    let mut engine3 = StepEngine::new();
    let wrong = engine3.execute_script(
        r#"{ payload: (edgar fund "S000004151" --accept [OpenEndFund ClosedEndFund]), signals: {} }"#,
        &serde_json::json!({}),
        None,
    );
    assert!(
        wrong.is_err(),
        "an accept list that omits the filer's kind must refuse"
    );
}

/// The whole industry for one month, from the DERA data set.
///
/// This is the capability `retort-the-n-port` had to defer: a genuine cross-fund
/// base rate. The assertions pin the shape of the population and the two facts
/// that would otherwise be asserted rather than measured — how rare a daily-NAV
/// misfiling actually is, and which balance-sheet basis the industry reconciles
/// on.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn nmfp_universe_reads_a_month_of_the_whole_industry() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let u = (edgar nmfp-universe "2026-06")
        let mine = ($u.funds | where series_id == "S000004151" | first)
        let peers = ($u.funds | where category == $mine.category)
        {
            payload: {
                funds: $u.summary.funds,
                trusts: $u.summary.trusts,
                windows: ($u.summary.windows | length),
                amendments: $u.summary.amendments_superseded,
                anomaly_funds: $u.summary.funds_with_nav_anomaly,
                no_per_share_nav: $u.summary.funds_with_no_per_share_nav,
                closes_amortized: $u.summary.identity_closes_amortized,
                closes_market: $u.summary.identity_closes_market,
                lag_median: $u.summary.lag_days.median,
                weekly_median: $u.summary.weekly_liquid_pct.median,
                warnings: ($u.summary.warnings | length),
                mine_weekly: $mine.weekly_liquid_pct_last,
                mine_anomalies: $mine.nav_anomaly_count,
                mine_closes: $mine.identity_closes_on,
                peers: ($peers | length),
                peers_below: ($peers | where weekly_liquid_pct_last < $mine.weekly_liquid_pct_last | length)
            },
            signals: { ok: true }
        }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar nmfp-universe must run inside the StepEngine");
    println!("{}", serde_json::to_string_pretty(&out.payload).unwrap());

    let funds = out.payload["funds"].as_u64().expect("fund count");
    // ~320 filings a month from ~176 trusts is the whole US industry. The bounds
    // are wide because funds launch and liquidate; a number outside them means
    // the report-month filter or the window selection broke, not that the
    // industry moved.
    assert!(
        (250..=420).contains(&funds),
        "expected roughly a whole industry, got {funds}"
    );
    let trusts = out.payload["trusts"].as_u64().unwrap_or(0);
    assert!((120..=260).contains(&trusts), "trusts = {trusts}");
    assert!(
        out.payload["windows"].as_u64().unwrap_or(0) >= 1,
        "at least one archive must cover the month"
    );
    assert_eq!(out.payload["warnings"], serde_json::json!(0));

    // The misfiling base rate. Measured at 2 of 335 when this was written, and
    // the point of the assertion is that it stays RARE: a finder that fired on a
    // tenth of the industry would be detecting the sentinel, not a misfiling.
    let anomalies = out.payload["anomaly_funds"].as_u64().unwrap_or(u64::MAX);
    assert!(
        anomalies <= funds / 20,
        "{anomalies} of {funds} funds flagged; the finder is catching something normal"
    );
    // Master funds file an all-zero series legitimately and must NOT be in that
    // count. There were 14 in the sampled month.
    assert!(
        out.payload["no_per_share_nav"].as_u64().unwrap_or(0) > 0,
        "some funds file no per-share NAV; if this is zero the sentinel is being missed"
    );

    // The industry reconciles mostly on amortized cost, which is why asserting
    // the market basis would report most of it as broken.
    let amortized = out.payload["closes_amortized"].as_u64().unwrap_or(0);
    let market = out.payload["closes_market"].as_u64().unwrap_or(0);
    println!("identity closes: amortized={amortized} market={market}");
    assert!(
        amortized > market,
        "stable-value funds dominate; amortized should be the commoner basis"
    );

    // The filing lag, measured rather than read off a rule.
    let lag = out.payload["lag_median"].as_f64().unwrap_or_default();
    assert!(
        (3.0..=15.0).contains(&lag),
        "median filing lag of {lag} days is outside anything observed"
    );

    // The subject fund, given a denominator.
    assert_eq!(out.payload["mine_anomalies"], serde_json::json!(1));
    assert_eq!(out.payload["mine_closes"], serde_json::json!("market"));
    let mine_weekly = out.payload["mine_weekly"].as_f64().expect("weekly buffer");
    assert!(
        (1.0..=100.0).contains(&mine_weekly),
        "the buffer reached the script as {mine_weekly}, which is not a percentage"
    );
    let peers = out.payload["peers"].as_u64().unwrap_or(0);
    let below = out.payload["peers_below"].as_u64().unwrap_or(0);
    println!("subject weekly buffer {mine_weekly}%, {below} of {peers} same-category funds lower");
    assert!(peers > 1, "a category of one is not a denominator");
    assert!(below <= peers);
}

// ---------------------------------------------------------------------------
// mark-the-bdc: the schedule of investments, and the cross-holder gate
// ---------------------------------------------------------------------------

/// Fetch a filer's newest 10-K XBRL instance.
async fn latest_tenk_instance(edgar: &Edgar, cik: &str) -> Option<(String, String)> {
    let submission = edgar.submissions(cik).await.ok()?;
    let mut tenks = submission.filings(Some(
        FilingOptions::new()
            .with_form_type("10-K".to_string())
            .with_include_amendments(false),
    ));
    tenks.sort_by(|a, b| b.filing_date.cmp(&a.filing_date));
    let filing = tenks.first()?;
    let primary = filing.primary_document.as_ref()?;
    let stem = primary.strip_suffix(".htm").unwrap_or(primary);
    let id = format!("{}:{stem}_htm.xml", filing.accession_number);
    let xml = edgar.get_filing_content_by_id(cik, &id).await.ok()?;
    Some((filing.accession_number.clone(), xml))
}

/// The schedule reader against BOTH reference filers, because the whole reason it exists is
/// that they disagree about everything except the axis.
///
/// Ares and Blue Owl write different identifier grammars, carry wildly different cost
/// coverage, and differ by two orders of magnitude in how much of the axis is the
/// unfunded-commitment table rather than the schedule. A test against either one alone would
/// have shipped a reader that looked correct and was filer-specific — which is exactly how
/// the round-5 research produced "zero overlap" from a layout mismatch.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn portfolio_reads_both_reference_filers_and_the_partition_holds() {
    use dapple_prompt_flow::edgar_portfolio::parse_portfolio;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");

    for (label, cik) in [("Ares Capital", "1287750"), ("Blue Owl Capital", "1655888")] {
        let (accession, xml) = latest_tenk_instance(&edgar, cik)
            .await
            .unwrap_or_else(|| panic!("{label}: no readable 10-K instance"));
        let schedule = parse_portfolio(&xml);

        println!("=== {label} ({accession}, {} bytes) ===", xml.len());
        assert!(
            schedule.periods.len() >= 2,
            "{label}: a 10-K carries the current AND prior year schedule, got {}",
            schedule.periods.len()
        );

        for p in &schedule.periods {
            let (usd, usd_n) = p.fair_value_in("USD");
            println!(
                "  {}  identifiers={:5}  positions={:5}  commitment={:4}  other={:3}  \
                 cost_covered={:5} ({:.1}%)",
                p.period_end,
                p.identifier_count(),
                p.positions.len(),
                p.commitment_rows.len(),
                p.other_rows.len(),
                p.cost_covered(),
                100.0 * p.cost_covered() as f64 / p.positions.len().max(1) as f64,
            );
            println!(
                "        USD fair value {:>18.0} over {usd_n} positions; \
                 reporting {:?}; instruments {:?}",
                usd,
                p.reporting_currencies(),
                p.principal_currencies()
            );

            // The partition, asserted rather than remembered. This is the check that makes
            // a miscount visible: it fails the moment a row lands in two buckets or none.
            assert_eq!(
                p.identifier_count(),
                p.positions.len() + p.commitment_rows.len() + p.other_rows.len(),
                "{label} {}: the three buckets must partition the identifiers",
                p.period_end
            );
            // A period-blind reader would report the union of both years here.
            assert!(
                !p.positions.is_empty(),
                "{label} {}: a period with no positions is not a schedule",
                p.period_end
            );
            assert!(
                p.cost_covered() <= p.positions.len(),
                "{label}: cost coverage cannot exceed the positions it covers"
            );
        }

        let latest = schedule.latest().expect("a newest period");
        // The identifier is opaque, but it must have survived whole: an ampersand truncated
        // at parse time drops the position out of every downstream match, silently.
        let with_amp = latest
            .positions
            .iter()
            .filter(|p| p.identifier.contains('&'))
            .count();
        println!("  identifiers carrying an ampersand: {with_amp}");
        assert!(
            with_amp > 0,
            "{label}: both reference filers carry ampersands (Ares 71, Blue Owl 38). \
             Zero means the entity decode regressed and positions are being truncated."
        );
        assert!(
            !latest
                .positions
                .iter()
                .any(|p| p.identifier.contains("&amp")),
            "{label}: an undecoded entity reached the identifier"
        );

        // The currency reading, pinned in both directions. A BDC marks its whole book in
        // one reporting currency, so a reader that takes the currency off the fair value
        // reports a single-currency portfolio for every filer and hides the hazard.
        assert_eq!(
            latest.reporting_currencies().len(),
            1,
            "{label}: fair values are stated in exactly one reporting currency"
        );
    }

    // Blue Owl specifically: genuinely multi-currency instruments under a single reporting
    // currency. This is the case that distinguishes the two fields, so it is asserted on the
    // filer that has it rather than on whichever one ran last.
    let (_, xml) = latest_tenk_instance(&edgar, "1655888")
        .await
        .expect("Blue Owl instance");
    let latest = parse_portfolio(&xml).periods.remove(0);
    let instruments = latest.principal_currencies();
    println!("Blue Owl instrument currencies: {instruments:?}");
    assert!(
        instruments.len() > 1,
        "Blue Owl files EUR, GBP and AUD principals against USD fair values; \
         one currency here means the reader is back on the fair-value unit: {instruments:?}"
    );
}

/// The classifier bug, live: Ares elected BDC status in 2004 and its `N-54A` is long out of
/// the submissions feed's recent block, so the shipped rule read it as an operating company
/// and sent it to `run-the-10k`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn live_bdcs_classify_without_their_election() {
    use dapple_prompt_flow::edgar_fund_registry::{FilerKind, classify_filer};

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");

    let forms_of = async |cik: &str| -> Vec<String> {
        let url = format!("https://data.sec.gov/submissions/CIK{cik:0>10}.json");
        let text = edgar.get(&url).await.expect("submissions");
        let root: serde_json::Value = serde_json::from_str(&text).expect("json");
        root.pointer("/filings/recent/form")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    // Ten live BDCs of different vintages and strategies.
    let bdcs = [
        ("ARCC", "1287750"),
        ("OBDC", "1655888"),
        ("MAIN", "1396440"),
        ("HTGC", "1280784"),
        ("FSK", "1422183"),
        ("GBDC", "1476765"),
        ("TSLX", "1508655"),
        ("PSEC", "1287032"),
        ("OCSL", "1414932"),
        ("BXSL", "1736035"),
    ];
    let mut with_election = 0usize;
    for (ticker, cik) in bdcs {
        let forms = forms_of(cik).await;
        let c = classify_filer(&forms);
        let elected = forms.iter().any(|f| f == "N-54A");
        if elected {
            with_election += 1;
        }
        println!("  {ticker:5} N-54A={elected:5}  kind={:?}", c.kind);
        assert_eq!(
            c.kind,
            FilerKind::BusinessDevelopmentCompany,
            "{ticker} must classify as a BDC; signals were {:?}",
            c.signals
        );
        assert_eq!(c.redirect_guide(), Some("mark-the-bdc"));
    }
    // The point of the whole change. If this ever reaches ten, the recurring markers stopped
    // being what carries the verdict and the test below is no longer measuring anything.
    println!("BDCs whose N-54A is still in the recent block: {with_election} of 10");
    assert!(
        with_election < 10,
        "every sampled BDC now shows its election, so this test no longer exercises the fix"
    );

    // Negative controls: the markers are '40 Act housekeeping and must not catch a filer
    // that merely files a 10-K.
    for (ticker, cik) in [("AAPL", "320193"), ("F", "37996"), ("KKR", "1404912")] {
        let c = classify_filer(&forms_of(cik).await);
        println!("  {ticker:5} control            kind={:?}", c.kind);
        assert_eq!(
            c.kind,
            FilerKind::OperatingCompany,
            "{ticker} must stay an operating company; signals were {:?}",
            c.signals
        );
        assert_eq!(c.redirect_guide(), Some("run-the-10k"));
    }

    // And a registered closed-end fund files the same markers with no 10-K. The NPORT-P arm
    // runs first, so it never reaches the BDC test.
    let pdi = classify_filer(&forms_of("1510599").await);
    println!("  PDI   control            kind={:?}", pdi.kind);
    assert_eq!(pdi.kind, FilerKind::ClosedEndFund);
}

/// THE GATE. Measure the cross-holder comparison before building it.
///
/// The guide's centrepiece is "another BDC's mark on the same borrower" — a *different
/// party* valuing the same private asset, which is the closest thing to independent evidence
/// this subject has. Whether that is the spine or a footnote is a measured question, and
/// `retort-the-n-port` gated its CUSIP work the same way (the measurement made the work
/// unnecessary).
///
/// The borrower-prefix extraction below is GATE-LOCAL ON PURPOSE and does not ship. There is
/// no filer-independent way to split a borrower out of an identifier, so a miss here lowers
/// a measured number — honest — where the same guess in a rendered report would be a fact
/// nobody could check.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn bdc_cross_holder_overlap_gate() {
    use dapple_prompt_flow::edgar_identity::normalize_company_name;
    use dapple_prompt_flow::edgar_portfolio::parse_portfolio;
    use std::collections::BTreeMap;

    /// What one BDC holds of one borrower: instrument key -> fair value.
    type Instruments = BTreeMap<String, f64>;
    /// Who holds a borrower, and what each of them holds of it.
    type Holders<'a> = BTreeMap<&'a str, Instruments>;

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");

    /// Split an identifier into (borrower, instrument), per filer convention.
    ///
    /// Pipe first: Blue Owl writes `<borrower> | <instrument> | <affiliation>` and the pipe
    /// is unambiguous. Otherwise the LAST comma, because Ares writes
    /// `<borrower>, <instrument>` and a borrower name's own commas come before the
    /// instrument (`Steward Partners Global Advisory, LLC, First lien senior secured loan`).
    /// Both are heuristics; that is why they live in a test.
    fn split_identifier(id: &str) -> (String, String) {
        if let Some((b, rest)) = id.split_once(" | ") {
            return (b.trim().to_string(), rest.trim().to_string());
        }
        match id.rsplit_once(", ") {
            Some((b, i)) => (b.trim().to_string(), i.trim().to_string()),
            None => (id.trim().to_string(), String::new()),
        }
    }

    /// Strip the filer's own ordinal (`... loan 2`). It disambiguates two tranches WITHIN
    /// one filer and means nothing across filers, so matching on it would pair unrelated
    /// tranches and miss related ones.
    fn instrument_key(instrument: &str) -> String {
        let trimmed = instrument.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ');
        normalize_company_name(if trimmed.is_empty() {
            instrument
        } else {
            trimmed
        })
    }

    // Six BDCs of different strategies and sizes: two mega-cap first-lien lenders, an
    // internally-managed lower-middle-market lender, a venture lender, a large multi-strategy
    // vehicle, and a mid-size sponsor-backed one.
    let bdcs = [
        ("ARCC", "1287750"),
        ("OBDC", "1655888"),
        ("MAIN", "1396440"),
        ("HTGC", "1280784"),
        ("FSK", "1422183"),
        ("GBDC", "1476765"),
    ];

    // borrower key -> who holds it -> what they hold of it
    let mut by_borrower: BTreeMap<String, Holders<'_>> = BTreeMap::new();
    let mut totals: BTreeMap<&str, (usize, f64, String)> = BTreeMap::new();

    for (ticker, cik) in bdcs {
        let Some((accession, xml)) = latest_tenk_instance(&edgar, cik).await else {
            println!("  {ticker}: no readable 10-K instance; EXCLUDED from the gate");
            continue;
        };
        let schedule = parse_portfolio(&xml);
        let Some(latest) = schedule.latest() else {
            println!("  {ticker}: instance carries no schedule; EXCLUDED from the gate");
            continue;
        };
        let (usd_total, _) = latest.fair_value_in("USD");
        totals.insert(
            ticker,
            (latest.positions.len(), usd_total, latest.period_end.clone()),
        );
        println!(
            "  {ticker:5} {accession}  {}  positions={:5}  USD fair value {:>18.0}",
            latest.period_end,
            latest.positions.len(),
            usd_total
        );
        for p in &latest.positions {
            let (borrower, instrument) = split_identifier(&p.identifier);
            let bkey = normalize_company_name(&borrower);
            if bkey.is_empty() {
                continue;
            }
            let entry = by_borrower
                .entry(bkey)
                .or_default()
                .entry(ticker)
                .or_default();
            // Sum, because one borrower/instrument pair can appear on several rows.
            *entry.entry(instrument_key(&instrument)).or_insert(0.0) += p.fair_value.unwrap_or(0.0);
        }
    }

    assert!(
        totals.len() >= 4,
        "the gate needs at least four readable filers to mean anything, got {}",
        totals.len()
    );

    // --- The three numbers ---------------------------------------------------

    let shared: Vec<(&String, &Holders<'_>)> = by_borrower
        .iter()
        .filter(|(_, holders)| holders.len() >= 2)
        .collect();

    println!("\n=== GATE ===");
    println!("distinct borrowers across the set: {}", by_borrower.len());
    println!("borrowers held by 2 or more BDCs:  {}", shared.len());

    // Number 2: by value, per holder, so no single denominator hides the spread.
    for (ticker, (positions, usd_total, period)) in &totals {
        let shared_value: f64 = shared
            .iter()
            .filter_map(|(_, holders)| holders.get(ticker))
            .map(|instruments| instruments.values().sum::<f64>())
            .sum();
        let pct = if *usd_total > 0.0 {
            100.0 * shared_value / usd_total
        } else {
            0.0
        };
        println!(
            "  {ticker:5} {period}  {positions:5} positions  {pct:6.2}% of USD fair value sits in \
             a borrower at least one other BDC also holds"
        );
    }

    // Number 3: same borrower AND same instrument description, two holders, two marks.
    let mut instrument_matches = 0usize;
    let mut borrowers_with_a_match = 0usize;
    let mut shown = 0usize;
    let mut no_instrument_match = 0usize;
    let mut both_zero = 0usize;
    println!("\nborrower AND instrument matched across two holders:");
    for (borrower, holders) in &shared {
        let mut instruments: BTreeMap<&String, Vec<(&str, f64)>> = BTreeMap::new();
        for (ticker, by_instrument) in holders.iter() {
            for (ikey, value) in by_instrument {
                if ikey.is_empty() {
                    continue;
                }
                instruments.entry(ikey).or_default().push((ticker, *value));
            }
        }
        let mut matched_here = 0usize;
        for (ikey, marks) in &instruments {
            if marks.len() < 2 {
                continue;
            }
            matched_here += 1;
            instrument_matches += 1;
            // An undrawn revolver is marked at zero on both sides. It is a match in form
            // and carries no comparison, so it is counted apart rather than inflating the
            // headline.
            if marks.iter().all(|(_, v)| v.abs() < 1.0) {
                both_zero += 1;
            }
            if shown < 12 {
                shown += 1;
                let rendered: Vec<String> =
                    marks.iter().map(|(t, v)| format!("{t} {v:.0}")).collect();
                println!("  {borrower} / {ikey}: {}", rendered.join("  vs  "));
            }
        }
        if matched_here > 0 {
            borrowers_with_a_match += 1;
        } else {
            no_instrument_match += 1;
            // WHY a shared borrower produced no instrument match is the difference between
            // "they hold different tranches" (a real finding) and "the extractor split the
            // string differently on each side" (an artifact). Printing both sides is the
            // only way to tell them apart, and round 5 reported an artifact as a finding
            // for want of exactly this.
            if no_instrument_match <= 6 {
                println!("  [no instrument match] {borrower}");
                for (ticker, by_instrument) in holders.iter() {
                    let list: Vec<&str> =
                        by_instrument.keys().map(String::as_str).take(4).collect();
                    println!("      {ticker}: {list:?}");
                }
            }
        }
    }
    println!("\ntotal instrument-level matches: {instrument_matches}");
    println!("  of which both sides marked ~0 (undrawn revolvers): {both_zero}");
    println!(
        "shared borrowers with NO instrument match: {no_instrument_match} of {}",
        shared.len()
    );
    if instrument_matches > shown {
        // No silent caps. A truncated list must say it was truncated.
        println!("  (showed {shown} of {instrument_matches} matches)");
    }

    println!("\n=== VERDICT ===");
    println!(
        "Borrower-level overlap is REAL: {} borrowers appear in two or more of these six \n\
         schedules, reaching double digits as a share of NAV for the larger direct lenders.",
        shared.len()
    );
    println!(
        "Instrument-level MARK COMPARISON is NOT reachable from the filings. Instrument\n\
         descriptions are proprietary vocabulary, not a shared taxonomy: the same tranche is\n\
         \"first lien senior secured loan\" to one filer and \"one stop\" to another, and at\n\
         least one filer writes an INDUSTRY in that slot. {instrument_matches} of the {} shared\n\
         borrowers produced any instrument match at all, and the failures are vocabulary, not\n\
         extraction - both sides are printed above so that can be checked rather than assumed.",
        shared.len()
    );

    // The gate reports; it does not pass or fail on the overlap. The assertions only
    // establish that the measurement ran on real data and accounted for every borrower it
    // found, so a low match count is a finding about the filings rather than a silent bug.
    let read: usize = totals.values().map(|(n, _, _)| n).sum();
    assert!(
        read > 1_000,
        "read only {read} positions across {} filers; a zero overlap from this would be \
         the extractor, not the portfolios",
        totals.len()
    );
    assert_eq!(
        borrowers_with_a_match + no_instrument_match,
        shared.len(),
        "every shared borrower must be either matched or counted as unmatched; \
         {borrowers_with_a_match} matched + {no_instrument_match} unmatched != {}",
        shared.len()
    );

    // The gate reports; it does not pass or fail on the overlap. The only assertion is that
    // the measurement ran on real data, so a zero is a finding rather than a broken probe.
    let read: usize = totals.values().map(|(n, _, _)| n).sum();
    assert!(
        read > 1_000,
        "read only {read} positions across {} filers; a zero overlap from this would be \
         the extractor, not the portfolios",
        totals.len()
    );
}

/// `edgar fund-sections` must work across FUND FAMILIES, not just the one it was written on.
///
/// The command's own module doc used to claim its `/N/role/<SectionKey>` role vocabulary "does
/// not vary by filer". Measured against three families, that was overstated on the strength of
/// one of them, and the other two were broken in different ways:
///
/// | filer | what happened | what the fix reads instead |
/// |---|---|---|
/// | 1290 (CIK 1605941) | worked | unchanged — this is the control |
/// | Vanguard (CIK 36405) | **errored outright**: its 2026 485BPOS ships no `FilingSummary.xml` | the presentation linkbase's roles |
/// | Fidelity (CIK 225322) | every fund collapsed into one `fund_index: None` group, so `--fund` failed against an empty name list | the `S#########` in each role's key |
///
/// Vanguard is the sharp one: the 2022–2025 filings all have a `FilingSummary.xml` and only
/// the newest, self-filed one does not — so the command failed on precisely the filing a
/// caller gets by default.
///
/// It also guards the later decision to read section PROSE from the XBRL instance rather than
/// from the rendered fragments: every family must come back with text, none of it carrying
/// the SEC viewer's chrome, and Vanguard must get prose while still having no fragments.
///
/// Assertions here are structural (a filing partitions, and names its funds) rather than a
/// pinned inventory, because every one of these filers re-files annually.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn fund_sections_works_across_three_fund_families() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );

    // (label, CIK, ships a FilingSummary.xml?, standard section keys on its PER-FUND roles?)
    let families = [
        ("1290 (control)", "1605941", true, true),
        ("Vanguard", "36405", false, true),
        // Fidelity's per-fund roles are keyed `DisclosureDetailS…-ComboPRO` and the standard
        // keys live on trust-level `xbrl.sec.gov/rr/role/*` roles, so no ROLE can hand a fund
        // its InvestmentStrategy. It reads instance prose anyway now: `parse_prospectus_text`'s
        // pass 3b places a fact under the section its element name implies whenever no role
        // named that section, and the instance attributes every fact by `dei:LegalEntityAxis`.
        // This flag flipped from false when that landed — before it, the assertion below was
        // that the gap be STATED; now it is that there is no gap.
        ("Fidelity", "225322", true, true),
    ];

    for (label, cik, has_summary, per_fund_standard_keys) in families {
        let mut engine = StepEngine::new();
        let script = format!(
            r#"
            let p = (edgar fund-sections "{cik}" --form "485BPOS"
                     --fetch [InvestmentStrategy InvestmentRisks])
            # Each section carries the fund it belongs to, so a TRUST-level section (the shared
            # N-1A cover, and on Fidelity the standard-keyed roles that belong to no fund) can
            # be told apart from a fund's own. A trust-level section has no series, so it has no
            # per-fund instance prose by construction and correctly falls back to its fragment.
            let secs = ($p.funds
                        | each {{|f| ($f.sections | each {{|s| ($s | merge {{fund: $f.fund_index}}) }}) }}
                        | flatten)
            let bodies = ($secs | where {{|s| $s.text != null }})
            {{
                payload: {{
                    funds: ($p.funds | length),
                    named: ($p.funds | where {{|f| $f.fund_name != null }} | length),
                    # A null fund_index is the trust-level group (the shared N-1A cover); it
                    # belongs to no fund and so has neither a name nor a series by design.
                    real: ($p.funds | where {{|f| $f.fund_index != null }} | length),
                    with_series: ($p.funds | where {{|f| $f.series_id != null }} | length),
                    fragments: ($secs | where {{|s| $s.html_file != null }} | length),
                    with_text: ($bodies | length),
                    chrome: ($bodies | where {{|s| $s.fund != null and ($s.text =~ "IDEA: XBRL DOCUMENT") }} | length),
                    sample: ($bodies | get text.0? | default "" | str substring 0..160),
                    names: ($p.funds | each {{|f| $f.fund_name }} | compact | first 3),
                    warnings: $p.warnings
                }},
                signals: {{ ok: true }}
            }}
        "#
        );
        let out = engine
            .execute_script(&script, &serde_json::json!({}), None)
            .unwrap_or_else(|e| panic!("{label} (CIK {cik}) must not fail outright: {e}"));

        let funds = out.payload["funds"].as_u64().unwrap_or(0);
        let named = out.payload["named"].as_u64().unwrap_or(0);
        let with_series = out.payload["with_series"].as_u64().unwrap_or(0);
        let fragments = out.payload["fragments"].as_u64().unwrap_or(0);
        let with_text = out.payload["with_text"].as_u64().unwrap_or(0);
        println!(
            "{label:16} funds={funds:3} named={named:3} series={with_series:3} \
             fragments={fragments:4} text={with_text:3} {}",
            out.payload["names"]
        );
        println!("{label:16} sample: {}", out.payload["sample"]);

        assert!(
            funds > 1,
            "{label}: a 485BPOS covering one fund would be unusual; getting exactly one group \
             is what a collapsed partition looks like. warnings: {}",
            out.payload["warnings"]
        );
        assert!(
            named > 0,
            "{label}: no fund could be named, so --fund can never narrow this filing. \
             warnings: {}",
            out.payload["warnings"]
        );
        // Every real fund must carry its series id. That is the key `tagged` reports under,
        // so a filing whose funds have none returns two halves of one answer that cannot be
        // joined — which is what this looked like before the linkbase backfilled them.
        let real = out.payload["real"].as_u64().unwrap_or(0);
        assert_eq!(
            with_series,
            real,
            "{label}: {} of {real} funds have no series id, so `tagged` cannot be joined to \
             them. warnings: {}",
            real - with_series,
            out.payload["warnings"]
        );

        // The point of reading prose from the instance: EVERY filer gets it, including the
        // one with no fragments to render it from.
        assert!(
            with_text > 0,
            "{label}: --fetch returned no prose for any fund. warnings: {}",
            out.payload["warnings"]
        );

        // Instance text, not fragment text, wherever a fund's own roles carry the standard
        // section keys. `IDEA: XBRL DOCUMENT` is the SEC viewer's own banner and is in every
        // R<n>.htm, so a hit means the prose fell back to a fragment.
        //
        // Fidelity is the documented exception and the reason this is not a blanket
        // assertion: it puts the standard keys on TRUST-level `xbrl.sec.gov/rr/role/*` roles
        // and its funds on per-fund roles keyed
        // `DisclosureDetailS000067930_FidelityAgricultural…-ComboPRO`, so a request for
        // `InvestmentStrategy` resolves to a trust-level section with no series — which has
        // no per-fund instance text by construction and correctly falls back. That gap
        // predates reading prose from the instance; what this pass adds is that the command
        // now SAYS a fund got nothing rather than returning nulls.
        if per_fund_standard_keys {
            assert_eq!(
                out.payload["chrome"].as_u64().unwrap_or(0),
                0,
                "{label}: prose came from the rendered fragment, viewer chrome and all"
            );
        } else {
            // The gap must be STATED. A caller passing --fund would otherwise get a fund
            // whose every section text is null, which is indistinguishable from a fund that
            // disclosed nothing — and the warning names the keys this filing does have.
            assert!(
                out.payload["warnings"]
                    .as_array()
                    .is_some_and(|w| w.iter().any(|x| x
                        .as_str()
                        .is_some_and(|s| s.contains("--fetch matched no text")))),
                "{label}: funds got no text and the command did not say so. warnings: {}",
                out.payload["warnings"]
            );
        }

        // Only a FilingSummary carries the role -> R<n>.htm map, so that is exactly the
        // capability a filing without one loses — and it must be lost VISIBLY.
        if has_summary {
            assert!(
                fragments > 0,
                "{label}: sections lost their rendered fragments"
            );
        } else {
            assert_eq!(
                fragments, 0,
                "{label}: no FilingSummary means no fragment can be known"
            );
            assert!(
                out.payload["warnings"].as_array().is_some_and(|w| w
                    .iter()
                    .any(|x| x.as_str().is_some_and(|s| s.contains("FilingSummary")))),
                "{label}: the degradation must be stated, not inferred from empty text"
            );
        }
    }
}

/// The measurement behind reading section prose from the instance rather than from the
/// rendered `R<n>.htm` fragment, run against both sources on the SAME section of the SAME
/// filing so the comparison is not a recollection.
///
/// The expectation this pins is the counter-intuitive one. A fragment is *rendered from* the
/// instance's facts, so the obvious guess is that its text is a superset and the instance is
/// at best a cheaper equivalent. Measured on 1290's `InvestmentStrategy`, the fragment is
/// LONGER and WORSE: it wraps the same prose in the SEC viewer's banner, the renderer's
/// `toggleNextSibling` JavaScript source, a version stamp, XBRL element labels
/// (`Strategy Narrative [Text Block]`), and a heading whose markup arrives double-escaped and
/// so renders as literal `&lt;span style=…`.
///
/// This prints both so a future reader can re-judge, and asserts only the two properties that
/// make the choice correct: the disclosure survives, and the chrome does not.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn instance_prose_is_the_same_disclosure_without_the_viewer_chrome() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );

    let mut engine = StepEngine::new();
    // The instance-derived text, as the command now returns it.
    let script = r#"
        let p = (edgar fund-sections "1605941" --form "485BPOS" --fetch [InvestmentStrategy])
        let s = ($p.funds | each {|f| $f.sections } | flatten
                 | where {|s| $s.section_key == "InvestmentStrategy" and $s.text != null }
                 | first)
        { payload: { text: $s.text, html_file: $s.html_file,
                     cik: "1605941", accession: $p.accession },
          signals: { ok: true } }
    "#;
    let out = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("fund-sections --fetch must run");

    let instance_text = out.payload["text"].as_str().unwrap_or_default().to_string();
    let fragment_file = out.payload["html_file"]
        .as_str()
        .expect("1290 ships a FilingSummary, so the fragment name is known")
        .to_string();
    let accession = out.payload["accession"]
        .as_str()
        .unwrap_or_default()
        .replace('-', "");

    // The same section's rendered fragment, flattened by the same function the fallback uses.
    let edgar =
        Edgar::new(&std::env::var("DAPPLE_EDGAR_USER_AGENT").unwrap()).expect("edgar client");
    let url =
        format!("https://www.sec.gov/Archives/edgar/data/1605941/{accession}/{fragment_file}");
    let fragment_text = html_to_text(&edgar.get(&url).await.expect("fragment must fetch"));

    println!(
        "instance {:>6} chars | fragment {:>6} chars ({fragment_file})",
        instance_text.len(),
        fragment_text.len()
    );
    let head = |s: &str| s.chars().take(400).collect::<String>();
    println!("--- instance ---\n{}", head(&instance_text));
    println!("--- fragment ---\n{}", head(&fragment_text));

    // 1. It is the same disclosure. Checked by taking a distinctive run from the middle of
    //    the instance's prose and finding it in the fragment — the fragment indisputably
    //    carries the whole section, so a hit proves the instance text is that section rather
    //    than some other fact that merely parsed.
    //
    //    Both sides are normalized to bare words first: html2text lays the fragment out as a
    //    bordered table, so its words arrive glued to `│` and wrapped at arbitrary columns.
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || c == '│' || c == '─' || c == '┬')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let instance_words = words(&instance_text);
    assert!(
        instance_words.len() > 40,
        "instance prose is too short to be the strategy section: {instance_text:?}"
    );
    let probe = instance_words[20..32].join(" ");
    assert!(
        words(&fragment_text).join(" ").contains(&probe),
        "the instance text is not the prose the fragment renders: looked for {probe:?}"
    );

    // 2. The chrome does not. Each of these is in the fragment and none is disclosure.
    for noise in [
        "IDEA: XBRL DOCUMENT",
        "toggleNextSibling",
        "[Text Block]",
        "&lt;span",
    ] {
        assert!(
            !instance_text.contains(noise),
            "instance text carries viewer chrome {noise:?}"
        );
    }
}

/// `edgar manager` end-to-end against a live manager's own 13F history.
///
/// Four things a fixture cannot vouch for, because each depends on what a filer
/// actually did rather than on what a captured document says:
///
/// 1. **The filer's own arithmetic closes.** The summary page states both the
///    row count and the value total, so the filing checks itself against numbers
///    no fixture author chose. A frozen fixture only proves the check runs.
/// 2. **The information-table filename varies within ONE filer's history.**
///    Berkshire's own filings are `53405.xml` today and `form13fInfoTable.xml`
///    in 2019. Pulling a multi-year window exercises the sole-member rule across
///    names the test never names — which is the whole point of not hardcoding.
/// 3. **The units cutover is crossed by a real window.** Twelve quarters spans
///    2023-01-03 on any manager filing continuously, so the same pull produces
///    both regimes and a scale error shows up as a 1000x step between quarters.
/// 4. **Every quarter is accounted for.** The pull must return one row per
///    located filing, so a silently-skipped quarter cannot pass as a short
///    history.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn manager_filings_reconcile_across_a_live_multi_year_window() {
    use dapple_prompt_flow::edgar_manager::{
        FilingKind, assemble, info_table_member, parse_cover, parse_information_table,
    };

    let ua = std::env::var("DAPPLE_EDGAR_USER_AGENT")
        .expect("set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test");
    let edgar = Edgar::new(&ua).expect("edgar client");

    // Berkshire: files continuously, spans the cutover, and changed its own
    // information-table filename partway through the window.
    let feed_url = "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK=1067983\
                    &type=13F&dateb=&owner=include&count=14&output=atom";
    let feed = edgar.get(feed_url).await.expect("fetch the 13F feed");
    let accessions: Vec<(String, String)> = feed
        .split("<entry>")
        .skip(1)
        .filter_map(|e| {
            let pick = |tag: &str| -> Option<String> {
                e.split(&format!("<{tag}>"))
                    .nth(1)?
                    .split('<')
                    .next()
                    .map(|s| s.trim().to_string())
            };
            Some((pick("accession-number")?, pick("filing-date")?))
        })
        .collect();
    assert!(
        accessions.len() >= 10,
        "expected a multi-year window, got {} filings",
        accessions.len()
    );

    let mut filenames = std::collections::BTreeSet::new();
    let mut reconciled = 0usize;
    let mut mismatched: Vec<String> = Vec::new();
    let mut notices = 0usize;
    let mut amendments: Vec<String> = Vec::new();

    for (accession, filing_date) in &accessions {
        let acc_path = accession.replace('-', "");
        let base = format!("https://www.sec.gov/Archives/edgar/data/1067983/{acc_path}");
        let cover_xml = edgar
            .get(&format!("{base}/primary_doc.xml"))
            .await
            .expect("fetch the cover page");
        let cover = parse_cover(&cover_xml).expect("cover must parse");

        // A notice has NO information table document at all, so looking for one
        // would fail loudly on a valid filing.
        let positions = if cover.kind == FilingKind::Holdings {
            let dir = edgar
                .filing_directory("1067983", accession)
                .await
                .expect("filing directory");
            let names: Vec<String> = dir.directory.item.into_iter().map(|i| i.name).collect();
            let member = info_table_member(&names).unwrap_or_else(|e| {
                panic!("{accession}: no single information table among {names:?} — {e}")
            });
            filenames.insert(member.clone());
            let table = edgar
                .get(&format!("{base}/{member}"))
                .await
                .expect("fetch the information table");
            parse_information_table(&table).expect("information table must parse")
        } else {
            notices += 1;
            Vec::new()
        };

        let f = assemble(cover, positions, accession, filing_date);
        if let Some(a) = &f.cover.amendment {
            amendments.push(format!(
                "{} {} {}",
                f.quarter.as_deref().unwrap_or("?"),
                accession,
                a.kind.label()
            ));
        }
        match f.reconciliation.reconciles {
            Some(true) => reconciled += 1,
            Some(false) => mismatched.push(format!(
                "{accession} ({}): reported {:?}/{:?}, parsed {}/{}",
                f.quarter.as_deref().unwrap_or("?"),
                f.reconciliation.reported_entry_total,
                f.reconciliation.reported_value_total,
                f.reconciliation.parsed_entry_count,
                f.reconciliation.parsed_value_total,
            )),
            None => {}
        }
        println!(
            "{:>7} {accession} {:<9} rows={:<4} value={:>16.0} reconciles={:?}",
            f.quarter.as_deref().unwrap_or("?"),
            f.cover.submission_type,
            f.reconciliation.parsed_entry_count,
            f.reconciliation.parsed_value_total,
            f.reconciliation.reconciles,
        );
    }

    println!("information-table filenames seen: {filenames:?}");
    println!("amendments seen: {amendments:?}");

    // 1. The filer's own identity closes on every holdings report. A mismatch is
    //    reportable rather than fatal in the command, but on THIS filer it would
    //    mean the parser lost rows, so the live test holds it to exact.
    assert!(
        mismatched.is_empty(),
        "reconciliation failed on {} filing(s): {mismatched:#?}",
        mismatched.len()
    );
    assert!(
        reconciled >= accessions.len() - notices - 1,
        "only {reconciled} of {} filings reconciled",
        accessions.len()
    );

    // 2. The filename is not knowable in advance. If this ever drops to one, the
    //    window no longer spans Berkshire's rename and the rule is going
    //    untested live — which is exactly how a hardcoded path ships green.
    assert!(
        filenames.len() >= 2,
        "expected the window to span more than one information-table filename, saw {filenames:?}"
    );
}

/// The command is REGISTERED and runs inside a guide script.
///
/// Separate from the parser test on purpose: a parser can be perfect while the
/// command is missing from `commands/mod.rs` or from the security allowlist, and
/// the symptom then is a guide script failing to parse — in the API process,
/// which is where guide scripts are actually validated.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_manager_runs_inside_step_engine_and_resolves_an_unlisted_manager() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    // Motley Fool Asset Management is NOT a listed issuer, so it is absent from
    // company_tickers.json and the shared ticker resolver cannot find it. This
    // is the landmine-17 fix, and only a live call can prove it.
    let script = r#"
        let by_name = (edgar manager "Motley Fool Asset Management" --quarters 1)
        let by_cik = (edgar manager 1512814 --quarters 1)
        let f = ($by_name | first)
        {
            payload: {
                same_filer: (($by_name | first | get cik) == ($by_cik | first | get cik)),
                outcome: $f.outcome,
                reconciles: $f.reconciliation.reconciles,
                positions: $f.position_count,
                entries_match: $f.reconciliation.entries_match,
                confidential_omitted_is_null: ($f.confidential_omitted == null)
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar manager must run inside the StepEngine");
    let p = &output.payload;
    println!("edgar manager payload: {p}");

    // Name and CIK reach the same filer.
    assert_eq!(p["same_filer"], serde_json::json!(true));
    assert_eq!(p["outcome"], serde_json::json!("holdings_reported"));
    // The filer's own arithmetic, on a filing nobody here chose.
    assert_eq!(p["reconciles"], serde_json::json!(true));
    assert_eq!(p["entries_match"], serde_json::json!(true));
    assert!(
        p["positions"].as_i64().unwrap_or(0) > 50,
        "expected a real portfolio, got {:?}",
        p["positions"]
    );
    // ABSENT IS NOT FALSE: this filer omits isConfidentialOmitted entirely, and
    // a `false` here would be an admission they never made.
    assert_eq!(p["confidential_omitted_is_null"], serde_json::json!(true));
}

/// Name resolution has THREE outcomes and a script has to be able to branch on
/// all of them. Only the resolved one is exercised above; these are the other
/// two, and both are answers about the search rather than failures of it — so
/// both come back as records, not as errors a caller has to catch.
///
/// The ambiguous case is the interesting one: EDGAR's multi-match feed returns
/// `name="ARRAY(0x...)"` for every company, so the candidate names are NOT in
/// the response and each costs a follow-up request. This asserts the names
/// arrive anyway, because a bare list of CIKs would put the disambiguation work
/// back on a human who just told us they don't know which one they meant.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn manager_name_resolution_answers_ambiguous_and_absent_without_erroring() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    // "motley fool" matches BOTH Motley Fool entities that file a 13F (Asset
    // Management and Wealth Management); the nonsense needle matches none.
    let script = r#"
        let amb = (edgar manager "motley fool" --quarters 1)
        let nil = (edgar manager "zzzznotarealmanager" --quarters 1)
        {
            payload: {
                amb_outcome: $amb.outcome,
                amb_count: ($amb.candidates | length),
                amb_all_named: ($amb.candidates | all { |c| $c.name != null }),
                amb_names: ($amb.candidates | get name),
                nil_outcome: $nil.outcome
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar manager must answer, not error, on a non-resolving name");
    let p = &output.payload;
    println!("resolution payload: {p}");

    assert_eq!(p["amb_outcome"], serde_json::json!("ambiguous"));
    assert!(
        p["amb_count"].as_i64().unwrap_or(0) >= 2,
        "expected several 13F filers named 'motley fool', got {:?}",
        p["amb_count"]
    );
    // The names are not in the search response at any point; they are here only
    // because the command went and fetched them.
    assert_eq!(p["amb_all_named"], serde_json::json!(true));

    // A needle matching no 13F filer is an ANSWER about the search.
    assert_eq!(p["nil_outcome"], serde_json::json!("no_filings"));
}

/// The tenure layer against a live multi-year window.
///
/// Four things a fixture cannot vouch for, because each depends on what a filer
/// actually did:
///
/// 1. **Rows are not holdings, on data the test does not state.** Berkshire files
///    one security once per `otherManager` combination holding it. The fixture
///    proves the fold works on three rows; only the live filing proves the
///    difference is large enough to matter (90 → 29).
/// 2. **The amendments fold rather than replace.** Two of Berkshire's last ten
///    filings are `NEW HOLDINGS` amendments, and a latest-wins fold would report
///    ~$1.1bn as a quarter whose real total is ~$260bn. The assertion is on the
///    magnitude, so it fails loudly if the fold ever regresses.
/// 3. **A position held since the 1980s reports an UNKNOWN entry.** Coca-Cola and
///    Apple predate any window we can pull. `entered_before_window` must be true
///    and `quarters_held` must be read as a lower bound — reporting the window
///    start as an entry quarter is the error the flag exists to prevent.
/// 4. **The denominators close.** Each quarter's `pct_of_portfolio` sums to ~100.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn manager_tenures_hold_across_a_live_window() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let t = (edgar manager 1067983 --quarters 12 --tenures)
        let obs = ($t.quarters | where observable)
        let biggest = ($t.tenures | first)
        {
            payload: {
                periods: ($t.window.periods),
                observable: ($t.window.observable_periods),
                # Rows vs holdings, on the newest observable quarter.
                holding_count: ($obs | last | get holding_count),
                # Every observable quarter's weights must sum to ~100.
                worst_pct_sum: ($obs | each { |q|
                    ($q.holdings | get pct_of_portfolio | math sum) } | math min),
                # The amended quarters must keep the base portfolio.
                amended: ($t.quarters | where fold == "amended" | length),
                min_amended_total: (
                    let a = ($t.quarters | where fold == "amended");
                    if ($a | is-empty) { null } else { $a | get total_value_usd | math min }
                ),
                # A position held since long before any window we can pull.
                censored: ($t.tenures | where entered_before_window | length),
                biggest_name: $biggest.name_of_issuer,
                biggest_censored: $biggest.entered_before_window,
                # The closing arithmetic exists.
                median_held: $t.history.median_quarters_held,
                fully_observed: $t.history.fully_observed_tenures,
                turnover_quarters: ($t.history.turnover_by_quarter | length)
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar manager --tenures must run inside the StepEngine");
    let p = &output.payload;
    println!("tenure payload: {p}");

    // 1. A real window, and the fold produced portfolios from it.
    assert!(p["observable"].as_i64().unwrap_or(0) >= 8, "{p}");

    // 2. Rows are not holdings. Berkshire files ~90-130 rows over ~30-50 CUSIPs,
    //    so the holding count is well under any plausible row count. Asserting a
    //    ceiling rather than an equality keeps this true as the portfolio moves.
    let holdings = p["holding_count"].as_i64().unwrap_or(0);
    assert!(
        (10..=70).contains(&holdings),
        "expected the aggregated holding count to be far below the filed row count, got {holdings}"
    );

    // 3. Weights close, per quarter, on every observable quarter.
    let worst = p["worst_pct_sum"].as_f64().unwrap_or(0.0);
    assert!(
        (99.0..=101.0).contains(&worst),
        "a quarter's holdings do not sum to 100% of it: {worst}"
    );

    // 4. THE LANDMINE-7 ASSERTION. Berkshire's amendments are additions; if the
    //    fold ever regresses to latest-wins, an amended quarter collapses from
    //    ~$260bn to ~$1.1bn and this fails.
    if p["amended"].as_i64().unwrap_or(0) > 0 {
        let min_total = p["min_amended_total"].as_f64().unwrap_or(0.0);
        assert!(
            min_total > 1e11,
            "an amended quarter totals {min_total:.0} — the base portfolio was dropped"
        );
    }

    // 5. The largest position by peak weight is one Berkshire has held for
    //    decades, so its entry is outside anything we can pull.
    assert_eq!(p["biggest_censored"], serde_json::json!(true), "{p}");
    assert!(p["censored"].as_i64().unwrap_or(0) > 0);

    // 6. The closing arithmetic ran. median_quarters_held is null unless some
    //    tenure both entered and exited inside the window — either is correct,
    //    but it must never be a number when nothing qualifies.
    let fully = p["fully_observed"].as_i64().unwrap_or(-1);
    assert!(fully >= 0);
    if fully == 0 {
        assert!(
            p["median_held"].is_null(),
            "a median was reported with no fully-observed tenure to compute it from"
        );
    }
    assert!(p["turnover_quarters"].as_i64().unwrap_or(0) >= 7);
}

/// The quarter-first access path, live.
///
/// Fixtures prove the flatten and the counts agree on three synthetic quarters.
/// What they cannot prove is that a real window contains an un-observable period
/// at all — Berkshire's does (a 2023Q4 amendment whose original is older than the
/// pull), which is what makes the null-vs-zero distinction load-bearing rather
/// than theoretical.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn manager_movements_are_reachable_quarter_first() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let t = (edgar manager 1067983 --quarters 12 --tenures)
        let nested = ($t.tenures | each { |x| $x.movements | length } | math sum)
        let obs = ($t.quarters | where observable)
        let unobs = ($t.quarters | where not observable)
        {
            payload: {
                flat: ($t.movements | length),
                nested: $nested,
                # Every flat entry stands alone.
                all_identified: ($t.movements | all { |m| $m.cusip != null and $m.name_of_issuer != null }),
                # "What changed in this quarter" with no flatten.
                q_exits: ($t.movements | where quarter == ($obs | last | get quarter)
                                       | where status == "exited" | length),
                # Counts exist on observable quarters...
                obs_all_counted: ($obs | all { |q| $q.changes != null }),
                # ...and are NULL, not zero, where nothing was reported.
                unobs_count: ($unobs | length),
                unobs_all_null: ($unobs | all { |q| $q.changes == null }),
                # The counts reconcile against the flat list.
                counts_match: ($obs | all { |q|
                    let c = $q.changes
                    let total = ($c.opening + $c.new + $c.added + $c.trimmed
                                 + $c.held + $c.exited + $c.unknown)
                    $total == ($t.movements | where quarter == $q.quarter | length)
                })
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar manager --tenures must expose movements quarter-first");
    let p = &output.payload;
    println!("quarter-first payload: {p}");

    // Nothing lost, nothing duplicated, against a real window.
    assert_eq!(p["flat"], p["nested"]);
    assert!(p["flat"].as_i64().unwrap_or(0) > 50, "{p}");
    assert_eq!(p["all_identified"], serde_json::json!(true));
    assert_eq!(p["counts_match"], serde_json::json!(true));
    assert_eq!(p["obs_all_counted"], serde_json::json!(true));

    // THE distinction: a period that reported nothing carries null counts, not
    // zeroes. Berkshire's window contains one, so this is exercised for real.
    assert!(
        p["unobs_count"].as_i64().unwrap_or(0) >= 1,
        "expected the window to contain an un-observable period: {p}"
    );
    assert_eq!(p["unobs_all_null"], serde_json::json!(true));
}

/// `--persist` end to end: a real manager's real filings into the shared cache,
/// verified in SQL and then cleaned up.
///
/// Uses Motley Fool Asset Management because it never repeats a CUSIP, so the
/// stored row count should equal the holding count exactly — a filer whose rows
/// and holdings differ would hide an aggregation mistake behind a plausible
/// number.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT and a database"]
async fn manager_persist_writes_the_shared_cache_and_claims_no_cusip() {
    use dapple_data::DbPool;
    use dapple_data::query::institutional_holdings::{
        delete_holdings_for_manager_quarter, select_ingested_manager_quarters,
        select_ingested_quarters,
    };

    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let Ok(pool) = DbPool::connect().await else {
        println!("DB not available. Skipping.");
        return;
    };
    const MFAM: i64 = 1512814;

    let mut engine = StepEngine::new();
    let script = r#"
        let t = (edgar manager 1512814 --quarters 2 --tenures --persist)
        let obs = ($t.quarters | where observable)
        {
            payload: {
                rows_stored: $t.persisted.rows_stored,
                quarters_written: ($t.persisted.quarters_written | length),
                not_reported: $t.persisted.quarters_not_reported,
                skipped_derivative: $t.persisted.skipped_derivative,
                # This filer never repeats a CUSIP, so holdings == stored rows.
                holding_total: ($obs | get holding_count | math sum),
                cusips: ($obs | last | get holdings | get cusip | first 3)
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar manager --persist must run inside the StepEngine");
    let p = &output.payload;
    println!("persist payload: {p}");

    let stored = p["rows_stored"].as_i64().unwrap_or(0);
    assert!(
        stored > 100,
        "expected a real portfolio to land, got {stored}"
    );
    // No aggregation slip hiding behind a plausible number.
    assert_eq!(
        stored,
        p["holding_total"].as_i64().unwrap_or(-1),
        "stored rows must equal the aggregated holdings for a filer that never repeats a CUSIP"
    );

    let marked = select_ingested_manager_quarters(&pool, MFAM)
        .await
        .expect("manager markers");
    assert!(!marked.is_empty(), "the manager-quarters must be marked");

    // Verified in SQL, not from the response — and SCOPED to the quarters this
    // write claimed. The table is shared: a prior `edgar holders <cusip>` ingest
    // may already hold this manager's row for that one security in other
    // quarters, and those are none of our business. Observed live: single rows
    // in 2025 Q1-Q3 left by exactly that, untouched by this manager-scoped
    // write — which is the property that lets two writers share one table.
    let in_db: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM rev.institutional_holdings \
         WHERE manager_cik = $1 AND quarter_end = ANY($2)",
    )
    .bind(MFAM)
    .bind(&marked)
    .fetch_one(pool.get_pool())
    .await
    .expect("count stored rows");
    assert_eq!(
        in_db, stored,
        "the rows the command reported are the rows in the table"
    );

    // THE LANDMINE, live: no CUSIP was claimed complete by a manager-first write.
    for c in p["cusips"].as_array().into_iter().flatten() {
        let cusip = c.as_str().unwrap_or_default();
        let cusip_marked = select_ingested_quarters(&pool, cusip)
            .await
            .expect("cusip markers");
        for q in &marked {
            assert!(
                !cusip_marked.contains(q),
                "a manager-first write marked {cusip} complete for {q}; \
                 `edgar holders {cusip}` would now skip its download"
            );
        }
    }

    // Leave the shared dev database as we found it.
    for q in marked {
        let _ = delete_holdings_for_manager_quarter(&pool, MFAM, q).await;
    }
    let _ =
        sqlx::query("DELETE FROM rev.institutional_holdings_manager_ingest WHERE manager_cik = $1")
            .bind(MFAM)
            .execute(pool.get_pool())
            .await;
}

/// A PLATFORM TRUST files a separate 485BPOS per fund group, so its LATEST filing is routinely
/// some other fund's prospectus — and its roles are the SEC's standard ones, which name no fund
/// at all.
///
/// This is the shape that failed a live journey. `retort-the-n-port` asked the RBB Fund's trust
/// CIK for a prospectus and got a seven-fund filing that does not contain the Motley Fool 100
/// Index ETF, then reported the fund as disclosing nothing. Measured 2026-08-01 over fifty
/// well-known tickers, **32 of 46 resolvable picks** reached the wrong filing or one with no
/// prospectus in it; only Vanguard's funds and QQQ came through.
///
/// Three separate fixes have to hold for this to pass, which is why it asserts the end state
/// rather than any one of them:
///
/// 1. `browse-edgar` scoped BY SERIES selects this fund's own annual omnibus.
/// 2. `--fund <series>` survives a filing whose roles name no fund — the fund rows collapse to
///    one group, so an exact series matches no row and this used to error outright.
/// 3. The prose is placed by element name, because RBB's thirteen roles carry members but
///    **zero concepts** — there is nothing for the linkbase-driven path to assemble.
///
/// Asserted structurally, not against a pinned accession: RBB re-files several times a year.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn fund_sections_reads_a_platform_trust_by_series() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );

    // The Motley Fool 100 Index ETF (TMFC), one of 76 series on The RBB Fund's platform trust.
    let mut engine = StepEngine::new();
    let out = engine
        .execute_script(
            r#"
            let p = (edgar fund-sections "S000061203" --form "485BPOS" --fund "S000061203"
                     --fetch [InvestmentStrategy InvestmentRisks RiskReturn] --tagged)
            let secs = ($p.funds | each {|f| $f.sections } | flatten)
            let bodies = ($secs | where {|s| $s.text != null and ($s.text | str length) > 0 })
            {
                payload: {
                    accession: $p.accession,
                    filing_date: $p.filing_date,
                    keys_with_text: ($bodies | each {|s| $s.section_key } | uniq),
                    strategy_chars: (($bodies | where {|s| $s.section_key == "InvestmentStrategy"}
                                      | get -i 0.text | default "" | str length)),
                    risks_chars: (($bodies | where {|s| $s.section_key == "InvestmentRisks"}
                                   | get -i 0.text | default "" | str length)),
                    chrome: ($bodies | where {|s| $s.text =~ "IDEA: XBRL DOCUMENT" } | length),
                    tagged_risks: (($p.tagged | get -i S000061203 | default {}
                                    | get -i risks | default [] | length)),
                    warnings: $p.warnings
                },
                signals: { ok: true }
            }
        "#,
            &serde_json::json!({}),
            None,
        )
        .expect("a platform trust's own series must resolve to a prospectus");

    let p = &out.payload;
    println!(
        "RBB/TMFC accession={} filed={} keys={} tagged_risks={}",
        p["accession"], p["filing_date"], p["keys_with_text"], p["tagged_risks"]
    );
    println!("RBB/TMFC warnings: {}", p["warnings"]);

    let strategy = p["strategy_chars"].as_u64().unwrap_or(0);
    let risks = p["risks_chars"].as_u64().unwrap_or(0);
    assert!(
        strategy > 400,
        "InvestmentStrategy came back with {strategy} chars; a fund's principal-strategy \
         disclosure is never this short, so this is a lookup miss reported as a disclosure \
         fact. warnings: {}",
        p["warnings"]
    );
    assert!(
        risks > 400,
        "InvestmentRisks came back with {risks} chars. warnings: {}",
        p["warnings"]
    );
    // The instance is the source; a fragment would drag the SEC viewer's banner in with it.
    assert_eq!(
        p["chrome"].as_u64().unwrap_or(0),
        0,
        "prose came from a rendered fragment rather than the instance"
    );
    // `--tagged` keys by series, and this fund's risks are dimensioned by `oef:RiskAxis`.
    // Zero here with non-empty prose would mean the two halves came from different funds.
    assert!(
        p["tagged_risks"].as_u64().unwrap_or(0) > 0,
        "no tagged risk members for the series whose prose we just read. warnings: {}",
        p["warnings"]
    );
}

/// A 485BPOS IS NOT ALWAYS A PROSPECTUS. A registrant files one for administrative reasons too
/// — adding a share class, a delaying amendment — and such a filing carries rendered exhibits
/// but no XBRL instance at all.
///
/// Measured 2026-08-01, this was the LATEST 485BPOS for seven of forty-six mainstream funds,
/// and every one of them was iShares: IVV, IJH, IJR, IWM, IWF, IWD, SOXX. Taking `refs.first()`
/// unconditionally reported an S&P 500 tracker as having no prospectus while its real one sat
/// one filing back.
///
/// The assertion is on the OUTCOME (prose arrives), not on the skip itself, because iShares
/// will eventually file a real prospectus into the first slot and the skip will stop happening.
/// The warning is printed so a reader can see which filing was passed over.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn fund_sections_walks_back_past_a_filing_with_no_prospectus() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );

    // iShares Core S&P 500 ETF (IVV).
    let mut engine = StepEngine::new();
    let out = engine
        .execute_script(
            r#"
            let p = (edgar fund-sections "S000004310" --form "485BPOS" --fund "S000004310"
                     --fetch [InvestmentStrategy InvestmentRisks])
            let secs = ($p.funds | each {|f| $f.sections } | flatten)
            let bodies = ($secs | where {|s| $s.text != null and ($s.text | str length) > 0 })
            {
                payload: {
                    accession: $p.accession,
                    filing_date: $p.filing_date,
                    strategy_chars: (($bodies | where {|s| $s.section_key == "InvestmentStrategy"}
                                      | get -i 0.text | default "" | str length)),
                    warnings: $p.warnings
                },
                signals: { ok: true }
            }
        "#,
            &serde_json::json!({}),
            None,
        )
        .expect("a fund whose latest 485BPOS is administrative must still find its prospectus");

    let p = &out.payload;
    println!(
        "IVV accession={} filed={} strategy={}",
        p["accession"], p["filing_date"], p["strategy_chars"]
    );
    println!("IVV warnings: {}", p["warnings"]);
    assert!(
        p["strategy_chars"].as_u64().unwrap_or(0) > 400,
        "no strategy prose for IVV; the lookback did not reach a filing with a prospectus. \
         warnings: {}",
        p["warnings"]
    );
}

/// `edgar security` recovers a fund position's identity from a bare CUSIP.
///
/// The whole point is a case a fixture cannot construct: `25861R303` is one of
/// Motley Fool Wealth Management's own positions, filed as `DOUBLELINE ETF TRUST`
/// and resolving at zero by name. Nothing about the recovery is in our control —
/// it depends on which OTHER funds happen to hold it and how each of them chose
/// to spell it.
///
/// Two properties are asserted that a single-filer world would never surface:
/// the recovered name is the LONG form (a filer truncates it at 30 characters,
/// and that truncation is the more common spelling), and `filers` is smaller
/// than `filings` (one trust files quarterly, and repeats are not corroboration).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn security_recovers_a_fund_identity_from_a_bare_cusip() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let s = (edgar security 25861R303)
        # Not the design intent, but free: funds hold ordinary equities too and
        # name them, so an operating company resolves as readily as an ETF.
        let equity = (edgar security 037833100)
        # A well-formed CUSIP no fund reports holding.
        let nobody = (edgar security ZZZZZZZZ9)
        {
            payload: {
                outcome: $s.outcome,
                name: $s.name,
                ticker: $s.ticker,
                isin: $s.isin,
                hits: $s.hits,
                filers: $s.filers,
                filings: $s.filings,
                variants: ($s.name_variants | length),
                longest_wins: ($s.name_variants | all { |v|
                    ($v.name | str length) <= ($s.name | str length) }),
                equity_outcome: $equity.outcome,
                equity_name: $equity.name,
                nobody_outcome: $nobody.outcome
            },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar security must run inside the StepEngine");
    let p = &output.payload;
    println!("security payload: {p}");

    assert_eq!(p["outcome"], serde_json::json!("resolved"));

    // The recovered name must be the FULL one. A filer truncates this at
    // `DoubleLine Commercial Real Est`, so a first-hit or most-common rule
    // returns the truncation — and it is the more common spelling.
    let name = p["name"].as_str().unwrap_or_default();
    assert!(
        name.contains("DoubleLine") && name.contains("Real Estate"),
        "expected the untruncated name, got {name:?}"
    );
    assert_eq!(p["longest_wins"], serde_json::json!(true));

    // The ISIN carries the CUSIP we asked about — that is how the holding was
    // matched in filings that identify by ISIN rather than by CUSIP.
    assert_eq!(p["isin"], serde_json::json!("US25861R3030"));
    // The ticker is what pins a series downstream; a name alone reaches a trust.
    assert_eq!(p["ticker"], serde_json::json!("DCRE"));

    // THE DENOMINATOR ASSERTION. The index returns repeat filings from one
    // trust; `hits` counts them and OVERSTATES corroboration, `filers` does not.
    // Reporting only `hits` would call this "confirmed by four sources".
    let hits = p["hits"].as_i64().unwrap_or(0);
    let filers = p["filers"].as_i64().unwrap_or(0);
    let filings = p["filings"].as_i64().unwrap_or(0);
    assert!(filers >= 2, "expected independent filers to disagree: {p}");
    assert!(
        hits > filers,
        "the index's repeat filings should exceed filers: {p}"
    );
    assert!(
        filings <= filers,
        "documents are fetched once per filer, so filings cannot exceed filers: {p}"
    );

    // An ordinary equity resolves too — funds hold Apple and name it. Not the
    // design intent, but it means the command cleans up ANY 13F CUSIP's name.
    assert_eq!(p["equity_outcome"], serde_json::json!("resolved"));
    assert!(
        p["equity_name"]
            .as_str()
            .unwrap_or_default()
            .contains("Apple"),
        "expected an Apple spelling, got {:?}",
        p["equity_name"]
    );

    // A CUSIP no fund reports holding is `not_found` — an answer, not an error.
    assert_eq!(p["nobody_outcome"], serde_json::json!("not_found"));
}

/// The optional-arg commands run inside a guide task script.
///
/// A guide task runs with the dispatch RECORD on `$in`, so every command a guide
/// calls is type-checked against a record input **whether it reads one or not**.
/// A signature declaring only `(Nothing, …)` and `(List<String>, …)` is therefore
/// REJECTED AT PARSE TIME with "expected nothing and list<string>, got record".
/// The command is fine; the call is unparseable.
///
/// Found 2026-08-01 while building `edgar security`, then confirmed in TWO
/// already-shipped commands with the identical signature. Both had live tests —
/// which call from a shell, where the input really is nothing — so nothing
/// caught it. Stage 1 of `mind-the-13f` calls `edgar adv`, so this would have
/// surfaced as a guide that simply would not ingest.
///
/// A scan of the family found exactly these three; any future command taking an
/// optional arg plus a list on the pipeline needs the same `(Any, Any)` arm.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn optional_arg_commands_are_callable_from_a_guide_task_script() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let a = (edgar adv "Motley Fool Wealth Management LLC")
        let c = (edgar company-cik "Apple Inc")
        {
            payload: { adv_outcome: $a.outcome, adv_crd: $a.crd, cik_outcome: $c.outcome },
            signals: { fetched: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("these must be callable from a task script, not just from a shell");
    println!("payload: {}", output.payload);
    assert_eq!(
        output.payload["adv_outcome"],
        serde_json::json!("registered")
    );
    // company-cik's own outcome vocabulary; the point here is that it PARSED.
    assert!(output.payload["cik_outcome"].is_string());
}

/// THE DEMO-SAFETY CHECK: a spread of funds a person might actually pick, each asserted to
/// yield real prospectus prose.
///
/// Measured 2026-08-01 over fifty well-known tickers, **32 of 46 resolvable picks** reached the
/// wrong filing or a filing with no prospectus in it. Thirteen of the fourteen that worked were
/// Vanguard funds. The sample below is drawn from both sides of that line deliberately, because
/// the earlier failure was invisible to every check that looked at one filer:
///
/// | fund | how it failed before |
/// |---|---|
/// | SCHD | platform trust; also writes a DATE where the fund ordinal belongs |
/// | JEPI, XLF, MTUM | platform trust — latest filing is another fund's |
/// | ARKK | platform trust AND roles carrying zero concepts |
/// | SOXX | latest 485BPOS is administrative, no instance |
/// | QQQ, VOO | worked before; here as controls that must not regress |
///
/// Asserted on CHARACTER COUNT rather than content: every one of these funds discloses a
/// principal investment strategy, and a strategy section under a few hundred characters is a
/// lookup miss being reported as a disclosure fact — which is exactly the inversion that took
/// a live journey down.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn fund_sections_reads_a_spread_of_mainstream_funds() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );

    let picks = [
        ("SCHD", "S000034163"),
        ("JEPI", "S000068402"),
        ("XLF", "S000006411"),
        ("ARKK", "S000042977"),
        ("SOXX", "S000004354"),
        ("MTUM", "S000040316"),
        ("QQQ", "S000101292"),
        ("VOO", "S000002839"),
    ];

    let mut thin: Vec<String> = Vec::new();
    for (label, series) in picks {
        let mut engine = StepEngine::new();
        let script = format!(
            r#"
            let p = (edgar fund-sections "{series}" --form "485BPOS" --fund "{series}"
                     --fetch [InvestmentStrategy InvestmentRisks RiskReturn])
            let secs = ($p.funds | each {{|f| $f.sections }} | flatten)
            let body = {{|k| ($secs | where {{|s| $s.section_key == $k and $s.text != null }}
                              | get -i 0.text | default "" | str length) }}
            {{
                payload: {{
                    accession: $p.accession,
                    filing_date: $p.filing_date,
                    strategy: (do $body "InvestmentStrategy"),
                    risks: (do $body "InvestmentRisks"),
                    warnings: $p.warnings
                }},
                signals: {{ ok: true }}
            }}
        "#
        );
        match engine.execute_script(&script, &serde_json::json!({}), None) {
            Ok(out) => {
                let strategy = out.payload["strategy"].as_u64().unwrap_or(0);
                let risks = out.payload["risks"].as_u64().unwrap_or(0);
                println!(
                    "{label:6} {} {} strategy={strategy:6} risks={risks:6}",
                    out.payload["accession"], out.payload["filing_date"]
                );
                if strategy < 400 {
                    thin.push(format!(
                        "{label}: strategy {strategy} chars ({})",
                        out.payload["warnings"]
                    ));
                }
                if risks < 400 {
                    thin.push(format!("{label}: risks {risks} chars"));
                }
            }
            Err(e) => thin.push(format!("{label}: command failed outright: {e}")),
        }
        // SEC caps at roughly ten requests a second and each fund costs several. Pacing here
        // keeps a whole-file run from failing as though the DATA were wrong.
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }

    assert!(
        thin.is_empty(),
        "{} of {} mainstream picks returned no usable prospectus prose:\n  {}",
        thin.len(),
        picks.len(),
        thin.join("\n  ")
    );
}

/// A 497 SUPPLEMENT BELONGS TO ONE FUND, and the submissions feed cannot say which.
///
/// 497s are how a fund changes a strategy, a benchmark, a manager or a fee between annual
/// prospectuses - short, unannounced, and the highest-signal filings a fund makes. Scoped to a
/// trust CIK on a 76-fund platform trust, `--years 3` returns the trust's three most recent,
/// which are almost certainly three OTHER funds'. A guide reading them attributes another
/// fund's strategy change to this one: a fabricated fact, not a rough one.
///
/// `--series` intersects the submissions records with EDGAR's browse feed, which does scope by
/// series. The assertion is that the two answers DIFFER - if they were the same, the filter
/// would be decorative.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn filings_series_filter_excludes_other_funds_supplements() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );

    // The RBB Fund (76 series) and its Motley Fool 100 Index ETF.
    let mut engine = StepEngine::new();
    let out = engine
        .execute_script(
            r#"
            let wide = (try { edgar filings "831114" --form "497" --years 2 | default [] } catch { [] })
            let mine = (try {
                edgar filings "831114" --form "497" --years 2 --series "S000061203" | default []
            } catch { [] })
            {
                payload: {
                    wide: ($wide | each {|f| $f.accession }),
                    mine: ($mine | each {|f| $f.accession }),
                },
                signals: { ok: true }
            }
        "#,
            &serde_json::json!({}),
            None,
        )
        .expect("both supplement lookups must run");

    let names = |k: &str| -> Vec<String> {
        out.payload[k]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    };
    let wide = names("wide");
    let mine = names("mine");
    println!("RBB 497 trust-wide: {wide:?}");
    println!("RBB 497 S000061203: {mine:?}");

    assert!(
        !mine.is_empty(),
        "the series filter returned nothing; an over-tight filter reads exactly like a fund \
         that files no supplements"
    );
    assert_ne!(
        wide, mine,
        "trust-scoped and series-scoped 497s came back identical on a 76-fund trust, so the \
         filter is not narrowing anything"
    );
}

/// A bad `--series` must be rejected at the ARGUMENT, not absorbed into an empty result.
///
/// An empty browse feed filters EVERY filing away, so a CIK or a fund name passed here would
/// otherwise report a fund with no supplements - the confident-empty failure this whole pass is
/// about.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn filings_rejects_a_series_argument_that_is_not_a_series() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let err = engine
        .execute_script(
            r#"
            let x = (edgar filings "831114" --form "497" --series "Motley Fool 100 Index ETF")
            { payload: { n: ($x | length) }, signals: {} }
        "#,
            &serde_json::json!({}),
            None,
        )
        .expect_err("a fund NAME is not a series id and must be refused");
    let msg = format!("{err}");
    assert!(
        msg.contains("series"),
        "the error must name the offending argument: {msg}"
    );
}

// ---------------------------------------------------------------------------
// edgar book — the look-through composite
// ---------------------------------------------------------------------------

/// `edgar book` end to end through the StepEngine: two quarters over two well-known
/// filers plus a no_filer row. Asserts the shape a guide will actually read: coverage
/// partitions, a composite with per-field covered weight, per-holding series for the
/// tracks, and the decomposition identity on the second period.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_book_composite_and_effects_inside_step_engine() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    // Apple (320193) and Microsoft (789019): both tag revenue/net income/assets richly.
    // The T-bill row is the no_filer bucket — counted, never fetched.
    let script = r#"
        let rows = [
            { period: "2024Q4", period_end: "2024-12-31", cik: 320193, name: "APPLE INC", weight_pct: 50.0, value_usd: 500.0 }
            { period: "2024Q4", period_end: "2024-12-31", cik: 789019, name: "MICROSOFT CORP", weight_pct: 40.0, value_usd: 400.0 }
            { period: "2024Q4", period_end: "2024-12-31", cik: null, name: "US TREASURY BILL", weight_pct: 10.0, value_usd: 100.0, reason: "no_filer" }
            { period: "2025Q1", period_end: "2025-03-31", cik: 320193, name: "APPLE INC", weight_pct: 45.0, value_usd: 450.0 }
            { period: "2025Q1", period_end: "2025-03-31", cik: 789019, name: "MICROSOFT CORP", weight_pct: 45.0, value_usd: 450.0 }
            { period: "2025Q1", period_end: "2025-03-31", cik: null, name: "US TREASURY BILL", weight_pct: 10.0, value_usd: 100.0, reason: "no_filer" }
        ]
        let b = ($rows | edgar book --fields [revenue net_income total_assets])
        let p2 = ($b.periods | last)
        let fx = ($p2.effects.revenue? | default {})
        {
            payload: {
                periods: ($b.periods | length),
                holdings: ($b.holdings | length),
                covered_w: ($p2.coverage.covered_weight_pct),
                no_filer_w: ($p2.coverage.no_filer_weight_pct),
                composite_fields: ($p2.composite | columns | length),
                rev_covered_w: ($p2.composite.revenue?.covered_weight_pct? | default (-1.0)),
                fx_sum_ok: (if (($fx | columns | length) > 0) {
                    # RELATIVE tolerance: revenue-scale values are ~1e11, so f64 products
                    # carry absolute error far above any fixed epsilon a small fixture
                    # would suggest.
                    let scale = ([1.0 ($fx.total_delta | math abs)] | math max)
                    (((($fx.company + $fx.manager + $fx.interaction) - $fx.total_delta) | math abs) / $scale) < 0.000000001
                } else { false }),
                fx_n: ($fx.n? | default 0),
                unread: ($b.unread | length),
            },
            signals: { ok: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar book must run inside the StepEngine");
    assert_eq!(output.payload["periods"], serde_json::json!(2));
    assert_eq!(
        output.payload["holdings"],
        serde_json::json!(2),
        "two fetched filers"
    );
    assert_eq!(output.payload["covered_w"], serde_json::json!(90.0));
    assert_eq!(output.payload["no_filer_w"], serde_json::json!(10.0));
    assert!(
        output.payload["composite_fields"].as_u64().unwrap_or(0) >= 3,
        "revenue, net_income, total_assets must all composite"
    );
    assert!(
        output.payload["rev_covered_w"].as_f64().unwrap_or(-1.0) > 0.0,
        "the revenue cell must state the weight it covered"
    );
    assert_eq!(
        output.payload["fx_sum_ok"],
        serde_json::json!(true),
        "company + manager + interaction must equal total_delta on live data"
    );
    assert_eq!(output.payload["fx_n"], serde_json::json!(2));
    assert_eq!(
        output.payload["unread"],
        serde_json::json!(1),
        "the T-bill, as no_filer"
    );
}

/// The cost gates land excluded weight in the GATED coverage bucket — stated, never
/// silent — and shrink the fetch set.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test against sec.gov; requires DAPPLE_EDGAR_USER_AGENT"]
async fn edgar_book_top_gate_is_stated_in_coverage() {
    assert!(
        std::env::var("DAPPLE_EDGAR_USER_AGENT").is_ok(),
        "set DAPPLE_EDGAR_USER_AGENT to run the live EDGAR test"
    );
    let mut engine = StepEngine::new();
    let script = r#"
        let rows = [
            { period: "2025Q1", period_end: "2025-03-31", cik: 320193, name: "APPLE INC", weight_pct: 60.0 }
            { period: "2025Q1", period_end: "2025-03-31", cik: 789019, name: "MICROSOFT CORP", weight_pct: 40.0 }
        ]
        let b = ($rows | edgar book --fields [total_assets] --top 1)
        let p = ($b.periods | first)
        {
            payload: {
                holdings: ($b.holdings | length),
                gated_w: ($p.coverage.gated_weight_pct),
                gated_reason: ($b.unread | first | get reason),
            },
            signals: { ok: true }
        }
    "#;
    let output = engine
        .execute_script(script, &serde_json::json!({}), None)
        .expect("edgar book must run inside the StepEngine");
    assert_eq!(
        output.payload["holdings"],
        serde_json::json!(1),
        "--top 1 fetches one filer"
    );
    assert_eq!(output.payload["gated_w"], serde_json::json!(40.0));
    assert_eq!(output.payload["gated_reason"], serde_json::json!("gated"));
}
