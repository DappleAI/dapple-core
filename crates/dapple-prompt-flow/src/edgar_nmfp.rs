//! Parser for N-MFP3, the monthly money market fund report.
//!
//! Pure logic — no network, no nushell.
//!
//! N-MFP3 is the strongest evidentiary document in the EDGAR guide family. A
//! money market fund's headline claim IS a number (a share is worth $1.00), and
//! this form exists to publish the fund's own market-based measurement of that
//! number, every business day, to four decimals. The claim and its test are the
//! same quantity, filed by the party making the claim.
//!
//! A DOM parse (`roxmltree`) like N-CEN rather than N-PORT's streaming pull.
//! Documents run 38 KB to 4 MB; the large end is a big prime fund's position
//! schedule, and a guide reads one series at a time.
//!
//! # The document's three scopes, which are not interchangeable
//!
//! This is the trap that produces plausible, wrong charts. `formData` holds:
//!
//! - `generalInfo` — identity, once.
//! - `seriesLevelInfo` — the fund. Its daily series are **daily NAV per share**,
//!   **seven-day GROSS yield** and **liquid assets**, one point per business day.
//! - `classLevelInfo`, repeated **once per share class**. Its daily series are
//!   **daily NAV per share class**, **seven-day NET yield** and **shareholder
//!   flows** — so a ten-class fund files ten of each, and a reader that zips
//!   net yield against gross yield on date alone fans 21 rows out to 210.
//! - `scheduleOfPortfolioSecuritiesInfo`, repeated once per position, a sibling
//!   of the above rather than a child of `seriesLevelInfo`.
//!
//! So gross-versus-net yield is a **per-class** comparison, and so is anything
//! about who owns the fund: `beneficialRecordOwnerCategory` and the
//! `percentShareHolderComp*` block are filed per share class, which is what
//! share classes are for.
//!
//! # Five more traps, all verified against live filings
//!
//! 1. **The daily series elements are self-nested.** The wrapper and the value
//!    carry the same name, so any "first descendant named X" lookup returns the
//!    wrapper and reads empty. [`direct_text`] exists for exactly this and
//!    [`descendant_text`] must never be used on a series element.
//! 2. **Percentages are FRACTIONS here**, and N-PORT's are percentages, and both
//!    are spelled `percentage*`. A filed `.2478` is 24.78%. [`Fraction`] carries
//!    the unit in the type so a later refactor cannot lose it.
//! 3. **Series lengths differ within one filing.** A seven-day yield cannot
//!    exist until the fund is seven days old, so a fund launched mid-month files
//!    fewer net-yield points than NAV points. Zip on date, never on index.
//! 4. **`stablePricePerShare` uses `0.0000` for "not applicable"**, and is not
//!    always 1.00: funds file at $1, $10, $31, $100 and $101.
//! 5. **`nameOfPersonDescExpensePay` holds a dollar amount, not a name**, and
//!    `LEIID` is the literal string `N/A` on collateral rows.
//!
//! # Misfiling detection, and the sentinel that looks exactly like it
//!
//! A daily NAV of `0.0000` is legitimate and common: a master fund in a
//! master-feeder structure files an all-zero series because it has no per-share
//! NAV to report. Measured over one month of the entire industry (335 filings,
//! 7,011 points), 14 filings file an all-zero series and are perfectly normal.
//!
//! What is not normal is a non-positive or wildly out-of-band point sitting
//! **among valid points in the same series** — two filings that month, 0.6%.
//! [`NmfpSeries::nav_anomalies`] encodes that distinction. A finder that merely
//! flagged non-positive points would call fourteen healthy master funds broken.

use std::collections::BTreeMap;

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

/// A count. Kept distinct from [`num`] so a share-class count renders as `10`
/// rather than `10.0`.
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

/// A proportion as N-MFP3 files it: `0.4018` means 40.18%.
///
/// The unit lives in the type because the sibling form spells the same concept
/// the opposite way. N-PORT's `pctVal` is already a percentage and sums to ~100;
/// N-MFP3's `percentage*` fields are fractions and sum to ~1. Both are spelled
/// `percentage`, so a helper ported across without checking reports a fund
/// holding 100% weekly liquid assets as holding 1%.
///
/// Nothing outside this type multiplies by 100.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Fraction(f64);

impl Fraction {
    pub fn new(v: f64) -> Self {
        Self(v)
    }

    /// The value exactly as filed.
    pub fn as_filed(self) -> f64 {
        self.0
    }

    /// The value as a percentage: a filed `.2478` returns `24.78`.
    pub fn as_percent(self) -> f64 {
        self.0 * 100.0
    }

    fn parse(raw: &str) -> Option<Self> {
        num(raw).map(Self)
    }
}

/// One dated point on a fund-level daily series.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesPoint {
    pub date: String,
    pub value: Option<f64>,
}

/// One dated point on a fund-level daily series whose value is a proportion.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesFraction {
    pub date: String,
    pub value: Option<Fraction>,
}

/// The daily liquidity buffer, both as dollars and as a share of net assets.
#[derive(Debug, Clone, PartialEq)]
pub struct LiquidAssetsPoint {
    pub date: String,
    pub daily_value: Option<f64>,
    pub weekly_value: Option<f64>,
    pub daily_pct: Option<Fraction>,
    pub weekly_pct: Option<Fraction>,
}

/// One business day of subscriptions and redemptions, per share class.
///
/// The wrapper element is misspelled in the schema — `dialyShareholderFlowReported`,
/// sic — while its date child is spelled `dailyShareHolderFlowDate` with an
/// inconsistent capital `H`. Both are matched verbatim. A parser that "corrects"
/// either never descends into the flow records at all and reports an empty
/// series with no error.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowPoint {
    pub date: String,
    pub subscriptions: Option<f64>,
    pub redemptions: Option<f64>,
}

/// A rating an NRSRO assigned to one position.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rating {
    pub nrsro: Option<String>,
    pub rating: Option<String>,
}

/// One security pledged against a repurchase agreement.
///
/// Repo collateral is itemized inside the same filing, which is unusual: the
/// look-through is filed rather than inferred.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collateral {
    pub issuer: Option<String>,
    pub cusip: Option<String>,
    pub lei: Option<String>,
    pub maturity_date: Option<String>,
    pub coupon: Option<f64>,
    pub principal_amount: Option<f64>,
    pub collateral_value: Option<f64>,
    pub category: Option<String>,
}

/// The repo terms carried on a position, plus its itemized collateral.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RepurchaseAgreement {
    pub open: Option<bool>,
    pub cleared: Option<bool>,
    pub triparty: Option<bool>,
    pub ccp_name: Option<String>,
    pub collateral: Vec<Collateral>,
}

/// One line of the portfolio schedule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NmfpPosition {
    pub issuer: Option<String>,
    pub title: Option<String>,
    pub coupon: Option<f64>,
    pub cusip: Option<String>,
    pub lei: Option<String>,
    pub isin: Option<String>,
    pub category: Option<String>,
    pub brief_description: Option<String>,
    pub eligible: Option<bool>,
    pub maturity_wam: Option<String>,
    pub maturity_wal: Option<String>,
    pub final_legal_maturity: Option<String>,
    pub demand_feature: Option<bool>,
    pub guarantee: Option<bool>,
    pub enhancements: Option<bool>,
    pub yield_at_report_date: Option<Fraction>,
    pub value_including_sponsor_support: Option<f64>,
    pub value_excluding_sponsor_support: Option<f64>,
    pub pct_of_net_assets: Option<Fraction>,
    pub level_3: Option<bool>,
    pub daily_liquid_asset: Option<bool>,
    pub weekly_liquid_asset: Option<bool>,
    pub illiquid: Option<bool>,
    pub explanatory_notes: Option<String>,
    pub ratings: Vec<Rating>,
    pub repo: Option<RepurchaseAgreement>,
}

impl NmfpPosition {
    /// Whether the fund's stated value for this position depends on somebody
    /// stepping in.
    ///
    /// A difference between the two filed values is sponsor support the fund
    /// reported itself. It is a fact, never a judgement about the sponsor.
    pub fn sponsor_support(&self) -> Option<f64> {
        let inc = self.value_including_sponsor_support?;
        let exc = self.value_excluding_sponsor_support?;
        let d = inc - exc;
        (d.abs() >= 0.005).then_some(d)
    }
}

