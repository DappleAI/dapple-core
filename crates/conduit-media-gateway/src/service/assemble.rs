//! Recap-reel assembly: render a `[text card → streamer's clip]` sequence to one MP4.
//!
//! This is Dapple's deliverable for the `learn-from-gaming-vod` guide. There is **no
//! narration/voice** — the streamer's own clip audio is preserved; Dapple's framing
//! is a short, neutral, deadpan intro card before each moment (and the selection + order
//! of the beats). Lives here because the media-gateway is the only service that can encode
//! H.264/AAC (conduit-oximedia is patent-free-codecs-only).
//!
//! The pure planning (card text, segment ordering) is unit-tested; the ffmpeg render
//! (`assemble_reel`) is runtime-verified.

use std::path::PathBuf;

use serde::Deserialize;
use uuid::Uuid;

use dapple_data::query::chunks::read_asset_content;

use crate::error::MediaGatewayError;
use crate::persist::persist_media_asset;
use crate::service::{media, vodid, AppState};

/// One curated beat to put in the reel: Dapple's intro + the source range to cut.
#[derive(Debug, Clone, Deserialize)]
pub struct AssembleBeat {
    /// Stable candidate id (unused by the render, carried for traceability).
    #[serde(default)]
    pub id: String,
    /// Dapple's one-line framing, shown on the intro card.
    #[serde(default)]
    pub intro: String,
    /// The detected kind (`educational`/`exciting`/…), carried for traceability; the card
    /// itself is neutral (no per-kind tint/label — that would telegraph the beat).
    #[serde(default)]
    pub kind: String,
    pub start: f64,
    pub end: f64,
}

/// Reel-render knobs (uniform output shape so segments concat cleanly).
pub struct AssembleParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Intro-card duration (seconds).
    pub card_secs: f64,
    /// Max clip rendition height pulled for each range.
    pub clip_height: u32,
    /// Hard cap on each clip's length (seconds); `0` disables. A scene window can
    /// span several beats over a long stretch, but the recap stays tight — cap the
    /// TAIL, keeping the start (which the detector already prerolled to the action
    /// onset). The guide's smart centering runs upstream; this is the backstop.
    pub clip_max_secs: f64,
    /// Cross-dissolve length (seconds) at every card seam; `0` disables (hard cut).
    /// A card gently melts in from the prior clip and out into its own clip instead
    /// of a jarring cut. Enabling it re-encodes the reel (a dissolve needs a
    /// filtergraph, not a stream-copy concat); the per-seam overlap is clamped to
    /// half of each adjacent segment.
    pub transition_secs: f64,
}

impl Default for AssembleParams {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
            card_secs: 2.5,
            clip_height: 720,
            clip_max_secs: 20.0,
            transition_secs: 0.4,
        }
    }
}

/// Cap a clip window to `max_secs`, keeping the start (the prerolled action onset)
/// and trimming the tail. `max_secs <= 0` disables; a window already within the cap
/// is returned unchanged (end clamped above start).
fn cap_clip(start: f64, end: f64, max_secs: f64) -> (f64, f64) {
    let end = end.max(start);
    if max_secs > 0.0 && end - start > max_secs {
        (start, start + max_secs)
    } else {
        (start, end)
    }
}

/// A planned reel segment: an intro card, or a source-range clip.
#[derive(Debug, PartialEq)]
pub enum ReelSegment {
    Card { text: String },
    Clip { start: f64, end: f64 },
}

/// Fraction of the shorter clip that must overlap for two beats to be treated as the
/// same footage (the later one dropped). The detector emits the same moment under
/// several kinds (e.g. `fails` + `frustration`), so without this a curated reel can
/// render the same death twice.
const OVERLAP_RATIO: f64 = 0.5;

