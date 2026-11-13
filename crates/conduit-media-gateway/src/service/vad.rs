//! Simple energy-envelope voice-activity detection.
//!
//! Windows the mono PCM, thresholds windowed RMS relative to the loudest window,
//! merges nearby speech, drops sub-threshold blips, and splits over-long regions
//! into whisper-friendly (<30 s) windows. Purely heuristic and tunable — the point
//! is to skip silence and pure-gameplay stretches so whisper only runs on speech,
//! not to be a precise VAD.

/// A speech region as a half-open sample range `[start, end)`.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

/// A coarse loudness track: mean RMS per `1/hz`-second bucket over the mono PCM.
///
/// Reuses the same windowed-RMS math as [`detect_speech_regions`] (which throws
/// its envelope away after thresholding). The detector fuses this into the
/// reaction kinds (exciting/fails) so a loud moment scores even when it's wordless
/// — a scream or a groan the transcript never captured. Returns an empty vec for
/// empty input or a non-positive `hz`.
pub fn energy_envelope(samples: &[i16], sample_rate: u32, hz: f64) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 || hz <= 0.0 {
        return Vec::new();
    }
    let bucket = (f64::from(sample_rate) / hz).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(samples.len() / bucket + 1);
    let mut i = 0;
    while i < samples.len() {
        let end = (i + bucket).min(samples.len());
        let mut sum = 0.0_f64;
        for &s in &samples[i..end] {
            let f = f64::from(s);
            sum += f * f;
        }
        let rms = (sum / (end - i) as f64).sqrt();
        out.push(rms as f32);
        i = end;
    }
    out
}

/// Detect speech regions in mono 16-bit PCM.
pub fn detect_speech_regions(samples: &[i16], sample_rate: u32) -> Vec<Region> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let sr = sample_rate as usize;
    let win = (sr * 30 / 1000).max(1); // 30 ms analysis window

    // Windowed RMS.
    let mut rms: Vec<f64> = Vec::new();
    let mut max_rms = 0.0_f64;
    let mut i = 0;
    while i < samples.len() {
        let end = (i + win).min(samples.len());
        let mut sum = 0.0_f64;
        for &s in &samples[i..end] {
            let f = f64::from(s);
            sum += f * f;
        }
        let value = (sum / (end - i) as f64).sqrt();
        max_rms = max_rms.max(value);
        rms.push(value);
        i = end;
    }

    // Speech = windows above a fraction of the loudest window (with an absolute
    // floor so a near-silent clip doesn't light up on noise).
    let threshold = (max_rms * 0.08).max(150.0);
    let speech: Vec<bool> = rms.iter().map(|r| *r >= threshold).collect();

    let merge_gap_windows = (sr * 400 / 1000 / win).max(1); // bridge gaps < 400 ms
    let min_region_windows = (sr * 400 / 1000 / win).max(1); // drop regions < 400 ms
    let max_region_samples = sr * 28; // split at 28 s (< whisper's 30 s)

    // Raw runs of speech windows.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut cur: Option<usize> = None;
    for (idx, &is_speech) in speech.iter().enumerate() {
        match (is_speech, cur) {
            (true, None) => cur = Some(idx),
            (false, Some(start)) => {
                runs.push((start, idx));
                cur = None;
            }
            _ => {}
        }
    }
    if let Some(start) = cur {
        runs.push((start, speech.len()));
    }

    // Merge runs separated by a small gap.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in runs {
        if let Some(last) = merged.last_mut() {
            if s.saturating_sub(last.1) <= merge_gap_windows {
                last.1 = e;
                continue;
            }
        }
        merged.push((s, e));
    }

    // Drop too-short, convert to sample space, split too-long.
    let mut out = Vec::new();
    for (s, e) in merged {
        if e - s < min_region_windows {
            continue;
        }
        let mut start_sample = s * win;
        let end_sample = (e * win).min(samples.len());
        while end_sample.saturating_sub(start_sample) > max_region_samples {
            out.push(Region {
                start: start_sample,
                end: start_sample + max_region_samples,
            });
            start_sample += max_region_samples;
        }
        if end_sample > start_sample {
            out.push(Region {
                start: start_sample,
                end: end_sample,
            });
        }
    }
    out
}