/// How much of the shareholder base each investor type accounts for, per class.
///
/// Filed as fractions. Every field is separately optional: a fund that leaves
/// the whole block blank has not told us its base is zero.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShareholderComposition {
    pub non_financial_corp: Option<Fraction>,
    pub pension_plan: Option<Fraction>,
    pub non_profit: Option<Fraction>,
    pub municipal: Option<Fraction>,
    pub registered_investment_company: Option<Fraction>,
    pub private_fund: Option<Fraction>,
    pub depository_institution: Option<Fraction>,
    pub sovereign_wealth_fund: Option<Fraction>,
    pub broker_dealer: Option<Fraction>,
    pub insurance_company: Option<Fraction>,
    pub other: Option<Fraction>,
    pub other_description: Option<String>,
}

impl ShareholderComposition {
    /// The filed categories that carry a value, largest first, as
    /// `(label, fraction)`.
    ///
    /// A category the fund left blank is omitted rather than rendered as zero:
    /// blank is "not told", and this guide does not turn silence into a number.
    pub fn ranked(&self) -> Vec<(&'static str, Fraction)> {
        let mut out: Vec<(&'static str, Fraction)> = [
            ("Non-financial corporation", self.non_financial_corp),
            ("Pension plan", self.pension_plan),
            ("Non-profit", self.non_profit),
            ("Municipality", self.municipal),
            (
                "Registered investment company",
                self.registered_investment_company,
            ),
            ("Private fund", self.private_fund),
            ("Depository institution", self.depository_institution),
            ("Sovereign wealth fund", self.sovereign_wealth_fund),
            ("Broker-dealer", self.broker_dealer),
            ("Insurance company", self.insurance_company),
            ("Other", self.other),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|f| (k, f)))
        .collect();
        out.sort_by(|a, b| {
            b.1.as_filed()
                .partial_cmp(&a.1.as_filed())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }
}

/// One record-owner concentration line, per class.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OwnerCategory {
    pub category: Option<String>,
    pub other_category: Option<String>,
    pub pct_of_shares_record: Option<Fraction>,
    pub pct_of_shares_beneficial: Option<Fraction>,
}

/// One share class.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NmfpClass {
    pub class_id: Option<String>,
    pub name: Option<String>,
    pub minimum_initial_investment: Option<f64>,
    pub net_assets: Option<f64>,
    pub shares_outstanding: Option<f64>,
    pub daily_nav: Vec<SeriesPoint>,
    pub seven_day_net_yield: Vec<SeriesFraction>,
    pub flows: Vec<FlowPoint>,
    pub monthly_subscriptions: Option<f64>,
    pub monthly_redemptions: Option<f64>,
    /// Whether somebody outside the fund paid one of its expenses this period.
    pub expense_paid_by_person: Option<bool>,
    /// The AMOUNT paid, despite the element being named `nameOfPersonDescExpensePay`.
    pub expense_payment_amount: Option<f64>,
    pub owner_categories: Vec<OwnerCategory>,
    pub composition: ShareholderComposition,
    pub shares_cancelled: Option<bool>,
}

/// A service provider named on the filing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Provider {
    pub name: Option<String>,
    pub file_number: Option<String>,
    pub cik: Option<String>,
    pub city: Option<String>,
    pub state_country: Option<String>,
}

/// Every provider the form names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServiceProviders {
    pub adviser: Provider,
    pub administrator: Provider,
    pub transfer_agent: Provider,
    pub accountant: Provider,
}

/// Which basis the asset identity closes on, and by how much it misses.
///
/// The form files portfolio securities twice, at market value and at amortized
/// cost, and `netAssetOfSeries` reconciles against exactly one of them. Measured
/// across one month of the whole industry (335 filings) the **amortized** basis
/// closes to the cent for 78.8% and the market basis for only 29.6%, because a
/// stable-value fund's net assets are struck on amortized cost. Asserting the
/// market basis would report seventy percent of the industry as broken.
///
/// The residual is not error, it is the material: the difference between the two
/// filed securities totals is the fund's own shadow-price deviation, which is
/// the most on-thesis number in the document. But it is only readable when the
/// amortized identity actually closes — a floating-NAV fund reports amortized
/// cost for the eligible subset only, so differencing the two totals there
/// produces a multi-billion-dollar phantom.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetIdentity {
    pub securities_market: Option<f64>,
    pub securities_amortized: Option<f64>,
    pub cash: Option<f64>,
    pub other_assets: Option<f64>,
    pub liabilities: Option<f64>,
    pub net_assets: Option<f64>,
}

/// Tolerance for "closes to the cent". Half a cent, so rounding either way
/// still counts as closed.
const CENT: f64 = 0.005;

impl AssetIdentity {
    fn residual(&self, securities: Option<f64>) -> Option<f64> {
        Some(securities? + self.cash? + self.other_assets? - self.liabilities? - self.net_assets?)
    }

    /// `securities(market) + cash + other - liabilities - net_assets`.
    pub fn market_gap(&self) -> Option<f64> {
        self.residual(self.securities_market)
    }

    /// `securities(amortized) + cash + other - liabilities - net_assets`.
    pub fn amortized_gap(&self) -> Option<f64> {
        self.residual(self.securities_amortized)
    }

    /// Which basis reconciles to the cent, if either.
    pub fn closes_on(&self) -> Option<&'static str> {
        if self.amortized_gap().is_some_and(|g| g.abs() < CENT) {
            Some("amortized")
        } else if self.market_gap().is_some_and(|g| g.abs() < CENT) {
            Some("market")
        } else {
            None
        }
    }

    /// The fund's own shadow-price deviation in dollars: market value of the
    /// portfolio minus its amortized cost.
    ///
    /// `None` unless the amortized identity closes, because otherwise amortized
    /// cost covers only part of the portfolio and the difference means nothing.
    pub fn shadow_price_deviation(&self) -> Option<f64> {
        if self.closes_on() != Some("amortized") {
            return None;
        }
        Some(self.securities_market? - self.securities_amortized?)
    }
}

/// A daily NAV point that does not belong with the rest of its series.
#[derive(Debug, Clone, PartialEq)]
pub struct NavAnomaly {
    pub date: String,
    pub value: Option<f64>,
    pub reference: f64,
    pub reason: &'static str,
}

/// Everything one N-MFP3 filing carries about one series.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NmfpSeries {
    pub schema_version: Option<String>,
    pub submission_type: Option<String>,
    pub report_date: Option<String>,
    pub registrant_name: Option<String>,
    pub registrant_cik: Option<String>,
    pub registrant_lei: Option<String>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub series_lei: Option<String>,
    pub total_share_classes: Option<i64>,
    pub final_filing: Option<bool>,
    pub acquired_or_merged: Option<bool>,

    pub securities_act_file_number: Option<String>,
    pub category: Option<String>,
    /// The fund's own `govMoneyMrktFundFlag`.
    ///
    /// Do NOT read this as "is this a government fund" and do NOT derive it from
    /// [`category`](Self::category) or the reverse: fourteen of fifteen sampled
    /// funds disagree with themselves on the pair. What the flag means was never
    /// established from the form instructions, so it is carried and not
    /// interpreted.
    pub gov_flag: Option<bool>,
    pub retail_flag: Option<bool>,
    pub master_fund: Option<bool>,
    pub feeder_fund: Option<bool>,
    pub insurance_separate_account: Option<bool>,
    pub cash_management_vehicle: Option<bool>,
    pub liquidity_fee_applies: Option<bool>,

    /// Weighted average maturity, in days, **as filed**. Not derived from the
    /// position ladder.
    pub wam_days: Option<i64>,
    /// Weighted average life, in days, as filed.
    pub wal_days: Option<i64>,

    pub seeks_stable_price: Option<bool>,
    /// The price the fund seeks. `0.0000` is filed as "not applicable" and
    /// normalizes to `None` here. Funds file $1, $10, $31, $100 and $101, so
    /// nothing downstream may assume a dollar.
    pub stable_price: Option<f64>,
    pub shares_outstanding: Option<f64>,

    pub identity: AssetIdentity,

    pub daily_nav: Vec<SeriesPoint>,
    pub seven_day_gross_yield: Vec<SeriesFraction>,
    pub liquid_assets: Vec<LiquidAssetsPoint>,

    pub classes: Vec<NmfpClass>,
    pub positions: Vec<NmfpPosition>,
    pub providers: ServiceProviders,

    pub warnings: Vec<String>,
}