/// Put beats in the order they occur in the VOD and drop near-duplicate footage.
///
/// Sorts ascending by `start` (tie-break `end`) so the reel runs linearly through the
/// session, then greedily keeps a beat unless it overlaps an already-kept beat by at
/// least [`OVERLAP_RATIO`] of the shorter clip's duration. This is the guarantee that
/// the final reel is linear and never repeats a clip, regardless of the order/selection
/// the LLM or operator produced — curation is selection, not ordering.
pub fn prepare_beats(beats: &[AssembleBeat]) -> Vec<AssembleBeat> {
    let mut sorted = beats.to_vec();
    sorted.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.end
                    .partial_cmp(&b.end)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut kept: Vec<AssembleBeat> = Vec::with_capacity(sorted.len());
    for beat in sorted {
        let (start, end) = (beat.start, beat.end.max(beat.start));
        let is_dup = kept.iter().any(|k| {
            let (k_start, k_end) = (k.start, k.end.max(k.start));
            let overlap = (end.min(k_end) - start.max(k_start)).max(0.0);
            if overlap <= 0.0 {
                return false;
            }
            let shorter = (end - start).min(k_end - k_start);
            // A zero-length clip that overlaps at all is a duplicate.
            shorter <= 0.0 || overlap / shorter >= OVERLAP_RATIO
        });
        if !is_dup {
            kept.push(beat);
        }
    }
    kept
}

/// Pure plan: expand beats into the ordered `card_0, clip_0, card_1, clip_1, …`
/// segment list, after [`prepare_beats`] (linear order + dedup). Unit-tested; the
/// render walks this same order.
pub fn plan_reel(beats: &[AssembleBeat]) -> Vec<ReelSegment> {
    let beats = prepare_beats(beats);
    let mut out = Vec::with_capacity(beats.len() * 2);
    for b in &beats {
        out.push(ReelSegment::Card { text: card_text(b) });
        out.push(ReelSegment::Clip {
            start: b.start,
            end: b.end.max(b.start),
        });
    }
    out
}

