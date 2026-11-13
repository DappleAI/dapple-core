//! The `edgar facts` command — XBRL company facts (numeric line items) from
//! SEC EDGAR. XBRL facts are inherently multi-year, so year-over-year numeric
//! rows come from a single request.
//!
//! Without `--concepts` the command returns a bounded INDEX of available
//! concepts (taxonomy/tag/label/units) — never the full multi-MB dump. With
//! `--concepts` it returns the value rows for exactly those tags.

use edgarkit::{CompanyConcept, CompanyOperations};
use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error, resolve_cik};
use crate::engine::json_to_nu_value;

/// The IFRS taxonomy name as EDGAR spells it. Foreign private issuers filing a
/// 20-F report under IFRS tag against this rather than `us-gaap`, and
/// `edgarkit`'s `TaxonomyGroups` does not model it, so the bulk `company_facts`
/// call cannot see it at all.
const IFRS_TAXONOMY: &str = "ifrs-full";

/// Split a requested concept into an optional taxonomy hint and its bare tag:
/// `"us-gaap:Assets"` -> `(Some("us-gaap"), "Assets")`, `"Assets"` -> `(None, "Assets")`.
/// XBRL facts are keyed by bare tag (the taxonomy is the group the tag lives in, not part
/// of the key), so a qualified `taxonomy:tag` qname must have its prefix peeled off before
/// lookup — otherwise every prefixed concept misses.
fn parse_concept(concept: &str) -> (Option<&str>, &str) {
    match concept.split_once(':') {
        Some((taxonomy, tag)) => (Some(taxonomy), tag),
        None => (None, concept),
    }
}

/// Whether a `(group_taxonomy, tag)` map entry satisfies a requested concept. The bare tag
/// must match case-insensitively; when the request carried a `taxonomy:` hint, the group's
/// taxonomy must match it too (case-insensitively). No hint -> any group is eligible.
fn concept_key_matches(group_taxonomy: &str, tag: &str, hint: Option<&str>, bare: &str) -> bool {
    if !tag.eq_ignore_ascii_case(bare) {
        return false;
    }
    match hint {
        Some(h) => group_taxonomy.eq_ignore_ascii_case(h),
        None => true,
    }
}

#[derive(Clone)]
pub struct EdgarFacts;

