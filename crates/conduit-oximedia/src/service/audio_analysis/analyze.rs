//! Per-stem feature extraction.
//!
//! Wraps `oximedia-mir` (tempo + mood, other branches disabled for speed) and
//! `oximedia-analysis`'s low-level `AudioAnalyzer` (silence / spectral / dynamic
//! range), and computes a windowed RMS energy envelope directly (neither crate
//! hands back a clean per-time energy contour, and a windowed RMS is the exact,
//! deterministic signal the shift-derivation needs).
//!
//! All OxiMedia types are quarantined to this file; the rest of the service sees
//! only the plain [`super`] types.

use std::time::Duration;

use oximedia_analysis::audio::AudioAnalyzer;
use oximedia_mir::section_segmenter::{SectionSegmenter, SegmenterConfig};
use oximedia_mir::{MirAnalyzer, MirConfig};

use super::progression::RawSection;
use super::{EnergyPoint, EnergySummary, Mood, Silence, Spectral, StemMap, Tempo};
use crate::service::wav::DecodedAudio;

/// Energy-envelope window length (seconds).
const WINDOW_SECONDS: f64 = 0.5;
/// Energy-envelope hop between windows (seconds).
const HOP_SECONDS: f64 = 0.25;
/// Silence detection threshold (dBFS) — strict, to flag near-silence (real gaps)
/// rather than quiet passages (those surface as energy valleys instead).
const SILENCE_THRESHOLD_DB: f64 = -50.0;
/// Minimum samples before MIR (tempo/mood) is attempted; shorter stems get
/// `None` rather than risking a degenerate analysis.
const MIN_MIR_SAMPLES: usize = 4096;

/// Analyze one decoded stem into a [`StemMap`].
pub fn analyze_stem(
    name: &str,
    role: Option<String>,
    decoded: &DecodedAudio,
) -> Result<StemMap, String> {
    let mono = &decoded.samples;
    let sample_rate = decoded.sample_rate.max(1);
    let sr_f = f64::from(sample_rate);

    if mono.is_empty() {
        return Err("decoded stem had no samples".to_string());
    }

    let duration_seconds = mono.len() as f64 / sr_f;

    // ── Feel signals via oximedia-analysis (silence / spectral / dynamics) ──
    // Feed in ~1s chunks so the spectral summary averages several windows
    // (the analyzer only computes one FFT window per call); peak/RMS/silence
    // accumulate correctly across chunks regardless.
    let mut audio_analyzer = AudioAnalyzer::new(SILENCE_THRESHOLD_DB, Duration::from_millis(500));
    let chunk = (sample_rate as usize).max(2048);
    for c in mono.chunks(chunk) {
        audio_analyzer
            .process_samples(c, sample_rate)
            .map_err(|e| format!("audio analysis failed: {e}"))?;
    }
    let audio = audio_analyzer.finalize();

    let silences = audio
        .silence_segments
        .iter()
        .map(|s| Silence {
            start: s.start_sample as f64 / sr_f,
            end: s.end_sample as f64 / sr_f,
            avg_level_db: s.avg_level_db,
        })
        .collect();

    let spectral = Spectral {
        centroid_hz: audio.spectral_stats.centroid_hz,
        flatness: audio.spectral_stats.flatness,
        rolloff_hz: audio.spectral_stats.rolloff_hz,
        low_freq_ratio: audio.spectral_stats.low_freq_ratio,
        high_freq_ratio: audio.spectral_stats.high_freq_ratio,
    };

    // ── Energy envelope (DIY windowed RMS — the shift driver) ──────────────
    let envelope = energy_envelope(mono, sample_rate, WINDOW_SECONDS, HOP_SECONDS);
    let energy = EnergySummary {
        window_seconds: WINDOW_SECONDS,
        hop_seconds: HOP_SECONDS,
        peak_dbfs: audio.peak_dbfs,
        rms_dbfs: audio.rms_dbfs,
        envelope,
    };

    // ── Tempo + mood via oximedia-mir (other branches disabled) ────────────
    let (tempo, mood) = if mono.len() >= MIN_MIR_SAMPLES {
        analyze_mir(mono, sample_rate)?
    } else {
        (None, None)
    };

    Ok(StemMap {
        name: name.to_string(),
        role,
        sample_rate,
        duration_seconds,
        tempo,
        mood,
        energy,
        silences,
        spectral,
        dynamic_range_db: audio.dynamic_range_db,
        // Classification + hits are derived in fuse::build_song_map, where the
        // whole stem set is in hand.
        class: Default::default(),
        hits: Vec::new(),
    })
}

