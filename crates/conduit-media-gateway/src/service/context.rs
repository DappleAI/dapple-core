//! Transcript → LLM-sized context windows.
//!
//! A 2–4 h VOD transcript is far too large to hand an LLM in one prompt, so the
//! guide condenses it map-reduce style: this splits the transcript into
//! ~`window_secs`-long blocks (tagged with the covering chapter), the guide
//! summarizes each block via `prompt send`, then stitches the summaries into a
//! play-by-play arc. Pure + unit-tested; the LLM never runs here.

use serde::Serialize;

use crate::service::detect::Chapter;
use crate::service::transcribe::TranscriptSegment;

/// One condensable block of transcript, covering roughly `window_secs` of the VOD.
#[derive(Clone, Debug, Serialize)]
pub struct ContextWindow {
    /// Dense 0-based position in the returned list (windows with no speech are skipped).
    pub index: usize,
    pub start: f64,
    pub end: f64,
    /// The chapter/game covering this window's midpoint, if any.
    pub chapter: Option<String>,
    /// The concatenated transcript text of the window.
    pub text: String,
    /// The same transcript as one `[<start-secs>] <text>` line per segment, so an
    /// LLM reading the window can hand back timestamped spans (e.g. the primer
    /// miner returning "this stretch sets up the match" with its window).
    pub timed_text: String,
}

/// Group transcript segments into ~`window_secs`-long blocks. Segments are assumed
/// ordered by `start` (whisper + VTT both emit in order). Empty windows (no speech)
/// are not produced. Returns `[]` for empty input or a non-positive `window_secs`.
pub fn window_transcript(
    segments: &[TranscriptSegment],
    chapters: &[Chapter],
    window_secs: f64,
) -> Vec<ContextWindow> {
    if segments.is_empty() || window_secs <= 0.0 {
        return Vec::new();
    }

    let mut out: Vec<ContextWindow> = Vec::new();
    let mut cur_bucket: Option<i64> = None;

    for seg in segments {
        let text = seg.text.trim();
        let timed_line = if text.is_empty() {
            String::new()
        } else {
            format!("[{}] {}", seg.start.floor() as i64, text)
        };
        let bucket = (seg.start / window_secs).floor() as i64;
        match cur_bucket {
            Some(b) if b == bucket => {
                let w = out.last_mut().expect("bucket set ⇒ a window exists");
                w.end = w.end.max(seg.end);
                if !text.is_empty() {
                    if !w.text.is_empty() {
                        w.text.push(' ');
                    }
                    w.text.push_str(text);
                    if !w.timed_text.is_empty() {
                        w.timed_text.push('\n');
                    }
                    w.timed_text.push_str(&timed_line);
                }
            }
            _ => {
                cur_bucket = Some(bucket);
                out.push(ContextWindow {
                    index: out.len(),
                    start: seg.start,
                    end: seg.end,
                    chapter: None,
                    text: text.to_string(),
                    timed_text: timed_line,
                });
            }
        }
    }

    // Drop windows that ended up with no text (segments were all whitespace).
    out.retain(|w| !w.text.is_empty());
    // Re-densify indices and tag chapters by window midpoint.
    for (i, w) in out.iter_mut().enumerate() {
        w.index = i;
        w.chapter = chapter_at(chapters, f64::midpoint(w.start, w.end));
    }
    out
}

/// The title of the chapter covering time `t` (open-ended chapters have `end <= 0`).
fn chapter_at(chapters: &[Chapter], t: f64) -> Option<String> {
    chapters
        .iter()
        .find(|c| t >= c.start && (c.end <= 0.0 || t < c.end))
        .map(|c| c.title.clone())
        .filter(|title| !title.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            start,
            end,
            text: text.to_string(),
        }
    }

    #[test]
    fn empty_when_no_segments() {
        assert!(window_transcript(&[], &[], 600.0).is_empty());
    }

    #[test]
    fn groups_by_window_bucket() {
        let segments = vec![
            seg(0.0, 5.0, "one"),
            seg(30.0, 35.0, "two"),
            seg(120.0, 125.0, "three"),
            seg(140.0, 145.0, "four"),
        ];
        // 100s windows: [0,100) holds one+two; [100,200) holds three+four.
        let w = window_transcript(&segments, &[], 100.0);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].index, 0);
        assert_eq!(w[0].text, "one two");
        assert_eq!(w[0].timed_text, "[0] one\n[30] two");
        assert_eq!(w[0].start, 0.0);
        assert_eq!(w[0].end, 35.0);
        assert_eq!(w[1].text, "three four");
        assert_eq!(w[1].start, 120.0);
        assert_eq!(w[1].end, 145.0);
    }

    #[test]
    fn tags_chapter_by_midpoint() {
        let segments = vec![seg(10.0, 20.0, "hello")];
        let chapters = vec![Chapter {
            start: 0.0,
            end: 100.0,
            title: "Elden Ring".to_string(),
        }];
        let w = window_transcript(&segments, &chapters, 600.0);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].chapter.as_deref(), Some("Elden Ring"));
    }

    #[test]
    fn non_positive_window_is_empty() {
        let segments = vec![seg(0.0, 1.0, "x")];
        assert!(window_transcript(&segments, &[], 0.0).is_empty());
    }
}