impl Command for EdgarFacts {
    fn name(&self) -> &str {
        "edgar facts"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar facts")
            .input_output_types(vec![(Type::Nothing, Type::record())])
            .required(
                "company",
                SyntaxShape::String,
                "Ticker symbol (AAPL) or numeric CIK",
            )
            .named(
                "concepts",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "us-gaap/dei concept tags to fetch values for (e.g. [Revenues NetIncomeLoss]); \
                 without this flag only the concept index is returned",
                Some('c'),
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fetch a company's XBRL facts (numeric financial line items) from SEC EDGAR."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a record { cik, entity_name, facts, missing }.

Without --concepts, `facts` is an index table:
  taxonomy, tag, label, units

With --concepts, `facts` holds the value rows for the requested tags:
  taxonomy, tag, label, unit, start, end, val, accn, fy, fp, form, filed
`missing` lists requested concepts that the company has never reported.

Filter to annual values in nu: `... | get facts | where fp == "FY" and form == "10-K"`.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # What concepts does the company report?
  edgar facts AAPL | get facts | where tag =~ Revenue

  # Three-year revenue and net income
  edgar facts AAPL --concepts [Revenues NetIncomeLoss]
    | get facts | where fp == "FY" and form == "10-K"
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "xbrl", "facts", "financials"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Index of reported concepts",
                example: r#"edgar facts AAPL"#,
                result: None,
            },
            Example {
                description: "Annual revenue values",
                example: r#"edgar facts AAPL --concepts [Revenues] | get facts | where fp == "FY""#,
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

        let company: String = call.req(engine_state, stack, 0)?;
        let concepts: Option<Vec<String>> = call
            .get_flag::<Vec<Value>>(engine_state, stack, "concepts")?
            .map(|vals| {
                vals.into_iter()
                    .filter_map(|v| v.coerce_into_string().ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            });

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let facts = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let cik = resolve_cik(&edgar, &company)
                    .await
                    .map_err(|e| edgar_error(&format!("resolve '{company}'"), &e, span))?;
                edgar
                    .company_facts(cik)
                    .await
                    .map_err(|e| edgar_error(&format!("fetch facts for CIK {cik}"), &e, span))
            })
        })?;

        // `edgarkit` doesn't export its `Fact` type, so the taxonomy groups stay
        // behind inference: each entry is (name, &HashMap<String, Fact>).
        let taxonomies = [
            ("us-gaap", &facts.taxonomies.us_gaap),
            ("dei", &facts.taxonomies.dei),
        ];

        let mut rows: Vec<Value> = Vec::new();
        let mut missing: Vec<String> = Vec::new();

        match &concepts {
            None => {
                // Bounded index: one row per concept, values omitted.
                let mut index: Vec<(String, String, String, Vec<String>)> = Vec::new();
                for (taxonomy, group) in taxonomies {
                    for (tag, fact) in group {
                        let mut units: Vec<String> = fact.units.keys().cloned().collect();
                        units.sort();
                        index.push((
                            taxonomy.to_string(),
                            tag.clone(),
                            fact.label.clone().unwrap_or_default(),
                            units,
                        ));
                    }
                }
                index.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
                for (taxonomy, tag, label, units) in index {
                    rows.push(Value::record(
                        record! {
                            "taxonomy" => Value::string(taxonomy, span),
                            "tag" => Value::string(tag, span),
                            "label" => Value::string(label, span),
                            "units" => Value::list(
                                units.into_iter().map(|u| Value::string(u, span)).collect(),
                                span,
                            ),
                        },
                        span,
                    ));
                }
            }
            Some(requested) => {
                for concept in requested {
                    // Accept both bare (`Assets`) and qualified (`us-gaap:Assets`) concepts:
                    // peel any taxonomy prefix, then match the bare tag — exact first, then
                    // case-insensitive — honoring the taxonomy hint when one was given.
                    let (hint, bare) = parse_concept(concept);
                    let hit = taxonomies
                        .iter()
                        .find_map(|(taxonomy, group)| {
                            let taxonomy_ok = match hint {
                                Some(h) => taxonomy.eq_ignore_ascii_case(h),
                                None => true,
                            };
                            if taxonomy_ok {
                                group.get(bare).map(|f| (*taxonomy, bare.to_string(), f))
                            } else {
                                None
                            }
                        })
                        .or_else(|| {
                            taxonomies.iter().find_map(|(taxonomy, group)| {
                                group
                                    .iter()
                                    .find(|(tag, _)| {
                                        concept_key_matches(taxonomy, tag.as_str(), hint, bare)
                                    })
                                    .map(|(tag, f)| (*taxonomy, tag.clone(), f))
                            })
                        });

                    let Some((taxonomy, tag, fact)) = hit else {
                        missing.push(concept.clone());
                        continue;
                    };

                    let label = fact.label.clone().unwrap_or_default();
                    for (unit, points) in &fact.units {
                        for point in points {
                            rows.push(Value::record(
                                record! {
                                    "taxonomy" => Value::string(taxonomy, span),
                                    "tag" => Value::string(tag.clone(), span),
                                    "label" => Value::string(label.clone(), span),
                                    "unit" => Value::string(unit.clone(), span),
                                    "start" => point.start.clone().map(|s| Value::string(s, span)).unwrap_or_else(|| Value::nothing(span)),
                                    "end" => Value::string(point.end.clone(), span),
                                    "val" => json_to_nu_value(&point.val),
                                    "accn" => Value::string(point.accn.clone(), span),
                                    "fy" => point.fy.map(|y| Value::int(y as i64, span)).unwrap_or_else(|| Value::nothing(span)),
                                    "fp" => point.fp.clone().map(|p| Value::string(p, span)).unwrap_or_else(|| Value::nothing(span)),
                                    "form" => Value::string(point.form.clone(), span),
                                    "filed" => Value::string(point.filed.clone(), span),
                                },
                                span,
                            ));
                        }
                    }
                }
            }
        }

        // === The IFRS fallback, and the silent-drop it exists to fix =============
        //
        // `edgarkit`'s `TaxonomyGroups` declares only `us_gaap` and `dei`. Serde
        // ignores unknown fields, so an IFRS filer's companyfacts payload
        // DESERIALIZES CLEANLY and its entire `ifrs-full` group is dropped on the
        // floor. Every requested concept then lands in `missing` and `facts` comes
        // back empty, which is indistinguishable from a company that tags nothing:
        // a global filer with a full XBRL exhibit reads as reporting no numbers.
        // That is precisely the silent-degrade class the whole report is built to
        // avoid, and it needed a route rather than a warning.
        //
        // `company_concept(cik, taxonomy, tag)` IS taxonomy-parameterized, so it
        // reaches `ifrs-full` without forking the crate. It costs one HTTP call per
        // concept, which is why this is GATED on the bulk pass having produced
        // nothing at all: a US filer keeps its single bulk call and pays zero here.
        // A filer that returned even one us-gaap row is not an IFRS filer and is
        // not retried, so a partial us-gaap match never triggers N calls.
        let mut answered_by = if rows.is_empty() { "none" } else { "us-gaap" };
        if rows.is_empty() && !missing.is_empty() {
            let cik = facts.cik;
            let retry: Vec<(String, Option<CompanyConcept>)> = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let mut out = Vec::new();
                    for concept in &missing {
                        let (_, bare) = parse_concept(concept);
                        // Warn-and-continue per concept: an IFRS filer that
                        // tags 20 of 30 concepts must return the 20 rather
                        // than fail the pull over the 10 it does not tag.
                        let hit = edgar.company_concept(cik, IFRS_TAXONOMY, bare).await.ok();
                        out.push((concept.clone(), hit));
                    }
                    out
                })
            });
            let mut still_missing: Vec<String> = Vec::new();
            for (concept, hit) in retry {
                let Some(cc) = hit else {
                    still_missing.push(concept);
                    continue;
                };
                if cc.units.is_empty() {
                    still_missing.push(concept);
                    continue;
                }
                let label = cc.label.clone().unwrap_or_default();
                for (unit, points) in &cc.units {
                    for point in points {
                        rows.push(Value::record(
                            record! {
                                "taxonomy" => Value::string(cc.taxonomy.clone(), span),
                                "tag" => Value::string(cc.tag.clone(), span),
                                "label" => Value::string(label.clone(), span),
                                "unit" => Value::string(unit.clone(), span),
                                "start" => point.start.clone().map(|s| Value::string(s, span)).unwrap_or_else(|| Value::nothing(span)),
                                "end" => Value::string(point.end.clone(), span),
                                "val" => json_to_nu_value(&point.val),
                                "accn" => Value::string(point.accn.clone(), span),
                                "fy" => point.fy.map(|y| Value::int(y as i64, span)).unwrap_or_else(|| Value::nothing(span)),
                                "fp" => point.fp.clone().map(|p| Value::string(p, span)).unwrap_or_else(|| Value::nothing(span)),
                                "form" => Value::string(point.form.clone(), span),
                                "filed" => Value::string(point.filed.clone(), span),
                            },
                            span,
                        ));
                    }
                }
            }
            if !rows.is_empty() {
                answered_by = IFRS_TAXONOMY;
            }
            missing = still_missing;
        }

        Ok(Value::record(
            record! {
                "cik" => Value::int(facts.cik as i64, span),
                "entity_name" => Value::string(facts.entity_name, span),
                "facts" => Value::list(rows, span),
                "missing" => Value::list(
                    missing.into_iter().map(|m| Value::string(m, span)).collect(),
                    span,
                ),
                // WHICH TAXONOMY ANSWERED, on the `scene_source` pattern. Without
                // this an operator cannot tell an IFRS filer from a broken pull,
                // and the guide cannot say which concept spellings its downstream
                // consumers should have been keyed on.
                "answered_by" => Value::string(answered_by, span),
            },
            span,
        )
        .into_pipeline_data())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarFacts.name(), "edgar facts");
    }

    #[test]
    fn test_command_description() {
        assert!(!EdgarFacts.description().is_empty());
    }

    #[test]
    fn parse_concept_splits_taxonomy_prefix() {
        assert_eq!(parse_concept("us-gaap:Assets"), (Some("us-gaap"), "Assets"));
        assert_eq!(parse_concept("Assets"), (None, "Assets"));
        assert_eq!(
            parse_concept("dei:EntityCommonStockSharesOutstanding"),
            (Some("dei"), "EntityCommonStockSharesOutstanding"),
        );
    }

    #[test]
    fn concept_key_matches_honors_prefix_and_case() {
        // Qualified concept resolves against the right group by bare tag — the bug that
        // marked every `us-gaap:`-prefixed concept missing.
        assert!(concept_key_matches(
            "us-gaap",
            "Assets",
            Some("us-gaap"),
            "Assets"
        ));
        // A mismatched taxonomy hint does not match.
        assert!(!concept_key_matches(
            "dei",
            "Assets",
            Some("us-gaap"),
            "Assets"
        ));
        // No hint: match by bare tag, case-insensitively, in any group.
        assert!(concept_key_matches("us-gaap", "Assets", None, "assets"));
        // The taxonomy hint itself is case-insensitive.
        assert!(concept_key_matches(
            "us-gaap",
            "Assets",
            Some("US-GAAP"),
            "Assets"
        ));
        // A different tag never matches.
        assert!(!concept_key_matches(
            "us-gaap",
            "NetIncomeLoss",
            Some("us-gaap"),
            "Revenues",
        ));
    }
}