/// Relative distance from the reference price beyond which a daily NAV point is
/// treated as out of band. A money market NAV moves in basis points; 5% is far
/// outside anything a functioning fund files.
const NAV_BAND: f64 = 0.05;

impl NmfpSeries {
    /// The reference price a daily NAV point should sit near.
    ///
    /// The filed stable price when there is one, otherwise the median of the
    /// series' own positive points. A floating-NAV fund files no stable price
    /// and still has a perfectly well-defined level.
    pub fn reference_price(&self) -> Option<f64> {
        if let Some(p) = self.stable_price {
            return Some(p);
        }
        median(
            &self
                .daily_nav
                .iter()
                .filter_map(|p| p.value)
                .filter(|v| *v > 0.0)
                .collect::<Vec<_>>(),
        )
    }

    /// Daily NAV points that do not belong with the rest of their own series.
    ///
    /// **An all-zero series returns nothing.** A master fund in a master-feeder
    /// structure has no per-share NAV to report and files `0.0000` throughout;
    /// fourteen filings in a one-month industry sample do exactly that. Only a
    /// point that is out of band *while its neighbours are not* is reported, and
    /// on the same sample that fires on two filings out of 335.
    ///
    /// This is a **misfiling**: something we detected by arithmetic on the filed
    /// record. It is never a violation, which is reserved for something the SEC
    /// itself flagged, and it carries no motive.
    pub fn nav_anomalies(&self) -> Vec<NavAnomaly> {
        let valid: Vec<f64> = self
            .daily_nav
            .iter()
            .filter_map(|p| p.value)
            .filter(|v| *v > 0.0)
            .collect();
        if valid.is_empty() {
            return Vec::new();
        }
        let Some(reference) = median(&valid) else {
            return Vec::new();
        };
        if reference <= 0.0 {
            return Vec::new();
        }
        self.daily_nav
            .iter()
            .filter_map(|p| {
                let reason = match p.value {
                    None => "no value filed for this date",
                    Some(v) if v <= 0.0 => "non-positive, among valid points in the same series",
                    Some(v) if ((v - reference) / reference).abs() > NAV_BAND => {
                        "more than 5% from the rest of the series"
                    }
                    Some(_) => return None,
                };
                Some(NavAnomaly {
                    date: p.date.clone(),
                    value: p.value,
                    reference,
                    reason,
                })
            })
            .collect()
    }

    /// Whether the whole daily-NAV series is the not-applicable sentinel.
    ///
    /// True for a master fund. Reported so a caller can say "this fund files no
    /// per-share NAV" rather than drawing a flat line at zero.
    pub fn nav_series_not_applicable(&self) -> bool {
        !self.daily_nav.is_empty()
            && self
                .daily_nav
                .iter()
                .all(|p| p.value.is_none_or(|v| v <= 0.0))
    }

    /// The gross-versus-net seven-day yield spread, **per share class**.
    ///
    /// Gross yield is filed at series level and net yield per class, so this is
    /// the only honest shape for the comparison. Joined on date, never on index:
    /// the two series legitimately differ in length, because a seven-day yield
    /// cannot exist until a fund is seven days old.
    ///
    /// Returns `(class_id, date, gross, net, spread)` with the spread as filed
    /// fractions. A class with no overlapping dates contributes nothing.
    pub fn yield_spread_by_class(&self) -> Vec<(String, String, Fraction, Fraction, f64)> {
        let gross: BTreeMap<&str, Fraction> = self
            .seven_day_gross_yield
            .iter()
            .filter_map(|p| p.value.map(|v| (p.date.as_str(), v)))
            .collect();
        let mut out = Vec::new();
        for c in &self.classes {
            let id = c.class_id.clone().unwrap_or_else(|| "unknown".to_string());
            for p in &c.seven_day_net_yield {
                let (Some(net), Some(g)) = (p.value, gross.get(p.date.as_str())) else {
                    continue;
                };
                out.push((
                    id.clone(),
                    p.date.clone(),
                    *g,
                    net,
                    g.as_filed() - net.as_filed(),
                ));
            }
        }
        out
    }

    /// Net assets per share implied by the *market* value of the portfolio.
    ///
    /// The fund files a daily market-based NAV per share directly, so this is a
    /// cross-check on that series rather than a substitute for it: arithmetic on
    /// the fund's own totals, held against the number the fund published.
    ///
    /// Only computable when the amortized identity closes — that is what makes
    /// `net_assets + shadow_price_deviation` the market-basis net assets. A
    /// floating-NAV fund already strikes its NAV on market value and needs no
    /// such reconstruction, and gets `None` here.
    pub fn market_nav_per_share(&self) -> Option<f64> {
        let deviation = self.identity.shadow_price_deviation()?;
        let shares = self.shares_outstanding.filter(|s| *s > 0.0)?;
        Some((self.identity.net_assets? + deviation) / shares)
    }

    /// The market-based NAV per share the fund itself filed for the last day of
    /// the series, if any.
    pub fn last_filed_nav(&self) -> Option<&SeriesPoint> {
        self.daily_nav.iter().rev().find(|p| p.value.is_some())
    }

    /// Positions whose stated value depends on sponsor support, with the amount.
    pub fn supported_positions(&self) -> Vec<(&NmfpPosition, f64)> {
        self.positions
            .iter()
            .filter_map(|p| p.sponsor_support().map(|d| (p, d)))
            .collect()
    }

    /// Every itemized repo collateral line, flattened, with its position's issuer.
    pub fn repo_collateral(&self) -> Vec<(&str, &Collateral)> {
        self.positions
            .iter()
            .filter_map(|p| p.repo.as_ref().map(|r| (p, r)))
            .flat_map(|(p, r)| {
                let issuer = p.issuer.as_deref().unwrap_or("unnamed counterparty");
                r.collateral.iter().map(move |c| (issuer, c))
            })
            .collect()
    }
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    Some(if v.len().is_multiple_of(2) {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    })
}

/// Text of a **direct child** named `tag`.
///
/// This is the accessor the daily series need, and the reason it exists rather
/// than reusing a descendant search. `dailyNetAssetValuePerShareSeries` wraps a
/// child of the same name, so `descendants().find(...)` matches the wrapper,
/// whose own text is whitespace. That reads as an absent value with no error and
/// produces a series of empty points.
fn direct_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<String> {
    node.children()
        .find(|n| n.is_element() && n.has_tag_name(tag))
        .and_then(|n| n.text())
        .and_then(clean)
}

/// Text of the first descendant named `tag`.
///
/// Safe for scalars that appear once inside a scope. **Never use this on a
/// self-nested series element** — see [`direct_text`].
fn descendant_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<String> {
    node.descendants()
        .find(|n| n.is_element() && n.has_tag_name(tag))
        .and_then(|n| n.text())
        .and_then(clean)
}

/// Direct element children named `tag`.
fn kids<'a>(
    node: roxmltree::Node<'a, 'a>,
    tag: &'a str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    node.children()
        .filter(move |n| n.is_element() && n.has_tag_name(tag))
}

fn frac_direct<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<Fraction> {
    direct_text(node, tag).as_deref().and_then(Fraction::parse)
}

fn num_direct<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<f64> {
    direct_text(node, tag).as_deref().and_then(num)
}

fn yn_direct<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<bool> {
    direct_text(node, tag).as_deref().and_then(yn)
}

