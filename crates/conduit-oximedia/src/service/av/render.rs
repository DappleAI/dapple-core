//! Render resolved clip placements down to one playable file.
//!
//! The render half of the A/V compositor (the temporal analog of image
//! `compose_with_layout`'s raster back-half). Placement — where each clip sits in
//! time, overlaps allowed — is resolved upstream (`super::resolve`); here we sum
//! the actual samples into one bus. Because each clip is a single voice, the
//! overlapping "talk-over" is just **summed PCM** with a per-clip gain and short
//! edge fades — no exotic DSP, no codec crate (WAV in, WAV out via `hound`).
//!
//! `AudioMixCompositor` is the audio impl of [`Compositor`]; a `VideoCompositor`
//! (frame composite/encode) drops onto the same seam later, consuming the same
//! resolved placements.

use crate::service::wav::{encode_wav_i16_mono, DecodedAudio};

/// A decoded clip placed at an absolute start time, with a playback gain. The
/// `gain` carries the author's dynamics (e.g. a conceding voice's mutter-down).
pub struct MixInput<'a> {
    /// Decoded mono samples + native rate.
    pub audio: &'a DecodedAudio,
    /// Absolute start on the mix timeline (seconds; resolved from the chain).
    pub start_seconds: f64,
    /// Linear playback gain (1.0 = unity).
    pub gain: f32,
    /// Absolute time (seconds) at which this clip must STOP because a later clip
    /// cuts in over it (from [`super::resolve::interrupt_caps`]); `None` = plays to
    /// its natural end. The clip is chopped here — a hard, floor-taking interruption
    /// — with the edge fade re-landed on the cut so it's abrupt but not a click-pop.
    pub end_cap_seconds: Option<f64>,
}

