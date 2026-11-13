//! The `edgar fund-sections` command — the promise side, already partitioned.
//!
//! Reads a 485BPOS or N-CSR's sections, grouped per fund by the role URI. No heading
//! taxonomy is involved: filers vary section wording, but the section-key vocabulary
//! (`InvestmentStrategy`, `InvestmentRisks`, `FeesAndExpenses`, …) does not.
//!
//! Three files, each answering something the others cannot:
//!
//! | file | what only it has | when fetched |
//! |---|---|---|
//! | `FilingSummary.xml` | the role → `R<n>.htm` fragment map | always, falling back to the linkbase |
//! | the XBRL instance | the tagged prose, risks, benchmarks | `--fetch` or `--tagged` |
//! | the presentation/label linkbases | which concepts a section presents; the fund names and series ids | with the instance |
//!
//! `--fetch` therefore costs ONE instance fetch for a whole filing rather than one request
//! per section, and works on a filing that ships no `FilingSummary.xml` at all. The rendered
//! fragments remain the fallback and remain the only usable source for the numeric tables.

use edgarkit::FilingOperations;
use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{
    archive_url, build_client, edgar_error, fetch_instance, locate_fund_filings,
};
use crate::edgar_fund_linkbase::{Linkbase, linkbase_refs, parse_linkbase, schema_ref};
use crate::edgar_fund_sections::{FilingSections, SectionRef, parse_filing_summary};
use crate::edgar_fund_tags::{InstanceTags, TaggedMember, parse_prospectus_instance_with_linkbase};
use crate::edgar_fund_text::{InstanceText, parse_prospectus_text};
use crate::edgar_sections::html_to_text;

/// Cap on fetched section bodies, so `--fetch` on an eight-fund prospectus
/// cannot quietly turn into ninety requests.
const MAX_FETCH: usize = 24;

/// How many recent filings to consider when the most recent one turns out to carry no
/// prospectus.
///
/// Five rather than one because a 485BPOS is not always a prospectus: administrative filings
/// (a new share class, a delaying amendment) carry no XBRL instance, and for seven of
/// forty-six mainstream funds measured 2026-08-01 — every iShares fund in the sample — that was
/// the LATEST 485BPOS. Five rather than twenty because the annual cycle means the real
/// prospectus is close behind, and each rejected filing costs a request. Filings are only
/// examined until one is usable, so the normal case still costs exactly one.
const FILING_LOOKBACK: usize = 5;

/// Fetch a filing's presentation and label linkbases, by the standard discovery chain:
/// instance -> `schemaRef` -> `.xsd` -> (inline, or `linkbaseRef` siblings).
///
/// `None` when nothing usable came back. Every failure along the way degrades to `None`
/// rather than erroring: the linkbase improves attribution and naming, and a filing that
/// does not ship one readably is still worth reporting on.
///
/// Two shapes, both real and both handled by the same chain — a filer that EMBEDS its
/// linkbases in the schema (1290's 1.1 MB `.xsd`, Vanguard's 5.9 MB) needs one fetch, and a
/// filer that SPLITS them out (Fidelity's 35 KB `.xsd` holds none) needs two more.
async fn fetch_linkbase(
    edgar: &edgarkit::Edgar,
    cik: &str,
    accession: &str,
    instance_xml: &str,
) -> Option<Linkbase> {
    let schema = schema_ref(instance_xml)?;
    linkbase_from_schema(edgar, cik, accession, &schema).await
}

/// As [`fetch_linkbase`], but locating the schema from the filing's file index rather than
/// from an instance.
///
/// Needed for the no-`FilingSummary.xml` path, where the instance's own filename is not yet
/// known: that file is what normally names the primary document.
async fn fetch_linkbase_by_listing(
    edgar: &edgarkit::Edgar,
    cik: &str,
    accession: &str,
) -> Option<Linkbase> {
    let dir = edgar.filing_directory(cik, accession).await.ok()?;
    let schema = dir
        .directory
        .item
        .iter()
        .map(|i| i.name.as_str())
        .find(|n| n.ends_with(".xsd"))?;
    linkbase_from_schema(edgar, cik, accession, schema).await
}

