//! Sampling a long document for a model's budget, without betting on its genre.
//!
//! # The bet that `str substring 0..N` makes
//!
//! Truncating a filing by position looks like a neutral token budget. It is not:
//! it is a bet that the first N characters are the most important ones, and
//! whether that bet holds depends entirely on the **genre** of the document.
//!
//! | genre | head is | head-truncation |
//! |---|---|---|
//! | shareholder letter, 497 supplement, proxy cover | the good news | **biased** |
//! | SEC comment letter (UPLOAD/CORRESP) | the staff's first and usually most serious question | fine |
//! | a 10-K Item section | the filer's own ordering, boilerplate at the tail | usually fine |
//! | a long proxy (DEF 14A) | procedural front matter; comp tables sit deep | suspect |
//!
//! Measured on a real 497 (228,787 chars, a mutual-fund-to-ETF conversion): a
//! 1,200-char head captured `"tax-free reorganization"` at char 947 and reached
//! **none** of `"will not recognize a taxable gain"` (2,453),
//! `"which will be taxable"` (11,683) or `"likely a taxable event"` (19,265). A
//! 497 opens *"Dear Valued Shareholder, we are pleased to inform you"* and puts
//! every qualification in the Q&A below, so the head captured the reassurance
//! and cut the carve-out **every time, by construction**.
//!
//! # The replacement, and the three ways the bias came back
//!
//! A short head for *what happened*, plus a window around each marked passage,
//! stamped with its character offset. Each of these was a real regression during
//! the original build and each is asserted against here:
//!
//! 1. **First-hit-per-marker.** The earliest instance of a word is the one the
//!    author led with. `"taxable"` first appears inside the reassurance, so a
//!    first-hit rule never looks at the carve-outs. [`marker_windows`] takes
//!    **every** occurrence.
//! 2. **Keep-first-N on overflow.** That is head-bias in a new place: a filing
//!    with forty marked passages would be read entirely from its opening pages.
//!    Overflow **spreads evenly, including the first AND last** window, because
//!    the tail is where "consult your tax advisor" lives.
//! 3. **Generic markers.** Measured on that filing: `reorganiz` **308** hits
//!    (it is the name of the agreement), `taxable` 38, `capital gain` 32 —
//!    against `taxable event` **1** and `will not recognize` 2. A generic marker
//!    floods the budget with restatements of a fact the opening already gave and
//!    silently crowds out every specific one. **Markers are PHRASES.** Adding one
//!    is a deliberate act: measure its hit count on a real document first.
//!
//! And the cap **says how many passages it did not show** ([`WindowResult::dropped`]).
//! A cap that bit silently would rebuild the same bias a fourth time.

/// A character range lifted from the source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// Inclusive char offset into the source.
    pub start: usize,
    /// Exclusive char offset into the source.
    pub end: usize,
}

/// What [`marker_windows`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowResult {
    /// Total length of the source, in chars.
    pub length: usize,
    /// Where the head ends (clamped to `length`).
    pub head_end: usize,
    /// The kept windows, in document order, non-overlapping.
    pub windows: Vec<Window>,
    /// Marked passages found but not shown, because the cap bit. **A caller must
    /// state this**; a silent cap reads as "we covered everything" when it did not.
    pub dropped: usize,
}

