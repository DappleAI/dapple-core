//! Filing deadlines, and the misfiling judgement built on them.
//!
//! Pure logic — no network, no nushell.
//!
//! # Misfiling is not violation
//!
//! A filing that arrived after its due date is a **misfiling**: something *we*
//! detected by arithmetic. It is never a **violation**, which is reserved for
//! something the SEC itself flagged (a comment letter, an enforcement action).
//! Blurring the two would let a routine late filing read as a regulatory finding.
//!
//! # The deadline is a versioned table, not a constant
//!
//! Rules change, and a filing must be judged against the rule in force **at its
//! own period**, not today's. Equally important: a rule we have not confirmed
//! yields [`Timeliness::Unknown`], never `Late`. A wrong deadline does not
//! produce a few wrong labels — it manufactures misfilings out of compliant
//! funds, which is worse than reporting none at all.
//!
//! # Say the base rate or say nothing
//!
//! "Late filing appears common" is a reassurance we made up. "12 days late; 34%
//! of filings that period were also late" is a fact. [`base_rate`] computes the
//! latter from real (period, filed) pairs the caller already holds.

/// One deadline rule, valid over a period range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineRule {
    /// Form this applies to, uppercase, amendment suffix already stripped.
    pub form: &'static str,
    /// Inclusive ISO date from which this rule governs a filing's PERIOD.
    pub period_from: &'static str,
    /// Exclusive ISO end, or `None` for "still in force".
    pub period_until: Option<&'static str>,
    pub days_after_period_end: i64,
    /// Whether this rule has been verified against the regulation.
    ///
    /// `false` means the guide must not call anything late under it. Set this
    /// `true` only after reading the rule text, not after it merely looks right.
    pub confirmed: bool,
    pub note: &'static str,
}

/// Deadline rules, newest period range last.
///
/// The 60-day N-PORT rule is corroborated by observed filing lags of 58, 57 and
/// 56 days across three consecutive filings for one series.
///
/// The monthly-regime row is **deliberately unconfirmed**. The SEC's August 2024
/// amendments move N-PORT toward monthly public filing on a phased schedule, and
/// live data through mid-2026 still shows quarterly filing at ~60 days. Until the
/// compliance date and the monthly deadline are read from the rule text, filings
/// in that window return `Unknown` rather than being judged.
pub const DEADLINES: &[DeadlineRule] = &[
    DeadlineRule {
        form: "NPORT-P",
        period_from: "2019-01-01",
        period_until: None,
        days_after_period_end: 60,
        confirmed: true,
        note: "60 days after period end; corroborated by observed lags of 56-58 days",
    },
    DeadlineRule {
        form: "N-CEN",
        period_from: "2018-01-01",
        period_until: None,
        days_after_period_end: 75,
        confirmed: false,
        note: "UNCONFIRMED: N-CEN deadline not yet read from the rule text",
    },
    DeadlineRule {
        form: "N-CSR",
        period_from: "2003-01-01",
        period_until: None,
        days_after_period_end: 70,
        confirmed: false,
        note: "UNCONFIRMED: N-CSR deadline not yet read from the rule text",
    },
    // N-MFP3 is the one form here whose deadline is not expressible in this
    // table AT ALL. It is set in BUSINESS days after a period end that is itself
    // the last business day of the month, and this module holds no holiday
    // calendar — 30 June 2026 filed 8 July is five business days once the
    // observed Independence Day is taken out, and seven calendar days is the
    // same rule in a month with no holiday. Encoding it as calendar days would
    // manufacture misfilings out of compliant funds in exactly the months where
    // a holiday falls, which is worse than judging nothing.
    //
    // So it ships unconfirmed and NOTHING is ever called late under it. The
    // observed lag is still reported, and [`lag_percentile`] gives it a real
    // denominator from the whole industry's filings for the same month, which
    // is stronger evidence than a rule comparison would have been.
    DeadlineRule {
        form: "N-MFP3",
        period_from: "2024-06-11",
        period_until: None,
        days_after_period_end: 7,
        confirmed: false,
        note: "UNCONFIRMED: five BUSINESS days after the period end, which this \
               calendar-day table cannot express; observed lags run 5 to 10 days, median 7",
    },
];