/// Fetch a schema and everything it points at, merged.
///
/// Every failure degrades to `None` rather than erroring: the linkbase improves attribution
/// and naming, and a filing that does not ship one readably is still worth reporting on.
async fn linkbase_from_schema(
    edgar: &edgarkit::Edgar,
    cik: &str,
    accession: &str,
    schema: &str,
) -> Option<Linkbase> {
    let Ok(xsd) = edgar.get(&archive_url(cik, accession, schema)).await else {
        log::warn!("edgar fund-sections: could not fetch schema {schema}");
        return None;
    };

    let mut lb = parse_linkbase(&xsd);
    for href in linkbase_refs(&xsd) {
        match edgar.get(&archive_url(cik, accession, &href)).await {
            Ok(x) => lb.merge(parse_linkbase(&x)),
            Err(_) => log::warn!("edgar fund-sections: could not fetch linkbase {href}"),
        }
    }

    (!lb.is_empty()).then_some(lb)
}

/// The section index of a filing that ships no `FilingSummary.xml`, recovered from its
/// presentation linkbase.
///
/// `None` when the filing has no readable linkbase either, which is the only case left where
/// the command genuinely cannot say anything.
async fn sections_from_linkbase(
    edgar: &edgarkit::Edgar,
    cik: &str,
    accession: &str,
) -> Option<FilingSections> {
    let lb = fetch_linkbase_by_listing(edgar, cik, accession).await?;
    let mut sections = FilingSections::from_linkbase(&lb);
    if sections.sections.is_empty() {
        return None;
    }
    // The instance is named the same way `fetch_instance` expects: the extracted `_htm.xml`
    // sits beside a primary `.htm`, so handing back the `.htm` stem keeps --tagged working
    // through the ordinary path.
    if let Ok(dir) = edgar.filing_directory(cik, accession).await {
        sections.instance = dir
            .directory
            .item
            .iter()
            .filter_map(|i| i.name.strip_suffix("_htm.xml"))
            .map(|stem| format!("{stem}.htm"))
            .next();
    }
    Some(sections)
}

#[derive(Clone)]
pub struct EdgarFundSections;

impl Command for EdgarFundSections {
    fn name(&self) -> &str {
        "edgar fund-sections"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar fund-sections")
            .input_output_types(vec![(Type::Nothing, Type::record())])
            .required(
                "identifier",
                SyntaxShape::String,
                "Trust CIK, or a series id (S000######) belonging to it",
            )
            .named(
                "form",
                SyntaxShape::String,
                "Form to read: 485BPOS (default) or N-CSR",
                Some('f'),
            )
            .named(
                "fund",
                SyntaxShape::String,
                "Keep only the fund whose name matches (tolerant of punctuation)",
                None,
            )
            .named(
                "fetch",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Section keys whose rendered text to fetch, e.g. [InvestmentStrategy InvestmentRisks]",
                None,
            )
            .switch(
                "tagged",
                "Also read the risks and benchmarks the filing TAGGED, from its XBRL instance",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Read a fund prospectus or shareholder report's section index."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a record { accession, filing_date, form, instance, funds, tagged,
unattributed_benchmarks, warnings }.

`funds` is a table of { fund_index, series_id, fund_name, sections }, where each
section is { section_key, short_name, html_file, position, text }. `text` is null
unless the key was named in --fetch. `series_id` is the fund's exact identity and
the key `tagged` reports under, so the two join without matching names.

--fetch returns a section's PROSE as the filing itself tagged it, read from the
XBRL instance rather than from the rendered R<n>.htm fragment. That text is the
same disclosure without the viewer chrome the fragment carries (a version stamp,
the renderer's JavaScript source, XBRL element labels, and heading markup that
arrives double-escaped), it works on a filing that ships no FilingSummary.xml at
all, and it costs one fetch for the whole filing instead of one per section.
The numeric tables — ShareholderFeesData, OperatingExpensesData,
PerformanceTableData — are held in the instance as bare numbers, so those keys
still fall back to their rendered fragment, which lays the table out properly.

--tagged additionally reads the filing's XBRL instance (one extra fetch, roughly
0.5-3 MB, shared with --fetch when both are passed) and returns `tagged`: a
record keyed by SERIES ID, each holding
{ risks, benchmarks } as tables of { member, name }. It is keyed by series id
rather than by fund_index because that is how the instance itself dimensions the
facts, and because the caller already holds the series id — re-attributing to this
filing's own fund grouping would swap an exact key for a name-match guess. Both
`tagged` and `unattributed_benchmarks` are null unless --tagged was passed, and
empty when it was passed and the filing tags nothing.

A benchmark is a performance measure carried by no share-classed context; the
fund's own (after-tax, NAV-based) returns are always share-classed and so are
excluded. Filings before the 2024q3 oef taxonomy carry no risk axis at all and
report empty risks with a warning saying so.

Section keys come from the filing's own role URIs:
  N1aCover, RiskReturn, FeesAndExpenses, InvestmentStrategy, InvestmentRisks,
  PerformanceManagement, ShareholderFeesData, OperatingExpensesData,
  ExpenseExample, BarChartData, PerformanceTableData

A prospectus covering several funds partitions itself, on the fund ordinal in the
role URI or on a series id written into the section key, whichever the filer uses.
The unit is the FUND (series): a filer writing one role group per prospectus
variant still reports one fund. Trust-level sections (the shared cover) appear
with a null fund_index.

A filing that ships no FilingSummary.xml still lists its sections, recovered from
the presentation linkbase, and --fetch still returns their prose from the
instance. Only html_file stays null, because the role-to-fragment map exists
nowhere else — so on such a filing the numeric-table keys have no text and
`warnings` says so.

Passing a series id scopes the FILING lookup to that series' trust, but the
prospectus still covers every fund in it — use --fund to narrow.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  edgar fund-sections 1605941 | get funds | select fund_index fund_name

