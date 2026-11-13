//! Transcription: VAD-gated whisper.cpp ASR over the pass-1 audio asset, plus a
//! VTT/SRT caption parser for the "captions if present" branch.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use dapple_data::query::chunks::read_asset_content;
use dapple_data::DbPool;

use crate::config::Config;
use crate::error::MediaGatewayError;
use crate::service::vad;

/// Resolution of the energy track carried on a transcript: RMS samples per second.
pub const ENERGY_HZ: f64 = 1.0;

/// One timestamped transcript segment (absolute seconds into the VOD).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A coarse loudness track over the whole VOD audio: `rms[i]` is the mean RMS of
/// the audio in `[i/hz, (i+1)/hz)` seconds. The detector fuses this into the
/// reaction kinds so wordless spikes (screams, groans, laughter) still score.
/// `Default` (empty `rms`) means "no energy signal" — reaction scoring then falls
/// back to lexicon-only.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnergyTrack {
    pub hz: f64,
    pub rms: Vec<f32>,
}

/// A full transcript + its provenance (`"whisper"` or `"captions"`) + a loudness
/// track for reaction detection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transcript {
    pub source: String,
    pub segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub energy: EnergyTrack,
}

/// Whisper path: read the audio asset, VAD-gate, transcribe speech regions, and
/// derive the energy track from the same decoded samples (no extra media I/O).
///
/// `initial_prompt` is an optional vocabulary bias fed to whisper (the streamer's
/// handle, the game name, recurring slang) so accented speech and proper nouns
/// decode correctly. Whisper has no "accent" knob — biasing the decoder with the
/// stream's own vocabulary is the effective lever, and a solo stream is one voice
/// with one vocabulary.
pub async fn transcribe_audio_asset(
    config: &Config,
    pool: &DbPool,
    audio_asset_id: Uuid,
    initial_prompt: Option<String>,
) -> Result<Transcript, MediaGatewayError> {
    let bytes = read_asset_content(pool, audio_asset_id).await?;
    let (samples, sample_rate) = decode_wav_mono(&bytes)?;
    // The raw WAV bytes are not needed past the decode; free them so they don't sit in
    // memory alongside `samples` for the whole (long) transcription of a multi-hour VOD.
    drop(bytes);
    let regions = vad::detect_speech_regions(&samples, sample_rate);
    // Compute the energy track before `samples` is moved into the whisper task.
    let energy = EnergyTrack {
        hz: ENERGY_HZ,
        rms: vad::energy_envelope(&samples, sample_rate, ENERGY_HZ),
    };
    let model_path = resolve_model_path(config);

    let segments = tokio::task::spawn_blocking(move || {
        transcribe_regions(
            &model_path,
            &samples,
            sample_rate,
            &regions,
            initial_prompt.as_deref(),
        )
    })
    .await
    .map_err(|e| MediaGatewayError::Transcribe(format!("whisper task join: {e}")))??;

    Ok(Transcript {
        source: "whisper".to_string(),
        segments,
        energy,
    })
}

/// Derive just the energy track from the pass-1 audio asset. Used by the captions
/// branch, which otherwise never decodes the WAV, so reaction detection still has a
/// loudness signal.
pub async fn energy_track_for_asset(
    pool: &DbPool,
    audio_asset_id: Uuid,
) -> Result<EnergyTrack, MediaGatewayError> {
    let bytes = read_asset_content(pool, audio_asset_id).await?;
    let (samples, sample_rate) = decode_wav_mono(&bytes)?;
    Ok(EnergyTrack {
        hz: ENERGY_HZ,
        rms: vad::energy_envelope(&samples, sample_rate, ENERGY_HZ),
    })
}