/// The verdict on one filing's timeliness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timeliness {
    OnTime {
        lag_days: i64,
        allowed_days: i64,
    },
    /// A **misfiling**, detected by us. Never a violation.
    Misfiled {
        lag_days: i64,
        allowed_days: i64,
        days_over: i64,
    },
    /// No judgement is possible or permitted. Carries why.
    Unknown {
        lag_days: Option<i64>,
        reason: &'static str,
    },
}

impl Timeliness {
    pub fn label(&self) -> &'static str {
        match self {
            Self::OnTime { .. } => "on_time",
            Self::Misfiled { .. } => "misfiled",
            Self::Unknown { .. } => "unknown",
        }
    }

    pub fn lag_days(&self) -> Option<i64> {
        match self {
            Self::OnTime { lag_days, .. } | Self::Misfiled { lag_days, .. } => Some(*lag_days),
            Self::Unknown { lag_days, .. } => *lag_days,
        }
    }

    /// Operator-facing sentence. States the fact and withholds the motive.
    ///
    /// `base_rate` is the share of comparable filings that were also late, as a
    /// percentage. Supplying it replaces an invented reassurance with a measured
    /// one; omitting it falls back to a plain qualifier.
    pub fn sentence(&self, base_rate: Option<f64>) -> String {
        match self {
            Self::OnTime {
                lag_days,
                allowed_days,
            } => {
                format!(
                    "Filed {lag_days} days after the period end, within the {allowed_days}-day requirement."
                )
            }
            Self::Misfiled {
                lag_days,
                allowed_days,
                days_over,
            } => {
                let context = match base_rate {
                    Some(pct) => {
                        format!(" {pct:.0}% of comparable filings that period were also late.")
                    }
                    None => " Late filing appears common and does not by itself indicate anything."
                        .to_string(),
                };
                format!(
                    "Filed {lag_days} days after the period end against a {allowed_days}-day \
                     requirement, {days_over} days over.{context}"
                )
            }
            Self::Unknown {
                lag_days: Some(d),
                reason,
            } => {
                format!("Filed {d} days after the period end. Not judged: {reason}.")
            }
            Self::Unknown {
                lag_days: None,
                reason,
            } => {
                format!("Filing timeliness not assessed: {reason}.")
            }
        }
    }
}

/// Days between two ISO `YYYY-MM-DD` dates (`filed - period`).
pub fn lag_days(period: &str, filed: &str) -> Option<i64> {
    Some(to_civil_days(filed)? - to_civil_days(period)?)
}

/// Days from a fixed epoch (Howard Hinnant's `days_from_civil`).
fn to_civil_days(s: &str) -> Option<i64> {
    let mut it = s.split('-');
    let y: i64 = it.next()?.trim().parse().ok()?;
    let m: i64 = it.next()?.trim().parse().ok()?;
    let d: i64 = it.next()?.trim().parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Strip an amendment suffix and uppercase: `NPORT-P/A` → `NPORT-P`.
fn normalize_form(form: &str) -> String {
    let upper = form.trim().to_ascii_uppercase();
    upper
        .strip_suffix("/A")
        .unwrap_or(&upper)
        .trim()
        .to_string()
}

/// The rule governing `form` at `period`, if any.
pub fn rule_for(form: &str, period: &str) -> Option<&'static DeadlineRule> {
    let f = normalize_form(form);
    let p = to_civil_days(period)?;
    DEADLINES.iter().find(|r| {
        r.form == f
            && to_civil_days(r.period_from).is_some_and(|s| p >= s)
            && r.period_until.and_then(to_civil_days).is_none_or(|e| p < e)
    })
}

/// Judge one filing.
///
/// Returns [`Timeliness::Unknown`] — carrying the observed lag, which is always
/// worth reporting — when there is no rule, or when the governing rule is not
/// confirmed. Being unable to judge is not the same as judging compliant, and
/// neither is the same as judging late.
pub fn assess(form: &str, period: &str, filed: &str) -> Timeliness {
    let lag = lag_days(period, filed);
    let Some(rule) = rule_for(form, period) else {
        return Timeliness::Unknown {
            lag_days: lag,
            reason: "no deadline rule covers this form and period",
        };
    };
    if !rule.confirmed {
        return Timeliness::Unknown {
            lag_days: lag,
            reason: "the deadline in force for this period is unconfirmed",
        };
    }
    let Some(lag_days) = lag else {
        return Timeliness::Unknown {
            lag_days: None,
            reason: "the filing or period date could not be read",
        };
    };
    let allowed = rule.days_after_period_end;
    if lag_days > allowed {
        Timeliness::Misfiled {
            lag_days,
            allowed_days: allowed,
            days_over: lag_days - allowed,
        }
    } else {
        Timeliness::OnTime {
            lag_days,
            allowed_days: allowed,
        }
    }
}

