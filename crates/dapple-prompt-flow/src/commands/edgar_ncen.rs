//! The `edgar ncen` command — a fund's structural and fee facts, already typed.
//!
//! N-CEN is where the guide gets facts it does not need a model to read: the
//! auditor, whether fees are waived and whether that waiver is **recoupable**,
//! how much securities-lending revenue the fund kept, and a handful of
//! registrant-level flags the fund filed about itself.

use nu_engine::command_prelude::*;

use crate::commands::edgar_common::{archive_url, build_client, edgar_error, locate_fund_filings};
use crate::edgar_ncen::{NcenFiling, parse_ncen};

fn opt_s(v: &Option<String>, span: Span) -> Value {
    v.as_ref()
        .map(|s| Value::string(s, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn opt_b(v: Option<bool>, span: Span) -> Value {
    v.map(|b| Value::bool(b, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn opt_f(v: Option<f64>, span: Span) -> Value {
    v.map(|n| Value::float(n, span))
        .unwrap_or_else(|| Value::nothing(span))
}

fn filing_value(f: &NcenFiling, accession: &str, filed: &str, span: Span) -> Value {
    let accountants: Vec<Value> = f
        .accountants
        .iter()
        .map(|a| {
            Value::record(
                record! {
                    "name" => opt_s(&a.name, span),
                    "pcaob_number" => opt_s(&a.pcaob_number, span),
                    "lei" => opt_s(&a.lei, span),
                },
                span,
            )
        })
        .collect();

    let series: Vec<Value> = f
        .series
        .iter()
        .map(|s| {
            let payments: Vec<Value> = s
                .payments_to_agents
                .iter()
                .map(|p| {
                    Value::record(
                        record! {
                            "kind" => Value::string(&p.kind, span),
                            "amount" => opt_f(p.amount, span),
                        },
                        span,
                    )
                })
                .collect();
            let strs = |v: &Vec<String>| {
                Value::list(v.iter().map(|x| Value::string(x, span)).collect(), span)
            };
            Value::record(
                record! {
                    "series_id" => opt_s(&s.series_id, span),
                    "fund_name" => opt_s(&s.fund_name, span),
                    "lei" => opt_s(&s.lei, span),
                    "non_diversified" => opt_b(s.non_diversified, span),
                    "expense_limitation" => opt_b(s.expense_limitation, span),
                    "fees_waived_recoupable" => opt_b(s.fees_waived_recoupable, span),
                    "securities_lending" => opt_b(s.securities_lending, span),
                    "sec_lending_net_income" => opt_f(s.sec_lending_net_income, span),
                    "payments_to_agents" => Value::list(payments, span),
                    "monthly_avg_net_assets" => opt_f(s.monthly_avg_net_assets, span),
                    "aggregate_commission" => opt_f(s.aggregate_commission, span),
                    "custodian_changed" => opt_b(s.custodian_changed, span),
                    "transfer_agent_changed" => opt_b(s.transfer_agent_changed, span),
                    "pricing_service_changed" => opt_b(s.pricing_service_changed, span),
                    "advisers" => strs(&s.advisers),
                    "sub_advisers" => strs(&s.sub_advisers),
                    "custodians" => strs(&s.custodians),
                    "liquidated" => opt_b(s.liquidated, span),
                    "first_filing" => opt_b(s.first_filing, span),
                    "terminated_classes" => match s.terminated_classes {
                        Some(n) => Value::int(n, span),
                        None => Value::nothing(span),
                    },
                    "financial_support" => opt_b(s.financial_support, span),
                    "lifecycle_flags" => Value::list(
                        s.lifecycle_flags().iter().map(|x| Value::string(x, span)).collect(),
                        span,
                    ),
                    "credit_line" => opt_s(&s.credit_line, span),
                    "credit_line_used" => opt_b(s.credit_line_used, span),
                    "index_affiliated" => opt_b(s.index_affiliated, span),
                    "tracking_diff_before_expense" => opt_f(s.tracking_diff_before_expense, span),
                    "tracking_diff_after_expense" => opt_f(s.tracking_diff_after_expense, span),
                    "adviser_crd" => opt_s(&s.adviser_crd, span),
                    "adviser_file_no" => opt_s(&s.adviser_file_no, span),
                    "adviser_lei" => opt_s(&s.adviser_lei, span),
                    "lending_agent_affiliated" => opt_b(s.lending_agent_affiliated, span),
                },
                span,
            )
        })
        .collect();

    Value::record(
        record! {
            "accession" => Value::string(accession, span),
            "filing_date" => Value::string(filed, span),
            "schema_version" => opt_s(&f.schema_version, span),
            "reg_cik" => opt_s(&f.reg_cik, span),
            "reg_name" => opt_s(&f.reg_name, span),
            "accountant_changed" => opt_b(f.accountant_changed, span),
            "material_weakness" => opt_b(f.material_weakness, span),
            "nav_pricing_error" => opt_b(f.nav_pricing_error, span),
            "legal_proceeding" => opt_b(f.legal_proceeding, span),
            "accounting_principle_change" => opt_b(f.accounting_principle_change, span),
            "raised_flags" => Value::list(
                f.raised_flags().iter().map(|s| Value::string(*s, span)).collect(),
                span,
            ),
            "accountants" => Value::list(accountants, span),
            "directors" => Value::list(
                f.directors
                    .iter()
                    .map(|d| {
                        Value::record(
                            record! {
                                "name" => opt_s(&d.name, span),
                                "interested" => opt_b(d.interested, span),
                            },
                            span,
                        )
                    })
                    .collect(),
                span,
            ),
            // Counts of what the fund itself classified. A trustee it did not
            // classify is in neither count, so these need not sum to
            // `directors | length` — an unanswered question is not an answer.
            "board_independent" => Value::int(f.board_split().0 as i64, span),
            "board_interested" => Value::int(f.board_split().1 as i64, span),
            "series" => Value::list(series, span),
            "warnings" => Value::list(
                f.warnings.iter().map(|w| Value::string(w, span)).collect(),
                span,
            ),
        },
        span,
    )
}

#[derive(Clone)]
pub struct EdgarNcen;

impl Command for EdgarNcen {
    fn name(&self) -> &str {
        "edgar ncen"
    }

    fn signature(&self) -> Signature {
        Signature::build("edgar ncen")
            .input_output_types(vec![(Type::Nothing, Type::record())])
            .required(
                "identifier",
                SyntaxShape::String,
                "Trust CIK, or a series id (S000######) belonging to it",
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Fetch a fund trust's N-CEN census report (auditor, fees, securities lending)."
    }

    fn extra_description(&self) -> &str {
        r#"Returns a record:
  { accession, filing_date, schema_version, reg_cik, reg_name,
    accountant_changed, material_weakness, nav_pricing_error, legal_proceeding,
    raised_flags, accountants, series, warnings }

N-CEN is filed per TRUST and covers every series in it, so `series` is a table.
Each entry carries non_diversified, expense_limitation, fees_waived_recoupable,
securities_lending, sec_lending_net_income, payments_to_agents,
monthly_avg_net_assets, aggregate_commission and the service-provider change
flags.

`raised_flags` lists only flags the fund answered YES to. An empty list is the
normal case and says so plainly, rather than four separate false readings.

`fees_waived_recoupable` is worth singling out: a waived fee the adviser may
claw back later is a deferred cost, not a discount, and it is disclosed nowhere
else.

`payments_to_agents` entries may carry a `kind` with a null `amount` — funds
often name a revenue-sharing arrangement without quantifying it.

Environment:
  DAPPLE_EDGAR_USER_AGENT must be set.

Examples:
  edgar ncen 1605941 | get raised_flags
  edgar ncen S000045910 | get series | where fees_waived_recoupable == true
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["edgar", "sec", "ncen", "fund", "auditor", "fees", "lending"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            description: "Registrant-level flags a fund filed about itself",
            example: r#"edgar ncen 1605941 | get raised_flags"#,
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
        let edgar = build_client(span)?;

        #[allow(clippy::result_large_err)]
        let out = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let refs = locate_fund_filings(&edgar, &identifier, "N-CEN", 1, span).await?;
                let Some(r) = refs.first() else {
                    return Err(ShellError::GenericError {
                        error: format!("No N-CEN filing for `{identifier}`"),
                        msg: "EDGAR returned no N-CEN filings for this identifier".into(),
                        span: Some(span),
                        help: Some(
                            "N-CEN is filed per trust; pass the trust CIK or any of its series ids."
                                .into(),
                        ),
                        inner: vec![],
                    });
                };
                let xml = edgar
                    .get(&archive_url(&r.cik, &r.accession, "primary_doc.xml"))
                    .await
                    .map_err(|e| edgar_error(&format!("fetch N-CEN {}", r.accession), &e, span))?;
                let parsed = parse_ncen(&xml).map_err(|msg| ShellError::GenericError {
                    error: "N-CEN did not parse".into(),
                    msg,
                    span: Some(span),
                    help: None,
                    inner: vec![],
                })?;
                Ok::<_, ShellError>(filing_value(&parsed, &r.accession, &r.filing_date, span))
            })
        })?;

        Ok(PipelineData::Value(out, None))
    }
}