/// Parse one N-MFP3 `primary_doc.xml`.
///
/// Malformed XML is an error. Missing fields are not: they stay `None`, and a
/// structural miss (no series block, no positions) becomes a warning, because a
/// fund that filed a thin report is a fact about the fund and a hard failure
/// there would hide it.
pub fn parse_nmfp(xml: &str) -> Result<NmfpSeries, String> {
    let doc =
        roxmltree::Document::parse(xml).map_err(|e| format!("N-MFP3 XML is malformed: {e}"))?;
    let root = doc.root_element();

    let mut out = NmfpSeries {
        schema_version: descendant_text(root, "schemaVersion"),
        submission_type: descendant_text(root, "submissionType"),
        ..Default::default()
    };

    if let Some(gi) = root.descendants().find(|n| n.has_tag_name("generalInfo")) {
        out.report_date = direct_text(gi, "reportDate");
        out.registrant_name = direct_text(gi, "registrantFullName");
        out.registrant_cik = direct_text(gi, "cik");
        out.registrant_lei = direct_text(gi, "registrantLEIId");
        out.series_id = direct_text(gi, "seriesId");
        out.series_name = direct_text(gi, "nameOfSeries");
        out.series_lei = direct_text(gi, "leiOfSeries");
        out.total_share_classes = direct_text(gi, "totalShareClassesInSeries")
            .as_deref()
            .and_then(count);
        out.final_filing = yn_direct(gi, "finalFilingFlag");
        out.acquired_or_merged = yn_direct(gi, "fundAcqrdOrMrgdWthAnthrFlag");
    } else {
        out.warnings
            .push("no generalInfo block; the filing may use an unrecognized schema".into());
    }

    match root
        .descendants()
        .find(|n| n.has_tag_name("seriesLevelInfo"))
    {
        Some(si) => parse_series_level(si, &mut out),
        None => out
            .warnings
            .push("no seriesLevelInfo block; nothing fund-level was read".into()),
    }

    for c in root
        .descendants()
        .filter(|n| n.has_tag_name("classLevelInfo"))
    {
        out.classes.push(parse_class(c));
    }
    for p in root
        .descendants()
        .filter(|n| n.has_tag_name("scheduleOfPortfolioSecuritiesInfo"))
    {
        out.positions.push(parse_position(p));
    }

    if out.classes.is_empty() {
        out.warnings.push("no share classes parsed".into());
    }
    if out.positions.is_empty() {
        out.warnings.push("no portfolio positions parsed".into());
    }
    if let Some(declared) = out.total_share_classes {
        let n = out.classes.len() as i64;
        if declared != n {
            out.warnings.push(format!(
                "the filing declares {declared} share classes and carries {n}"
            ));
        }
    }

    Ok(out)
}

fn parse_series_level(si: roxmltree::Node<'_, '_>, out: &mut NmfpSeries) {
    out.securities_act_file_number = direct_text(si, "securitiesActFileNumber");
    out.category = direct_text(si, "moneyMarketFundCategory");
    out.gov_flag = yn_direct(si, "govMoneyMrktFundFlag");
    out.retail_flag = yn_direct(si, "fundRetailMoneyMarketFlag");
    out.master_fund = yn_direct(si, "masterFundFlag");
    out.feeder_fund = yn_direct(si, "feederFundFlag");
    out.insurance_separate_account = yn_direct(si, "seriesFundInsuCmpnySepAccntFlag");
    out.cash_management_vehicle = yn_direct(si, "cashMgmtVehicleAffliatedFundFlag");
    out.liquidity_fee_applies = yn_direct(si, "liquidityFeeFundApplyFlag");
    out.wam_days = direct_text(si, "averagePortfolioMaturity")
        .as_deref()
        .and_then(count);
    out.wal_days = direct_text(si, "averageLifeMaturity")
        .as_deref()
        .and_then(count);
    out.seeks_stable_price = yn_direct(si, "seekStablePricePerShare");
    // 0.0000 is the form's "not applicable", not a price of nothing.
    out.stable_price = num_direct(si, "stablePricePerShare").filter(|p| *p > 0.0);
    out.shares_outstanding = num_direct(si, "numberOfSharesOutstanding");

    out.identity = AssetIdentity {
        securities_market: num_direct(si, "totalValuePortfolioSecurities"),
        securities_amortized: num_direct(si, "amortizedCostPortfolioSecurities"),
        cash: num_direct(si, "cash"),
        other_assets: num_direct(si, "totalValueOtherAssets"),
        liabilities: num_direct(si, "totalValueLiabilities"),
        net_assets: num_direct(si, "netAssetOfSeries"),
    };

    // Self-nested: the wrapper and the value share a name, so read the value as
    // a DIRECT child of the wrapper.
    out.daily_nav = kids(si, "dailyNetAssetValuePerShareSeries")
        .filter_map(|w| {
            direct_text(w, "dailyNetAssetValuePerShareDateSeries").map(|date| SeriesPoint {
                date,
                value: num_direct(w, "dailyNetAssetValuePerShareSeries"),
            })
        })
        .collect();

    out.seven_day_gross_yield = kids(si, "sevenDayGrossYield")
        .filter_map(|w| {
            direct_text(w, "sevenDayGrossYieldDate").map(|date| SeriesFraction {
                date,
                value: frac_direct(w, "sevenDayGrossYieldValue"),
            })
        })
        .collect();

    out.liquid_assets = kids(si, "liquidAssetsDetails")
        .filter_map(|w| {
            direct_text(w, "totalLiquidAssetsNearPercentDate").map(|date| LiquidAssetsPoint {
                date,
                daily_value: num_direct(w, "totalValueDailyLiquidAssets"),
                weekly_value: num_direct(w, "totalValueWeeklyLiquidAssets"),
                daily_pct: frac_direct(w, "percentageDailyLiquidAssets"),
                weekly_pct: frac_direct(w, "percentageWeeklyLiquidAssets"),
            })
        })
        .collect();

    out.providers = ServiceProviders {
        adviser: si
            .children()
            .find(|n| n.is_element() && n.has_tag_name("adviser"))
            .map(|n| Provider {
                name: direct_text(n, "adviserName"),
                file_number: direct_text(n, "adviserFileNumber"),
                ..Default::default()
            })
            .unwrap_or_default(),
        administrator: si
            .children()
            .find(|n| n.is_element() && n.has_tag_name("administrator"))
            .map(|n| Provider {
                name: direct_text(n, "administratorName"),
                ..Default::default()
            })
            .unwrap_or_default(),
        transfer_agent: si
            .children()
            .find(|n| n.is_element() && n.has_tag_name("transferAgent"))
            .map(|n| Provider {
                name: direct_text(n, "name"),
                cik: direct_text(n, "cik"),
                file_number: direct_text(n, "fileNumber"),
                ..Default::default()
            })
            .unwrap_or_default(),
        accountant: si
            .children()
            .find(|n| n.is_element() && n.has_tag_name("indpPubAccountant"))
            .map(|n| Provider {
                name: direct_text(n, "name"),
                city: direct_text(n, "city"),
                state_country: direct_text(n, "stateCountry"),
                ..Default::default()
            })
            .unwrap_or_default(),
    };
}

fn parse_class(c: roxmltree::Node<'_, '_>) -> NmfpClass {
    let monthly = c
        .children()
        .find(|n| n.is_element() && n.has_tag_name("monthlyShareholderFlowReported"));

    NmfpClass {
        class_id: direct_text(c, "classesId"),
        name: direct_text(c, "classFullName"),
        minimum_initial_investment: num_direct(c, "minInitialInvestment"),
        net_assets: num_direct(c, "netAssetsOfClass"),
        shares_outstanding: num_direct(c, "numberOfSharesOutstanding"),

        daily_nav: kids(c, "dailyNetAssetValuePerShareClass")
            .filter_map(|w| {
                direct_text(w, "dailyNetAssetValuePerShareDateClass").map(|date| SeriesPoint {
                    date,
                    value: num_direct(w, "dailyNetAssetValuePerShareClass"),
                })
            })
            .collect(),

        seven_day_net_yield: kids(c, "sevenDayNetYield")
            .filter_map(|w| {
                direct_text(w, "sevenDayNetYieldDate").map(|date| SeriesFraction {
                    date,
                    value: frac_direct(w, "sevenDayNetYieldValue"),
                })
            })
            .collect(),

        // `dialyShareholderFlowReported` is misspelled in the schema and its date
        // child carries an inconsistent capital H. Both matched verbatim.
        flows: kids(c, "dialyShareholderFlowReported")
            .filter_map(|w| {
                direct_text(w, "dailyShareHolderFlowDate").map(|date| FlowPoint {
                    date,
                    subscriptions: num_direct(w, "dailyGrossSubscriptions"),
                    redemptions: num_direct(w, "dailyGrossRedemptions"),
                })
            })
            .collect(),

        monthly_subscriptions: monthly.and_then(|m| num_direct(m, "totalGrossSubscriptions")),
        monthly_redemptions: monthly.and_then(|m| num_direct(m, "totalGrossRedemptions")),

        expense_paid_by_person: yn_direct(c, "personPayForFundFlag"),
        // Named `nameOf...` and filed as a dollar amount.
        expense_payment_amount: num_direct(c, "nameOfPersonDescExpensePay"),

        owner_categories: kids(c, "beneficialRecordOwnerCategory")
            .map(|o| OwnerCategory {
                category: direct_text(o, "beneficialRecordOwnerCategoryType"),
                other_category: direct_text(o, "otherInvestorCategory"),
                pct_of_shares_record: frac_direct(o, "percentOutstandingSharesRecord"),
                pct_of_shares_beneficial: frac_direct(o, "percentOutstandingSharesBeneficial"),
            })
            .collect(),

        composition: ShareholderComposition {
            non_financial_corp: frac_direct(c, "percentShareHolderCompNonFinancialCorp"),
            pension_plan: frac_direct(c, "percentShareHolderCompPensionPlan"),
            non_profit: frac_direct(c, "percentShareHolderCompNonProfit"),
            municipal: frac_direct(c, "percentShareHolderCompMunicipal"),
            registered_investment_company: frac_direct(c, "percentShareHolderCompRegInvestment"),
            private_fund: frac_direct(c, "percentShareHolderCompPrivateFunds"),
            depository_institution: frac_direct(c, "percentShareHolderCompDepositoryInst"),
            sovereign_wealth_fund: frac_direct(c, "percentShareHolderCompSovereignFund"),
            broker_dealer: frac_direct(c, "percentShareHolderCompBrokerDealer"),
            insurance_company: frac_direct(c, "percentShareHolderCompInsurance"),
            other: frac_direct(c, "percentShareHolderCompOther"),
            other_description: direct_text(c, "otherInvestorTypeDescription"),
        },

        shares_cancelled: yn_direct(c, "shareCancellationReportingPeriod"),
    }
}