/// Render the reel: for each beat, render its intro card + cut its clip from source,
/// normalize both to a uniform shape, then concat in order. Persists the result as a
/// `video` asset. `scratch_id` (the job id) namespaces the scratch dir.
pub async fn assemble_reel(
    state: &AppState,
    scratch_id: Uuid,
    source_vod: &str,
    beats: &[AssembleBeat],
    audio_asset_id: Option<Uuid>,
    params: &AssembleParams,
) -> Result<serde_json::Value, MediaGatewayError> {
    if beats.is_empty() {
        return Err(MediaGatewayError::Assemble(
            "no beats to assemble".to_string(),
        ));
    }
    // Linear order + dedup: the reel always runs through the VOD in time order and
    // never repeats a clip, whatever order/selection the caller passed.
    let mut beats = prepare_beats(beats);
    // Keep the recap tight: cap each clip to clip_max_secs (trim the tail).
    for b in &mut beats {
        let (s, e) = cap_clip(b.start, b.end, params.clip_max_secs);
        b.start = s;
        b.end = e;
    }
    let (_, url) = vodid::parse_vod(source_vod)?;
    let dir = state
        .config
        .scratch_dir
        .join(format!("assemble-{scratch_id}"));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| MediaGatewayError::Assemble(format!("scratch mkdir: {e}")))?;
    let font = font_path();
    let logo = resolve_logo().await;

    let mut parts: Vec<PathBuf> = Vec::new();
    let result = async {
        // Operator-uploaded audio, if provided, played under every intro card.
        let card_audio: Option<PathBuf> = match audio_asset_id {
            Some(id) => {
                let bytes = read_asset_content(&state.pool, id).await?;
                let path = dir.join("card-audio");
                tokio::fs::write(&path, &bytes)
                    .await
                    .map_err(|e| MediaGatewayError::Assemble(format!("write card audio: {e}")))?;
                Some(path)
            }
            None => None,
        };
        // Duration of the card-audio track (0.0 when absent or unprobeable): drives the
        // per-card seek so successive cards advance through the song instead of replaying
        // its head. A probe failure degrades gracefully to the single-swoop behavior.
        let card_audio_secs: f64 = match card_audio.as_deref() {
            Some(p) => match media::probe_duration(p).await {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("card audio duration probe failed, disabling interweave: {e}");
                    0.0
                }
            },
            None => 0.0,
        };
        log::info!(
            "assemble {scratch_id}: rendering {} beats (card {}s, clip cap {}s)",
            beats.len(),
            params.card_secs,
            params.clip_max_secs
        );
        // Each card freezes a dimmed still of the clip it introduces — its FIRST frame — so
        // when the card ends and the clip starts the image is continuous: it reads as the
        // frame unfreezing while the text disappears, not a cut to unrelated footage. (Cards
        // after the first previously used the PREVIOUS clip's LAST frame, which froze the
        // moment that just ended and then cut to a different upcoming clip.) Each clip is
        // downloaded BEFORE its card renders. Frame extraction is best-effort — a failure
        // falls back to the solid card background.
        for (i, b) in beats.iter().enumerate() {
            // The streamer's clip (its own audio), cut from the source range.
            let clip_dir = dir.join(format!("clip-{i}"));
            tokio::fs::create_dir_all(&clip_dir)
                .await
                .map_err(|e| MediaGatewayError::Assemble(format!("clip mkdir: {e}")))?;
            let raw = media::download_clip(
                &url,
                b.start,
                b.end.max(b.start),
                params.clip_height,
                &clip_dir,
            )
            .await?;
            log::info!(
                "assemble beat {}/{}: clip cut [{:.1}s-{:.1}s]",
                i + 1,
                beats.len(),
                b.start,
                b.end
            );

            // Background still for this card: the upcoming clip's FIRST frame, so the
            // card → clip boundary reads as an unfreeze (same image, now in motion).
            let bg_frame: Option<PathBuf> = {
                let f = dir.join(format!("frame-{i}.png"));
                match media::extract_frame(&raw, false, &f).await {
                    Ok(()) => Some(f),
                    Err(e) => {
                        log::warn!("card frame extract (first) failed: {e}");
                        None
                    }
                }
            };

            // Intro card (over the dimmed still, white text unchanged).
            let card_line = card_text(b);
            let caption_lines = card_line.lines().count();
            let txt = dir.join(format!("card-{i}.txt"));
            tokio::fs::write(&txt, &card_line)
                .await
                .map_err(|e| MediaGatewayError::Assemble(format!("write card text: {e}")))?;
            // The upcoming clip's source-VOD start, stamped under the caption.
            let ts_txt = dir.join(format!("ts-{i}.txt"));
            tokio::fs::write(&ts_txt, format_source_timestamp(b.start))
                .await
                .map_err(|e| MediaGatewayError::Assemble(format!("write card timestamp: {e}")))?;
            // Per-card audio: advance through the track one card-length at a time,
            // silencing the opener in swoop mode (None → render this card silent).
            // Absent audio stays silent regardless (`card_audio` is None).
            let (card_audio_arg, audio_offset) =
                match card_audio_plan(i, card_audio_secs, params.card_secs) {
                    Some(off) => (card_audio.as_deref(), off),
                    None => (None, 0.0),
                };
            let card_mp4 = dir.join(format!("card-{i}.mp4"));
            media::render_card(
                &txt,
                CARD_BG,
                params.width,
                params.height,
                params.fps,
                params.card_secs,
                &font,
                logo.as_deref(),
                card_audio_arg,
                audio_offset,
                bg_frame.as_deref(),
                caption_lines,
                Some(&ts_txt),
                &card_mp4,
            )
            .await?;
            parts.push(card_mp4);

            // Normalize the clip to the uniform shape and add it after its card.
            let norm = dir.join(format!("norm-{i}.mp4"));
            media::normalize_video(&raw, params.width, params.height, params.fps, &norm).await?;
            parts.push(norm);
            log::info!(
                "assemble beat {}/{}: card + clip rendered",
                i + 1,
                beats.len()
            );
        }

        let out = dir.join("recap.mp4");
        // Cross-dissolve every card seam (video xfade + audio acrossfade) when a
        // transition is configured — the cards melt in/out instead of hard-cutting;
        // else the fast stream-copy concat. A dissolve needs a re-encode, so it is
        // opt-out via `transition_secs = 0`.
        log::info!(
            "assemble: joining {} parts (dissolve {}s)",
            parts.len(),
            params.transition_secs
        );
        if params.transition_secs > 0.0 && parts.len() > 1 {
            media::xfade_reel(&parts, params.transition_secs, &out).await?;
        } else {
            let list = dir.join("concat.txt");
            media::concat_mp4(&parts, &list, &out).await?;
        }

        let bytes = tokio::fs::read(&out)
            .await
            .map_err(|e| MediaGatewayError::Assemble(format!("read recap: {e}")))?;
        let (recap_asset_id, content_length) =
            persist_media_asset(&state.pool, &bytes, "video", "video/mp4").await?;
        log::info!("assemble: recap asset {recap_asset_id} persisted ({content_length} bytes)");

        // Probe the real output duration — the dissolves overlap each seam, so the reel
        // is shorter than the sum of the card + clip lengths. Falls back to the computed
        // sum if the probe fails.
        let duration = match media::probe_duration(&out).await {
            Ok(d) => d,
            Err(e) => {
                log::warn!("recap duration probe failed, reporting computed sum: {e}");
                params.card_secs * beats.len() as f64
                    + beats
                        .iter()
                        .map(|b| (b.end - b.start).max(0.0))
                        .sum::<f64>()
            }
        };

        Ok::<_, MediaGatewayError>(serde_json::json!({
            "recap_asset_id": recap_asset_id,
            "beat_count": beats.len(),
            "duration_secs": duration,
            "content_length": content_length,
        }))
    }
    .await;

    let _ = tokio::fs::remove_dir_all(&dir).await;
    result
}

