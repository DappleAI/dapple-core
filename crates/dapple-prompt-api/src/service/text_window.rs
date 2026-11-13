//! Windowing long guide-block text into several embeddable chunks.
//!
//! # Why this exists
//!
//! conduit-onnx caps embedding input at `MAX_SEQUENCE_TOKENS` and **truncates**
//! anything longer. A truncated block's tail is then *never retrievable* — for a
//! platform whose premise is that agent prose is RAG-reachable, that is silent
//! index loss, not a capacity footnote. Windowing turns one over-cap block into
//! several embedding rows so the whole block stays reachable.
//!
//! # Threshold-triggered, deliberately
//!
//! Text at or under [`EMBED_WINDOW_CHARS`] returns **exactly one window whose
//! `.text` is byte-identical to the input** — today's behaviour, unchanged. Only
//! blocks that are currently losing their tail split. The blast radius is
//! precisely the set of blocks that are already broken.
//!
//! # Not [`dapple_prompt_flow::extractors::embedding`]'s `semantic_chunk`
//!
//! That function exists and windows text, but the two are deliberately separate:
//! it returns a **byte offset** consumed by `page_for_offset` to map a chunk to a
//! PDF page, it is unconditional rather than threshold-triggered, and it is
//! boundary-blind. Making it boundary-aware would move every asset chunk's byte
//! offsets and therefore every `source_position.page` in
//! `rev.asset_content_embeddings` — a different subsystem, changed for no reason.
//! Do not "dedupe" these.

/// Char budget for one embedding window.
///
/// **PAIRED WITH conduit-onnx's `MAX_SEQUENCE_TOKENS`**
/// (`crates/conduit-onnx/src/service/embedding/modernbert.rs` — currently
/// 4,096). This crate does not depend on conduit-onnx, so the relationship is
/// documented rather than compile-enforced. The arithmetic:
///
/// ```text
/// 12,000 chars / ~4 chars-per-token  ~=  3,000 tokens
/// 3,000 tokens  <  4,096 = MAX_SEQUENCE_TOKENS       (~27% margin)
/// ```
///
/// The margin is not decoration. 4 chars/token is a Latin-prose average; dense
/// markup, tables and CJK tokenize far worse, so a 12,000-char window of CJK can
/// still exceed the cap. **That case is caught, not lost**: conduit-onnx reports
/// how many inputs it truncated and `handler::guide_embeddings` fails the ingest
/// rather than committing a lossy index. So this estimate being wrong costs
/// availability, never content — which is the right way round to fail.
pub const EMBED_WINDOW_CHARS: usize = 12_000;

/// Overlap between adjacent windows, ~12.5% of the window, so a fact spanning a
/// window boundary is still wholly present in at least one window. Overlap costs
/// index size, not accuracy.
pub const EMBED_WINDOW_OVERLAP_CHARS: usize = 1_500;

/// Assumed chars per token. Only ever used to document and assert the pairing
/// above — nothing in the algorithm tokenizes.
const ASSUMED_CHARS_PER_TOKEN: usize = 4;

/// Hand-maintained mirror of conduit-onnx's `MAX_SEQUENCE_TOKENS`. NOT imported
/// — there is no dependency edge between the crates. If you change it there,
/// change it here; the assert below then re-checks the pairing at build time.
const CONDUIT_MAX_SEQUENCE_TOKENS_MIRROR: usize = 4_096;

/// Earliest a window may end after boundary snapping, as a fraction of the
/// window. Bounds how far the boundary ladder walks backwards so one convenient
/// early paragraph break cannot collapse a window to near-nothing.
const MIN_WINDOW_DIVISOR: usize = 2;

// Enforced at COMPILE time, in the idiom of modernbert.rs's own invariant assert.
// Overlap must be strictly under the minimum window length or the loop below can
// fail to advance. See the termination argument on `window_text_with`.
const _: () = assert!(EMBED_WINDOW_OVERLAP_CHARS < EMBED_WINDOW_CHARS / MIN_WINDOW_DIVISOR);
// A window must be estimable as fitting inside the conduit's token cap. This
// fires on the likelier drift direction — someone raising the window for bigger
// excerpts without checking what the conduit will accept.
const _: () =
    assert!(EMBED_WINDOW_CHARS / ASSUMED_CHARS_PER_TOKEN < CONDUIT_MAX_SEQUENCE_TOKENS_MIRROR);