fn parse_position(p: roxmltree::Node<'_, '_>) -> NmfpPosition {
    NmfpPosition {
        issuer: direct_text(p, "nameOfIssuer"),
        title: direct_text(p, "titleOfIssuer"),
        coupon: num_direct(p, "coupon"),
        cusip: direct_text(p, "CUSIPMember"),
        lei: direct_text(p, "LEIID"),
        isin: direct_text(p, "ISINId"),
        category: direct_text(p, "investmentCategory"),
        brief_description: direct_text(p, "briefDescription"),
        eligible: yn_direct(p, "securityEligibilityFlag"),
        maturity_wam: direct_text(p, "investmentMaturityDateWAM"),
        maturity_wal: direct_text(p, "investmentMaturityDateWAL"),
        final_legal_maturity: direct_text(p, "finalLegalInvestmentMaturityDate"),
        demand_feature: yn_direct(p, "securityDemandFeatureFlag"),
        guarantee: yn_direct(p, "securityGuaranteeFlag"),
        enhancements: yn_direct(p, "securityEnhancementsFlag"),
        yield_at_report_date: frac_direct(p, "yieldOfTheSecurityAsOfReportingDate"),
        value_including_sponsor_support: num_direct(p, "includingValueOfAnySponsorSupport"),
        value_excluding_sponsor_support: num_direct(p, "excludingValueOfAnySponsorSupport"),
        pct_of_net_assets: frac_direct(p, "percentageOfMoneyMarketFundNetAssets"),
        level_3: yn_direct(p, "securityCategorizedAtLevel3Flag"),
        daily_liquid_asset: yn_direct(p, "dailyLiquidAssetSecurityFlag"),
        weekly_liquid_asset: yn_direct(p, "weeklyLiquidAssetSecurityFlag"),
        illiquid: yn_direct(p, "illiquidSecurityFlag"),
        explanatory_notes: direct_text(p, "explanatoryNotes"),

        ratings: kids(p, "assigningNRSRORating")
            .map(|r| Rating {
                nrsro: direct_text(r, "nameOfNRSRO"),
                rating: direct_text(r, "rating"),
            })
            .collect(),

        repo: p
            .children()
            .find(|n| n.is_element() && n.has_tag_name("repurchaseAgreement"))
            .map(|r| RepurchaseAgreement {
                open: yn_direct(r, "repurchaseAgreementOpenFlag"),
                cleared: yn_direct(r, "repurchaseAgreementClearedFlag"),
                triparty: yn_direct(r, "repurchaseAgreementTripartyFlag"),
                ccp_name: direct_text(r, "nameOfCCP"),
                collateral: kids(r, "collateralIssuers")
                    .map(|c| Collateral {
                        issuer: direct_text(c, "nameOfCollateralIssuer"),
                        cusip: direct_text(c, "CUSIPMember"),
                        lei: direct_text(c, "LEIID"),
                        // `maturityDate` wraps a `date` child rather than
                        // carrying the value itself.
                        maturity_date: c
                            .children()
                            .find(|n| n.is_element() && n.has_tag_name("maturityDate"))
                            .and_then(|m| direct_text(m, "date")),
                        coupon: num_direct(c, "coupon"),
                        principal_amount: num_direct(c, "principalAmountToTheNearestCent"),
                        collateral_value: num_direct(c, "valueOfCollateralToTheNearestCent"),
                        category: direct_text(c, "ctgryInvestmentsRprsntsCollateral"),
                    })
                    .collect(),
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built to carry every trap at once, with the shapes taken from live
    /// filings: the self-nested series wrappers, leading-dot fractions, a
    /// class-scoped net yield shorter than the series-scoped gross yield, a
    /// `0.0000` stable price beside `seekStablePricePerShare = N`, an expense
    /// payment filed under a `nameOf...` element, an asset identity that closes
    /// on amortized cost and not on market value, a negative daily NAV among
    /// valid points, and a repo whose collateral nests its maturity date one
    /// level deeper and files `N/A` for the LEI.
    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<edgarSubmission xmlns="http://www.sec.gov/edgar/nmfp3">
  <headerData>
    <submissionType>N-MFP3</submissionType>
  </headerData>
  <formData>
    <generalInfo>
      <reportDate>2026-06-30</reportDate>
      <registrantFullName>Probe Liquidity Trust</registrantFullName>
      <cik>0001234567</cik>
      <nameOfSeries>Probe Prime Portfolio</nameOfSeries>
      <seriesId>S000099999</seriesId>
      <totalShareClassesInSeries>2</totalShareClassesInSeries>
      <finalFilingFlag>N</finalFilingFlag>
    </generalInfo>
    <seriesLevelInfo>
      <securitiesActFileNumber>333-000000</securitiesActFileNumber>
      <adviser>
        <adviserName>Probe Asset Management</adviserName>
        <adviserFileNumber>801-00000</adviserFileNumber>
      </adviser>
      <indpPubAccountant>
        <name>Probe &amp; Co LLP</name>
        <city>Boston</city>
        <stateCountry>MA</stateCountry>
      </indpPubAccountant>
      <administrator>
        <administratorName>Probe Administration</administratorName>
      </administrator>
      <transferAgent>
        <name>Probe Transfer</name>
        <cik>0000000027</cik>
        <fileNumber>084-00000</fileNumber>
      </transferAgent>
      <feederFundFlag>N</feederFundFlag>
      <masterFundFlag>N</masterFundFlag>
      <moneyMarketFundCategory>Prime</moneyMarketFundCategory>
      <fundRetailMoneyMarketFlag>N</fundRetailMoneyMarketFlag>
      <govMoneyMrktFundFlag>N</govMoneyMrktFundFlag>
      <averagePortfolioMaturity>40</averagePortfolioMaturity>
      <averageLifeMaturity>75</averageLifeMaturity>
      <liquidAssetsDetails>
        <totalValueDailyLiquidAssets>402.00</totalValueDailyLiquidAssets>
        <totalValueWeeklyLiquidAssets>559.00</totalValueWeeklyLiquidAssets>
        <percentageDailyLiquidAssets>0.4018</percentageDailyLiquidAssets>
        <percentageWeeklyLiquidAssets>0.5594</percentageWeeklyLiquidAssets>
        <totalLiquidAssetsNearPercentDate>2026-06-01</totalLiquidAssetsNearPercentDate>
      </liquidAssetsDetails>
      <cash>100.00</cash>
      <totalValuePortfolioSecurities>1005.50</totalValuePortfolioSecurities>
      <amortizedCostPortfolioSecurities>1000.00</amortizedCostPortfolioSecurities>
      <totalValueOtherAssets>10.00</totalValueOtherAssets>
      <totalValueLiabilities>20.00</totalValueLiabilities>
      <netAssetOfSeries>1090.00</netAssetOfSeries>
      <numberOfSharesOutstanding>1090.0000</numberOfSharesOutstanding>
      <seekStablePricePerShare>N</seekStablePricePerShare>
      <stablePricePerShare>0.0000</stablePricePerShare>
      <dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareSeries>1.0000</dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareDateSeries>2026-06-01</dailyNetAssetValuePerShareDateSeries>
      </dailyNetAssetValuePerShareSeries>
      <dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareSeries>-0.0144</dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareDateSeries>2026-06-02</dailyNetAssetValuePerShareDateSeries>
      </dailyNetAssetValuePerShareSeries>
      <dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareSeries>1.0000</dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareDateSeries>2026-06-03</dailyNetAssetValuePerShareDateSeries>
      </dailyNetAssetValuePerShareSeries>
      <dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareSeries>0.9999</dailyNetAssetValuePerShareSeries>
        <dailyNetAssetValuePerShareDateSeries>2026-06-04</dailyNetAssetValuePerShareDateSeries>
      </dailyNetAssetValuePerShareSeries>
      <sevenDayGrossYield>
        <sevenDayGrossYieldValue>.0388</sevenDayGrossYieldValue>
        <sevenDayGrossYieldDate>2026-06-01</sevenDayGrossYieldDate>
      </sevenDayGrossYield>
      <sevenDayGrossYield>
        <sevenDayGrossYieldValue>.0388</sevenDayGrossYieldValue>
        <sevenDayGrossYieldDate>2026-06-02</sevenDayGrossYieldDate>
      </sevenDayGrossYield>
      <sevenDayGrossYield>
        <sevenDayGrossYieldValue>.0387</sevenDayGrossYieldValue>
        <sevenDayGrossYieldDate>2026-06-03</sevenDayGrossYieldDate>
      </sevenDayGrossYield>
      <sevenDayGrossYield>
        <sevenDayGrossYieldValue>.0389</sevenDayGrossYieldValue>
        <sevenDayGrossYieldDate>2026-06-04</sevenDayGrossYieldDate>
      </sevenDayGrossYield>
      <liquidityFeeFundApplyFlag>N</liquidityFeeFundApplyFlag>
    </seriesLevelInfo>
    <classLevelInfo>
      <classFullName>Institutional Class</classFullName>
      <classesId>C000000001</classesId>
      <netAssetsOfClass>1000.00</netAssetsOfClass>
      <numberOfSharesOutstanding>1000.0000</numberOfSharesOutstanding>
      <dailyNetAssetValuePerShareClass>
        <dailyNetAssetValuePerShareClass>1.0001</dailyNetAssetValuePerShareClass>
        <dailyNetAssetValuePerShareDateClass>2026-06-03</dailyNetAssetValuePerShareDateClass>
      </dailyNetAssetValuePerShareClass>
      <dailyNetAssetValuePerShareClass>
        <dailyNetAssetValuePerShareClass>0.9998</dailyNetAssetValuePerShareClass>
        <dailyNetAssetValuePerShareDateClass>2026-06-04</dailyNetAssetValuePerShareDateClass>
      </dailyNetAssetValuePerShareClass>
      <dialyShareholderFlowReported>
        <dailyGrossSubscriptions>25.00</dailyGrossSubscriptions>
        <dailyGrossRedemptions>5.00</dailyGrossRedemptions>
        <dailyShareHolderFlowDate>2026-06-03</dailyShareHolderFlowDate>
      </dialyShareholderFlowReported>
      <dialyShareholderFlowReported>
        <dailyGrossSubscriptions>0.00</dailyGrossSubscriptions>
        <dailyGrossRedemptions>40.00</dailyGrossRedemptions>
        <dailyShareHolderFlowDate>2026-06-04</dailyShareHolderFlowDate>
      </dialyShareholderFlowReported>
      <monthlyShareholderFlowReported>
        <totalGrossSubscriptions>25.00</totalGrossSubscriptions>
        <totalGrossRedemptions>45.00</totalGrossRedemptions>
      </monthlyShareholderFlowReported>
      <sevenDayNetYield>
        <sevenDayNetYieldValue>.0343</sevenDayNetYieldValue>
        <sevenDayNetYieldDate>2026-06-03</sevenDayNetYieldDate>
      </sevenDayNetYield>
      <sevenDayNetYield>
        <sevenDayNetYieldValue>.0344</sevenDayNetYieldValue>
        <sevenDayNetYieldDate>2026-06-04</sevenDayNetYieldDate>
      </sevenDayNetYield>
      <personPayForFundFlag>Y</personPayForFundFlag>
      <nameOfPersonDescExpensePay>37393.00</nameOfPersonDescExpensePay>
      <beneficialRecordOwnerCategory>
        <beneficialRecordOwnerCategoryType>Non-financial corporation</beneficialRecordOwnerCategoryType>
        <percentOutstandingSharesRecord>.9960</percentOutstandingSharesRecord>
        <percentOutstandingSharesBeneficial>.9960</percentOutstandingSharesBeneficial>
      </beneficialRecordOwnerCategory>
      <percentShareHolderCompNonFinancialCorp>.7500</percentShareHolderCompNonFinancialCorp>
      <percentShareHolderCompPensionPlan>.2500</percentShareHolderCompPensionPlan>
      <shareCancellationReportingPeriod>N</shareCancellationReportingPeriod>
    </classLevelInfo>
    <classLevelInfo>
      <classFullName>Investor Class</classFullName>
      <classesId>C000000002</classesId>
      <netAssetsOfClass>90.00</netAssetsOfClass>
      <sevenDayNetYield>
        <sevenDayNetYieldValue>.0300</sevenDayNetYieldValue>
        <sevenDayNetYieldDate>2026-06-04</sevenDayNetYieldDate>
      </sevenDayNetYield>
      <personPayForFundFlag>N</personPayForFundFlag>
      <nameOfPersonDescExpensePay>0.00</nameOfPersonDescExpensePay>
    </classLevelInfo>
    <scheduleOfPortfolioSecuritiesInfo>
      <nameOfIssuer>ASB Bank Ltd.</nameOfIssuer>
      <titleOfIssuer>ASB Bank Ltd. 3.940000%</titleOfIssuer>
      <CUSIPMember>0020NACL2</CUSIPMember>
      <LEIID>549300IBZWZL</LEIID>
      <investmentCategory>Financial Company Commercial Paper</investmentCategory>
      <securityEligibilityFlag>Y</securityEligibilityFlag>
      <assigningNRSRORating>
        <nameOfNRSRO>Moody's Investors Service, Inc.</nameOfNRSRO>
        <rating>P-1</rating>
      </assigningNRSRORating>
      <investmentMaturityDateWAM>2026-07-01</investmentMaturityDateWAM>
      <yieldOfTheSecurityAsOfReportingDate>.0376</yieldOfTheSecurityAsOfReportingDate>
      <includingValueOfAnySponsorSupport>50.00</includingValueOfAnySponsorSupport>
      <excludingValueOfAnySponsorSupport>45.00</excludingValueOfAnySponsorSupport>
      <percentageOfMoneyMarketFundNetAssets>.2478</percentageOfMoneyMarketFundNetAssets>
      <securityCategorizedAtLevel3Flag>N</securityCategorizedAtLevel3Flag>
      <dailyLiquidAssetSecurityFlag>N</dailyLiquidAssetSecurityFlag>
      <weeklyLiquidAssetSecurityFlag>N</weeklyLiquidAssetSecurityFlag>
      <illiquidSecurityFlag>N</illiquidSecurityFlag>
    </scheduleOfPortfolioSecuritiesInfo>
    <scheduleOfPortfolioSecuritiesInfo>
      <nameOfIssuer>Barclays Bank plc</nameOfIssuer>
      <titleOfIssuer>Barclays Bank plc, REPO, 3.640%, 7/1/2026</titleOfIssuer>
      <coupon>3.640</coupon>
      <CUSIPMember>0673PA005</CUSIPMember>
      <LEIID>G5GSEF7VJP5I7OUK5573</LEIID>
      <investmentCategory>U.S. Treasury Repurchase Agreement</investmentCategory>
      <repurchaseAgreement>
        <repurchaseAgreementOpenFlag>N</repurchaseAgreementOpenFlag>
        <repurchaseAgreementClearedFlag>N</repurchaseAgreementClearedFlag>
        <repurchaseAgreementTripartyFlag>Y</repurchaseAgreementTripartyFlag>
        <collateralIssuers>
          <nameOfCollateralIssuer>USTR 3 3/4 NOTE BA 27</nameOfCollateralIssuer>
          <LEIID>N/A</LEIID>
          <CUSIPMember>91282CMY4</CUSIPMember>
          <maturityDate>
            <date>2027-04-30</date>
          </maturityDate>
          <coupon>3.75</coupon>
          <principalAmountToTheNearestCent>12.44</principalAmountToTheNearestCent>
          <valueOfCollateralToTheNearestCent>12.49</valueOfCollateralToTheNearestCent>
          <ctgryInvestmentsRprsntsCollateral>U.S. Treasuries (including strips)</ctgryInvestmentsRprsntsCollateral>
        </collateralIssuers>
      </repurchaseAgreement>
      <yieldOfTheSecurityAsOfReportingDate>.0369</yieldOfTheSecurityAsOfReportingDate>
      <includingValueOfAnySponsorSupport>6220.00</includingValueOfAnySponsorSupport>
      <excludingValueOfAnySponsorSupport>6220.00</excludingValueOfAnySponsorSupport>
      <percentageOfMoneyMarketFundNetAssets>.7500</percentageOfMoneyMarketFundNetAssets>
      <dailyLiquidAssetSecurityFlag>Y</dailyLiquidAssetSecurityFlag>
      <weeklyLiquidAssetSecurityFlag>Y</weeklyLiquidAssetSecurityFlag>
      <illiquidSecurityFlag>N</illiquidSecurityFlag>
    </scheduleOfPortfolioSecuritiesInfo>
  </formData>
</edgarSubmission>
"#;

    fn parsed() -> NmfpSeries {
        parse_nmfp(SAMPLE).expect("sample parses")
    }

    #[test]
    fn self_nested_series_reads_the_value_not_the_wrapper() {
        let f = parsed();
        // Four wrappers, four values. A descendant-first lookup would return the
        // wrapper's own whitespace and leave every value None.
        assert_eq!(f.daily_nav.len(), 4);
        assert_eq!(f.daily_nav[0].date, "2026-06-01");
        assert_eq!(f.daily_nav[0].value, Some(1.0));
        assert_eq!(f.daily_nav[3].value, Some(0.9999));
        assert!(
            f.daily_nav.iter().all(|p| p.value.is_some()),
            "every dated point carries a value"
        );

        // The class-level series is the same shape and must not be conflated
        // with the series-level one.
        assert_eq!(f.classes[0].daily_nav.len(), 2);
        assert_eq!(f.classes[0].daily_nav[0].value, Some(1.0001));
    }

    #[test]
    fn percentages_are_fractions_and_a_filed_2478_is_2478_percent() {
        let f = parsed();
        let p = &f.positions[0];
        let pct = p.pct_of_net_assets.expect("filed");
        assert_eq!(pct.as_filed(), 0.2478);
        assert!(
            (pct.as_percent() - 24.78).abs() < 1e-9,
            "a filed .2478 is 24.78 percent, not 0.2478 and not 2478"
        );

        let la = &f.liquid_assets[0];
        assert!((la.daily_pct.unwrap().as_percent() - 40.18).abs() < 1e-9);
        assert!((la.weekly_pct.unwrap().as_percent() - 55.94).abs() < 1e-9);

        // The yield is a fraction too, and leading-dot parses.
        assert_eq!(p.yield_at_report_date.unwrap().as_filed(), 0.0376);
    }

    #[test]
    fn the_daily_series_do_not_share_a_scope() {
        let f = parsed();
        // Gross yield is filed once for the fund; net yield once per class.
        assert_eq!(f.seven_day_gross_yield.len(), 4);
        assert_eq!(f.classes.len(), 2);
        assert_eq!(f.classes[0].seven_day_net_yield.len(), 2);
        assert_eq!(f.classes[1].seven_day_net_yield.len(), 1);
        assert!(
            f.classes.iter().all(|c| c.class_id.is_some()),
            "a class-scoped series is useless without its class id"
        );
    }

    #[test]
    fn yield_spread_joins_on_date_and_never_on_index() {
        let f = parsed();
        let rows = f.yield_spread_by_class();
        // Two classes, three net-yield points between them, all with a matching
        // gross date. Three rows, not four (the gross series length) and not six.
        assert_eq!(rows.len(), 3);

        let inst: Vec<_> = rows.iter().filter(|r| r.0 == "C000000001").collect();
        assert_eq!(inst.len(), 2);
        // Index-zipping would pair the class's FIRST net point with the fund's
        // FIRST gross point, dated 06-01. It is dated 06-03.
        assert_eq!(inst[0].1, "2026-06-03");
        assert_eq!(inst[0].2.as_filed(), 0.0387);
        assert_eq!(inst[0].3.as_filed(), 0.0343);
        assert!((inst[0].4 - 0.0044).abs() < 1e-9);

        let investor: Vec<_> = rows.iter().filter(|r| r.0 == "C000000002").collect();
        assert_eq!(investor.len(), 1);
        assert_eq!(investor[0].1, "2026-06-04");
        assert_eq!(investor[0].2.as_filed(), 0.0389);
    }

    #[test]
    fn stable_price_zero_is_not_applicable_not_a_price() {
        let f = parsed();
        assert_eq!(f.seeks_stable_price, Some(false));
        assert_eq!(f.stable_price, None, "0.0000 means not applicable");

        // A fund that does seek a stable price keeps it, and it is not always $1.
        let hundred = SAMPLE
            .replace(
                "<seekStablePricePerShare>N</seekStablePricePerShare>",
                "<seekStablePricePerShare>Y</seekStablePricePerShare>",
            )
            .replace(
                "<stablePricePerShare>0.0000</stablePricePerShare>",
                "<stablePricePerShare>100.0000</stablePricePerShare>",
            );
        let g = parse_nmfp(&hundred).unwrap();
        assert_eq!(g.stable_price, Some(100.0));
        assert_eq!(g.reference_price(), Some(100.0));
    }

    #[test]
    fn expense_payment_is_an_amount_not_a_name() {
        let f = parsed();
        assert_eq!(f.classes[0].expense_paid_by_person, Some(true));
        assert_eq!(f.classes[0].expense_payment_amount, Some(37393.0));
        // And the no-payment case files 0.00 rather than leaving it blank.
        assert_eq!(f.classes[1].expense_paid_by_person, Some(false));
        assert_eq!(f.classes[1].expense_payment_amount, Some(0.0));
    }

    #[test]
    fn asset_identity_closes_on_amortized_cost_and_not_on_market_value() {
        let f = parsed();
        let id = &f.identity;
        assert!(id.amortized_gap().unwrap().abs() < CENT);
        assert!(
            (id.market_gap().unwrap() - 5.50).abs() < 1e-9,
            "the market basis misses, and that miss is the shadow price"
        );
        assert_eq!(id.closes_on(), Some("amortized"));
        assert!((id.shadow_price_deviation().unwrap() - 5.50).abs() < 1e-9);
    }

    #[test]
    fn a_seeded_corruption_breaks_the_identity() {
        let broken = SAMPLE.replace(
            "<netAssetOfSeries>1090.00</netAssetOfSeries>",
            "<netAssetOfSeries>1090.01</netAssetOfSeries>",
        );
        let f = parse_nmfp(&broken).unwrap();
        assert!(f.identity.amortized_gap().unwrap().abs() >= CENT);
        assert_eq!(f.identity.closes_on(), None);
        assert_eq!(
            f.identity.shadow_price_deviation(),
            None,
            "a shadow price read off an identity that does not close is a phantom"
        );
    }

    #[test]
    fn shadow_price_is_withheld_when_amortized_cost_covers_only_part() {
        // A floating-NAV fund reports amortized cost for the eligible subset
        // only. Differencing the two totals there produced a $5.9bn phantom on a
        // live filing whose NAV was within four basis points of a dollar.
        let partial = SAMPLE
            .replace(
                "<amortizedCostPortfolioSecurities>1000.00</amortizedCostPortfolioSecurities>",
                "<amortizedCostPortfolioSecurities>400.00</amortizedCostPortfolioSecurities>",
            )
            // A floating-NAV fund strikes net assets on market value, so THAT
            // basis is the one that closes.
            .replace(
                "<netAssetOfSeries>1090.00</netAssetOfSeries>",
                "<netAssetOfSeries>1095.50</netAssetOfSeries>",
            );
        let f = parse_nmfp(&partial).unwrap();
        assert_eq!(f.identity.closes_on(), Some("market"));
        assert!(f.identity.amortized_gap().unwrap().abs() > 1.0);
        assert_eq!(
            f.identity.shadow_price_deviation(),
            None,
            "market minus a partial amortized cost is a phantom, not a shadow price"
        );
    }

    #[test]
    fn a_bad_nav_point_among_valid_ones_is_a_misfiling() {
        let f = parsed();
        let bad = f.nav_anomalies();
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].date, "2026-06-02");
        assert_eq!(bad[0].value, Some(-0.0144));
        assert_eq!(bad[0].reference, 1.0);
        assert!(!f.nav_series_not_applicable());
    }

    #[test]
    fn an_all_zero_nav_series_is_a_master_fund_not_a_broken_one() {
        // Fourteen filings in a one-month industry sample file an all-zero daily
        // NAV series because a master fund has no per-share NAV to report. A
        // finder that flagged non-positive points alone would call every one of
        // them broken.
        let master = SAMPLE
            .replace(
                "<dailyNetAssetValuePerShareSeries>1.0000</dailyNetAssetValuePerShareSeries>",
                "<dailyNetAssetValuePerShareSeries>0.0000</dailyNetAssetValuePerShareSeries>",
            )
            .replace(
                "<dailyNetAssetValuePerShareSeries>-0.0144</dailyNetAssetValuePerShareSeries>",
                "<dailyNetAssetValuePerShareSeries>0.0000</dailyNetAssetValuePerShareSeries>",
            )
            .replace(
                "<dailyNetAssetValuePerShareSeries>0.9999</dailyNetAssetValuePerShareSeries>",
                "<dailyNetAssetValuePerShareSeries>0.0000</dailyNetAssetValuePerShareSeries>",
            );
        let f = parse_nmfp(&master).unwrap();
        assert_eq!(f.daily_nav.len(), 4);
        // Assert the zeroes were READ, not merely absent. Without this the test
        // passes just as happily when the accessor is broken and every value
        // comes back None, which is a parser fault wearing this fact's costume.
        assert!(
            f.daily_nav.iter().all(|p| p.value == Some(0.0)),
            "the sentinel is a filed 0.0000, not a missing value"
        );
        assert!(f.nav_series_not_applicable());
        assert!(
            f.nav_anomalies().is_empty(),
            "an all-zero series is the not-applicable sentinel, not a finding"
        );
    }

    #[test]
    fn the_misspelled_flow_wrapper_is_matched_verbatim() {
        let f = parsed();
        let flows = &f.classes[0].flows;
        assert_eq!(flows.len(), 2);
        assert_eq!(flows[0].date, "2026-06-03");
        assert_eq!(flows[0].subscriptions, Some(25.0));
        assert_eq!(flows[1].redemptions, Some(40.0));
        // The correctly-spelled monthly element is a different shape entirely.
        assert_eq!(f.classes[0].monthly_subscriptions, Some(25.0));
        assert_eq!(f.classes[0].monthly_redemptions, Some(45.0));

        // Spelling the wrapper "correctly" finds nothing, silently.
        let corrected = SAMPLE.replace(
            "dialyShareholderFlowReported",
            "dailyShareholderFlowReported",
        );
        let g = parse_nmfp(&corrected).unwrap();
        assert!(
            g.classes[0].flows.is_empty(),
            "this is the failure mode the verbatim match exists to prevent"
        );
    }

    #[test]
    fn repo_collateral_is_read_through_two_levels_of_nesting() {
        let f = parsed();
        let repo = f.positions[1]
            .repo
            .as_ref()
            .expect("second position is a repo");
        assert_eq!(repo.triparty, Some(true));
        assert_eq!(repo.cleared, Some(false));
        assert_eq!(repo.collateral.len(), 1);
        let c = &repo.collateral[0];
        // maturityDate wraps a `date` child rather than carrying the value.
        assert_eq!(c.maturity_date.as_deref(), Some("2027-04-30"));
        // LEIID is the literal string N/A on collateral rows.
        assert_eq!(c.lei, None);
        assert_eq!(c.principal_amount, Some(12.44));
        assert_eq!(c.collateral_value, Some(12.49));

        let flat = f.repo_collateral();
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].0, "Barclays Bank plc");
    }

    #[test]
    fn sponsor_support_is_the_difference_between_two_filed_values() {
        let f = parsed();
        assert_eq!(f.positions[0].sponsor_support(), Some(5.0));
        assert_eq!(
            f.positions[1].sponsor_support(),
            None,
            "equal values are no support, not zero support"
        );
        let supported = f.supported_positions();
        assert_eq!(supported.len(), 1);
        assert_eq!(supported[0].0.issuer.as_deref(), Some("ASB Bank Ltd."));
    }

    #[test]
    fn shareholder_composition_is_per_class_and_omits_what_was_not_filed() {
        let f = parsed();
        let ranked = f.classes[0].composition.ranked();
        assert_eq!(ranked.len(), 2, "only the two filed categories appear");
        assert_eq!(ranked[0].0, "Non-financial corporation");
        assert!((ranked[0].1.as_percent() - 75.0).abs() < 1e-9);
        assert_eq!(ranked[1].0, "Pension plan");
        // The second class filed none of the block; that is silence, not zeroes.
        assert!(f.classes[1].composition.ranked().is_empty());

        let owners = &f.classes[0].owner_categories;
        assert_eq!(owners.len(), 1);
        assert!((owners[0].pct_of_shares_record.unwrap().as_percent() - 99.60).abs() < 1e-9);
    }

    #[test]
    fn wam_and_wal_are_read_as_filed_days() {
        let f = parsed();
        assert_eq!(f.wam_days, Some(40));
        assert_eq!(f.wal_days, Some(75));
    }

    #[test]
    fn the_category_flag_pair_is_carried_and_never_derived() {
        let f = parsed();
        // A Prime fund filing govMoneyMrktFundFlag = N is unremarkable; so is a
        // Government fund filing the same. Neither is derived from the other.
        assert_eq!(f.category.as_deref(), Some("Prime"));
        assert_eq!(f.gov_flag, Some(false));

        let gov = SAMPLE.replace(
            "<moneyMarketFundCategory>Prime</moneyMarketFundCategory>",
            "<moneyMarketFundCategory>Government</moneyMarketFundCategory>",
        );
        let g = parse_nmfp(&gov).unwrap();
        assert_eq!(g.category.as_deref(), Some("Government"));
        assert_eq!(
            g.gov_flag,
            Some(false),
            "fourteen of fifteen sampled funds disagree with themselves here"
        );
    }

    #[test]
    fn identity_and_providers_survive_the_parse() {
        let f = parsed();
        assert_eq!(f.series_id.as_deref(), Some("S000099999"));
        assert_eq!(f.report_date.as_deref(), Some("2026-06-30"));
        assert_eq!(f.submission_type.as_deref(), Some("N-MFP3"));
        assert_eq!(f.total_share_classes, Some(2));
        assert_eq!(
            f.providers.adviser.name.as_deref(),
            Some("Probe Asset Management")
        );
        assert_eq!(f.providers.accountant.city.as_deref(), Some("Boston"));
        assert_eq!(
            f.providers.transfer_agent.cik.as_deref(),
            Some("0000000027")
        );
        assert!(
            f.warnings.is_empty(),
            "the sample is complete: {:?}",
            f.warnings
        );
    }

    #[test]
    fn malformed_is_an_error_and_thin_is_a_warning() {
        assert!(parse_nmfp("<edgarSubmission><unclosed>").is_err());

        let thin = r#"<edgarSubmission><formData><generalInfo>
            <seriesId>S000000001</seriesId>
            <totalShareClassesInSeries>3</totalShareClassesInSeries>
        </generalInfo></formData></edgarSubmission>"#;
        let f = parse_nmfp(thin).expect("a thin filing still parses");
        assert_eq!(f.series_id.as_deref(), Some("S000000001"));
        assert!(f.warnings.iter().any(|w| w.contains("seriesLevelInfo")));
        assert!(f.warnings.iter().any(|w| w.contains("share classes")));
        assert!(f.warnings.iter().any(|w| w.contains("positions")));
    }
}
