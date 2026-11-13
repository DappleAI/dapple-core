//! Display strings for numbers, shared across guides.
//!
//! Separate from [`crate::md`] on purpose. Markdown emission is used today by
//! exactly the four guides that assemble a long document; **number formatting
//! has already escaped that family** — `multi-timelined-video`,
//! `draft-presentation-slides` and `vod-league-of-legends-review` all round
//! numbers and none of them writes a report. Filing this under a document-shaped
//! name would have been wrong on the day it shipped.
//!
//! # What the copies did to each other
//!
//! Measured across the three shipped financial guides on 2026-08-01:
//!
//! | helper | copies | how they diverged |
//! |---|---|---|
//! | `round2` | 9 | **the one that held**: 9 copies, 1 implementation, zero drift |
//! | `scale_num` | 4 | one re-applies the sign it stripped, one **does not** — so `-2e9` renders `2B` in one guide and `-2B` in another, from code that reads identically at a glance |
//! | `fmt_usd` | 3 | all three strip the sign and none restore it |
//!
//! `round2` surviving nine copies intact is worth reading as evidence: a helper
//! simple enough to retype correctly does not need sharing. The ones that drifted
//! drifted in proportion to how much invisible detail they carried.

/// Round to `places` decimals, half away from zero.
///
/// Matches nushell's `math round` (and `f64::round`) so a value formatted here
/// and a value formatted in a script agree to the last digit.
pub fn round_to(value: f64, places: u32) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let factor = 10f64.powi(places as i32);
    (value * factor).round() / factor
}

/// Format a bare magnitude with a `K`/`M`/`B` suffix.
///
/// # The sign is preserved, and that is a fix rather than a preference
///
/// Of the four copies this replaces, one re-applied the sign it had stripped
/// with `math abs` and one did not — so `-2_000_000_000` rendered as `2B` in one
/// guide and `-2B` in another. A negative number printing as positive is
/// indistinguishable, on the page, from a real gain.
///
/// A caller who genuinely wants a magnitude takes the absolute value itself
/// (`($v | math abs) | fmt scale`), which makes discarding a sign a **visible
/// decision at the call site** instead of a property of the formatter nobody
/// reads.
pub fn scale_num(value: f64, places: u32) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let a = value.abs();
    let (scaled, suffix) = if a >= 1e9 {
        (a / 1e9, "B")
    } else if a >= 1e6 {
        (a / 1e6, "M")
    } else if a >= 1e3 {
        (a / 1e3, "K")
    } else {
        (a, "")
    };
    format!("{sign}{}{suffix}", trim_float(round_to(scaled, places)))
}

/// Format money.
///
/// Precision **decreases as magnitude grows** — two decimals at `T`, one at `B`,
/// none at `M` and below — which is not an oversight: `$1.23T`, `$4.5B` and
/// `$45M` each carry about the same number of meaningful digits, and a reader
/// handed `$1,234,567,891` has been given precision nobody can use.
///
/// The sign is preserved as `-$1.5B`, for the reason in [`scale_num`]: all three
/// copies this replaces stripped it and none restored it.
pub fn fmt_usd(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let a = value.abs();
    let (scaled, places, suffix) = if a >= 1e12 {
        (a / 1e12, 2, "T")
    } else if a >= 1e9 {
        (a / 1e9, 1, "B")
    } else if a >= 1e6 {
        (a / 1e6, 0, "M")
    } else {
        (a, 0, "")
    };
    format!("{sign}${}{suffix}", trim_float(round_to(scaled, places)))
}

/// Render a float without a trailing `.0`, so `2.0` prints `2` the way a
/// nushell `into string` on a rounded value does.
fn trim_float(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_num_keeps_the_sign() {
        // THE regression this consolidation exists to prevent: one of the four
        // copies stripped the sign with `math abs` and never restored it.
        assert_eq!(scale_num(-2.0e9, 2), "-2B");
        assert_eq!(scale_num(2.0e9, 2), "2B");
        assert_eq!(scale_num(1.234e6, 2), "1.23M");
        assert_eq!(scale_num(-1_500.0, 2), "-1.5K");
        assert_eq!(scale_num(42.0, 2), "42");
        assert_eq!(scale_num(0.0, 2), "0");
    }

    #[test]
    fn scale_num_switches_suffix_exactly_at_the_boundary() {
        // 999 999 is still K and 1 000 000 is M; an off-by-one here would print
        // "1000K" beside "1M" in the same column.
        assert_eq!(scale_num(999_999.0, 2), "1000K");
        assert_eq!(scale_num(1_000_000.0, 2), "1M");
        assert_eq!(scale_num(999.0, 2), "999");
        assert_eq!(scale_num(1_000.0, 2), "1K");
    }

    #[test]
    fn fmt_usd_loses_precision_as_it_gains_magnitude() {
        assert_eq!(fmt_usd(1.2345e12), "$1.23T");
        assert_eq!(fmt_usd(1.2345e9), "$1.2B");
        assert_eq!(fmt_usd(1.2345e6), "$1M");
        assert_eq!(fmt_usd(1234.0), "$1234");
        assert_eq!(fmt_usd(-1.2345e9), "-$1.2B");
    }

    #[test]
    fn the_minus_leads_the_dollar_sign() {
        // "-$1.2B", not "$-1.2B": the first is how money is written, the second
        // reads as a typo and sorts differently in a column of figures.
        assert!(fmt_usd(-1.2e9).starts_with("-$"));
    }

    #[test]
    fn rounding_is_half_away_from_zero_like_nushell() {
        assert_eq!(round_to(2.5, 0), 3.0);
        assert_eq!(round_to(-2.5, 0), -3.0);
        assert_eq!(round_to(1.23456, 3), 1.235);
        assert_eq!(round_to(1.0, 2), 1.0);
    }

    #[test]
    fn a_non_finite_value_yields_nothing_rather_than_the_word_nan() {
        // `NaN` on the page is worse than a blank: it reads as a value.
        assert_eq!(scale_num(f64::NAN, 2), "");
        assert_eq!(scale_num(f64::INFINITY, 2), "");
        assert_eq!(fmt_usd(f64::INFINITY), "");
        assert_eq!(fmt_usd(f64::NEG_INFINITY), "");
        assert!(round_to(f64::NAN, 2).is_nan());
    }
}
