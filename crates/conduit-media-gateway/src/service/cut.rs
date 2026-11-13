//! Clip planning + EDL reel emission.
//!
//! Takes the operator-approved candidate rows, snaps each to whisper-sentence
//! boundaries, adds a min pad, merges near-overlapping ranges, applies the
//! length policy, and titles each clip. Also emits a CMX 3600 EDL reel that
//! references the source VOD by timecode (hand-emitted to keep the service lean —
//! same editor-openable artifact as OxiMedia's EDL crate without its dep tree).

use serde::Deserialize;

use crate::service::transcribe::TranscriptSegment;

/// An operator-approved candidate row (loosely typed — extra fields are ignored).
#[derive(Debug, Deserialize)]
pub struct CutSegment {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub game: Option<String>,
    /// The detected kind (`educational`/`informative`/`exciting`/`fails`), used to
    /// tag the clip title / reel entry so a woven reel is legible.
    #[serde(default)]
    pub kind: Option<String>,
}

/// A shaped, titled clip ready to download + cut.
#[derive(Clone, Debug)]
pub struct PlannedClip {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// Clip-shaping knobs (all flag-tunable from the request).
pub struct CutParams {
    pub pad_secs: f64,
    pub min_secs: f64,
    pub max_secs: f64,
    pub merge_gap_secs: f64,
}

impl Default for CutParams {
    fn default() -> Self {
        Self {
            pad_secs: 2.0,
            min_secs: 15.0,
            max_secs: 75.0,
            merge_gap_secs: 10.0,
        }
    }
}

/// Snap → pad → merge → length-policy → title the approved segments into clips.
pub fn plan_clips(
    approved: &[CutSegment],
    transcript: Option<&[TranscriptSegment]>,
    params: &CutParams,
) -> Vec<PlannedClip> {
    // Snap to sentence boundaries (if we have a transcript), pad, and title.
    let mut ranges: Vec<(f64, f64, Option<String>)> = approved
        .iter()
        .map(|seg| {
            let mut start = seg.start;
            let mut end = seg.end.max(seg.start);
            if let Some(ts) = transcript {
                start = snap_start(ts, start);
                end = snap_end(ts, end);
            }
            start = (start - params.pad_secs).max(0.0);
            end += params.pad_secs;
            (start, end, title_for(seg))
        })
        .collect();

    ranges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Merge overlapping / near-adjacent ranges.
    let mut merged: Vec<(f64, f64, Option<String>)> = Vec::new();
    for (start, end, title) in ranges {
        if let Some(last) = merged.last_mut() {
            if start - last.1 <= params.merge_gap_secs {
                last.1 = last.1.max(end);
                if last.2.is_none() {
                    last.2 = title;
                }
                continue;
            }
        }
        merged.push((start, end, title));
    }

    // Length policy: grow short clips toward min, cap long clips at max.
    merged
        .into_iter()
        .map(|(mut start, mut end, title)| {
            let dur = end - start;
            if dur < params.min_secs {
                let deficit = params.min_secs - dur;
                start = (start - deficit / 2.0).max(0.0);
                end += deficit / 2.0;
            }
            if end - start > params.max_secs {
                end = start + params.max_secs;
            }
            PlannedClip {
                start,
                end,
                title: title.unwrap_or_else(|| format!("clip at {:.0}s", start)),
            }
        })
        .collect()
}

/// Emit a CMX 3600 EDL reel referencing the source VOD by timecode.
pub fn build_edl(title: &str, clips: &[PlannedClip], fps: u32) -> String {
    let fps = fps.max(1);
    let mut out = String::new();
    out.push_str(&format!("TITLE: {title}\n"));
    out.push_str("FCM: NON-DROP FRAME\n\n");

    let mut record = 0.0_f64;
    for (i, clip) in clips.iter().enumerate() {
        let dur = (clip.end - clip.start).max(0.0);
        let src_in = secs_to_tc(clip.start, fps);
        let src_out = secs_to_tc(clip.end, fps);
        let rec_in = secs_to_tc(record, fps);
        let rec_out = secs_to_tc(record + dur, fps);
        out.push_str(&format!(
            "{:03}  AX       AA/V  C        {src_in} {src_out} {rec_in} {rec_out}\n",
            i + 1
        ));
        out.push_str(&format!(
            "* FROM CLIP NAME: {}\n\n",
            sanitize_edl(&clip.title)
        ));
        record += dur;
    }
    out
}

fn secs_to_tc(secs: f64, fps: u32) -> String {
    let total_frames = (secs.max(0.0) * f64::from(fps)).round() as i64;
    let fps = i64::from(fps);
    let frames = total_frames % fps;
    let total_secs = total_frames / fps;
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    format!("{h:02}:{m:02}:{s:02}:{frames:02}")
}

fn snap_start(ts: &[TranscriptSegment], t: f64) -> f64 {
    ts.iter()
        .find(|s| t >= s.start && t < s.end)
        .map_or(t, |s| s.start)
}

fn snap_end(ts: &[TranscriptSegment], t: f64) -> f64 {
    ts.iter()
        .find(|s| t > s.start && t <= s.end)
        .map_or(t, |s| s.end)
}

/// Title = `"[kind] <game> — <first sentence of snippet>"` (each part optional).
fn title_for(seg: &CutSegment) -> Option<String> {
    let base = seg
        .title
        .clone()
        .or_else(|| seg.snippet.as_ref().map(|s| first_sentence(s)));
    let core = match (&seg.game, base) {
        (Some(game), Some(base)) => Some(format!("{game} — {base}")),
        (None, Some(base)) => Some(base),
        (Some(game), None) => Some(game.clone()),
        (None, None) => None,
    };
    match (kind_tag(seg.kind.as_deref()), core) {
        (Some(tag), Some(core)) => Some(format!("{tag} {core}")),
        (Some(tag), None) => Some(tag.to_string()),
        (None, core) => core,
    }
}

/// Short bracket tag for a detected kind, used to prefix the clip title / reel entry.
fn kind_tag(kind: Option<&str>) -> Option<&'static str> {
    match kind {
        Some("educational") => Some("[teach]"),
        Some("exciting") => Some("[hype]"),
        Some("fails") => Some("[fail]"),
        Some("informative") => Some("[info]"),
        Some("funny") => Some("[funny]"),
        Some("frustration") => Some("[frust]"),
        _ => None,
    }
}

fn first_sentence(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = s
        .char_indices()
        .find(|(_, c)| matches!(c, '.' | '!' | '?'))
        .map_or(s.len(), |(i, _)| i);
    let sentence = s[..cut].trim();
    if sentence.chars().count() <= 70 {
        return sentence.to_string();
    }
    let mut out: String = sentence.chars().take(70).collect();
    if let Some(idx) = out.rfind(' ') {
        out.truncate(idx);
    }
    out.push('…');
    out
}

fn sanitize_edl(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}