/// The single neutral card background (`0xRRGGBB`). No per-kind tint — a color hint would
/// telegraph the beat and spoil the deadpan setup→cut joke.
const CARD_BG: &str = "0x14181F";

/// The intro-card text: just Dapple's deadpan line, wrapped. **No kind label** — a label
/// announces the category and kills the deadpan. drawtext needs a non-empty textfile, so an
/// empty line renders a blank card via a single space.
fn card_text(b: &AssembleBeat) -> String {
    let intro = wrap_text(b.intro.trim(), 28);
    if intro.is_empty() {
        " ".to_string()
    } else {
        intro
    }
}

/// Per-card audio decision. `None` = render this card silent; `Some(offset)` = play the
/// operator's card-audio track from `offset` seconds in.
///
/// **Interweave** (a real song — `audio_secs >= 2 * card_secs`): every card plays a consecutive
/// `card_secs`-long slice, `Some((index mod floor(audio_secs / card_secs)) * card_secs)`, so
/// card 0 is the song's opening (offset 0) and splicing just the cards replays the song. When the
/// reel outruns the track the cursor restarts from 0 — the last slice used is the last one that
/// fully fits; any sub-slice tail is skipped.
///
/// **Swoop** (too short to interweave): the first card is **silent** — the opener isn't a
/// transition *between* two clips, so it gets no sting — and every later card plays the head
/// (`Some(0.0)`, the single "swoop").
fn card_audio_plan(index: usize, audio_secs: f64, card_secs: f64) -> Option<f64> {
    if card_secs <= 0.0 {
        return Some(0.0);
    }
    let chunks = (audio_secs / card_secs).floor() as i64;
    if chunks < 2 {
        // Swoop: silent opener, head on every later card.
        return if index == 0 { None } else { Some(0.0) };
    }
    let chunks = chunks as usize;
    Some((index % chunks) as f64 * card_secs)
}