/// Find a window around every marked passage outside the head.
///
/// Offsets are **char** offsets, matching nushell's `str substring`.
///
/// - Every occurrence of every marker counts, not the first per marker.
/// - A hit inside the head is skipped: it is already on the page, and showing it
///   twice spends budget a later carve-out needs.
/// - Overlapping windows merge, so a dense paragraph carrying six markers is
///   read once rather than six times.
/// - On overflow the kept windows are spread evenly across the document and
///   always include the first and the last.
///
/// # `before` and `after` are separate on purpose
///
/// A marker phrase is usually a *label* and the material it labels **follows**
/// it: `"termination fee"` names the clause, and the fee, the triggers and the
/// carve-outs come after. The shipped guide windows are 700 before against 2,600
/// after for exactly that reason. A symmetric radius would spend three quarters
/// of its budget on the paragraph above the thing it was looking for.
pub fn marker_windows(
    text: &str,
    markers: &[String],
    head: usize,
    before: usize,
    after: usize,
    max_windows: usize,
) -> WindowResult {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let head_end = head.min(len);
    let lower: Vec<char> = chars
        .iter()
        // One char in, one char out, so the index mapping to `chars` holds.
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();

    let mut hits: Vec<usize> = Vec::new();
    for marker in markers {
        let needle: Vec<char> = marker
            .chars()
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect();
        if needle.is_empty() || needle.len() > len {
            continue;
        }
        for i in head_end..=(len - needle.len()) {
            // A hit inside the head is already on the page; showing it twice
            // spends budget that a later carve-out needs.
            if lower[i..i + needle.len()] == needle[..] {
                hits.push(i);
            }
        }
    }
    hits.sort_unstable();
    hits.dedup();

    // Merge overlapping windows.
    let mut merged: Vec<Window> = Vec::new();
    for at in hits {
        let start = at.saturating_sub(before);
        let end = (at + after).min(len);
        match merged.last_mut() {
            Some(last) if start <= last.end => last.end = last.end.max(end),
            _ => merged.push(Window { start, end }),
        }
    }

    let total = merged.len();
    let kept = if max_windows == 0 {
        Vec::new()
    } else if total <= max_windows {
        merged
    } else if max_windows == 1 {
        // With room for one, take the LAST: the head already carries the
        // opening, so the tail is the part nothing else has shown.
        vec![merged[total - 1].clone()]
    } else {
        let mut picked: Vec<Window> = (0..max_windows)
            .map(|k| merged[k * (total - 1) / (max_windows - 1)].clone())
            .collect();
        picked.dedup_by(|a, b| a.start == b.start);
        picked
    };

    WindowResult {
        length: len,
        head_end,
        windows: kept.clone(),
        dropped: total - kept.len(),
    }
}