/// Render resolved placements to encoded bytes + mime. Deterministic; runs on a
/// `spawn_blocking` thread at the handler.
pub trait Compositor {
    /// Mix/composite the placed clips into one encoded artifact at `out_rate`.
    fn render(&self, inputs: &[MixInput], out_rate: u32)
        -> Result<(Vec<u8>, &'static str), String>;
}

/// Sums overlapping single-voice WAV clips into one mixed WAV — the conversation
/// deliverable. Per-clip gain + short linear edge fades (declick), then a
/// transparent peak-normalize (down-only) so pile-on overlaps that sum past unity
/// are tamed without clipping distortion.
pub struct AudioMixCompositor {
    /// Linear edge-fade length in milliseconds (declicks clip boundaries under
    /// overlap). ~8ms is inaudible but kills seams.
    pub fade_ms: f32,
}

impl Default for AudioMixCompositor {
    fn default() -> Self {
        Self { fade_ms: 8.0 }
    }
}

impl Compositor for AudioMixCompositor {
    fn render(
        &self,
        inputs: &[MixInput],
        out_rate: u32,
    ) -> Result<(Vec<u8>, &'static str), String> {
        if inputs.is_empty() {
            return Err("audio mix: no clips to render".to_string());
        }
        if out_rate == 0 {
            return Err("audio mix: output sample rate is 0".to_string());
        }
        let fade_samples = ((self.fade_ms / 1000.0) * out_rate as f32).round().max(0.0) as usize;

        // Resample + gain + fade each clip, and find the total timeline length.
        struct Placed {
            samples: Vec<f32>,
            start: usize,
        }
        let mut placed = Vec::with_capacity(inputs.len());
        let mut total = 0usize;
        for inp in inputs {
            let mut samples = resample_linear(&inp.audio.samples, inp.audio.sample_rate, out_rate);
            // Truncate at the cut point BEFORE fading, so the ~8ms declick fade-out
            // re-lands on the new (cut) end — an abrupt, floor-taking stop that still
            // avoids a boundary click. (The original fade-out lived on the natural
            // end we just chopped off.)
            if let Some(cap) = inp.end_cap_seconds {
                let cap_len =
                    ((cap - inp.start_seconds).max(0.0) * f64::from(out_rate)).round() as usize;
                if cap_len < samples.len() {
                    samples.truncate(cap_len);
                }
            }
            apply_gain_and_fades(&mut samples, inp.gain, fade_samples);
            let start = (inp.start_seconds.max(0.0) * f64::from(out_rate)).round() as usize;
            total = total.max(start + samples.len());
            placed.push(Placed { samples, start });
        }
        if total == 0 {
            return Err("audio mix: all clips were empty".to_string());
        }

        // Sum every clip into one bus at its start offset (overlaps add).
        let mut bus = vec![0.0f32; total];
        for p in &placed {
            for (k, &v) in p.samples.iter().enumerate() {
                bus[p.start + k] += v;
            }
        }

        // Peak-normalize DOWN only: transparent (pure gain, no distortion), and
        // only kicks in when overlaps push past unity. Quiet content is untouched.
        let peak = bus.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        if peak > 0.95 {
            let scale = 0.95 / peak;
            for x in &mut bus {
                *x *= scale;
            }
        }

        let bytes = encode_wav_i16_mono(&bus, out_rate)?;
        Ok((bytes, "audio/wav"))
    }
}

/// Naive linear resample from `from` Hz to `to` Hz. TTS clips are uniform (24 kHz),
/// so this is usually a no-op; it's here so mixed-rate inputs don't misalign.
fn resample_linear(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if samples.is_empty() || from == to || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let ratio = f64::from(to) / f64::from(from);
    let out_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = samples.get(idx).copied().unwrap_or(0.0);
        let b = samples.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

/// Apply a linear gain to every sample, then a symmetric linear fade over the
/// first/last `fade_samples` (clamped to half the clip so short clips still fade).
fn apply_gain_and_fades(samples: &mut [f32], gain: f32, fade_samples: usize) {
    for s in samples.iter_mut() {
        *s *= gain;
    }
    let n = samples.len();
    if n == 0 {
        return;
    }
    let f = fade_samples.min(n / 2);
    for i in 0..f {
        let g = (i as f32 + 0.5) / f as f32;
        samples[i] *= g; // fade in
        samples[n - 1 - i] *= g; // fade out (symmetric)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 24_000;

    fn constant(amp: f32, len: usize) -> DecodedAudio {
        DecodedAudio {
            samples: vec![amp; len],
            sample_rate: RATE,
        }
    }

    fn decode(bytes: &[u8]) -> DecodedAudio {
        crate::service::wav::decode_wav(bytes).expect("decode mixed output")
    }

    #[test]
    fn consecutive_length_is_the_sum() {
        // Two 0.1s clips butted end-to-end (no overlap) → 0.2s total.
        let a = constant(0.2, 2400);
        let b = constant(0.2, 2400);
        let inputs = vec![
            MixInput {
                audio: &a,
                start_seconds: 0.0,
                gain: 1.0,
                end_cap_seconds: None,
            },
            MixInput {
                audio: &b,
                start_seconds: 0.1, // butts right after A
                gain: 1.0,
                end_cap_seconds: None,
            },
        ];
        let (bytes, mime) = AudioMixCompositor::default().render(&inputs, RATE).unwrap();
        assert_eq!(mime, "audio/wav");
        let out = decode(&bytes);
        assert_eq!(out.sample_rate, RATE);
        assert_eq!(
            out.samples.len(),
            4800,
            "gapless consecutive = sum of lengths"
        );
    }

    #[test]
    fn interrupt_overlaps_shorten_total_and_sum_energy() {
        // A: [0, 0.1s); B interrupts at 0.05s → [0.05, 0.15s). Overlap [0.05,0.1).
        let a = constant(0.2, 2400);
        let b = constant(0.2, 2400);
        let inputs = vec![
            MixInput {
                audio: &a,
                start_seconds: 0.0,
                gain: 1.0,
                end_cap_seconds: None,
            },
            MixInput {
                audio: &b,
                start_seconds: 0.05,
                gain: 1.0,
                end_cap_seconds: None,
            },
        ];
        let (bytes, _) = AudioMixCompositor::default().render(&inputs, RATE).unwrap();
        let out = decode(&bytes);
        // total = max(2400, 1200 + 2400) = 3600, strictly less than the 4800 sum.
        assert_eq!(out.samples.len(), 3600);
        assert!(out.samples.len() < 4800, "overlap shortens the total");

        // A-only region (sample 400) ≈ 0.2; overlap (sample 1800, both present) ≈ 0.4;
        // B-only region (sample 3000) ≈ 0.2. (0.4 < 0.95 so no normalize; samples are
        // mid-clip, past the edge fades.)
        assert!((out.samples[400].abs() - 0.2).abs() < 0.02, "A only");
        assert!((out.samples[1800].abs() - 0.4).abs() < 0.02, "overlap sums");
        assert!((out.samples[3000].abs() - 0.2).abs() < 0.02, "B only");
    }

    #[test]
    fn gain_scales_a_clip() {
        let a = constant(0.4, 2400);
        let inputs = vec![MixInput {
            audio: &a,
            start_seconds: 0.0,
            gain: 0.5, // mutter-down
            end_cap_seconds: None,
        }];
        let (bytes, _) = AudioMixCompositor::default().render(&inputs, RATE).unwrap();
        let out = decode(&bytes);
        assert!(
            (out.samples[1200].abs() - 0.2).abs() < 0.02,
            "0.4 * 0.5 = 0.2"
        );
    }

    #[test]
    fn pile_on_is_peak_normalized_not_clipped() {
        // Three loud clips fully overlapping would sum to 2.4; peak-normalize brings
        // the bus to 0.95 without hard clipping to 1.0.
        let a = constant(0.8, 2400);
        let b = constant(0.8, 2400);
        let c = constant(0.8, 2400);
        let inputs = vec![
            MixInput {
                audio: &a,
                start_seconds: 0.0,
                gain: 1.0,
                end_cap_seconds: None,
            },
            MixInput {
                audio: &b,
                start_seconds: 0.0,
                gain: 1.0,
                end_cap_seconds: None,
            },
            MixInput {
                audio: &c,
                start_seconds: 0.0,
                gain: 1.0,
                end_cap_seconds: None,
            },
        ];
        let (bytes, _) = AudioMixCompositor::default().render(&inputs, RATE).unwrap();
        let out = decode(&bytes);
        let peak = out.samples.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak <= 0.96, "peak held near 0.95, got {peak}");
        assert!(peak > 0.9, "but not crushed, got {peak}");
    }

    #[test]
    fn resample_changes_length_by_ratio() {
        let s = vec![0.1f32; 1000];
        let up = resample_linear(&s, 24_000, 48_000);
        assert_eq!(up.len(), 2000);
        let same = resample_linear(&s, 24_000, 24_000);
        assert_eq!(same.len(), 1000);
    }

    #[test]
    fn empty_inputs_error() {
        assert!(AudioMixCompositor::default().render(&[], RATE).is_err());
    }

    #[test]
    fn end_cap_truncates_the_anchor_tail() {
        // A: [0, 0.1s). Cut A at 0.05s (a floor-taking interruption). B comes in at
        // 0.08s. Because A was chopped at 0.05s, the region [0.05, 0.08) is SILENT —
        // without truncation A would still be sounding there. That silent gap is the
        // proof the interrupted voice actually stopped instead of plowing on.
        let a = constant(0.2, 2400); // 0.1s
        let b = constant(0.2, 2400); // 0.1s
        let inputs = vec![
            MixInput {
                audio: &a,
                start_seconds: 0.0,
                gain: 1.0,
                end_cap_seconds: Some(0.05), // chopped where B takes the floor
            },
            MixInput {
                audio: &b,
                start_seconds: 0.08,
                gain: 1.0,
                end_cap_seconds: None,
            },
        ];
        let (bytes, _) = AudioMixCompositor::default().render(&inputs, RATE).unwrap();
        let out = decode(&bytes);
        // total = max(cap 1200, 1920 + 2400) = 4320.
        assert_eq!(out.samples.len(), 4320);
        assert!(
            (out.samples[600].abs() - 0.2).abs() < 0.02,
            "A sounds before the cut"
        );
        assert!(
            out.samples[1500].abs() < 0.02,
            "A is silent after the cut (truncated)"
        );
        assert!(
            (out.samples[3000].abs() - 0.2).abs() < 0.02,
            "B sounds after it takes over"
        );
    }

    #[test]
    fn end_cap_past_natural_end_is_a_noop() {
        // A cap beyond the clip's own length must not shorten it (a barge-in that
        // lands after the anchor would naturally end never truncates).
        let a = constant(0.2, 2400);
        let inputs = vec![MixInput {
            audio: &a,
            start_seconds: 0.0,
            gain: 1.0,
            end_cap_seconds: Some(0.2), // 4800 samples > the clip's 2400
        }];
        let (bytes, _) = AudioMixCompositor::default().render(&inputs, RATE).unwrap();
        let out = decode(&bytes);
        assert_eq!(
            out.samples.len(),
            2400,
            "a past-the-end cap leaves the clip whole"
        );
    }
}