/// Segment the decoded full mix into raw (unsnapped) structural sections.
///
/// Wraps `oximedia-mir`'s `SectionSegmenter` (chroma SSM → novelty →
/// boundaries → labels) and projects into the plain [`RawSection`] carrier;
/// downbeat snapping + grouping happen in the pure `progression` layer. A mix
/// too short to segment yields an empty list rather than an error — the map
/// then simply carries no sections (through-composed handling downstream).
pub fn segment_mix(decoded: &DecodedAudio) -> Result<Vec<RawSection>, String> {
    let sample_rate = decoded.sample_rate.max(1);
    if decoded.samples.len() < MIN_MIR_SAMPLES {
        return Ok(Vec::new());
    }

    let segmenter = SectionSegmenter::new(sample_rate, SegmenterConfig::default());
    let result = match segmenter.segment(&decoded.samples) {
        Ok(r) => r,
        // Insufficient data → no sections, not a failed analysis.
        Err(e) => {
            log::warn!("mix section segmentation inconclusive: {e}");
            return Ok(Vec::new());
        }
    };

    Ok(result
        .sections
        .iter()
        .map(|s| RawSection {
            start_s: f64::from(s.start_s),
            end_s: f64::from(s.end_s),
            label: s.label.name().to_string(),
            mean_energy: f64::from(s.mean_energy),
            confidence: f64::from(s.confidence),
        })
        .collect())
}

/// Run the (tempo + mood only) MIR analysis and project it into our types.
fn analyze_mir(mono: &[f32], sample_rate: u32) -> Result<(Option<Tempo>, Option<Mood>), String> {
    let config = MirConfig {
        enable_beat_tracking: true,
        enable_mood_detection: true,
        enable_key_detection: false,
        enable_chord_recognition: false,
        enable_melody_extraction: false,
        enable_structure_analysis: false,
        enable_genre_classification: false,
        enable_spectral_features: false,
        enable_rhythm_features: false,
        enable_harmonic_analysis: false,
        ..MirConfig::default()
    };
    let analyzer = MirAnalyzer::new(config);

    // Force MIR to treat our already-mono buffer as mono: its stereo-detection
    // heuristic returns odd-length input unchanged, so we hand it an odd-length
    // slice to avoid a spurious L/R downmix that would halve the signal.
    let mir_input: &[f32] = if mono.len() >= 4 && mono.len().is_multiple_of(2) {
        &mono[..mono.len() - 1]
    } else {
        mono
    };

    let result = analyzer
        .analyze(mir_input, sample_rate as f32)
        .map_err(|e| format!("mir analysis failed: {e}"))?;

    let tempo = result.tempo.as_ref().map(|t| Tempo {
        bpm: t.bpm,
        confidence: t.confidence,
        stability: t.stability,
        beat_times: result
            .beat
            .as_ref()
            .map(|b| b.beat_times.clone())
            .unwrap_or_default(),
        downbeat_times: result
            .beat
            .as_ref()
            .map(|b| b.downbeat_times.clone())
            .unwrap_or_default(),
    });

    let mood = result.mood.as_ref().map(|m| {
        let top = m
            .moods
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, _)| k.clone());
        Mood {
            valence: m.valence,
            arousal: m.arousal,
            intensity: m.intensity,
            top,
        }
    });

    Ok((tempo, mood))
}