/// One embedding window over a source text.
///
/// Offsets are **char** offsets, not byte offsets, so they can be compared
/// directly against `text.chars()` positions without risking a split multi-byte
/// character. They exist so coverage is directly assertable and so a window can
/// later be located within its source block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextWindow {
    /// Char offset of this window's first char in the source.
    pub char_start: usize,
    /// Char offset one past this window's last char.
    pub char_end: usize,
    pub text: String,
}

/// Window `text` for embedding using the production constants.
///
/// Text at or under [`EMBED_WINDOW_CHARS`] yields exactly one window that is
/// byte-identical to the input.
pub fn window_text(text: &str) -> Vec<TextWindow> {
    window_text_with(text, EMBED_WINDOW_CHARS, EMBED_WINDOW_OVERLAP_CHARS)
}

/// The testable core, so the boundary ladder can be exercised without building
/// 12,000-char fixtures.
///
/// # Guarantees
///
/// - **Coverage.** The windows' `[char_start, char_end)` ranges cover
///   `[0, total)` with no gap, so no span of the source is ever dropped. This is
///   the whole point of the module — a windower that can lose a span is just a
///   slower truncation.
/// - **Termination.** Every window advances `start` by at least 1 char, and in
///   practice by `min_len - overlap` (4,500 chars with the production
///   constants). A 20,000-char "word" containing no boundary at all still
///   terminates, via the hard-cut tier.
/// - **Char safety.** All slicing goes through `char_indices()`, so a multi-byte
///   character is never halved.
///
/// `overlap_chars` is clamped below the minimum window length so the guarantees
/// hold for *any* caller-supplied pair, not only the asserted production ones.
fn window_text_with(text: &str, window_chars: usize, overlap_chars: usize) -> Vec<TextWindow> {
    if text.is_empty() {
        return Vec::new();
    }

    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let total = chars.len();
    let window_chars = window_chars.max(1);

    // THRESHOLD TRIGGER — the "nothing changes for content that already works"
    // guarantee. Byte-identical, single window, no allocation of offsets.
    if total <= window_chars {
        return vec![TextWindow {
            char_start: 0,
            char_end: total,
            text: text.to_string(),
        }];
    }

    let min_len = (window_chars / MIN_WINDOW_DIVISOR).max(1);
    let overlap = overlap_chars.min(min_len.saturating_sub(1));

    let slice = |from: usize, to: usize| -> String {
        let byte_from = chars[from].0;
        let byte_to = if to < total { chars[to].0 } else { text.len() };
        text[byte_from..byte_to].to_string()
    };

    let mut windows = Vec::new();
    let mut start = 0usize;
    loop {
        let hard_end = (start + window_chars).min(total);
        let end = if hard_end == total {
            total
        } else {
            snap_end(&chars, start + min_len, hard_end)
        };

        windows.push(TextWindow {
            char_start: start,
            char_end: end,
            text: slice(start, end),
        });

        if end >= total {
            break;
        }
        // Overlap is measured back from the REAL cut, and the next start is
        // deliberately not re-snapped: a window beginning mid-sentence is
        // harmless (the overlap region is duplicated context, not the primary
        // carrier), and snapping both ends would create a second place where
        // forward progress could be undone.
        start = (end - overlap).max(start + 1);
    }

    windows
}

/// Largest index in `min_end..hard_end` that is a clean place to cut, preferring
/// the highest-priority boundary tier that has any match at all:
///
/// 1. **paragraph** — a blank line; the next window then starts at a paragraph head
/// 2. **sentence** — a terminator (with optional closing quote/bracket) plus its
///    trailing whitespace
/// 3. **line** — a bare newline. Markdown tables and tight lists have neither
///    blank lines nor sentence terminators, and without this tier they fall
///    straight through to whitespace and get cut mid-row
/// 4. **whitespace** — never split a word
/// 5. **hard cut** at `hard_end` — the boundaryless-input escape hatch
///
/// A higher tier wins even when it sits further back, which is why `min_end`
/// exists: it bounds the sacrifice to half a window.
fn snap_end(chars: &[(usize, char)], min_end: usize, hard_end: usize) -> usize {
    let mut sentence = None;
    let mut line = None;
    let mut whitespace = None;

    // Scanning downward means the first match found for any tier is also the
    // largest, so a paragraph hit can return immediately.
    let mut end = hard_end;
    while end > min_end {
        if ends_paragraph(chars, end) {
            return end;
        }
        let prev = chars[end - 1].1;
        if sentence.is_none() && ends_sentence(chars, end) {
            sentence = Some(end);
        }
        if line.is_none() && prev == '\n' {
            line = Some(end);
        }
        if whitespace.is_none() && prev.is_whitespace() {
            whitespace = Some(end);
        }
        end -= 1;
    }

    sentence.or(line).or(whitespace).unwrap_or(hard_end)
}