  edgar fund-sections 1605941 --fund "1290 High Yield Bond Fund" \
    --fetch [InvestmentStrategy InvestmentRisks]
    | get funds.0.sections | where text != null
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "prospectus", "485bpos", "ncsr", "fund"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Which funds does this prospectus cover?",
            example: r#"edgar fund-sections 1605941 | get funds | select fund_index fund_name"#,
            result: None,
        }]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let identifier: String = call
            .req::<String>(engine_state, stack, 0)?
            .trim()
            .to_string();
        let form = call
            .get_flag::<String>(engine_state, stack, "form")?
            .unwrap_or_else(|| "485BPOS".to_string())
            .trim()
            .to_ascii_uppercase();
        let only_fund = call.get_flag::<String>(engine_state, stack, "fund")?;
        let fetch: Vec<String> = call
            .get_flag::<Vec<Value>>(engine_state, stack, "fetch")?
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.coerce_into_string().ok())
            .collect();
        let tagged = call.has_flag(engine_state, stack, "tagged")?;

        let edgar = build_client(span)?;

        // The instance and the linkbases answer BOTH `--tagged` and `--fetch`, so they are
        // fetched once, up front, and only when something asks for them. `--fetch` needs them
        // because a section's prose comes from the instance (see `crate::edgar_fund_text`);
        // `--tagged` because its risks and benchmarks do. Decided here because it is also what
        // makes a filing worth choosing below.
        let needs_instance = tagged || !fetch.is_empty();

        #[allow(clippy::result_large_err)]
        let (sections, accession, filed, cik, skipped) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let refs =
                    locate_fund_filings(&edgar, &identifier, &form, FILING_LOOKBACK, span).await?;
                if refs.is_empty() {
                    return Err(ShellError::GenericError {
                        error: format!("No {form} filing for `{identifier}`"),
                        msg: "EDGAR returned no such filings for this identifier".into(),
                        span: Some(span),
                        help: Some(
                            "Try --form N-CSR, or pass the trust CIK rather than a series id."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }

                let mut tried: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                let mut skipped: Vec<String> = Vec::new();
                let mut chosen: Option<(FilingSections, String, String, String)> = None;
                let mut first_failure: Option<ShellError> = None;

                for r in &refs {
                    // browse-edgar's atom feed lists each filing TWICE (once per <entry> shape),
                    // so without this the lookback burns half its budget re-reading one filing.
                    if !tried.insert(r.accession.clone()) {
                        continue;
                    }
                    // FilingSummary.xml is the fast path, not the only one. It carries the
                    // role->fragment map nothing else has, but a filing that ships none still
                    // declares every role in its presentation linkbase — and used to fail here
                    // outright, taking a whole filer family with it.
                    let parsed: Option<FilingSections> = match edgar
                        .get(&archive_url(&r.cik, &r.accession, "FilingSummary.xml"))
                        .await
                    {
                        Ok(xml) => match parse_filing_summary(&xml) {
                            Ok(p) => Some(p),
                            Err(msg) => {
                                first_failure.get_or_insert(ShellError::GenericError {
                                    error: "FilingSummary.xml did not parse".into(),
                                    msg,
                                    span: Some(span),
                                    help: None,
                                    inner: vec![],
                                });
                                sections_from_linkbase(&edgar, &r.cik, &r.accession).await
                            }
                        },
                        Err(e) => {
                            first_failure.get_or_insert(edgar_error(
                                &format!(
                                    "fetch FilingSummary for {} (and no presentation linkbase \
                                     was readable either)",
                                    r.accession
                                ),
                                &e,
                                span,
                            ));
                            sections_from_linkbase(&edgar, &r.cik, &r.accession).await
                        }
                    };

                    // A FILING IS NOT A PROSPECTUS. A registrant files 485BPOS for
                    // administrative reasons too — adding a share class, a delaying amendment —
                    // and such a filing carries rendered exhibits but no XBRL instance and no
                    // section index at all. Taking `refs.first()` unconditionally made those
                    // funds indistinguishable from funds that disclose nothing: measured
                    // 2026-08-01, SEVEN of forty-six mainstream picks (IVV, IJH, IJR, IWM, IWF,
                    // IWD, SOXX — every one an iShares fund) had exactly this as their latest
                    // 485BPOS, with the real prospectus one filing further back.
                    let Some(parsed) = parsed else {
                        skipped.push(format!(
                            "{} ({}): no readable section index",
                            r.accession, r.filing_date
                        ));
                        continue;
                    };
                    if needs_instance && parsed.instance.is_none() {
                        skipped.push(format!(
                            "{} ({}): no extracted XBRL instance",
                            r.accession, r.filing_date
                        ));
                        continue;
                    }
                    chosen = Some((
                        parsed,
                        r.accession.clone(),
                        r.filing_date.clone(),
                        r.cik.clone(),
                    ));
                    break;
                }

                match chosen {
                    Some((parsed, accession, filed, cik)) => {
                        Ok::<_, ShellError>((parsed, accession, filed, cik, skipped))
                    }
                    // Every filing in the lookback window was unreadable. Report the first real
                    // error rather than a synthesized one, so the cause is not laundered into
                    // "nothing found".
                    None => Err(first_failure.unwrap_or_else(|| ShellError::GenericError {
                        error: format!(
                            "No {form} filing for `{identifier}` carries a readable prospectus"
                        ),
                        msg: format!(
                            "checked the {} most recent: {}",
                            tried.len(),
                            skipped.join("; ")
                        ),
                        span: Some(span),
                        help: Some(
                            "The filings exist but are administrative (no XBRL instance). Try \
                             --form N-CSR, or a wider window."
                                .into(),
                        ),
                        inner: vec![],
                    })),
                }
            })
        })?;

        let mut sections = sections;
        #[allow(clippy::result_large_err)]
        let (instance_xml, linkbase): (Option<String>, Option<Linkbase>) = if needs_instance {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let Some(primary) = sections.instance.clone() else {
                        return (None, None);
                    };
                    let Some(xml) = fetch_instance(&edgar, &cik, &accession, &primary).await else {
                        return (None, None);
                    };
                    let lb = fetch_linkbase(&edgar, &cik, &accession, &xml).await;
                    (Some(xml), lb)
                })
            })
        } else {
            (None, None)
        };
        if let Some(lb) = &linkbase {
            // Most filers' role URIs name an ordinal, not a series, so without this the fund
            // rows carry `series_id: null` while `tagged` is keyed by series — two halves of
            // one answer that a caller cannot join.
            sections.attribute_series_from_linkbase(lb);
        }

        // The prose the filing tagged, per fund and section. A MISSING LINKBASE IS NO LONGER
        // DISQUALIFYING: the instance attributes its own facts by `dei:LegalEntityAxis` and
        // `parse_prospectus_text` places them by element name when no role can (see that
        // module's pass 3b), so the linkbase improves the result rather than gating it.
        let instance_text: InstanceText = match &instance_xml {
            Some(xml) if !fetch.is_empty() => {
                parse_prospectus_text(xml, linkbase.as_ref().unwrap_or(&Linkbase::default()))
            }
            _ => InstanceText::default(),
        };

        // The series the caller named, when they named one EXACTLY. `--fund` also takes a fund
        // name, which is matched loosely and cannot carry this authority.
        let wanted_series: Option<String> = only_fund.as_deref().and_then(|n| {
            let t = n.trim();
            crate::edgar_fund_sections::series_in_role(t).filter(|s| s.len() == t.len())
        });

        // Which fund groups survive the --fund filter.
        let keep: Option<u32> = match &only_fund {
            Some(name) => match sections.fund_index_for_name(name) {
                Some(i) => Some(i),
                // NOT MATCHING A FUND ROW IS NOT THE SAME AS THE FUND NOT BEING HERE. A filing
                // whose roles are the SEC's standard ones (`.../oef/role/InvestmentStrategy`)
                // names no fund in any role — it reaches every series in the trust — so the
                // fund rows collapse to one group and an exact series id matches none of them.
                // The instance says otherwise, exactly, and it is the authority on which funds
                // a filing covers. Measured: RBB's 2025 omnibus carries 76 series this way,
                // and `--fund S000061203` failed here against the CORRECT filing.
                None if wanted_series
                    .as_deref()
                    .is_some_and(|s| instance_text.by_series.contains_key(s)) =>
                {
                    None
                }
                None => {
                    let known: Vec<&str> =
                        sections.fund_names.values().map(String::as_str).collect();
                    return Err(ShellError::GenericError {
                        error: format!("No fund matching `{name}` in this filing"),
                        msg: format!("filing covers: {}", known.join("; ")),
                        span: Some(span),
                        help: Some(
                            "Names are matched loosely on letters and digits; a name matching \
                             two funds is rejected rather than guessed."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }
            },
            None => None,
        };

        let wanted: Vec<(u32, String)> = plan_fetches(
            &sections,
            keep,
            &fetch,
            &instance_text,
            wanted_series.as_deref(),
        );
        #[allow(clippy::result_large_err)]
        let bodies: std::collections::BTreeMap<String, String> =
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let mut out = std::collections::BTreeMap::new();
                    for (_, file) in wanted.iter().take(MAX_FETCH) {
                        // A fragment that will not load narrows the result
                        // rather than failing the command.
                        if let Ok(html) = edgar.get(&archive_url(&cik, &accession, file)).await {
                            out.insert(file.clone(), html_to_text(&html));
                        } else {
                            log::warn!("edgar fund-sections: could not fetch {file}");
                        }
                    }
                    out
                })
            });

        // The tagged risks/benchmarks, keyed by SERIES ID rather than by this filing's own
        // fund grouping. The series id is what the instance itself dimensions the facts by
        // (`dei:LegalEntityAxis` members are literal `S#########` ids), and it is what a
        // caller already holds — `edgar fund` resolves it before this command is ever called.
        // Re-attributing to `fund_index`, which is derived from a role URI and matched to a
        // fund by name-squashing, would replace an exact key with a guess.
        let instance_tags: Option<InstanceTags> = if tagged {
            match (&instance_xml, sections.instance.as_deref()) {
                // The instance alone attributes almost no benchmark to a fund and names
                // extension members by splitting their QNames; the linkbases fix both. See
                // `edgar_fund_linkbase`.
                (Some(xml), _) => Some(parse_prospectus_instance_with_linkbase(
                    xml,
                    linkbase.as_ref(),
                )),
                // A filing with no extracted instance is a fact about the filing, not a
                // failure of the command that asked for it.
                (None, Some(primary)) => Some(InstanceTags {
                    warnings: vec![format!(
                        "no extracted XBRL instance sits beside {primary}; this filing exposes \
                         no tagged risks or benchmarks"
                    )],
                    ..Default::default()
                }),
                (None, None) => Some(InstanceTags {
                    warnings: vec![
                        "this filing names no primary document, so no XBRL instance could be \
                         located"
                            .into(),
                    ],
                    ..Default::default()
                }),
            }
        } else {
            None
        };

        let mut fund_rows: Vec<Value> = Vec::new();
        let mut indices: Vec<Option<u32>> = sections
            .sections
            .iter()
            .map(|s| s.fund_index)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        indices.sort();

        // Funds for which --fetch named keys and none of them produced any text. Collected so
        // the command can SAY so: a fund whose sections come back with null text everywhere
        // reads exactly like a fund that disclosed nothing, and the two are very different.
        let mut starved: Vec<String> = Vec::new();

        for idx in indices {
            if let (Some(k), Some(i)) = (keep, idx)
                && k != i
            {
                continue;
            }
            if keep.is_some() && idx.is_none() {
                continue;
            }
            // The series is what `tagged` keys on, so carrying it here is what lets a caller
            // join the two without matching fund names. Resolved BEFORE the rows because it is
            // also what lets a fund claim instance prose its own roles never named.
            let series = sections
                .sections
                .iter()
                .find(|s| s.fund_index == idx && s.series_id.is_some())
                .and_then(|s| s.series_id.clone())
                .or_else(|| idx.is_some().then(|| wanted_series.clone()).flatten());

            // SECTIONS THIS FUND HAS PROSE FOR THAT NO ROLE OF ITS OWN NAMES.
            //
            // Fidelity is the measured case: its per-fund roles are keyed
            // `DisclosureDetailS000067930_Fidelity…`, and the standard keys a caller asks for
            // (`InvestmentStrategy`) sit on TRUST-level `xbrl.sec.gov/rr/role/*` roles that
            // belong to no fund. So `--fetch [InvestmentStrategy]` matched only trust-level
            // rows, every fund's own sections came back null, and the two sections that did
            // have text got it from a rendered fragment, viewer chrome and all.
            //
            // The instance has the prose and attributes it to this exact series. Emitting a row
            // for it is not inventing a section — it is naming one the filer tagged and the
            // linkbase declined to place.
            let synthesized: Vec<(String, &str)> = match series.as_deref() {
                Some(s) if !fetch.is_empty() => instance_text
                    .by_series
                    .get(s)
                    .into_iter()
                    .flatten()
                    .filter(|(key, _)| fetch.iter().any(|k| k.eq_ignore_ascii_case(key)))
                    .filter(|(key, _)| {
                        !sections
                            .sections
                            .iter()
                            .any(|r| r.fund_index == idx && r.section_key.eq_ignore_ascii_case(key))
                    })
                    .map(|(key, text)| (key.clone(), text.as_str()))
                    .collect(),
                _ => Vec::new(),
            };

            if !fetch.is_empty() && idx.is_some() {
                let mine = sections.sections.iter().filter(|s| s.fund_index == idx);
                let (mut matched, mut got_text) = (synthesized.len(), !synthesized.is_empty());
                for s in mine {
                    if fetch.iter().any(|k| k.eq_ignore_ascii_case(&s.section_key)) {
                        matched += 1;
                        got_text |= section_text(
                            &fetch,
                            &instance_text,
                            &bodies,
                            s,
                            wanted_series.as_deref(),
                        )
                        .is_some();
                    }
                }
                if !got_text {
                    let name = idx
                        .and_then(|i| sections.fund_names.get(&i))
                        .cloned()
                        .unwrap_or_else(|| format!("fund {}", idx.unwrap_or(0)));
                    // The count separates the two causes, which need opposite fixes: zero
                    // matches means the caller asked for a key this filing does not use, and
                    // a nonzero count means the key was right but neither source produced
                    // text (a filing with no fragments whose instance could not be read).
                    starved.push(format!("{name} ({matched} key(s) matched)"));
                }
            }
            let mut rows: Vec<Value> = sections
                .sections
                .iter()
                .filter(|s| s.fund_index == idx)
                .map(|s| {
                    Value::record(
                        record! {
                            "section_key" => Value::string(&s.section_key, span),
                            "short_name" => Value::string(&s.short_name, span),
                            "html_file" => s.html_file.as_deref()
                                .map(|f| Value::string(f, span))
                                .unwrap_or_else(|| Value::nothing(span)),
                            "position" => s.position
                                .map(|p| Value::int(p as i64, span))
                                .unwrap_or_else(|| Value::nothing(span)),
                            "text" => section_text(&fetch, &instance_text, &bodies, s, wanted_series.as_deref())
                                .map(|t| Value::string(t, span))
                                .unwrap_or_else(|| Value::nothing(span)),
                        },
                        span,
                    )
                })
                .collect();
            rows.extend(synthesized.into_iter().map(|(key, text)| {
                Value::record(
                    record! {
                        "section_key" => Value::string(&key, span),
                        // The key itself, rather than a prettified form: this row has no role
                        // and so no display name of the filer's own, and inventing one would
                        // read as though the filing had provided it.
                        "short_name" => Value::string(&key, span),
                        // No role means no fragment. Null here is the honest answer and keeps
                        // `html_file != null` usable as "the filer rendered this section".
                        "html_file" => Value::nothing(span),
                        "position" => Value::nothing(span),
                        "text" => Value::string(text, span),
                    },
                    span,
                )
            }));

            fund_rows.push(Value::record(
                record! {
                    "fund_index" => idx.map(|i| Value::int(i as i64, span))
                        .unwrap_or_else(|| Value::nothing(span)),
                    "series_id" => series.map(|s| Value::string(s, span))
                        .unwrap_or_else(|| Value::nothing(span)),
                    "fund_name" => idx.and_then(|i| sections.fund_names.get(&i))
                        .map(|n| Value::string(n, span))
                        .unwrap_or_else(|| Value::nothing(span)),
                    "sections" => Value::list(rows, span),
                },
                span,
            ));
        }

        let mut warnings = sections.warnings.clone();
        if let Some(t) = &instance_tags {
            warnings.extend(t.warnings.iter().cloned());
        }
        warnings.extend(instance_text.warnings.iter().cloned());
        // WHICH FILING WAS PASSED OVER, AND WHY. Silently reading an older filing than the one
        // EDGAR lists first would be a reasonable thing to be surprised by, so it is stated
        // rather than inferred from the accession.
        if !skipped.is_empty() {
            warnings.push(format!(
                "read {accession} ({filed}); skipped {} more recent {form} filing(s) carrying no \
                 prospectus: {}",
                skipped.len(),
                skipped.join("; ")
            ));
        }
        if !starved.is_empty() {
            // Naming the keys the filing DOES have is the whole value of this warning. Some
            // filers put the standard section keys on TRUST-level roles and the funds on
            // per-fund roles with opaque keys (Fidelity writes
            // `.../role/DisclosureDetailS000067930_FidelityClimateActionFund-ComboPRO`), so
            // asking for `InvestmentStrategy` scoped to a fund matches nothing — and silence
            // there reads as "this fund disclosed nothing", the inverted answer.
            let available: Vec<&str> = sections
                .sections
                .iter()
                .filter(|s| s.fund_index.is_some())
                .map(|s| s.section_key.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            warnings.push(format!(
                "--fetch matched no text for {}: {}. This filing's per-fund section keys are: \
                 {}. Requested: {}.",
                if starved.len() == 1 { "fund" } else { "funds" },
                starved.join("; "),
                available.join(", "),
                fetch.join(", ")
            ));
        }
        if wanted.len() > MAX_FETCH {
            // Never truncate silently: a capped fetch that says nothing reads as
            // "that is all there was".
            warnings.push(format!(
                "--fetch matched {} sections; only the first {MAX_FETCH} were fetched. \
                 Narrow with --fund or fewer keys.",
                wanted.len()
            ));
        }

        let out = record! {
            "accession" => Value::string(&accession, span),
            "filing_date" => Value::string(&filed, span),
            "form" => Value::string(&form, span),
            "instance" => sections.instance.as_ref()
                .map(|i| Value::string(i, span))
                .unwrap_or_else(|| Value::nothing(span)),
            "funds" => Value::list(fund_rows, span),
            // Series id -> { risks, benchmarks }. `null` when --tagged was not passed;
            // an empty record when it was and the filing tags nothing — the same
            // "null = you didn't ask, empty = you asked and there is none" distinction
            // section `text` already makes.
            "tagged" => match &instance_tags {
                None => Value::nothing(span),
                Some(t) => {
                    let mut rec = nu_protocol::Record::new();
                    let series: std::collections::BTreeSet<&String> =
                        t.risks.keys().chain(t.benchmarks.keys()).collect();
                    for s in series {
                        rec.push(
                            s.clone(),
                            Value::record(
                                record! {
                                    "risks" => members_value(t.risks.get(s), span),
                                    "benchmarks" => members_value(t.benchmarks.get(s), span),
                                },
                                span,
                            ),
                        );
                    }
                    Value::record(rec, span)
                }
            },
            // Benchmarks the instance attributes to no series. Kept APART from `tagged`
            // rather than copied into every fund: a trust-wide list spread across funds
            // would report each fund as measured against every other fund's index.
            "unattributed_benchmarks" => match &instance_tags {
                None => Value::nothing(span),
                Some(t) => members_value(Some(&t.unattributed_benchmarks), span),
            },
            "warnings" => Value::list(
                warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        };
        Ok(PipelineData::Value(Value::record(out, span), None))
    }
}

