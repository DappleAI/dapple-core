//! Heading anchor ids — THE cross-render anchor contract.
//!
//! One markdown body serves three renders (journey pane, PDF, HTML), so all
//! three must derive byte-identical ids from the same heading text. dapple-ui
//! implements this same algorithm; its implementation must copy the test
//! vectors below verbatim, never re-derive them.
//!
//! # The algorithm (GFM/github-slugger)
//!
//! 1. Unicode-lowercase the heading text.
//! 2. Drop every character that is not: Unicode alphanumeric, whitespace,
//!    `-`, or `_`.
//! 3. Trim leading/trailing whitespace.
//! 4. Replace each remaining whitespace character with `-` (consecutive
//!    spaces produce consecutive dashes — no collapsing, matching
//!    github-slugger).
//! 5. Deduplicate within one document scope: the second occurrence of a slug
//!    becomes `slug-1`, the third `slug-2`, …; a suffixed form itself enters
//!    the collision map (so an explicit later `slug-1` yields `slug-1-1`).
//!
//! Explicit ids (`### Heading {#custom-id}`) register verbatim — no
//! transformation — but still occupy the collision map.

use std::collections::HashMap;

/// Single-shot slug of one heading text — steps 1–4 only (no deduplication).
pub fn gfm_slug_base(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c.is_whitespace() {
            out.push(' ');
        }
        // Everything else (punctuation, emoji, symbols) is dropped.
    }
    let trimmed = out.trim();
    trimmed
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// Per-document slug scope with github-slugger collision semantics.
#[derive(Default)]
pub struct GfmSlugger {
    /// slug → number of times a `-N` suffix has been minted off it.
    seen: HashMap<String, usize>,
}

impl GfmSlugger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Slug a heading's rendered text and deduplicate within this scope.
    pub fn slug(&mut self, text: &str) -> String {
        self.dedup(gfm_slug_base(text))
    }

    /// Register an explicit `{#id}` verbatim. It occupies the collision map
    /// like any other id; a repeated explicit id is suffixed like a repeated
    /// auto slug (duplicate anchors would otherwise be unaddressable).
    pub fn register_explicit(&mut self, id: &str) -> String {
        self.dedup(id.to_string())
    }

    fn dedup(&mut self, base: String) -> String {
        let mut candidate = base.clone();
        while self.seen.contains_key(&candidate) {
            let n = self.seen.entry(base.clone()).or_insert(0);
            *n += 1;
            candidate = format!("{base}-{n}");
        }
        self.seen.entry(base).or_insert(0);
        self.seen.insert(candidate.clone(), 0);
        candidate
    }
}

/// Injective mapping from a markdown fragment id to a valid Typst label name.
///
/// Typst labels allow `[a-zA-Z0-9_.:-]` (and must be non-empty). Allowed ASCII
/// passes through; every other char becomes `_xNNNN_` (lowercase hex of the
/// Unicode scalar), which contains no chars that could collide with a
/// passthrough sequence's meaning. The `a--` prefix namespaces heading anchors
/// away from other generated labels (bibliography entries use `bib-N`) and
/// guarantees non-emptiness.
pub fn label_for_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len() + 3);
    out.push_str("a--");
    for c in id.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | ':' | '-' => out.push(c),
            other => {
                out.push_str(&format!("_x{:x}_", other as u32));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE shared test vectors. dapple-ui's slugger must reproduce every row
    /// byte-for-byte — copy these, do not re-derive.
    #[test]
    fn contract_vectors() {
        let cases = [
            ("The Timeline", "the-timeline"),
            ("What They Filed", "what-they-filed"),
            ("  Padded  ", "padded"),
            (
                "Cloud is now half of revenue",
                "cloud-is-now-half-of-revenue",
            ),
            // Dropped punctuation leaves its flanking spaces in place, and
            // each space becomes a dash (github-slugger semantics).
            ("C# & F#!", "c--f"),
            ("Über Uns", "über-uns"),
            ("2024 [Business] Cloud", "2024-business-cloud"),
            ("a  b", "a--b"), // consecutive spaces are NOT collapsed
            ("émotion Réserve", "émotion-réserve"),
            ("snake_case kept", "snake_case-kept"),
            ("dash-kept", "dash-kept"),
            ("💡 Insight", "insight"),
            ("100% Growth", "100-growth"),
        ];
        for (input, expected) in cases {
            assert_eq!(gfm_slug_base(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn collision_suffixing_matches_github_slugger() {
        let mut s = GfmSlugger::new();
        assert_eq!(s.slug("Setup"), "setup");
        assert_eq!(s.slug("Setup"), "setup-1");
        // "setup-1" is occupied, so an actual "Setup-1" heading nests a suffix.
        assert_eq!(s.slug("Setup-1"), "setup-1-1");
        assert_eq!(s.slug("Setup"), "setup-2");
    }

    #[test]
    fn explicit_ids_win_verbatim_and_occupy_the_map() {
        let mut s = GfmSlugger::new();
        assert_eq!(
            s.register_explicit("tl-bz-2024-cloud-3"),
            "tl-bz-2024-cloud-3"
        );
        // Explicit ids are NOT transformed (uppercase preserved).
        assert_eq!(s.register_explicit("Mixed_Case.Id"), "Mixed_Case.Id");
        // An auto slug that collides with an explicit id gets suffixed.
        assert_eq!(s.register_explicit("overview"), "overview");
        assert_eq!(s.slug("Overview"), "overview-1");
        // A repeated explicit id is suffixed rather than duplicated.
        assert_eq!(s.register_explicit("overview"), "overview-2");
    }

    #[test]
    fn label_mapping_is_injective_and_valid() {
        // Distinct ids must map to distinct labels.
        let ids = [
            "the-timeline",
            "the_timeline",
            "über-uns",
            "uber-uns",
            "a.b:c",
            "x",
            "_x5f_",
        ];
        let labels: Vec<String> = ids.iter().map(|i| label_for_id(i)).collect();
        let mut deduped = labels.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(labels.len(), deduped.len(), "label collision: {labels:?}");
        // Every produced label is valid Typst label charset.
        for label in &labels {
            assert!(
                label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c)),
                "invalid label chars: {label}"
            );
        }
        assert_eq!(label_for_id("the-timeline"), "a--the-timeline");
        assert_eq!(label_for_id("über"), "a--_xfc_ber");
    }

    #[test]
    fn empty_and_symbol_only_headings_still_get_usable_ids() {
        assert_eq!(gfm_slug_base("!!!"), "");
        let mut s = GfmSlugger::new();
        // Empty base still dedups deterministically ("", "-1", "-2", …).
        assert_eq!(s.slug("!!!"), "");
        assert_eq!(s.slug("???"), "-1");
        // And the label mapping stays non-empty thanks to the prefix.
        assert_eq!(label_for_id(""), "a--");
    }
}
