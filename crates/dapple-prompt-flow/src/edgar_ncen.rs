//! Parser for N-CEN, the annual investment-company census report.
//!
//! Pure logic — no network, no nushell.
//!
//! N-CEN is where a fund's **structural** facts live, already typed: who audits
//! it, who holds the assets, whether fees are waived and whether that waiver is
//! recoupable, how much securities-lending revenue the fund kept versus what the
//! lending agent took, and a short list of registrant-level red flags.
//!
//! Unlike N-PORT this is a DOM parse (`roxmltree`) — ~280 KB, not 3 MB.
//!
//! # Why these fields
//!
//! Most of the guide's "what they say" material is prose that needs a model to
//! read. These do not: `isMaterialWeakness`, `isPaymentErrorInNetAssetValue`,
//! `isPublicAccountantChanged` and `isPreviousLegalProceeding` are **booleans the
//! fund filed about itself**. They support findings that are neutral by
//! construction, in the same spirit as the numerical finders `run-the-10k`
//! already uses.

/// `N/A` and empty both mean absent.
fn clean(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("N/A") {
        None
    } else {
        Some(t.to_string())
    }
}

fn num(raw: &str) -> Option<f64> {
    clean(raw).and_then(|v| v.parse::<f64>().ok())
}

/// A count. Kept distinct from [`num`] so a class count renders as `1` rather
/// than `1.0` — the difference matters in a rendered report.
fn count(raw: &str) -> Option<i64> {
    clean(raw).and_then(|v| v.parse::<i64>().ok())
}

fn yn(raw: &str) -> Option<bool> {
    match clean(raw)?.to_ascii_uppercase().as_str() {
        "Y" => Some(true),
        "N" => Some(false),
        _ => None,
    }
}

/// A payment to a securities-lending agent. `amount` is frequently absent even
/// when `kind` is disclosed.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentPayment {
    pub kind: String,
    pub amount: Option<f64>,
}

/// The fund's independent accountant, with its PCAOB registration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Accountant {
    pub name: Option<String>,
    pub pcaob_number: Option<String>,
    pub lei: Option<String>,
}

/// A trustee, with the fund's own answer to whether they are an *interested*
/// person under the '40 Act.
///
/// The independent/interested split is the mechanism the statute relies on to
/// resolve the adviser's divided loyalties, so it is a fact worth carrying —
/// but it is a **filed classification**, never a judgement we make. Report the
/// counts and the names; impute nothing about anybody.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Director {
    pub name: Option<String>,
    pub interested: Option<bool>,
}