/// Windowed RMS energy envelope. Pure + deterministic.
///
/// Emits one [`EnergyPoint`] per hop, each the RMS of a `window`-length slice
/// centred at its time. The final window is clamped to the buffer end.
pub(crate) fn energy_envelope(
    mono: &[f32],
    sample_rate: u32,
    window_s: f64,
    hop_s: f64,
) -> Vec<EnergyPoint> {
    let sr = f64::from(sample_rate.max(1));
    let window = ((window_s * sr).round() as usize).max(1);
    let hop = ((hop_s * sr).round() as usize).max(1);

    if mono.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    loop {
        let end = (start + window).min(mono.len());
        let slice = &mono[start..end];
        let sum_sq: f64 = slice.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        let rms = (sum_sq / slice.len() as f64).sqrt();
        let centre = (start as f64 + slice.len() as f64 / 2.0) / sr;
        out.push(EnergyPoint { t: centre, rms });
        if end >= mono.len() {
            break;
        }
        start += hop;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decaying 1 kHz click every beat — a clean tempo target.
    fn click_track(bpm: f64, seconds: f64, sr: u32) -> Vec<f32> {
        let sr_f = f64::from(sr);
        let total = (seconds * sr_f) as usize;
        let period = (60.0 / bpm * sr_f) as usize;
        let click_len = (0.01 * sr_f) as usize;
        let mut s = vec![0.0f32; total];
        let mut b = 0usize;
        while b < total {
            for k in 0..click_len {
                if b + k >= total {
                    break;
                }
                let t = k as f64 / sr_f;
                let env = 1.0 - k as f64 / click_len as f64;
                s[b + k] += (env * (2.0 * std::f64::consts::PI * 1000.0 * t).sin())
                    .mul_add(0.8, 0.0) as f32;
            }
            b += period;
        }
        s
    }

    fn sine(freq: f64, seconds: f64, sr: u32, amp: f32) -> Vec<f32> {
        let sr_f = f64::from(sr);
        let n = (seconds * sr_f) as usize;
        (0..n)
            .map(|i| ((2.0 * std::f64::consts::PI * freq * i as f64 / sr_f).sin() as f32) * amp)
            .collect()
    }

    #[test]
    fn envelope_constant_signal_is_flat_and_nonzero() {
        let sr = 44100;
        let samples = vec![0.5f32; sr as usize * 2];
        let env = energy_envelope(&samples, sr, 0.5, 0.25);
        assert!(!env.is_empty());
        for p in &env {
            assert!((p.rms - 0.5).abs() < 1e-3, "rms {} != 0.5", p.rms);
        }
    }

    #[test]
    fn envelope_dips_in_a_silent_gap() {
        let sr = 44100;
        let mut samples = vec![0.5f32; sr as usize]; // 1s loud
        samples.extend(vec![0.0f32; sr as usize]); // 1s silent
        samples.extend(vec![0.5f32; sr as usize]); // 1s loud
        let env = energy_envelope(&samples, sr, 0.5, 0.25);
        // Some window centred in the gap (~1.5s) must be near-zero.
        let gap_min = env
            .iter()
            .filter(|p| p.t > 1.3 && p.t < 1.7)
            .map(|p| p.rms)
            .fold(f64::INFINITY, f64::min);
        assert!(gap_min < 0.05, "gap rms {gap_min} not near zero");
    }

    // ── The OxiMedia de-risk: real analysis on synthetic audio ──────────────

    #[test]
    fn analyze_stem_reads_tempo_of_a_click_track() {
        let sr = 44100;
        let samples = click_track(120.0, 6.0, sr);
        let decoded = DecodedAudio {
            samples,
            sample_rate: sr,
        };
        let stem = analyze_stem("drums", Some("drums".to_string()), &decoded).expect("analyze");

        assert!(!stem.energy.envelope.is_empty());
        let tempo = stem.tempo.expect("a click track should yield a tempo");
        assert!(
            tempo.bpm.is_finite() && (40.0..=260.0).contains(&tempo.bpm),
            "bpm {} out of sane range",
            tempo.bpm
        );
        // Tempo detection commonly lands on an octave of the true tempo; accept
        // 120 or its ×0.5 / ×2 neighbours within tolerance.
        let near = |target: f32| (tempo.bpm - target).abs() <= 8.0;
        assert!(
            near(120.0) || near(60.0) || near(240.0),
            "bpm {} not near 120 (or an octave)",
            tempo.bpm
        );
    }

    #[test]
    fn analyze_stem_flags_a_long_silence() {
        let sr = 44100;
        let mut samples = sine(220.0, 2.0, sr, 0.5); // 2s tone
        samples.extend(vec![0.0f32; sr as usize * 2]); // 2s silence
        samples.extend(sine(220.0, 2.0, sr, 0.5)); // 2s tone
        let decoded = DecodedAudio {
            samples,
            sample_rate: sr,
        };
        let stem = analyze_stem("vocals", Some("vocals".to_string()), &decoded).expect("analyze");

        // Energy is present overall.
        let max_rms = stem
            .energy
            .envelope
            .iter()
            .map(|p| p.rms)
            .fold(0.0f64, f64::max);
        assert!(max_rms > 0.1, "expected energy, got max rms {max_rms}");

        // The 2s middle gap (well over the ~1s min-duration) must be flagged.
        assert!(
            stem.silences
                .iter()
                .any(|s| s.start > 1.5 && s.end < 4.5 && s.end > s.start),
            "expected a silence in [1.5, 4.5], got {:?}",
            stem.silences
        );
    }
}