/// Decode a 16-bit PCM WAV into mono `i16` + its sample rate (downmixing if the
/// source happens to be multi-channel — the pipeline produces mono 16 kHz).
fn decode_wav_mono(bytes: &[u8]) -> Result<(Vec<i16>, u32), MediaGatewayError> {
    let reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| MediaGatewayError::Transcribe(format!("wav open: {e}")))?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let raw: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|e| MediaGatewayError::Transcribe(format!("wav read: {e}")))?;

    let mono = if channels <= 1 {
        raw
    } else {
        raw.chunks(channels)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|s| i32::from(*s)).sum();
                (sum / channels as i32) as i16
            })
            .collect()
    };
    Ok((mono, spec.sample_rate))
}

/// Resolve the ggml model file. A `whisper_model` containing `/` is used as a
/// path; otherwise it names a model under `WHISPER_MODEL_DIR` (default `/models`,
/// where the Docker image bakes `ggml-<name>.bin`).
fn resolve_model_path(config: &Config) -> PathBuf {
    let model = &config.whisper_model;
    if model.contains('/') {
        return PathBuf::from(model);
    }
    let dir = std::env::var("WHISPER_MODEL_DIR").unwrap_or_else(|_| "/models".to_string());
    PathBuf::from(dir).join(format!("ggml-{model}.bin"))
}

/// Max audio a single whisper call transcribes, in seconds. A VAD "region" can be
/// nearly the whole VOD for a streamer who talks non-stop; whisper allocates its mel +
/// internal buffers for the WHOLE call, so an unchunked long region is the OOM driver
/// (a ~1h VOD OOM-killed a 4Gi pod). Splitting each region into ≤ this many seconds
/// bounds per-call memory (~tens of MB) regardless of region/VOD length. 4 minutes.
const MAX_CHUNK_SECS: usize = 4 * 60;

/// Split the sample range `[start, end)` into consecutive windows of at most `max_len`
/// samples — the windows tile the range with no gaps or overlaps. Empty when
/// `start >= end` or `max_len == 0`. Pure (no whisper), so the tiling is unit-tested;
/// `transcribe_regions` feeds it a region's bounds to cap whisper's per-call audio.
fn chunk_bounds(start: usize, end: usize, max_len: usize) -> Vec<(usize, usize)> {
    if start >= end || max_len == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let next = (cursor + max_len).min(end);
        out.push((cursor, next));
        cursor = next;
    }
    out
}

/// Audio-window margin (seconds) added on each side of an owned tile so a segment
/// straddling a chunk cut is transcribed with FULL audio; the owned-span keep-filter
/// then keeps each segment exactly once (no clip, no duplicate). Covers a word / short
/// phrase. `owned_step = MAX_CHUNK_SECS - 2*this` keeps the extended audio ≤ the memory
/// bound.
const CHUNK_OVERLAP_SECS: usize = 15;

/// The audio window to transcribe for an owned tile `[own0, own1)`: the tile extended
/// by `margin` samples on each side, clamped to the region `[region_start, region_end]`.
/// Internal boundaries get the margin on both sides (so whisper sees the whole of a
/// boundary word/phrase); the region's own ends get none (natural VAD-speech
/// boundaries). Pure — unit-tested.
fn audio_window(
    own0: usize,
    own1: usize,
    region_start: usize,
    region_end: usize,
    margin: usize,
) -> (usize, usize) {
    let a0 = own0.saturating_sub(margin).max(region_start);
    let a1 = (own1 + margin).min(region_end);
    (a0, a1)
}