/// A tagged-member list as a nushell table of `{ member, name }`.
///
/// `member` rides alongside `name` because the two answer different questions: a consumer
/// matching a tagged risk against a portfolio matches on the STABLE QName, while a consumer
/// rendering it to a reader wants the name. Returning only the name would force the first
/// consumer to match on prose that varies with which rung of the label ladder supplied it.
fn members_value(members: Option<&Vec<TaggedMember>>, span: Span) -> Value {
    Value::list(
        members
            .map(|v| {
                v.iter()
                    .map(|m| {
                        Value::record(
                            record! {
                                "member" => Value::string(&m.member, span),
                                "name" => Value::string(&m.name, span),
                            },
                            span,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        span,
    )
}

/// Sections whose rendered text was requested AND which the instance could not supply, as
/// `(fund_index, html_file)`.
///
/// The instance filter is what turns `--fetch` from up to `MAX_FETCH` requests into zero
/// extra ones on a filing whose prose is fully tagged: only the numeric-table sections
/// (`ShareholderFeesData`, `OperatingExpensesData`, `PerformanceTableData`) still need their
/// rendered fragment, because a fee table is not prose. See [`crate::edgar_fund_text`].
fn plan_fetches(
    sections: &FilingSections,
    keep: Option<u32>,
    fetch: &[String],
    text: &InstanceText,
    wanted: Option<&str>,
) -> Vec<(u32, String)> {
    if fetch.is_empty() {
        return Vec::new();
    }
    sections
        .sections
        .iter()
        .filter(|s| keep.is_none() || s.fund_index == keep)
        .filter(|s| fetch.iter().any(|k| k.eq_ignore_ascii_case(&s.section_key)))
        .filter(|s| instance_text_for(text, s, wanted).is_none())
        // A section with no fragment has nothing to fetch — see `SectionRef::html_file`.
        .filter_map(|s| Some((s.fund_index.unwrap_or(0), s.html_file.clone()?)))
        .collect()
}

/// A section's text: the filing's own tagged prose first, its rendered fragment second.
///
/// Both are the same disclosure. The fragment arrives wrapped in the SEC viewer's banner, a
/// version stamp, the renderer's JavaScript source and double-escaped heading markup, so it
/// is the fallback rather than the source — but it stays the ONLY usable source for the
/// numeric tables, which the instance holds as bare numbers.
///
/// Gated on `--fetch` even though the instance is parsed whole: one parse yields every
/// section, and returning the ones nobody asked for would quietly turn a two-key request into
/// an eleven-section payload.
///
/// One function rather than two so the emitted rows and the "this fund got nothing" warning
/// cannot disagree about what counts as text.
fn section_text<'a>(
    fetch: &[String],
    text: &'a InstanceText,
    bodies: &'a std::collections::BTreeMap<String, String>,
    s: &'a SectionRef,
    wanted: Option<&str>,
) -> Option<&'a str> {
    if !fetch.iter().any(|k| k.eq_ignore_ascii_case(&s.section_key)) {
        return None;
    }
    instance_text_for(text, s, wanted).or_else(|| {
        s.html_file
            .as_ref()
            .and_then(|f| bodies.get(f))
            .map(String::as_str)
    })
}

/// The prose the filing's own instance tagged for this section, if any.
///
/// Keyed by series because that is what the instance dimensions its facts by; a section whose
/// fund could not be identified by series has no instance text by construction, and falls
/// back to its fragment.
fn instance_text_for<'a>(
    text: &'a InstanceText,
    section: &SectionRef,
    wanted: Option<&str>,
) -> Option<&'a str> {
    // AN EXACT `--fund` OUTRANKS THE ROLE'S OWN ATTRIBUTION, and this ordering is the fix for
    // two measured failures at once. A role URI can name the wrong fund — a filer writing a
    // DATE where the fund ordinal belongs collapses every role onto one series — or no fund at
    // all, which is the shape a filing gets when its roles are the SEC's standard ones and
    // reach every fund in the trust. In both, the role cannot say which fund this is. The
    // caller already did, exactly, and a series id cannot be ambiguous.
    if let Some(w) = wanted
        && let Some(t) = text.get(w, &section.section_key)
    {
        return Some(t);
    }
    text.get(section.series_id.as_deref()?, &section.section_key)
}
