//! Extract a security's 9-character CUSIP from an SC 13D / SC 13G cover page.
//!
//! Every SC 13D/13G names the subject security's CUSIP on its cover (a required
//! field), so a company's common-stock CUSIP can be resolved from filings it does
//! not even file itself (the activist/holder files them, but they are indexed under
//! the subject company). This keeps the whole institutional-holders pipeline inside
//! SEC-disclosed public data with no licensed CUSIP master.
//!
//! Covers print the number many ways: "CUSIP No. 88160R101", "CUSIP Number:
//! 037833 10 5", a bare "037833105" beside the label, sometimes split by spaces.
//! The scan finds the "CUSIP" label, skips a "No./Number/#/:" connector, then reads
//! the following alphanumerics (spaces allowed, as filers split the 6-2-1 groups)
//! and keeps the first 9 — a CUSIP is exactly 9 chars (6 issuer, 2 issue, 1 check).

/// Pull the first plausible 9-char CUSIP that follows a "CUSIP" label in `text`.
/// Returns the uppercased 9-char string, or `None` if no label is followed by a
/// 9-alphanumeric run. Purely textual — no check-digit validation (a filer's
/// typo should not drop the whole lookup; the 13F match is exact anyway).
pub fn extract_cusip(text: &str) -> Option<String> {
    let upper = text.to_uppercase();
    for (idx, _) in upper.match_indices("CUSIP") {
        let after = &upper[idx + "CUSIP".len()..];
        // Look only a short distance past the label so we don't grab an unrelated
        // number further down the page.
        let window: String = after.chars().take(48).collect();
        if let Some(cusip) = first_cusip(&window) {
            return Some(cusip);
        }
    }
    None
}

/// From the text right after a "CUSIP" label, skip an optional connector
/// (No./Number/#/:/whitespace), then read alphanumerics with single spaces allowed
/// between the printed groups, and return the first 9 alphanumeric characters.
fn first_cusip(window: &str) -> Option<String> {
    let chars: Vec<char> = window.chars().collect();
    let mut i = 0;
    // Skip a leading connector word/punctuation: NO, NO., NUMBER, #, :, spaces,
    // periods. Stop skipping once we reach a digit (a CUSIP almost always starts
    // with a digit for US issues) or after a bounded skip.
    let connectors = ["NUMBER", "NO.", "NO", "#", ":", "."];
    loop {
        // eat whitespace
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let rest: String = chars[i..].iter().collect();
        let mut advanced = false;
        for c in connectors {
            if rest.starts_with(c) {
                i += c.len();
                advanced = true;
                break;
            }
        }
        if !advanced {
            break;
        }
    }
    // eat whitespace before the value
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    // Read up to 9 alphanumerics, allowing single spaces between printed groups.
    let mut out = String::new();
    while i < chars.len() && out.len() < 9 {
        let c = chars[i];
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if c == ' ' && !out.is_empty() {
            // a group separator; keep going
        } else {
            break;
        }
        i += 1;
    }
    // A real CUSIP is digit-heavy and its 9th char (the check digit) is numeric, so
    // a plain word that happens to follow the label ("CUSIP conventions" -> "CONVENTIO")
    // is rejected and the scan falls through to a later, real occurrence.
    let digits = out.chars().filter(|c| c.is_ascii_digit()).count();
    let check_numeric = out.chars().last().is_some_and(|c| c.is_ascii_digit());
    if out.len() == 9 && digits >= 5 && check_numeric {
        Some(out)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_no_dot() {
        assert_eq!(
            extract_cusip("... CUSIP No. 88160R101 (the class) ...").as_deref(),
            Some("88160R101")
        );
    }

    #[test]
    fn spaced_groups() {
        // Filers commonly print the 6-2-1 groups spaced.
        assert_eq!(
            extract_cusip("CUSIP Number: 037833 10 5").as_deref(),
            Some("037833105")
        );
    }

    #[test]
    fn bare_number_beside_label() {
        assert_eq!(
            extract_cusip("CUSIP\n459200101\nCommon Stock").as_deref(),
            Some("459200101")
        );
    }

    #[test]
    fn stops_at_nine_and_ignores_trailing() {
        assert_eq!(
            extract_cusip("CUSIP No. 594918104  (Common)").as_deref(),
            Some("594918104")
        );
    }

    #[test]
    fn none_when_no_number() {
        assert_eq!(
            extract_cusip("This document mentions CUSIP conventions."),
            None
        );
    }

    #[test]
    fn none_when_absent() {
        assert_eq!(extract_cusip("No identifier here at all."), None);
    }

    #[test]
    fn first_of_multiple_wins() {
        // The cover CUSIP appears before any incidental later mention.
        let t = "CUSIP No. 023135106\n... later ... CUSIP 999999999";
        assert_eq!(extract_cusip(t).as_deref(), Some("023135106"));
    }
}