/// Blocking whisper run over the VAD regions (CPU-bound — call under
/// `spawn_blocking`). Loads the model once, reuses one state with `no_context`. Long
/// regions are split into OWNED tiles (`chunk_bounds`) and each is transcribed over an
/// audio window extended by `CHUNK_OVERLAP_SECS` on each side; segments are kept only
/// in the tile owning their start, so per-call memory stays bounded AND boundary
/// words are never clipped or duplicated.
fn transcribe_regions(
    model_path: &Path,
    samples: &[i16],
    sample_rate: u32,
    regions: &[vad::Region],
    initial_prompt: Option<&str>,
) -> Result<Vec<TranscriptSegment>, MediaGatewayError> {
    let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .map_err(|e| {
            MediaGatewayError::Transcribe(format!("load model {}: {e}", model_path.display()))
        })?;
    let mut state = ctx
        .create_state()
        .map_err(|e| MediaGatewayError::Transcribe(format!("whisper state: {e}")))?;
    // Cap whisper threads to the pod's CPU BUDGET, not the node's core count.
    // `available_parallelism()` returns the NODE's cores (cgroup CPU *limits* don't
    // lower it), so on a 32-core node whisper would run 32-wide on a 4-CPU quota:
    // it saturates the quota so the tokio runtime can't serve /api/health_check
    // (readiness drops the pod → the flow's poll fails; liveness SIGKILLs it), and it
    // throttles the transcribe to a crawl. Prefer MEDIA_GATEWAY_WHISPER_THREADS (set to
    // the pod cpu limit in the manifest); else a conservative min(cores, 4), leaving
    // CPU for the async runtime + health checks.
    let threads = std::env::var("MEDIA_GATEWAY_WHISPER_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<i32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() as i32)
                .unwrap_or(4)
                .min(4)
        });
    // An empty/whitespace bias string is the same as none.
    let initial_prompt = initial_prompt.filter(|p| !p.trim().is_empty());

    let sr = sample_rate as usize;
    let margin_samples = CHUNK_OVERLAP_SECS * sr;
    // Owned tiles PARTITION each region (no gaps); a segment is kept only if its start
    // falls in its owning tile. The audio transcribed is each tile EXTENDED by
    // margin_samples on both sides (clamped to the region), so a segment straddling an
    // owned boundary is captured intact by the owning tile. owned_step =
    // MAX_CHUNK_SECS - 2*overlap keeps the extended audio window ≤ MAX_CHUNK_SECS.
    let owned_step_samples = MAX_CHUNK_SECS.saturating_sub(2 * CHUNK_OVERLAP_SECS).max(1) * sr;
    let mut out = Vec::new();
    for region in regions {
        let end = region.end.min(samples.len());
        if region.start >= end {
            continue;
        }
        for (own0, own1) in chunk_bounds(region.start, end, owned_step_samples) {
            let (a0, a1) = audio_window(own0, own1, region.start, end, margin_samples);
            let audio_offset = a0 as f64 / f64::from(sample_rate);
            let own0_secs = own0 as f64 / f64::from(sample_rate);
            let own1_secs = own1 as f64 / f64::from(sample_rate);
            let audio: Vec<f32> = samples[a0..a1]
                .iter()
                .map(|s| f32::from(*s) / 32768.0)
                .collect();

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_language(Some("en"));
            if let Some(prompt) = initial_prompt {
                params.set_initial_prompt(prompt);
            }
            params.set_n_threads(threads);
            params.set_translate(false);
            params.set_no_context(true);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);

            state
                .full(params, &audio)
                .map_err(|e| MediaGatewayError::Transcribe(format!("whisper full: {e}")))?;

            for i in 0..state.full_n_segments() {
                let Some(segment) = state.get_segment(i) else {
                    continue;
                };
                let text = segment
                    .to_str_lossy()
                    .map(|c| c.trim().to_string())
                    .unwrap_or_default();
                if text.is_empty() {
                    continue;
                }
                // whisper timestamps are centiseconds, relative to the AUDIO window start.
                let t0 = segment.start_timestamp() as f64 / 100.0;
                let t1 = segment.end_timestamp() as f64 / 100.0;
                let abs_start = audio_offset + t0;
                // Keep each segment ONLY in the tile that owns its start: the left
                // margin's early segments belong to the previous tile (dedup), and the
                // owning tile transcribed the whole segment thanks to the margin.
                if abs_start < own0_secs || abs_start >= own1_secs {
                    continue;
                }
                out.push(TranscriptSegment {
                    start: abs_start,
                    end: audio_offset + t1,
                    text,
                });
            }
        }
    }
    Ok(out)
}

