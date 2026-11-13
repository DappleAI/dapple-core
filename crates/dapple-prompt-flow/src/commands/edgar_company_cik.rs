//! The `edgar company-cik` command — resolve a company NAME to an SEC filer.
//!
//! The logic already existed in [`crate::edgar_identity`] for holdings
//! resolution; this exposes it to a guide script for a different question.
//!
//! # Why this is not `edgar resolve`
//!
//! `edgar resolve` is holdings-shaped: it short-circuits on `asset_cat`,
//! `issuer_cat` and the derivative flag before it ever looks anything up,
//! because a Treasury bill must never be searched for as a company. Feeding an
//! adviser's name through it would take one of those early exits. The lookup is
//! the same; the surrounding question is not.
//!
//! # The three outcomes, and why `not_listed` is an ANSWER
//!
//! Asking whether a fund's adviser is itself a public company has a real
//! negative: Vanguard is owned by its own funds, and Fidelity, Capital Group and
//! Dodge & Cox are private. Those firms resolve `not_listed` correctly and
//! permanently, and that is a fact about how the adviser is owned rather than a
//! gap in the reading. Folding it in with `unresolved` would throw away the
//! answer and report a coverage failure instead.
//!
//! `unresolved` is reserved for the cases where the registry refuses to guess:
//! a name matching two filers, or one that reduces to nothing once corporate
//! form words are stripped.
//!
//! # What this cannot do
//!
//! `company_tickers.json` lists **exchange-listed filers only and holds no
//! parent/subsidiary edges.** A subsidiary adviser whose parent is listed
//! ("OppenheimerFunds" under Invesco) answers `not_listed` — correct about the
//! entity named, incomplete about the group it belongs to. A consumer must not
//! read `not_listed` as "has no public parent".

use nu_engine::command_prelude::*;

use crate::commands::edgar_resolve::load_filers;
use crate::edgar_identity::{NameLookup, normalize_company_name};

fn lookup_value(name: &str, look: NameLookup, span: Span) -> Value {
    Value::record(
        record! {
            "name" => Value::string(name, span),
            "normalized" => Value::string(normalize_company_name(name), span),
            "cik" => match look {
                NameLookup::Listed(cik) => Value::int(cik as i64, span),
                _ => Value::nothing(span),
            },
            "outcome" => Value::string(look.outcome(), span),
            "reason" => look.reason()
                .map(|r| Value::string(r, span))
                .unwrap_or_else(|| Value::nothing(span)),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarCompanyCik;

impl Command for EdgarCompanyCik {
    fn name(&self) -> &str {
        "edgar company-cik"
    }

    fn description(&self) -> &str {
        "Resolve a company name to an SEC filer CIK, keeping not-listed apart from unresolved."
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar company-cik")
            .input_output_types(vec![
                (Type::Nothing, Type::record()),
                (Type::List(Box::new(Type::String)), Type::table()),
                // ⚠️ A guide task script runs with the dispatch RECORD on `$in`,
                // so this call is type-checked against a record input whether it
                // reads one or not. Without this arm the call is rejected AT
                // PARSE TIME — "expected nothing and list<string>, got record".
                // Found 2026-08-01 across three commands sharing this signature.
                (Type::Any, Type::Any),
            ])
            .optional("name", SyntaxShape::String, "Company name to resolve")
            .category(Category::Custom("dapple".into()))
    }

    fn extra_description(&self) -> &str {
        r#"
Takes a name as an argument, or a list of names on the pipeline, and returns:

  name        the name as given
  normalized  what it reduced to for matching
  cik         the SEC filer, or null
  outcome     "listed" | "not_listed" | "unresolved"
  reason      why an unresolved name is unresolved, or null

NOT_LISTED IS AN ANSWER, NOT A MISS. The registry covers exchange-listed filers,
so a mutually-owned adviser (Vanguard, owned by its own funds) or a private one
(Fidelity, Capital Group, Dodge & Cox) resolves `not_listed` correctly and
permanently. That is a fact about how the firm is owned. `unresolved` is
reserved for a name matching two filers, or one that carries no distinctive word
after corporate-form stripping — cases where the registry declines to guess.

NO PARENT/SUBSIDIARY EDGES. A subsidiary whose parent is listed still answers
`not_listed`: correct about the entity named, incomplete about its group. Do not
read `not_listed` as "has no public parent".

AMBIGUITY FAILS CLOSED. A normalized name matching two different filers resolves
to neither. Picking one would attribute another company's filings to this one.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set (SEC fair-access requires a declared
  User-Agent with contact info).

Examples:
  # Is this fund's adviser itself a public company?
  edgar company-cik "Franklin Resources Inc"

  # Several at once, off the census
  edgar ncen "0000036405" | get series.0.advisers | edgar company-cik
"#
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let arg: Option<String> = call.opt(_engine_state, _stack, 0)?;

        // Names from the argument, else from the pipeline. Anything that is not
        // a usable string is skipped rather than looked up as one — an empty
        // name would normalize to nothing and report `unresolved`, which reads
        // as a finding about a company that was never named.
        let names: Vec<String> = match arg {
            Some(n) => vec![n],
            None => match input {
                PipelineData::Empty => Vec::new(),
                other => other
                    .into_iter()
                    .filter_map(|v| v.coerce_str().ok().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect(),
            },
        };

        if names.is_empty() {
            return Err(ShellError::GenericError {
                error: "No name to resolve".into(),
                msg: "pass a company name as an argument, or a list of names on the pipeline"
                    .into(),
                span: Some(span),
                help: Some("edgar company-cik \"Franklin Resources Inc\"".into()),
                inner: vec![],
            });
        }

        let filers = load_filers(span)?;
        let single = names.len() == 1 && call.opt::<String>(_engine_state, _stack, 0)?.is_some();

        let rows: Vec<Value> = names
            .iter()
            .map(|n| lookup_value(n, filers.lookup_name(n), span))
            .collect();

        // A single named argument answers with a record; a pipeline of names
        // answers with a table, so a caller can `where outcome == "listed"`.
        Ok(if single {
            rows.into_iter()
                .next()
                .expect("one row")
                .into_pipeline_data()
        } else {
            Value::list(rows, span).into_pipeline_data()
        })
    }
}