/// Per-series facts. One N-CEN covers every series in the trust.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NcenSeries {
    pub series_id: Option<String>,
    pub fund_name: Option<String>,
    pub lei: Option<String>,
    /// A '40 Act election, not a description. Worth holding against the
    /// portfolio's actual concentration and against whatever the prospectus
    /// tags as its principal risks.
    pub non_diversified: Option<bool>,
    pub expense_limitation: Option<bool>,
    /// A waived fee the adviser may claw back later. Materially different from a
    /// permanent reduction, and disclosed only here.
    pub fees_waived_recoupable: Option<bool>,
    pub securities_lending: Option<bool>,
    /// What the FUND kept from securities lending. Set against
    /// [`Self::payments_to_agents`] to see the split.
    pub sec_lending_net_income: Option<f64>,
    /// What the lending agent was paid.
    ///
    /// Verified live: funds often disclose the **arrangement type without an
    /// amount** (`"Revenue sharing split"` and nothing else). Keying this on
    /// type→amount would drop those rows entirely, reporting "no agent payments"
    /// for a fund that plainly has one. The type alone is the fact.
    pub payments_to_agents: Vec<AgentPayment>,
    pub monthly_avg_net_assets: Option<f64>,
    pub aggregate_commission: Option<f64>,
    pub custodian_changed: Option<bool>,
    pub transfer_agent_changed: Option<bool>,
    pub pricing_service_changed: Option<bool>,
    pub advisers: Vec<String>,
    pub sub_advisers: Vec<String>,
    pub custodians: Vec<String>,

    // --- Lifecycle. A fund ending is routine, quiet and different in kind from
    // a company failing: nobody loses the money, it comes back as cash on a date
    // the holder did not pick. These are the filed facts of that, and none of
    // them predicts anything.
    /// The fund liquidated during the period. Terminal, and stated as such.
    pub liquidated: Option<bool>,
    /// The fund's first N-CEN. The other end of the lifecycle, and it bounds
    /// every claim: a fund with one filing has no record to be held to.
    pub first_filing: Option<bool>,
    /// Share classes terminated during the period. Class consolidation runs
    /// ahead of a fund closing often enough to be worth carrying, and it is a
    /// count rather than a judgement.
    pub terminated_classes: Option<i64>,
    /// The adviser financially supported the fund during the period. Distinct
    /// from a fee waiver: this is money in, not fees forgone.
    pub financial_support: Option<bool>,

    // --- Liquidity backstops (see also N-PORT's rule 22e-4 buckets).
    /// The schema answers this with a WORD (`"Committed"`), not `Y`/`N`, so it
    /// is carried as text. Passing it through `yn` returns `None` for every
    /// fund that has one.
    pub credit_line: Option<String>,
    pub credit_line_used: Option<bool>,

    // --- Index-tracking, filed by the fund about itself.
    /// Whether the tracked index is affiliated with the adviser.
    pub index_affiliated: Option<bool>,
    /// Return difference against the tracked index, before expenses.
    pub tracking_diff_before_expense: Option<f64>,
    /// Return difference against the tracked index, after expenses.
    ///
    /// For an index fund "we track the index" IS the claim, and the gap between
    /// this and [`Self::tracking_diff_before_expense`] is the fee's drag on it —
    /// the fund's own measurement of its own promise, with nothing to interpret.
    pub tracking_diff_after_expense: Option<f64>,

    // --- Adviser identity, for resolving the adviser to a filer.
    /// CRD number — a stable registration key, and a far better identity than a
    /// name match, which fails closed on ambiguity by design.
    pub adviser_crd: Option<String>,
    pub adviser_file_no: Option<String>,
    pub adviser_lei: Option<String>,
    /// The securities-lending agent is affiliated with the adviser. A fact
    /// about who is on both sides of the lending split, not an allegation.
    pub lending_agent_affiliated: Option<bool>,
}

/// A parsed N-CEN filing.
#[derive(Debug, Clone, Default)]
pub struct NcenFiling {
    /// Root `<schemaVersion>`. Version the parse rather than assuming stability.
    pub schema_version: Option<String>,
    pub reg_cik: Option<String>,
    pub reg_name: Option<String>,
    // Registrant-level red flags. Filed by the fund about itself, so a finding
    // built on them imputes nothing.
    pub accountant_changed: Option<bool>,
    pub material_weakness: Option<bool>,
    pub nav_pricing_error: Option<bool>,
    pub legal_proceeding: Option<bool>,
    /// A change in accounting principle. Registrant-level, and it bears on
    /// whether figures either side of it are comparable.
    pub accounting_principle_change: Option<bool>,
    pub accountants: Vec<Accountant>,
    /// The board. Its independent/interested composition is the mechanism the
    /// '40 Act relies on where the adviser's duties and its own interests point
    /// different ways.
    pub directors: Vec<Director>,
    pub series: Vec<NcenSeries>,
    pub warnings: Vec<String>,
}

impl NcenFiling {
    /// Registrant-level flags that are affirmatively `true`, as human labels.
    ///
    /// Empty is the normal case and is itself worth stating — "nothing flagged"
    /// is a finding, not an absence of one.
    pub fn raised_flags(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (v, label) in [
            (
                self.material_weakness,
                "material weakness in internal control",
            ),
            (self.nav_pricing_error, "error in net asset value pricing"),
            (self.accountant_changed, "independent accountant changed"),
            (self.legal_proceeding, "legal proceeding disclosed"),
        ] {
            if v == Some(true) {
                out.push(label);
            }
        }
        out
    }