/// Where one filing's lag sits among a population of lags, with no deadline
/// rule involved at all.
///
/// Returns `(filed_later, level, measured)`. The sentence it supports is
/// *"filed 8 days after the period end; across the 325 funds filing that month
/// the median was 7 days and 41 filed later"* — every term of which is
/// measured. [`assess`] can say nothing at all when the governing rule is
/// unconfirmed, and [`base_rate`] then returns `None` because nothing was
/// judgeable, so for a form like N-MFP3 this is the only honest statement
/// available. It is also the stronger one: it compares the fund against what
/// its peers actually did rather than against our reading of a rule.
///
/// Ties are counted apart rather than folded into either side, because the
/// subject filing is normally IN the population and "filed later than 41 of 325"
/// plus "level with 12" is the whole truth where either number alone is a
/// choice.
pub fn lag_percentile(lag: i64, population: &[i64]) -> (usize, usize, usize) {
    let later = population.iter().filter(|d| **d > lag).count();
    let level = population.iter().filter(|d| **d == lag).count();
    (later, level, population.len())
}

/// The median of a lag population, for the same sentence.
pub fn lag_median(population: &[i64]) -> Option<f64> {
    if population.is_empty() {
        return None;
    }
    let mut v = population.to_vec();
    v.sort_unstable();
    let mid = v.len() / 2;
    Some(if v.len().is_multiple_of(2) {
        (v[mid - 1] + v[mid]) as f64 / 2.0
    } else {
        v[mid] as f64
    })
}

