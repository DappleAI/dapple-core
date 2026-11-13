//! The `edgar exhibits` command — enumerate a filing's exhibits.
//!
//! `edgar filings` returns a filing's PRIMARY document. Exhibits (the charter and
//! bylaws in EX-3.x, the material contracts in EX-10.x, and so on) are separate
//! documents inside the filing's accession directory. This command lists them from
//! the accession file index (edgarkit `filing_directory`) and, for the type
//! prefixes named in `--fetch-types`, also downloads the exhibit body.
//!
//! The intended split (and what the run-the-10k guide's Ownership and Control pass
//! consumes): list the material contracts (`--types EX-10`) as an index without
//! bodies, and fetch the charter/bylaws (`--fetch-types EX-3`) so their
//! anti-takeover provisions can be read. Deep-parsing each contract is deliberately
//! out of scope; the index is the deliverable for those.
//!
//! The SEC file index carries each document's `type` (EX-10.1, EX-3.2, ...) and
//! filename, but not a prose description, so `description` is the filename and the
//! `exhibit_type` is the load-bearing identifier. A richer description would need
//! the filing index page; that is a future enhancement, not needed here.
//!
//! `--years N` scans the newest N filings of `--form` (annual cadence). A filing
//! whose directory cannot be listed is skipped, not fatal — the command returns the
//! exhibits it could enumerate.

use edgarkit::{FilingOperations, FilingOptions};
use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{build_client, edgar_error, resolve_cik};
use crate::edgar_sections::{html_to_text, strip_ix_header, truncate_chars};

const DEFAULT_YEARS: usize = 3;
const MAX_YEARS: usize = 10;
const DEFAULT_MAX_CHARS: usize = 40_000;
const DEFAULT_FORM: &str = "10-K";
const DEFAULT_TYPES: &str = "EX-3,EX-10";

#[derive(Clone)]
pub struct EdgarExhibits;

impl Command for EdgarExhibits {
    fn name(&self) -> &str {
        "edgar exhibits"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar exhibits")
            .input_output_types(vec![
                (Type::Nothing, Type::table()),
                (Type::Nothing, Type::list(Type::record())),
            ])
            .required(
                "company",
                SyntaxShape::String,
                "Ticker symbol (AAPL) or numeric CIK",
            )
            .named(
                "form",
                SyntaxShape::String,
                "SEC form whose exhibits to enumerate (default: 10-K)",
                Some('f'),
            )
            .named(
                "years",
                SyntaxShape::Int,
                "Newest N filings of the form to scan (default: 3, max: 10)",
                Some('y'),
            )
            .named(
                "types",
                SyntaxShape::String,
                "Comma-separated exhibit-type prefixes to list, e.g. \"EX-3,EX-10\" (default: EX-3,EX-10)",
                Some('t'),
            )
            .named(
                "fetch-types",
                SyntaxShape::String,
                "Exhibit-type prefixes whose body to download, e.g. \"EX-3\" (default: none — index only)",
                None,
            )
            .named(
                "max-chars",
                SyntaxShape::Int,
                "Per-exhibit body truncation in chars; 0 = unlimited (default: 40000)",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "List a filing's exhibits (bylaws EX-3.x, material contracts EX-10.x); optionally fetch bodies."
    }

    fn extra_description(&self) -> &str {
        r#"Resolves the company (ticker -> CIK), scans the newest N filings of the
requested form, and returns one record per matching exhibit:

  form, fiscal_year, filing_date, accession, exhibit_type, description,
  document, url, text

`exhibit_type` is the SEC file-index type (EX-10.1, EX-3.2, ...); `document` and
`description` are the exhibit's filename (the index carries no prose
description); `url` is the sec.gov address of the exhibit document. `text` is the
flattened body, populated ONLY for exhibits whose type prefix-matches
--fetch-types (and only for htm/html/txt documents); it is empty otherwise, so
material contracts list without the cost of downloading each one.

--types filters which exhibit-type prefixes are listed (default EX-3,EX-10).
--fetch-types is the subset whose bodies are downloaded (default none). Matching
is a case-insensitive prefix compare, so "EX-10" matches EX-10.1, EX-10.2, ...

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info, e.g. "dapple ops@example.com").

Examples:
  # The bylaws body plus the material-contract index from the latest 10-Ks
  edgar exhibits AAPL --types "EX-3,EX-10" --fetch-types "EX-3"

