//! The `edgar fund` command — resolve a fund identifier and classify the filer.
//!
//! This is the front door of the `retort-the-n-port` guide. It answers two
//! questions in one round trip:
//!
//! 1. **Which fund is this?** A ticker, CIK, `S000######` or `C000######` all
//!    resolve to `{trust_cik, series, classes}`. A bare trust CIK is ambiguous by
//!    construction (one trust holds many funds, one N-PORT covers one), so it
//!    returns every series for the caller to offer as a picker rather than
//!    guessing.
//! 2. **Is it in scope?** Judged by the forms the CIK actually files, not by the
//!    identifier. Anything that does not file N-PORT fails loud and **names the
//!    guide that does handle it** — an operator who typed a ticker deserves a
//!    destination, not just a refusal.
//!
//! The submissions fetch that drives classification is one the pull needs
//! anyway, so scope enforcement is effectively free.

use edgarkit::Edgar;
use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error, not_configured, resolve_cik};
use crate::edgar_fund_registry::{
    FundIdentifier, FundResolution, FundStructure, MF_TICKERS_URL, MfTickerRow, RegistryParse,
    SERIES_CLASS_REGISTRY_URL, classify_filer, classify_identifier, parse_mf_tickers,
    parse_series_class_csv, resolve, resolve_tail, series_ordinal,
};

/// Process-local memo for the ~8 MB series/class registry.
///
/// One fetch per flow process rather than per call. On a race both writers
/// parsed the same bytes, so first-wins is safe and needs no lock.
static REGISTRY: std::sync::OnceLock<RegistryParse> = std::sync::OnceLock::new();

/// Process-local memo for the tail source (`company_tickers_mf.json`, ~1.2 MB).
///
/// Fetched **lazily** — only when the registry snapshot misses — so the common
/// case still costs one request.
static MF_TICKERS: std::sync::OnceLock<Vec<MfTickerRow>> = std::sync::OnceLock::new();

#[allow(clippy::result_large_err)]
async fn load_registry(edgar: &Edgar, span: Span) -> Result<&'static RegistryParse, ShellError> {
    if let Some(cached) = REGISTRY.get() {
        return Ok(cached);
    }
    let text = edgar
        .get(SERIES_CLASS_REGISTRY_URL)
        .await
        .map_err(|e| edgar_error("fetch the SEC series/class registry", &e, span))?;
    let parsed = parse_series_class_csv(&text).map_err(|msg| ShellError::GenericError {
        error: "Series/class registry unreadable".into(),
        msg,
        span: Some(span),
        help: Some(format!(
            "The SEC may have changed the registry layout. Source: {SERIES_CLASS_REGISTRY_URL}"
        )),
        inner: vec![],
    })?;
    let _ = REGISTRY.set(parsed);
    Ok(REGISTRY.get().expect("registry set above"))
}

/// Load the tail source: funds registered after the pinned snapshot was cut.
#[allow(clippy::result_large_err)]
async fn load_mf_tickers(
    edgar: &Edgar,
    span: Span,
) -> Result<&'static Vec<MfTickerRow>, ShellError> {
    if let Some(cached) = MF_TICKERS.get() {
        return Ok(cached);
    }
    let text = edgar
        .get(MF_TICKERS_URL)
        .await
        .map_err(|e| edgar_error("fetch the SEC fund ticker map", &e, span))?;
    let (rows, skipped) = parse_mf_tickers(&text).map_err(|msg| ShellError::GenericError {
        error: "Fund ticker map unreadable".into(),
        msg,
        span: Some(span),
        help: Some(format!(
            "The SEC may have changed the file's shape. Source: {MF_TICKERS_URL}"
        )),
        inner: vec![],
    })?;
    if skipped > 0 {
        log::warn!("fund ticker map: {skipped} unusable rows skipped");
    }
    let _ = MF_TICKERS.set(rows);
    Ok(MF_TICKERS.get().expect("tickers set above"))
}