    pub fn series_by_id(&self, series_id: &str) -> Option<&NcenSeries> {
        self.series
            .iter()
            .find(|s| s.series_id.as_deref() == Some(series_id))
    }

    /// Trustees the fund itself classified as independent, and as interested.
    ///
    /// Returned as `(independent, interested)` counts. A trustee the filing did
    /// not classify counts as neither — an unanswered question is not an answer,
    /// and rolling it into either side would put a number in the report that
    /// nothing supports.
    pub fn board_split(&self) -> (usize, usize) {
        let independent = self
            .directors
            .iter()
            .filter(|d| d.interested == Some(false))
            .count();
        let interested = self
            .directors
            .iter()
            .filter(|d| d.interested == Some(true))
            .count();
        (independent, interested)
    }
}

impl NcenSeries {
    /// Lifecycle facts that are affirmatively present, as human labels.
    ///
    /// Empty is the ordinary case. **This is exposure, never prediction** — the
    /// list says which conditions a fund meets and stops there. A fund closing
    /// is a routine event, and nothing here forecasts one.
    pub fn lifecycle_flags(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.liquidated == Some(true) {
            out.push("the fund liquidated during the period".to_string());
        }
        if self.first_filing == Some(true) {
            out.push("this is the fund's first annual census".to_string());
        }
        if let Some(n) = self.terminated_classes
            && n > 0
        {
            out.push(format!(
                "{n} share class{} terminated during the period",
                if n == 1 { "" } else { "es" }
            ));
        }
        if self.financial_support == Some(true) {
            out.push("the adviser financially supported the fund during the period".to_string());
        }
        if self.credit_line_used == Some(true) {
            out.push("the fund drew on a line of credit during the period".to_string());
        }
        out
    }
}

/// Text of the first descendant with this local name.
fn child_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<String> {
    node.descendants()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .and_then(clean)
}

/// Text of every descendant with this local name.
fn all_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Vec<String> {
    node.descendants()
        .filter(|n| n.has_tag_name(tag))
        .filter_map(|n| n.text())
        .filter_map(clean)
        .collect()
}