  # Just list every exhibit on the newest 10-K
  edgar exhibits MSFT --years 1 --types "EX"
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec![
            "edgar", "sec", "exhibit", "bylaws", "charter", "contract", "10-K",
        ]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Bylaws body plus the material-contract index",
            example: r#"edgar exhibits AAPL --types "EX-3,EX-10" --fetch-types "EX-3""#,
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

        let company: String = call.req(engine_state, stack, 0)?;
        if company.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty company".into(),
                msg: "Pass a ticker symbol or numeric CIK".into(),
                span: Some(span),
                help: None,
                inner: vec![],
            });
        }

        let years: usize = call
            .get_flag::<i64>(engine_state, stack, "years")?
            .map(|y| y.clamp(1, MAX_YEARS as i64) as usize)
            .unwrap_or(DEFAULT_YEARS);
        let form: String = call
            .get_flag::<String>(engine_state, stack, "form")?
            .map(|f| f.trim().to_ascii_uppercase())
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| DEFAULT_FORM.to_string());
        let types = parse_type_prefixes(
            call.get_flag::<String>(engine_state, stack, "types")?
                .as_deref()
                .unwrap_or(DEFAULT_TYPES),
        );
        let fetch_types = parse_type_prefixes(
            call.get_flag::<String>(engine_state, stack, "fetch-types")?
                .as_deref()
                .unwrap_or(""),
        );
        let max_chars: usize = call
            .get_flag::<i64>(engine_state, stack, "max-chars")?
            .map(|c| c.max(0) as usize)
            .unwrap_or(DEFAULT_MAX_CHARS);

        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let records: Vec<Value> = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let cik = resolve_cik(&edgar, &company)
                    .await
                    .map_err(|e| edgar_error(&format!("resolve '{company}'"), &e, span))?;
                let cik_str = cik.to_string();

                let submission = edgar
                    .submissions(&cik_str)
                    .await
                    .map_err(|e| edgar_error(&format!("list filings for CIK {cik}"), &e, span))?;

                let filings = submission.filings(Some(
                    FilingOptions::new()
                        .with_form_type(form.clone())
                        .with_include_amendments(false),
                ));
                if filings.is_empty() {
                    return Err(ShellError::GenericError {
                        error: format!("No {form} filings"),
                        msg: format!("CIK {cik} has no {form} filings on EDGAR"),
                        span: Some(span),
                        help: Some(
                            "Check the --form spelling against the company's EDGAR filing index."
                                .into(),
                        ),
                        inner: vec![],
                    });
                }

                let mut records: Vec<Value> = Vec::new();
                for filing in filings.into_iter().take(years) {
                    let dir = match edgar
                        .filing_directory(&cik_str, &filing.accession_number)
                        .await
                    {
                        Ok(dir) => dir,
                        Err(e) => {
                            // A single filing's index failure is not fatal — the caller
                            // still gets the exhibits from the filings that did list.
                            log::warn!(
                                "edgar exhibits {company}: directory for {} failed: {e}",
                                filing.accession_number
                            );
                            continue;
                        }
                    };

                    let fiscal_year = filing
                        .report_date
                        .as_deref()
                        .or(Some(filing.filing_date.as_str()))
                        .and_then(|d| d.get(..4))
                        .and_then(|y| y.parse::<i64>().ok());

                    for item in &dir.directory.item {
                        if !type_matches(&item.type_, &types) {
                            continue;
                        }
                        let url = filing_url(&cik_str, &filing.accession_number, &item.name);
                        // Fetch the body only for the fetch-type subset, and only for a
                        // text document (an image or PDF exhibit can't be flattened).
                        let text =
                            if type_matches(&item.type_, &fetch_types) && is_readable(&item.name) {
                                match edgar
                                    .get_filing_content_by_id(
                                        &cik_str,
                                        &format!("{}:{}", filing.accession_number, item.name),
                                    )
                                    .await
                                {
                                    Ok(html) => {
                                        let body = html_to_text(&strip_ix_header(&html));
                                        truncate_chars(&body, max_chars).0
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "edgar exhibits {company}: fetch {} failed: {e}",
                                            item.name
                                        );
                                        String::new()
                                    }
                                }
                            } else {
                                String::new()
                            };

                        records.push(Value::record(
                            record! {
                                "form" => Value::string(filing.form.clone(), span),
                                "fiscal_year" => fiscal_year
                                    .map(|y| Value::int(y, span))
                                    .unwrap_or_else(|| Value::nothing(span)),
                                "filing_date" => Value::string(filing.filing_date.clone(), span),
                                "accession" => Value::string(filing.accession_number.clone(), span),
                                "exhibit_type" => Value::string(item.type_.clone(), span),
                                // The file index carries no prose description; the filename is
                                // the best available locator, and exhibit_type is the identifier.
                                "description" => Value::string(item.name.clone(), span),
                                "document" => Value::string(item.name.clone(), span),
                                "url" => Value::string(url, span),
                                "text" => Value::string(text, span),
                            },
                            span,
                        ));
                    }
                }
                Ok(records)
            })
        })?;

        Ok(Value::list(records, span).into_pipeline_data())
    }
}