/// Error for a fund the pinned snapshot is simply too old to know about.
///
/// The distinction this preserves is the whole point: a miss **below** the
/// ceiling means the fund does not exist, while a miss **above** it means our
/// snapshot predates the fund. Collapsing them is what let a fund query fall
/// through to a company lookup and answer confidently about a different entity.
#[allow(clippy::result_large_err)]
fn stale_snapshot_error(id: &str, ceiling: Option<u64>, span: Span) -> ShellError {
    let ceiling_note = ceiling.map_or_else(
        || "the pinned registry snapshot".to_string(),
        |c| format!("the pinned registry snapshot (reaches S{c:09})"),
    );
    ShellError::GenericError {
        error: format!("`{id}` is newer than the fund registry"),
        msg: format!("{id} is not in {ceiling_note}, and it is not in the fund ticker map either"),
        span: Some(span),
        help: Some(
            "The SEC publishes this registry as periodic snapshots. If this fund is real \
             and very recently registered, it may not be in any published source yet — \
             try its trust CIK instead."
                .into(),
        ),
        inner: vec![],
    }
}

/// Distinct form types in a CIK's recent submissions, plus the filer's name.
///
/// The recent block is deliberately enough **for a recurring form**: a filer's
/// *kind* is evident from what it files now, so this does not pay for the
/// older-segment traversal that `edgar filings` needs for historical coverage.
/// The name comes along because a unit investment trust has no registry row to
/// take one from.
///
/// ⚠️ It is NOT enough for a one-time filing, and
/// [`crate::edgar_fund_registry::BDC_RECURRING_MARKERS`] documents where that
/// bit: a BDC's `N-54A` election scrolls out of the block, so eight of eleven
/// live BDCs were classified as operating companies until the rule stopped
/// depending on it. Any future kind keyed on a once-ever form needs the same
/// treatment, not a deeper traversal.
///
/// Shared with `edgar bdc`, which asks the same question of the same feed.
pub(crate) async fn recent_forms(
    edgar: &Edgar,
    cik: u64,
    span: Span,
) -> Result<(Vec<String>, String), ShellError> {
    let url = format!("{}/submissions/CIK{:0>10}.json", edgar.data_url(), cik);
    let text = edgar
        .get(&url)
        .await
        .map_err(|e| edgar_error(&format!("list filings for CIK {cik}"), &e, span))?;
    let root: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| ShellError::GenericError {
            error: "Unreadable submissions payload".into(),
            msg: format!("CIK {cik}: {e}"),
            span: Some(span),
            help: None,
            inner: vec![],
        })?;
    let forms = root
        .pointer("/filings/recent/form")
        .and_then(|v| v.as_array())
        .map(|forms| {
            forms
                .iter()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let name = root
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((forms, name))
}

#[derive(Clone)]
pub struct EdgarFund;

impl Command for EdgarFund {
    fn name(&self) -> &str {
        "edgar fund"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar fund")
            .input_output_types(vec![(Type::Nothing, Type::record())])
            .required(
                "identifier",
                SyntaxShape::String,
                "Fund ticker (VTSAX), trust CIK, series id (S000002848) or class id (C000007806)",
            )
            // The scope check used to be hard-wired to N-PORT filers, which is
            // one guide's rule living in a shared command: it refuses a money
            // market fund outright, and `price-the-mfp` reads nothing else.
            // Omitting the flag preserves that behaviour exactly.
            .named(
                "accept",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Filer kinds this caller can read, e.g. [MoneyMarketFund]. \
                 Default: N-PORT filers (OpenEndFund, ClosedEndFund)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Resolve a fund identifier to its trust/series/classes and classify the filer."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a record:
  { trust_cik, entity_name, matched_on, pinned, series_name_provisional,
    kind, kind_label, in_scope, redirect_guide, signals, series,
    registry_skipped, warnings }

`series` is a table of { series_id, series_name, classes }, where each class is
{ class_id, class_name, ticker }. Classes legitimately have no ticker
(institutional and retirement classes), so `ticker` may be null.

The SEC's series/class registry is a periodic snapshot, so a fund registered
after it was cut is resolved from the fund ticker map instead. That map carries
identity but no names, so `series_name_provisional` is true and `series_name`
holds the series id as a placeholder — the real name comes from the N-PORT's own
genInfo on the next call. A bare trust CIK is never resolved this way, because a
picker without names would be unusable.

`structure` is "series" or "unitary" and decides how N-PORT is selected:
  series  — one trust, many funds; N-PORT is filed per series, key on series_id
  unitary — a unit investment trust (SPY, QQQ, DIA); the trust IS the portfolio,
            N-PORT is filed per CIK and `series` is empty

`pinned` is true when the identifier named exactly one portfolio. A bare trust
CIK returns every series with pinned=false — offer the operator a picker; it is
not an error. A series fund is not a CIK: one trust holds many funds and one
N-PORT covers exactly one, so the series id is what downstream calls need. A
unitary trust is always pinned; its empty `series` list is completeness, not
ambiguity.

Filers that do not file N-PORT are a hard error naming the guide that does
handle them (money market funds, 13F managers, BDCs, operating companies).

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # A share-class ticker pins one fund
  edgar fund VTSAX | get series.0.series_id

  # A trust CIK returns the picker
  edgar fund 36405 | get series | select series_id series_name
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "fund", "series", "nport", "mutual"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Resolve a fund by share-class ticker",
                example: r#"edgar fund VTSAX"#,
                result: None,
            },
            Example {
                description: "List every fund in a trust",
                example: r#"edgar fund 36405 | get series | select series_id series_name"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let raw: String = call.req(engine_state, stack, 0)?;
        let accept: Option<Vec<String>> = call.get_flag(engine_state, stack, "accept")?;

        let identifier = classify_identifier(&raw)
            .ok_or_else(|| not_configured("empty fund identifier".to_string(), span))?;

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let (resolution, registry_skipped, warnings, cik, from_tail) =
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let registry = load_registry(&edgar, span).await?;
                    let ceiling = registry.max_series_ordinal;

                    // The registry is a periodic snapshot, so a miss here does not
                    // mean the fund is absent — it may simply be newer than the
                    // file. A pinned identifier therefore gets a second look in the
                    // ticker map, which carries no names but is years fresher.
                    //
                    // A bare trust CIK is not retried: it needs names to render a
                    // picker and the tail source has none.
                    let (resolution, from_tail) = match resolve(&registry.rows, &identifier) {
                        Some(found) => (Some(found), false),
                        None => match &identifier {
                            FundIdentifier::Cik(_) => (None, false),
                            _ => {
                                let hit =
                                    resolve_tail(load_mf_tickers(&edgar, span).await?, &identifier);
                                let hit_tail = hit.is_some();
                                (hit, hit_tail)
                            }
                        },
                    };

                    // Still nothing. That is not yet a failure: it may be an
                    // operating-company ticker, in which case the honest answer is
                    // "wrong guide, here is the right one" — which needs a CIK to
                    // classify against.
                    let cik = match &resolution {
                        Some(r) => r.trust_cik,
                        None => match &identifier {
                            FundIdentifier::Cik(c) => *c,
                            FundIdentifier::Ticker(t) => resolve_cik(&edgar, t)
                                .await
                                .map_err(|e| edgar_error(&format!("resolve '{t}'"), &e, span))?,
                            FundIdentifier::SeriesId(id) | FundIdentifier::ClassId(id) => {
                                // Above the snapshot's reach is a different sentence
                                // from "no such fund", and saying the wrong one sends
                                // the operator to check an id that is fine.
                                if series_ordinal(id).zip(ceiling).is_some_and(|(o, c)| o > c) {
                                    return Err(stale_snapshot_error(id, ceiling, span));
                                }
                                return Err(ShellError::GenericError {
                                    error: "Unknown series or class id".into(),
                                    msg: format!("{id} is not in the SEC series/class registry"),
                                    span: Some(span),
                                    help: Some(
                                        "Check the id, or pass the fund's ticker or trust CIK"
                                            .into(),
                                    ),
                                    inner: vec![],
                                });
                            }
                        },
                    };

                    Ok::<_, ShellError>((
                        resolution,
                        registry.skipped,
                        registry.warnings.clone(),
                        cik,
                        from_tail,
                    ))
                })
            })?;

        #[allow(clippy::result_large_err)]
        let (forms, filer_name) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(recent_forms(&edgar, cik, span))
        })?;

        let classification = classify_filer(&forms);

        // Fail loud, and name the destination. A caller that cannot read this
        // kind of filer must not be handed a record it will misread, and
        // silently returning an empty fund record would look like a fund with no
        // holdings.
        //
        // What counts as in scope belongs to the CALLER, not to this command.
        // Without `--accept` the answer is the historical one (N-PORT filers),
        // so every existing call behaves exactly as before.
        let accepted = match &accept {
            None => classification.in_scope(),
            Some(kinds) => kinds.iter().any(|k| classification.kind.matches_key(k)),
        };
        if !accepted {
            let seen = if classification.signals.is_empty() {
                "no recognizable forms".to_string()
            } else {
                classification.signals.join(", ")
            };
            let wanted = match &accept {
                None => "N-PORT filers (open-end funds, ETFs, closed-end funds)".to_string(),
                Some(kinds) => kinds.join(", "),
            };
            return Err(ShellError::GenericError {
                error: format!("`{raw}` is not a filer this caller can read"),
                msg: format!(
                    "CIK {cik} files as {} ({seen})",
                    classification.kind.label()
                ),
                span: Some(span),
                help: Some(match classification.redirect_guide() {
                    Some(guide) => format!(
                        "This caller reads {wanted}. Use the `{guide}` guide for this filer instead."
                    ),
                    None => format!(
                        "This caller reads {wanted}. Nothing in this filer's recent history \
                         identifies it."
                    ),
                }),
                inner: vec![],
            });
        }

        // Files N-PORT but has no series rows: a unit investment trust. Verified
        // live — SPY (CIK 884394) files NPORT-P/N-CSR/N-CEN/485BPOS yet registers
        // on S-6 with no S000/C000 structure, as do QQQ and DIA. The trust IS the
        // portfolio, so this is a complete answer, not a degraded one.
        let mut resolution = resolution.unwrap_or_else(|| {
            FundResolution::unitary(cik, filer_name.clone(), identifier.clone())
        });

        // A tail resolution carries identity without a name, because the ticker
        // map has none. The submissions feed just handed us the trust's own name
        // for this exact CIK, so use it rather than emitting an empty string.
        if resolution.entity_name.is_empty() {
            resolution.entity_name = filer_name.clone();
        }

        let series: Vec<Value> = resolution
            .series
            .iter()
            .map(|s| {
                let classes: Vec<Value> = s
                    .classes
                    .iter()
                    .map(|c| {
                        Value::record(
                            record! {
                                "class_id" => Value::string(&c.class_id, span),
                                "class_name" => c.class_name.as_ref()
                                    .map(|n| Value::string(n, span))
                                    .unwrap_or_else(|| Value::nothing(span)),
                                "ticker" => c.ticker.as_ref()
                                    .map(|t| Value::string(t, span))
                                    .unwrap_or_else(|| Value::nothing(span)),
                            },
                            span,
                        )
                    })
                    .collect();
                Value::record(
                    record! {
                        "series_id" => Value::string(&s.series_id, span),
                        "series_name" => Value::string(&s.series_name, span),
                        "classes" => Value::list(classes, span),
                    },
                    span,
                )
            })
            .collect();

        let matched_on = match &resolution.matched_on {
            FundIdentifier::Ticker(_) => "ticker",
            FundIdentifier::Cik(_) => "cik",
            FundIdentifier::SeriesId(_) => "series_id",
            FundIdentifier::ClassId(_) => "class_id",
        };

        let out = record! {
            "trust_cik" => Value::int(resolution.trust_cik as i64, span),
            "entity_name" => Value::string(&resolution.entity_name, span),
            "matched_on" => Value::string(matched_on, span),
            "structure" => Value::string(
                match resolution.structure {
                    FundStructure::Series => "series",
                    FundStructure::Unitary => "unitary",
                },
                span,
            ),
            "pinned" => Value::bool(resolution.is_pinned(), span),
            // True when the fund was newer than the registry snapshot and had to
            // come from the ticker map, which carries no names. `series_name` is
            // then the series id standing in for one; the real name arrives in
            // the N-PORT's own `genInfo`.
            "series_name_provisional" => Value::bool(from_tail, span),
            "kind" => Value::string(format!("{:?}", classification.kind), span),
            "kind_label" => Value::string(classification.kind.label(), span),
            "in_scope" => Value::bool(classification.in_scope(), span),
            "redirect_guide" => classification.redirect_guide()
                .map(|g| Value::string(g, span))
                .unwrap_or_else(|| Value::nothing(span)),
            "signals" => Value::list(
                classification.signals.iter().map(|s| Value::string(s, span)).collect(),
                span,
            ),
            "series" => Value::list(series, span),
            "registry_skipped" => Value::int(registry_skipped as i64, span),
            "warnings" => Value::list(
                warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        };

        Ok(PipelineData::Value(Value::record(out, span), None))
    }
}