/// Parse an N-CEN `primary_doc.xml`.
pub fn parse_ncen(xml: &str) -> Result<NcenFiling, String> {
    let doc =
        roxmltree::Document::parse(xml).map_err(|e| format!("N-CEN XML is malformed: {e}"))?;
    let root = doc.root_element();
    let mut out = NcenFiling {
        schema_version: child_text(root, "schemaVersion"),
        reg_cik: child_text(root, "regCik").or_else(|| child_text(root, "cik")),
        reg_name: child_text(root, "regName").or_else(|| child_text(root, "registrantName")),
        ..Default::default()
    };

    if let Some(info) = doc.descendants().find(|n| n.has_tag_name("registrantInfo")) {
        out.accountant_changed = child_text(info, "isPublicAccountantChanged")
            .as_deref()
            .and_then(yn);
        out.material_weakness = child_text(info, "isMaterialWeakness")
            .as_deref()
            .and_then(yn);
        out.nav_pricing_error = child_text(info, "isPaymentErrorInNetAssetValue")
            .as_deref()
            .and_then(yn);
        out.legal_proceeding = child_text(info, "isPreviousLegalProceeding")
            .as_deref()
            .and_then(yn);
        out.accounting_principle_change = child_text(info, "isAccountingPrincipleChange")
            .as_deref()
            .and_then(yn);

        for a in info
            .descendants()
            .filter(|n| n.has_tag_name("publicAccountant"))
        {
            out.accountants.push(Accountant {
                name: child_text(a, "publicAccountantName"),
                pcaob_number: child_text(a, "pcaobNumber"),
                lei: child_text(a, "publicAccountantLei"),
            });
        }
    }

    // The board sits outside `registrantInfo` in observed filings, so it is
    // matched from the document root. Pair each name with its own interested
    // flag from the SAME node — reading the two as parallel lists would
    // misattribute the classification the moment one director omits either.
    for d in doc.descendants().filter(|n| n.has_tag_name("director")) {
        let dir = Director {
            name: child_text(d, "directorName"),
            interested: child_text(d, "isDirectorInterestedPerson")
                .as_deref()
                .and_then(yn),
        };
        if dir.name.is_some() {
            out.directors.push(dir);
        }
    }

    // One block per series. The element name carries the form's own wording;
    // match on the series-id child rather than assuming the wrapper's name.
    for blk in doc
        .descendants()
        .filter(|n| n.children().any(|c| c.has_tag_name("mgmtInvSeriesId")))
    {
        let mut s = NcenSeries {
            series_id: child_text(blk, "mgmtInvSeriesId"),
            fund_name: child_text(blk, "mgmtInvFundName"),
            lei: child_text(blk, "mgmtInvLei"),
            non_diversified: child_text(blk, "isNonDiversifiedCompany")
                .as_deref()
                .and_then(yn),
            expense_limitation: child_text(blk, "isExpenseLimitationInPlace")
                .as_deref()
                .and_then(yn),
            fees_waived_recoupable: child_text(blk, "isFeesWaivedRecoupable")
                .as_deref()
                .and_then(yn),
            securities_lending: child_text(blk, "isFundSecuritiesLending")
                .as_deref()
                .and_then(yn),
            sec_lending_net_income: child_text(blk, "netIncomeSecuritiesLending")
                .as_deref()
                .and_then(num),
            // The schema's own abbreviation — `mnthly`, not `monthly`.
            monthly_avg_net_assets: child_text(blk, "mnthlyAvgNetAssets")
                .as_deref()
                .and_then(num),
            aggregate_commission: child_text(blk, "aggregateCommission")
                .as_deref()
                .and_then(num),
            custodian_changed: child_text(blk, "isCustodianHiredOrTerminated")
                .as_deref()
                .and_then(yn),
            transfer_agent_changed: child_text(blk, "isTransferAgentHiredOrTerminated")
                .as_deref()
                .and_then(yn),
            pricing_service_changed: child_text(blk, "isPricingServiceHiredOrTerminated")
                .as_deref()
                .and_then(yn),
            advisers: all_text(blk, "investmentAdviserName"),
            sub_advisers: all_text(blk, "subAdviserName"),
            custodians: all_text(blk, "custodianName"),

            liquidated: child_text(blk, "isFundLiquidated").as_deref().and_then(yn),
            first_filing: child_text(blk, "isFirstFilingByFund")
                .as_deref()
                .and_then(yn),
            terminated_classes: child_text(blk, "numTerminatedClass")
                .as_deref()
                .and_then(count),
            financial_support: child_text(blk, "isFinancialSupportDuringPeriod")
                .as_deref()
                .and_then(yn),

            // Text, not a boolean: the schema answers with `"Committed"`.
            credit_line: child_text(blk, "isCreditLineCommitted"),
            credit_line_used: child_text(blk, "isCreditLineUsed").as_deref().and_then(yn),

            index_affiliated: child_text(blk, "isIndexFundAffiliated")
                .as_deref()
                .and_then(yn),
            tracking_diff_before_expense: child_text(blk, "indexFundReturnDiffBeforeExpense")
                .as_deref()
                .and_then(num),
            tracking_diff_after_expense: child_text(blk, "indexFundReturnDiffAfterExpense")
                .as_deref()
                .and_then(num),

            adviser_crd: child_text(blk, "investmentAdviserCrdNo"),
            adviser_file_no: child_text(blk, "investmentAdviserFileNo"),
            adviser_lei: child_text(blk, "investmentAdviserLei"),
            lending_agent_affiliated: child_text(blk, "isSecuritiesAgentAffiliated")
                .as_deref()
                .and_then(yn),

            ..Default::default()
        };

        for p in blk
            .descendants()
            .filter(|n| n.has_tag_name("paymentToAgentManagers"))
        {
            let kind = child_text(p, "paymentToAgentManagerType")
                .unwrap_or_else(|| "unspecified".to_string());
            s.payments_to_agents.push(AgentPayment {
                kind,
                amount: child_text(p, "paymentToAgentManagerValue")
                    .as_deref()
                    .and_then(num),
            });
        }
        if s.series_id.is_some() {
            out.series.push(s);
        }
    }

    if out.series.is_empty() {
        out.warnings
            .push("no series blocks parsed; the filing may use an unrecognized schema".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<edgarSubmission xmlns="http://www.sec.gov/edgar/ncen">
  <schemaVersion>X0505</schemaVersion>
  <formData>
    <registrantInfo>
      <regCik>0001605941</regCik>
      <regName>1290 Funds</regName>
      <isPublicAccountantChanged>N</isPublicAccountantChanged>
      <isMaterialWeakness>Y</isMaterialWeakness>
      <isPaymentErrorInNetAssetValue>N</isPaymentErrorInNetAssetValue>
      <isPreviousLegalProceeding>N</isPreviousLegalProceeding>
      <isAccountingPrincipleChange>N</isAccountingPrincipleChange>
      <publicAccountants>
        <publicAccountant>
          <publicAccountantName>PricewaterhouseCoopers LLP</publicAccountantName>
          <pcaobNumber>238</pcaobNumber>
          <publicAccountantLei>5493002GVO7EO8RNNS37</publicAccountantLei>
        </publicAccountant>
      </publicAccountants>
    </registrantInfo>
    <directors>
      <director>
        <directorName>Andre F. Perold</directorName>
        <isDirectorInterestedPerson>N</isDirectorInterestedPerson>
      </director>
      <director>
        <directorName>Salim Ramji</directorName>
        <isDirectorInterestedPerson>Y</isDirectorInterestedPerson>
      </director>
      <director>
        <directorName>Unclassified Trustee</directorName>
      </director>
    </directors>
    <managementInvestmentQuestion>
      <mgmtInvSeriesId>S000045910</mgmtInvSeriesId>
      <mgmtInvFundName>1290 GAMCO Small/Mid Cap Value Fund</mgmtInvFundName>
      <isNonDiversifiedCompany>N</isNonDiversifiedCompany>
      <isExpenseLimitationInPlace>Y</isExpenseLimitationInPlace>
      <isFeesWaivedRecoupable>Y</isFeesWaivedRecoupable>
      <isFundSecuritiesLending>Y</isFundSecuritiesLending>
      <netIncomeSecuritiesLending>42785.00000000</netIncomeSecuritiesLending>
      <securityLendings>
        <securityLending>
          <securitiesAgentName>State Street Bank</securitiesAgentName>
          <isSecuritiesAgentAffiliated>Y</isSecuritiesAgentAffiliated>
        </securityLending>
      </securityLendings>
      <paymentToAgentManagers>
        <paymentToAgentManagerType>Revenue share</paymentToAgentManagerType>
        <paymentToAgentManagerValue>11000.00</paymentToAgentManagerValue>
      </paymentToAgentManagers>
      <mnthlyAvgNetAssets>164159420.17000000</mnthlyAvgNetAssets>
      <aggregateCommission>24230.49000000</aggregateCommission>
      <isCustodianHiredOrTerminated>N</isCustodianHiredOrTerminated>
      <investmentAdvisers>
        <investmentAdviser>
          <investmentAdviserName>GAMCO Asset Management Inc.</investmentAdviserName>
          <investmentAdviserCrdNo>000105958</investmentAdviserCrdNo>
          <investmentAdviserFileNo>801-11953</investmentAdviserFileNo>
          <investmentAdviserLei>5493002789CX3L0CJP65</investmentAdviserLei>
        </investmentAdviser>
      </investmentAdvisers>
      <isFundLiquidated>N</isFundLiquidated>
      <isFirstFilingByFund>N</isFirstFilingByFund>
      <numTerminatedClass>2</numTerminatedClass>
      <isFinancialSupportDuringPeriod>Y</isFinancialSupportDuringPeriod>
      <isCreditLineCommitted>Committed</isCreditLineCommitted>
      <isCreditLineUsed>Y</isCreditLineUsed>
      <indexFundInfo>
        <isIndexFundAffiliated>N</isIndexFundAffiliated>
        <indexFundReturnDiffBeforeExpense>-0.00397999</indexFundReturnDiffBeforeExpense>
        <indexFundReturnDiffAfterExpense>-0.16890000</indexFundReturnDiffAfterExpense>
      </indexFundInfo>
    </managementInvestmentQuestion>
    <managementInvestmentQuestion>
      <mgmtInvSeriesId>S000045911</mgmtInvSeriesId>
      <mgmtInvFundName>1290 Other Fund</mgmtInvFundName>
      <isNonDiversifiedCompany>Y</isNonDiversifiedCompany>
      <isFundSecuritiesLending>N</isFundSecuritiesLending>
      <numTerminatedClass>0</numTerminatedClass>
    </managementInvestmentQuestion>
  </formData>
</edgarSubmission>"#;

    fn parsed() -> NcenFiling {
        parse_ncen(SAMPLE).expect("sample parses")
    }

    #[test]
    fn registrant_level_facts_and_auditor() {
        let f = parsed();
        assert_eq!(f.schema_version.as_deref(), Some("X0505"));
        assert_eq!(f.reg_name.as_deref(), Some("1290 Funds"));
        assert_eq!(f.accountants.len(), 1);
        assert_eq!(f.accountants[0].pcaob_number.as_deref(), Some("238"));
    }

    /// Only affirmative flags are raised. A fund that filed `N` everywhere must
    /// produce an EMPTY list, not four false positives.
    #[test]
    fn only_affirmative_flags_are_raised() {
        let f = parsed();
        assert_eq!(
            f.raised_flags(),
            vec!["material weakness in internal control"]
        );

        let clean_filing = parse_ncen(&SAMPLE.replace(
            "<isMaterialWeakness>Y</isMaterialWeakness>",
            "<isMaterialWeakness>N</isMaterialWeakness>",
        ))
        .unwrap();
        assert!(clean_filing.raised_flags().is_empty());
    }

    #[test]
    fn per_series_fee_and_lending_economics() {
        let f = parsed();
        assert_eq!(f.series.len(), 2);
        let s = f.series_by_id("S000045910").expect("series present");
        assert_eq!(
            s.fund_name.as_deref(),
            Some("1290 GAMCO Small/Mid Cap Value Fund")
        );
        assert_eq!(s.non_diversified, Some(false));
        // A recoupable waiver is a deferred cost, not a discount — and it is
        // disclosed nowhere else.
        assert_eq!(s.fees_waived_recoupable, Some(true));
        assert_eq!(s.sec_lending_net_income, Some(42785.0));
        assert_eq!(s.payments_to_agents.len(), 1);
        assert_eq!(s.payments_to_agents[0].kind, "Revenue share");
        assert_eq!(s.payments_to_agents[0].amount, Some(11000.0));
        assert_eq!(s.monthly_avg_net_assets, Some(164_159_420.17));

        // An arrangement disclosed WITHOUT an amount must still be reported —
        // "no agent payments" would be the wrong answer for a fund that names
        // a revenue-sharing split but does not quantify it.
        let unpriced = parse_ncen(&SAMPLE.replace(
            "<paymentToAgentManagerValue>11000.00</paymentToAgentManagerValue>",
            "",
        ))
        .unwrap();
        let s2 = unpriced.series_by_id("S000045910").unwrap();
        assert_eq!(s2.payments_to_agents.len(), 1);
        assert_eq!(s2.payments_to_agents[0].kind, "Revenue share");
        assert_eq!(s2.payments_to_agents[0].amount, None);
    }

    /// Absent fields must stay `None` rather than defaulting to `false`, which
    /// would report "not flagged" for something the fund never answered.
    #[test]
    fn missing_fields_stay_unknown() {
        let s = parsed().series_by_id("S000045911").unwrap().clone();
        assert_eq!(s.non_diversified, Some(true));
        assert_eq!(s.fees_waived_recoupable, None);
        assert_eq!(s.sec_lending_net_income, None);
        assert!(s.payments_to_agents.is_empty());
    }

    /// The service-provider names were parsed but never asserted, so a schema
    /// that moved the adviser block would have returned `[]` silently.
    #[test]
    fn service_providers_are_actually_populated() {
        let s = parsed().series_by_id("S000045910").unwrap().clone();
        assert_eq!(s.advisers, vec!["GAMCO Asset Management Inc."]);
        assert_eq!(s.adviser_crd.as_deref(), Some("000105958"));
        assert_eq!(s.adviser_file_no.as_deref(), Some("801-11953"));
        assert_eq!(s.adviser_lei.as_deref(), Some("5493002789CX3L0CJP65"));
        // Who sits on both sides of the lending split is a filed fact.
        assert_eq!(s.lending_agent_affiliated, Some(true));
    }

    /// Lifecycle facts are reported as conditions met, never as a forecast.
    #[test]
    fn lifecycle_flags_report_conditions_not_predictions() {
        let f = parsed();
        let s = f.series_by_id("S000045910").unwrap();
        assert_eq!(s.liquidated, Some(false));
        assert_eq!(s.first_filing, Some(false));
        assert_eq!(s.terminated_classes, Some(2));
        assert_eq!(s.financial_support, Some(true));

        let flags = s.lifecycle_flags();
        assert!(
            flags
                .iter()
                .any(|x| x.contains("2 share classes terminated"))
        );
        assert!(flags.iter().any(|x| x.contains("financially supported")));
        assert!(flags.iter().any(|x| x.contains("line of credit")));
        // A fund that did NOT liquidate must not be flagged as one that did.
        assert!(!flags.iter().any(|x| x.contains("liquidated")));

        // Zero terminated classes is a filed zero, not a condition met.
        let quiet = f.series_by_id("S000045911").unwrap();
        assert_eq!(quiet.terminated_classes, Some(0));
        assert!(quiet.lifecycle_flags().is_empty());
    }

    /// `isCreditLineCommitted` answers with a WORD, not `Y`/`N`. Routing it
    /// through `yn` returns `None` for every fund that has a facility.
    #[test]
    fn credit_line_is_text_not_a_boolean() {
        let s = parsed().series_by_id("S000045910").unwrap().clone();
        assert_eq!(s.credit_line.as_deref(), Some("Committed"));
        assert_eq!(s.credit_line_used, Some(true));
    }

    /// The gap between the two tracking figures is the fee's drag on the
    /// promise, filed by the fund about itself.
    #[test]
    fn index_tracking_difference_is_carried_both_sides_of_expenses() {
        let s = parsed().series_by_id("S000045910").unwrap().clone();
        assert_eq!(s.index_affiliated, Some(false));
        assert_eq!(s.tracking_diff_before_expense, Some(-0.00397999));
        assert_eq!(s.tracking_diff_after_expense, Some(-0.1689));
    }

    /// A trustee the filing did not classify counts as neither independent nor
    /// interested. An unanswered question is not an answer.
    #[test]
    fn board_split_excludes_the_unclassified() {
        let f = parsed();
        assert_eq!(f.directors.len(), 3);
        assert_eq!(f.board_split(), (1, 1));
        assert_eq!(f.directors[2].name.as_deref(), Some("Unclassified Trustee"));
        assert_eq!(f.directors[2].interested, None);
    }

    #[test]
    fn malformed_is_an_error_and_thin_is_a_warning() {
        assert!(parse_ncen("<a><b>").is_err());
        let thin = parse_ncen("<edgarSubmission/>").expect("well-formed");
        assert!(thin.series.is_empty());
        assert_eq!(thin.warnings.len(), 1);
    }
}