/// Parse a comma-separated list of exhibit-type prefixes, uppercased and trimmed.
/// An empty/blank input yields an empty list (matches nothing).
fn parse_type_prefixes(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|t| t.trim().to_ascii_uppercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Does an exhibit's SEC file-index `type` prefix-match any requested prefix?
/// Case-insensitive, so "EX-10" matches EX-10.1, EX-10.2, ... An empty prefix
/// list never matches (so `--fetch-types` unset downloads nothing).
fn type_matches(type_: &str, prefixes: &[String]) -> bool {
    let t = type_.to_ascii_uppercase();
    prefixes.iter().any(|p| t.starts_with(p.as_str()))
}

/// A text document whose body can be flattened (htm/html/txt); an image or PDF
/// exhibit is listed but never fetched for text.
fn is_readable(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".htm") || l.ends_with(".html") || l.ends_with(".txt")
}

/// The sec.gov URL of an exhibit document — the citable source link. Same format
/// as `edgar filings` (accession de-dashed in the path segment, filename as-is).
fn filing_url(cik: &str, accession: &str, document: &str) -> String {
    format!(
        "https://www.sec.gov/Archives/edgar/data/{cik}/{}/{document}",
        accession.replace('-', "")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        assert_eq!(EdgarExhibits.name(), "edgar exhibits");
    }

    #[test]
    fn test_signature_has_flags() {
        // The run-the-10k guide parses against exactly this signature at ingest:
        // `edgar exhibits $ticker --form "10-K" --years N --types "..." --fetch-types "..."`.
        let sig = EdgarExhibits.signature();
        let flag_names: Vec<&str> = sig.named.iter().map(|f| f.long.as_str()).collect();
        for expected in ["form", "years", "types", "fetch-types", "max-chars"] {
            assert!(flag_names.contains(&expected), "missing --{expected}");
        }
    }

    #[test]
    fn parse_prefixes_trims_and_uppercases() {
        assert_eq!(parse_type_prefixes("ex-3, ex-10 "), vec!["EX-3", "EX-10"]);
        assert!(parse_type_prefixes("").is_empty());
        assert!(parse_type_prefixes("  ,  ").is_empty());
    }

    #[test]
    fn type_matches_is_case_insensitive_prefix() {
        let prefixes = parse_type_prefixes("EX-3,EX-10");
        assert!(type_matches("EX-10.1", &prefixes));
        assert!(type_matches("ex-10.14", &prefixes));
        assert!(type_matches("EX-3.2", &prefixes));
        assert!(!type_matches("EX-99.1", &prefixes));
        assert!(!type_matches("10-K", &prefixes));
        // An empty prefix list (the default --fetch-types) matches nothing.
        assert!(!type_matches("EX-3.2", &[]));
    }

    #[test]
    fn readable_only_text_documents() {
        assert!(is_readable("bylaws.htm"));
        assert!(is_readable("EX3-2.HTML"));
        assert!(is_readable("contract.txt"));
        assert!(!is_readable("scan.pdf"));
        assert!(!is_readable("logo.jpg"));
    }

    #[test]
    fn exhibit_url_matches_edgar_format() {
        assert_eq!(
            filing_url("320193", "0000320193-25-000079", "ex-3-2_bylaws.htm"),
            "https://www.sec.gov/Archives/edgar/data/320193/000032019325000079/ex-3-2_bylaws.htm"
        );
    }
}