/// Lift a char range out of `text`.
pub fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(items: &[&str]) -> Vec<String> {
        items.iter().map(|i| i.to_string()).collect()
    }

    /// A synthetic document shaped like the 497 that motivated this: the
    /// reassurance up front, the carve-out far below.
    fn shareholder_letter() -> String {
        let mut s = String::from("Dear Valued Shareholder, we are pleased to inform you ");
        s.push_str("that the transaction is intended to be a tax-free reorganization. ");
        s.push_str(&"filler. ".repeat(2000)); // ~16k chars of Q&A boilerplate
        s.push_str("Fractional shares will be redeemed for cash, which will be taxable.");
        s
    }

    #[test]
    fn a_marked_passage_deep_in_the_document_is_reached() {
        // The whole point. Assert against an offset far outside any plausible
        // head window, or the test passes on the broken implementation too.
        let text = shareholder_letter();
        let hit = text.find("which will be taxable").unwrap();
        assert!(hit > 15_000, "fixture must place the carve-out deep");

        let r = marker_windows(&text, &m(&["will be taxable"]), 800, 300, 300, 10);
        assert_eq!(r.windows.len(), 1);
        assert!(r.windows[0].start > 15_000);
        let lifted = slice_chars(&text, r.windows[0].start, r.windows[0].end);
        assert!(lifted.contains("which will be taxable"), "got: {lifted}");
    }

    #[test]
    fn every_occurrence_counts_not_the_first_per_marker() {
        // Regression 1. A first-hit rule would find only the char-10 hit and
        // never look at the one at the end.
        let text = format!("xxx taxable {} taxable yyy", "z".repeat(5000));
        let r = marker_windows(&text, &m(&["taxable"]), 4, 10, 10, 10);
        assert_eq!(r.windows.len(), 2, "both occurrences must open a window");
    }

    #[test]
    fn a_hit_inside_the_head_is_not_windowed_twice() {
        let text = format!("the tax-free reorganization {}", "z".repeat(2000));
        let r = marker_windows(&text, &m(&["tax-free"]), 800, 300, 300, 10);
        assert!(
            r.windows.is_empty(),
            "the head already carries it; a second copy spends budget"
        );
    }

    #[test]
    fn overlapping_windows_merge() {
        // Two markers three chars apart must read as one passage, not two.
        let text = format!("{}alpha beta{}", "z".repeat(1000), "z".repeat(1000));
        let r = marker_windows(&text, &m(&["alpha", "beta"]), 0, 300, 300, 10);
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.dropped, 0);
    }

    #[test]
    fn overflow_spreads_across_the_document_and_keeps_the_last() {
        // Regression 2. Keep-first-N is head bias in a new place: the tail is
        // where the carve-outs live.
        let mut text = String::new();
        for _ in 0..20 {
            text.push_str("mark");
            text.push_str(&"z".repeat(1000));
        }
        let r = marker_windows(&text, &m(&["mark"]), 0, 100, 100, 5);
        assert_eq!(r.windows.len(), 5);
        assert_eq!(r.dropped, 15, "the cap must say what it did not show");

        // First and last are both present, which is what "spread" has to mean.
        assert_eq!(r.windows[0].start, 0);
        let last_mark = text.rfind("mark").unwrap();
        assert!(
            r.windows[4].end >= last_mark,
            "the final marked passage must survive the cap"
        );
    }

    #[test]
    fn the_cap_reports_zero_when_it_did_not_bite() {
        let text = format!("{}mark{}", "z".repeat(1000), "z".repeat(1000));
        let r = marker_windows(&text, &m(&["mark"]), 0, 100, 100, 10);
        assert_eq!(r.dropped, 0);
    }

    #[test]
    fn matching_is_case_insensitive_on_both_sides() {
        let text = format!("{}Taxable Event here", "z".repeat(900));
        let r = marker_windows(&text, &m(&["TAXABLE event"]), 800, 100, 100, 10);
        assert_eq!(r.windows.len(), 1);
    }

    #[test]
    fn a_marker_longer_than_the_text_is_not_a_panic() {
        let r = marker_windows("short", &m(&["a very long marker indeed"]), 0, 10, 10, 10);
        assert!(r.windows.is_empty());
    }

    #[test]
    fn no_markers_yields_no_windows_rather_than_the_whole_document() {
        let r = marker_windows(&"z".repeat(5000), &[], 800, 300, 300, 10);
        assert!(r.windows.is_empty());
        assert_eq!(r.dropped, 0);
        assert_eq!(r.head_end, 800);
    }

    #[test]
    fn a_head_past_the_end_clamps_instead_of_panicking() {
        let r = marker_windows("tiny", &m(&["tiny"]), 9999, 300, 300, 10);
        assert_eq!(r.head_end, 4);
        assert_eq!(r.length, 4);
    }

    #[test]
    fn offsets_are_chars_not_bytes() {
        // A multi-byte prefix must not shift the reported offset, or a caller's
        // `str substring` lands mid-word.
        let text = format!("{}MARK", "é".repeat(100));
        let r = marker_windows(&text, &m(&["mark"]), 0, 10, 10, 10);
        assert_eq!(r.length, 104);
        assert_eq!(r.windows[0].start, 90);
        assert!(slice_chars(&text, r.windows[0].start, r.windows[0].end).contains("MARK"));
    }

    #[test]
    fn before_and_after_are_independent() {
        // The shipped guide windows are 700 before against 2,600 after, because
        // a marker phrase labels material that FOLLOWS it. A symmetric radius
        // would spend most of the budget above the thing it was looking for.
        let text = format!("{}termination fee{}", "a".repeat(1000), "b".repeat(3000));
        let r = marker_windows(&text, &m(&["termination fee"]), 0, 700, 2600, 10);
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.windows[0].start, 300, "1000 - 700 before the hit");
        assert_eq!(r.windows[0].end, 3600, "1000 + 2600 after the hit");
    }

    #[test]
    fn a_cap_of_one_keeps_the_tail_because_the_head_already_ran() {
        let mut text = String::new();
        for _ in 0..5 {
            text.push_str("mark");
            text.push_str(&"z".repeat(1000));
        }
        let r = marker_windows(&text, &m(&["mark"]), 0, 50, 50, 1);
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.dropped, 4);
        assert!(r.windows[0].start > 3000, "must be the last, not the first");
    }
}