/// True when `chars[..end]` ends on a blank line (two newlines, allowing only
/// horizontal whitespace between them).
fn ends_paragraph(chars: &[(usize, char)], end: usize) -> bool {
    if end == 0 || chars[end - 1].1 != '\n' {
        return false;
    }
    let mut i = end - 1;
    while i > 0 && matches!(chars[i - 1].1, ' ' | '\t' | '\r') {
        i -= 1;
    }
    i > 0 && chars[i - 1].1 == '\n'
}

/// True when `chars[..end]` ends on sentence-terminating punctuation followed by
/// whitespace, so the next window starts at a sentence head. Closing quotes and
/// brackets are skipped, since `He said "stop." Then...` is a sentence boundary.
fn ends_sentence(chars: &[(usize, char)], end: usize) -> bool {
    if end == 0 || !chars[end - 1].1.is_whitespace() {
        return false;
    }
    let mut i = end;
    while i > 0 && chars[i - 1].1.is_whitespace() {
        i -= 1;
    }
    while i > 0 && matches!(chars[i - 1].1, '"' | '\'' | ')' | ']' | '”' | '’') {
        i -= 1;
    }
    i > 0 && matches!(chars[i - 1].1, '.' | '!' | '?' | '…')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reconstruct a window's text from the source by char offsets. Used to
    /// prove `char_start`/`char_end` genuinely describe `.text` — a windower
    /// whose offsets lie is worse than none.
    fn slice_by_chars(source: &str, w: &TextWindow) -> String {
        source
            .chars()
            .skip(w.char_start)
            .take(w.char_end - w.char_start)
            .collect()
    }

    fn assert_covers(source: &str, windows: &[TextWindow]) {
        let total = source.chars().count();
        assert!(!windows.is_empty(), "no windows for {total} chars");
        assert_eq!(windows[0].char_start, 0, "first window must start at 0");
        assert_eq!(
            windows.last().unwrap().char_end,
            total,
            "last window must reach the end of the source"
        );
        for pair in windows.windows(2) {
            assert!(
                pair[1].char_start <= pair[0].char_end,
                "gap between {:?} and {:?} — a dropped span is silent index loss",
                pair[0].char_start..pair[0].char_end,
                pair[1].char_start..pair[1].char_end
            );
            assert!(
                pair[1].char_start > pair[0].char_start,
                "windows must strictly advance"
            );
        }
        for w in windows {
            assert_eq!(
                slice_by_chars(source, w),
                w.text,
                "offsets disagree with text"
            );
        }
    }

    // -- threshold trigger: today's behaviour, unchanged -------------------

    #[test]
    fn under_cap_returns_the_input_byte_identical() {
        for source in [
            "hello world".to_string(),
            "a".repeat(EMBED_WINDOW_CHARS - 1),
            "a".repeat(EMBED_WINDOW_CHARS), // exactly at the threshold
        ] {
            let windows = window_text(&source);
            assert_eq!(windows.len(), 1, "len {}", source.chars().count());
            assert_eq!(windows[0].text, source);
            assert_eq!(windows[0].char_start, 0);
            assert_eq!(windows[0].char_end, source.chars().count());
        }
    }

    #[test]
    fn empty_input_yields_no_windows() {
        assert!(window_text("").is_empty());
    }

    #[test]
    fn one_char_over_the_cap_splits() {
        let source = "a ".repeat(EMBED_WINDOW_CHARS); // 2x the cap, whitespace-separated
        let windows = window_text(&source);
        assert!(windows.len() > 1);
        assert_covers(&source, &windows);
    }

    // -- coverage: the guarantee the whole design rests on -----------------

    #[test]
    fn covers_the_whole_source_with_no_gap() {
        let para = "The fund reported non-accrual loans across its portfolio. ".repeat(15);
        let source = std::iter::repeat_n(para.as_str(), 60)
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(source.chars().count() > EMBED_WINDOW_CHARS * 3);

        let windows = window_text(&source);
        assert!(windows.len() >= 4);
        assert_covers(&source, &windows);
    }

    #[test]
    fn the_tail_of_an_over_cap_block_is_present_in_some_window() {
        // The exact content that is lost today: everything past the cap.
        let source = format!("{}TAIL-SENTINEL-9f2c", "filler text. ".repeat(4_000));
        assert!(source.chars().count() > EMBED_WINDOW_CHARS);

        let windows = window_text(&source);
        assert!(
            windows
                .iter()
                .any(|w| w.text.contains("TAIL-SENTINEL-9f2c")),
            "the block's tail must survive windowing"
        );
    }

    // -- the boundary ladder ----------------------------------------------

    #[test]
    fn prefers_paragraph_boundaries() {
        let source = std::iter::repeat_n("word ".repeat(20).trim().to_string(), 40)
            .collect::<Vec<_>>()
            .join("\n\n");
        let windows = window_text_with(&source, 200, 40);
        assert!(windows.len() > 2);
        for w in &windows[..windows.len() - 1] {
            assert!(
                w.text.ends_with("\n\n"),
                "non-final window should end on a paragraph break, got {:?}",
                &w.text[w.text.len().saturating_sub(20)..]
            );
        }
        assert_covers(&source, &windows);
    }

    #[test]
    fn snaps_to_a_sentence_when_there_is_no_paragraph() {
        // One giant paragraph: no blank lines anywhere.
        let source = "Alpha beta gamma delta. ".repeat(60);
        let windows = window_text_with(&source, 200, 40);
        assert!(windows.len() > 2);
        for w in &windows[..windows.len() - 1] {
            assert!(
                w.text.trim_end().ends_with('.'),
                "expected a sentence cut, got {:?}",
                w.text
            );
        }
        assert_covers(&source, &windows);
    }

    #[test]
    fn snaps_to_a_line_for_markdown_tables_with_no_sentences() {
        // Table rows: newlines but no blank lines and no sentence terminators.
        let source = std::iter::repeat_n("| aaaa | bbbb | cccc |", 80)
            .collect::<Vec<_>>()
            .join("\n");
        let windows = window_text_with(&source, 200, 40);
        assert!(windows.len() > 2);
        for w in &windows[..windows.len() - 1] {
            assert!(
                w.text.ends_with('\n'),
                "expected a row boundary, got {:?}",
                w.text
            );
        }
        assert_covers(&source, &windows);
    }

    #[test]
    fn snaps_to_whitespace_and_never_splits_a_word() {
        let source = "alpha bravo charlie delta echo foxtrot ".repeat(40);
        let windows = window_text_with(&source, 200, 40);
        assert!(windows.len() > 2);
        for w in &windows[..windows.len() - 1] {
            assert!(
                w.text.ends_with(' '),
                "cut mid-word: {:?}",
                &w.text[w.text.len().saturating_sub(20)..]
            );
        }
        assert_covers(&source, &windows);
    }

    // -- pathological input ------------------------------------------------

    #[test]
    fn terminates_on_a_boundaryless_word() {
        // No paragraph, sentence, line or whitespace anywhere: every tier must
        // fall through to the hard cut. A non-terminating loop hangs the ingest.
        let source = "a".repeat(20_000);
        let windows = window_text(&source);
        assert!(windows.len() >= 2);
        assert_covers(&source, &windows);
        for w in &windows {
            assert!(w.text.chars().count() <= EMBED_WINDOW_CHARS);
        }
    }

    #[test]
    fn never_splits_a_multibyte_char() {
        // assert_covers reconstructs each window from source chars, so a halved
        // or dropped char fails there rather than panicking on a byte boundary.
        for unit in ["é", "🙂", "日本"] {
            let source = unit.repeat(20_000);
            let windows = window_text(&source);
            assert!(windows.len() >= 2, "unit {unit}");
            assert_covers(&source, &windows);
        }
    }

    #[test]
    fn degenerate_parameters_still_terminate_and_cover() {
        // Overlap >= window would deadlock without the clamp in window_text_with.
        let source = "alpha bravo charlie ".repeat(50);
        for (win, overlap) in [(1, 0), (1, 99), (2, 100), (10, 10), (7, 6)] {
            let windows = window_text_with(&source, win, overlap);
            assert_covers(&source, &windows);
        }
    }

    // -- overlap ------------------------------------------------------------

    #[test]
    fn adjacent_windows_overlap_by_the_configured_amount() {
        let source = "alpha bravo charlie delta ".repeat(2_000);
        let windows = window_text(&source);
        assert!(windows.len() > 2);
        for pair in windows.windows(2) {
            assert_eq!(
                pair[0].char_end - pair[1].char_start,
                EMBED_WINDOW_OVERLAP_CHARS,
                "overlap must be exact so a boundary-spanning fact is whole somewhere"
            );
        }
    }
}