/// Format a source-VOD offset as an "overly exact" timestamp for the intro card:
/// `H:MM:SS.mmm`, dropping the hour segment when zero (`M:SS.mmm`). e.g. `297.5 -> "4:57.500"`.
fn format_source_timestamp(secs: f64) -> String {
    let total_ms = (secs.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m}:{s:02}.{ms:03}")
    }
}

/// The DappleAi cursive logo PNG to composite at the bottom of each card, if present.
/// Reads `MEDIA_GATEWAY_LOGO` (default `/models/dapple-logo.png`, alongside the baked
/// whisper model); returns `None` when the file is absent so the render still succeeds
/// without a logo (the user supplies the PNG).
async fn resolve_logo() -> Option<PathBuf> {
    let path = std::env::var("MEDIA_GATEWAY_LOGO")
        .unwrap_or_else(|_| "/models/dapple-logo.png".to_string());
    let path = PathBuf::from(path);
    match tokio::fs::metadata(&path).await {
        Ok(m) if m.is_file() => Some(path),
        _ => None,
    }
}

/// Greedy word-wrap to at most `max_cols` columns per line.
fn wrap_text(text: &str, max_cols: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.is_empty() {
            cur.push_str(word);
        } else if cur.len() + 1 + word.len() <= max_cols {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines.join("\n")
}

/// The drawtext font file — override with `MEDIA_GATEWAY_FONT`, else DejaVu Sans Bold
/// (installed via `fonts-dejavu-core` in the Docker runtime).
fn font_path() -> String {
    std::env::var("MEDIA_GATEWAY_FONT")
        .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(kind: &str, intro: &str, s: f64, e: f64) -> AssembleBeat {
        AssembleBeat {
            id: String::new(),
            intro: intro.to_string(),
            kind: kind.to_string(),
            start: s,
            end: e,
        }
    }

    #[test]
    fn plan_alternates_card_then_clip() {
        let beats = vec![
            beat("fails", "died to a trap", 10.0, 25.0),
            beat("funny", "chat lost it", 40.0, 55.0),
        ];
        let plan = plan_reel(&beats);
        assert_eq!(plan.len(), 4);
        assert!(matches!(plan[0], ReelSegment::Card { .. }));
        assert!(
            matches!(plan[1], ReelSegment::Clip { start, end } if start == 10.0 && end == 25.0)
        );
        assert!(matches!(plan[2], ReelSegment::Card { .. }));
        assert!(matches!(plan[3], ReelSegment::Clip { start, .. } if start == 40.0));
    }

    #[test]
    fn clip_end_is_clamped_above_start() {
        let plan = plan_reel(&[beat("exciting", "x", 30.0, 10.0)]);
        assert!(
            matches!(plan[1], ReelSegment::Clip { start, end } if start == 30.0 && end == 30.0)
        );
    }

    #[test]
    fn prepare_sorts_beats_by_start() {
        // Curated out of order -> the reel runs linearly through the VOD.
        let beats = vec![
            beat("funny", "later", 40.0, 55.0),
            beat("fails", "earlier", 10.0, 25.0),
        ];
        let prepared = prepare_beats(&beats);
        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[0].start, 10.0);
        assert_eq!(prepared[1].start, 40.0);
        // plan_reel reflects the linear order.
        let plan = plan_reel(&beats);
        assert!(matches!(plan[1], ReelSegment::Clip { start, .. } if start == 10.0));
        assert!(matches!(plan[3], ReelSegment::Clip { start, .. } if start == 40.0));
    }

    #[test]
    fn prepare_drops_overlapping_duplicate() {
        // The same death flagged twice (fails + frustration) over near-identical ranges.
        let beats = vec![
            beat("fails", "died", 100.0, 116.0),
            beat("frustration", "still mad about it", 103.0, 115.0),
        ];
        let prepared = prepare_beats(&beats);
        assert_eq!(prepared.len(), 1, "overlapping duplicate footage collapsed");
        assert_eq!(prepared[0].start, 100.0);
    }

    #[test]
    fn prepare_keeps_distinct_adjacent() {
        // Back-to-back but non-overlapping beats are both kept.
        let beats = vec![
            beat("fails", "one", 10.0, 20.0),
            beat("exciting", "two", 20.0, 30.0),
        ];
        assert_eq!(prepare_beats(&beats).len(), 2);
    }

    #[test]
    fn card_text_is_just_the_deadpan_line() {
        let t = card_text(&beat("fails", "big whiff into the pit", 1.0, 2.0));
        assert!(t.contains("big whiff"));
        // No kind label — the card never announces the category (that would kill the deadpan).
        assert!(!t.to_uppercase().contains("FAIL"));
    }

    #[test]
    fn wrap_breaks_long_lines() {
        let w = wrap_text("one two three four five six seven eight", 12);
        assert!(w.contains('\n'));
        for line in w.lines() {
            // A line only exceeds max_cols if it's a single unsplittable word.
            assert!(line.len() <= 12 || !line.contains(' '));
        }
    }

    #[test]
    fn card_audio_plan_swoop_silences_first_card_only() {
        // Shorter than two card lengths (< 2*2.5) -> swoop: first card silent,
        // every later card plays the head.
        assert_eq!(card_audio_plan(0, 4.0, 2.5), None);
        assert_eq!(card_audio_plan(1, 4.0, 2.5), Some(0.0));
        assert_eq!(card_audio_plan(2, 4.0, 2.5), Some(0.0));
    }

    #[test]
    fn card_audio_plan_interweaves_from_the_opener() {
        // Exactly two whole chunks is the threshold: >= 2*card_secs interweaves,
        // and card 0 is the song's opening (offset 0), not silent.
        assert_eq!(card_audio_plan(0, 5.0, 2.5), Some(0.0));
        assert_eq!(card_audio_plan(1, 5.0, 2.5), Some(2.5));
    }

    #[test]
    fn card_audio_plan_advances_then_restarts_from_zero() {
        // 13s track, 2.5s cards -> floor(13/2.5) = 5 usable chunks; the 0.5s tail is dropped.
        let d = 2.5;
        assert_eq!(card_audio_plan(0, 13.0, d), Some(0.0));
        assert_eq!(card_audio_plan(4, 13.0, d), Some(10.0)); // last chunk [10, 12.5], fully fits
        assert_eq!(card_audio_plan(5, 13.0, d), Some(0.0)); // tail < d -> restart from 0
        assert_eq!(card_audio_plan(6, 13.0, d), Some(2.5));
    }

    #[test]
    fn card_audio_plan_zero_card_secs_is_safe() {
        assert_eq!(card_audio_plan(3, 30.0, 0.0), Some(0.0));
    }

    #[test]
    fn cap_clip_trims_long_tail_keeps_start() {
        // A 40s scene window capped to 20s keeps the (prerolled) start, trims the tail.
        assert_eq!(cap_clip(100.0, 140.0, 20.0), (100.0, 120.0));
        // Within the cap → unchanged.
        assert_eq!(cap_clip(100.0, 112.0, 20.0), (100.0, 112.0));
        // Disabled (0) → unchanged (end still clamped above start).
        assert_eq!(cap_clip(100.0, 140.0, 0.0), (100.0, 140.0));
        assert_eq!(cap_clip(100.0, 90.0, 20.0), (100.0, 100.0));
    }

    #[test]
    fn format_source_timestamp_is_ms_precise() {
        assert_eq!(format_source_timestamp(297.5), "4:57.500");
        assert_eq!(format_source_timestamp(4024.25), "1:07:04.250");
        assert_eq!(format_source_timestamp(0.0), "0:00.000");
        // Negative clamps to zero.
        assert_eq!(format_source_timestamp(-3.0), "0:00.000");
    }
}