/// Parse WebVTT (or SRT) cue text into timestamped segments. Tolerant: ignores
/// headers, cue settings, and inline `<...>` tags; accepts `HH:MM:SS.mmm`,
/// `MM:SS.mmm`, and the SRT `,` decimal separator.
pub fn parse_vtt(text: &str) -> Vec<TranscriptSegment> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((start, end)) = parse_cue_timing(line) else {
            continue;
        };
        let mut buf = String::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            let stripped = strip_tags(next.trim());
            let stripped = stripped.trim();
            if !stripped.is_empty() {
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str(stripped);
            }
            lines.next();
        }
        let text = buf.trim().to_string();
        if !text.is_empty() {
            out.push(TranscriptSegment { start, end, text });
        }
    }
    out
}

fn parse_cue_timing(line: &str) -> Option<(f64, f64)> {
    let (a, b) = line.trim().split_once("-->")?;
    let start = parse_timestamp(a.trim())?;
    let end = parse_timestamp(b.split_whitespace().next()?)?;
    Some((start, end))
}

fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (
            h.parse::<f64>().ok()?,
            m.parse::<f64>().ok()?,
            s.parse::<f64>().ok()?,
        ),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + sec)
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_bounds_degenerate_is_empty() {
        assert!(chunk_bounds(10, 10, 4).is_empty()); // start == end
        assert!(chunk_bounds(20, 10, 4).is_empty()); // start > end
        assert!(chunk_bounds(0, 100, 0).is_empty()); // max_len == 0
    }

    #[test]
    fn chunk_bounds_region_within_cap_is_one_window() {
        assert_eq!(chunk_bounds(5, 9, 10), vec![(5, 9)]);
        assert_eq!(chunk_bounds(0, 10, 10), vec![(0, 10)]); // exactly the cap
    }

    #[test]
    fn chunk_bounds_exact_multiple_splits_evenly() {
        assert_eq!(chunk_bounds(0, 12, 4), vec![(0, 4), (4, 8), (8, 12)]);
    }

    #[test]
    fn chunk_bounds_remainder_leaves_shorter_last_window() {
        assert_eq!(chunk_bounds(0, 10, 4), vec![(0, 4), (4, 8), (8, 10)]);
        // A non-zero region start carries through (absolute sample indices).
        assert_eq!(
            chunk_bounds(100, 110, 4),
            vec![(100, 104), (104, 108), (108, 110)]
        );
    }

    #[test]
    fn chunk_bounds_tiles_range_without_gaps_or_overlaps() {
        let wins = chunk_bounds(7, 30, 5);
        assert_eq!(
            wins.first().map(|w| w.0),
            Some(7),
            "starts at the region start"
        );
        assert_eq!(wins.last().map(|w| w.1), Some(30), "ends at the region end");
        for pair in wins.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "contiguous — no gap or overlap");
        }
        for &(a, b) in &wins {
            assert!(a < b && b - a <= 5, "each window length in (0, max_len]");
        }
    }

    #[test]
    fn audio_window_first_tile_has_no_left_margin() {
        // own0 == region_start → the left side clamps to the region start (no pre-roll).
        assert_eq!(audio_window(100, 310, 100, 1000, 15), (100, 325));
    }

    #[test]
    fn audio_window_internal_tile_extends_both_sides() {
        assert_eq!(audio_window(310, 520, 100, 1000, 15), (295, 535));
    }

    #[test]
    fn audio_window_last_tile_has_no_right_margin() {
        // own1 == region_end → the right side clamps to the region end.
        assert_eq!(audio_window(730, 1000, 100, 1000, 15), (715, 1000));
    }

    #[test]
    fn audio_window_huge_margin_clamps_to_region() {
        assert_eq!(audio_window(400, 600, 100, 1000, 100_000), (100, 1000));
    }
}