/// Share of `(period, filed)` pairs that were late, as a percentage.
///
/// Returns `None` when nothing in the population could be judged — which is the
/// honest answer, and the caller must then fall back to the plain qualifier
/// rather than printing `0%`.
pub fn base_rate(form: &str, pairs: &[(String, String)]) -> Option<f64> {
    let mut judged = 0usize;
    let mut late = 0usize;
    for (period, filed) in pairs {
        match assess(form, period, filed) {
            Timeliness::Misfiled { .. } => {
                judged += 1;
                late += 1;
            }
            Timeliness::OnTime { .. } => judged += 1,
            Timeliness::Unknown { .. } => {}
        }
    }
    (judged > 0).then(|| late as f64 / judged as f64 * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_spans_months_and_leap_years() {
        assert_eq!(lag_days("2026-03-31", "2026-05-28"), Some(58));
        assert_eq!(
            lag_days("2024-02-28", "2024-03-01"),
            Some(2),
            "2024 is a leap year"
        );
        assert_eq!(lag_days("2025-12-31", "2026-02-26"), Some(57));
        assert_eq!(lag_days("garbage", "2026-01-01"), None);
    }

    /// The observed lags that corroborate the 60-day rule.
    #[test]
    fn observed_filings_are_on_time() {
        for (period, filed, lag) in [
            ("2026-03-31", "2026-05-28", 58),
            ("2025-12-31", "2026-02-26", 57),
            ("2025-09-30", "2025-11-25", 56),
        ] {
            assert_eq!(
                assess("NPORT-P", period, filed),
                Timeliness::OnTime {
                    lag_days: lag,
                    allowed_days: 60
                }
            );
        }
    }

    #[test]
    fn a_late_filing_is_a_misfiling_not_a_violation() {
        let t = assess("NPORT-P", "2026-03-31", "2026-06-10");
        assert_eq!(
            t,
            Timeliness::Misfiled {
                lag_days: 71,
                allowed_days: 60,
                days_over: 11
            }
        );
        assert_eq!(t.label(), "misfiled");
        let s = t.sentence(None);
        assert!(s.contains("71 days"), "{s}");
        assert!(s.contains("11 days over"), "{s}");
        // States the fact, withholds the motive.
        for forbidden in ["suspicious", "deliberate", "concealed", "violation"] {
            assert!(!s.to_lowercase().contains(forbidden), "editorializing: {s}");
        }
    }

    /// A measured base rate replaces the invented reassurance.
    #[test]
    fn base_rate_replaces_the_hand_wave() {
        let t = assess("NPORT-P", "2026-03-31", "2026-06-10");
        assert!(t.sentence(None).contains("appears common"));
        let with_rate = t.sentence(Some(34.0));
        assert!(
            with_rate.contains("34% of comparable filings"),
            "{with_rate}"
        );
        assert!(!with_rate.contains("appears common"), "{with_rate}");
    }

    /// An UNCONFIRMED rule must never produce a misfiling. A wrong deadline
    /// manufactures misfilings out of compliant funds.
    #[test]
    fn unconfirmed_rules_never_call_anything_late() {
        // N-CEN's row is unconfirmed, and this filing is a year late by its
        // nominal 75 days — still Unknown, not Misfiled.
        let t = assess("N-CEN", "2025-01-31", "2026-06-01");
        assert!(matches!(t, Timeliness::Unknown { .. }), "{t:?}");
        assert_eq!(
            t.lag_days(),
            Some(486),
            "the observed lag is still reported"
        );
        assert!(t.sentence(None).contains("Not judged"));
    }

    #[test]
    fn amendments_are_judged_under_the_base_form() {
        assert_eq!(
            assess("NPORT-P/A", "2026-03-31", "2026-05-28"),
            Timeliness::OnTime {
                lag_days: 58,
                allowed_days: 60
            }
        );
    }

    #[test]
    fn an_unknown_form_is_unjudged_but_still_measured() {
        let t = assess("10-K", "2026-03-31", "2026-05-28");
        assert!(matches!(t, Timeliness::Unknown { .. }));
        assert_eq!(t.lag_days(), Some(58));
    }

    #[test]
    fn base_rate_counts_only_judgeable_filings() {
        let pairs: Vec<(String, String)> = [
            ("2026-03-31", "2026-06-10"), // late
            ("2025-12-31", "2026-02-26"), // on time
            ("2025-09-30", "2025-11-25"), // on time
            ("2025-06-30", "2025-09-30"), // late
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(base_rate("NPORT-P", &pairs), Some(50.0));

        // Nothing judgeable => None, so the caller falls back to the plain
        // qualifier rather than printing a misleading 0%.
        assert_eq!(base_rate("N-CEN", &pairs), None);
        assert_eq!(base_rate("NPORT-P", &[]), None);
    }

    #[test]
    fn n_mfp3_reports_its_lag_and_never_calls_anything_late() {
        // Five business days across a month with a holiday is eight calendar
        // days. A calendar-day rule of seven would call this compliant filing
        // late, which is why the row is unconfirmed.
        let t = assess("N-MFP3", "2026-06-30", "2026-07-08");
        assert_eq!(t.label(), "unknown");
        assert_eq!(t.lag_days(), Some(8));
        assert!(t.sentence(None).contains("Not judged"));

        // Even an absurdly late filing is not called late under an unconfirmed
        // rule; the lag is reported and the judgement is withheld.
        let very = assess("N-MFP3", "2026-06-30", "2027-06-30");
        assert_eq!(very.label(), "unknown");
        assert_eq!(very.lag_days(), Some(365));

        // And no base rate is available, which is the honest answer given no
        // rule was applied.
        let pairs = vec![
            ("2026-06-30".to_string(), "2026-07-08".to_string()),
            ("2026-05-29".to_string(), "2026-06-05".to_string()),
        ];
        assert_eq!(base_rate("N-MFP3", &pairs), None);

        // The amendment suffix still resolves to the same row.
        assert!(rule_for("N-MFP3/A", "2026-06-30").is_some());
        // A different form version does NOT silently inherit it.
        assert!(rule_for("N-MFP2", "2026-06-30").is_none());
    }

    #[test]
    fn a_lag_gets_a_denominator_without_any_rule() {
        // The population a month of the whole industry produces.
        let population = vec![5, 7, 7, 7, 8, 8, 10, 7, 7, 8];
        let (later, level, of) = lag_percentile(8, &population);
        assert_eq!(of, 10);
        assert_eq!(later, 1, "only the 10-day filer was slower");
        assert_eq!(level, 3, "three filings, including this one, sat at 8 days");
        assert_eq!(lag_median(&population), Some(7.0));

        // The fastest filer has everybody behind it and no ties but itself.
        let (later, level, _) = lag_percentile(5, &population);
        assert_eq!((later, level), (9, 1));

        // An empty population yields no median rather than a zero.
        assert_eq!(lag_median(&[]), None);
        assert_eq!(lag_percentile(8, &[]), (0, 0, 0));
    }
}
