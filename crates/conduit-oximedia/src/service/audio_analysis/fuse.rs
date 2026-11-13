//! Pure fusion of per-stem analyses into a [`SongMap`] + its derived [`Shift`]s.
//!
//! Shift derivation is intentionally simple, deterministic, and driven by each
//! stem's energy envelope + silences: stem entrances/exits, energy peaks and
//! valleys, and silence boundaries. Tempo/mood are single global readings per
//! stem (not time series), so they inform the map but don't emit shifts here —
//! section/mood-change shifts are a later slice.

use super::progression::{
    classify_stem, densest_downbeat_grid, derive_hits, snap_and_group_sections, RawSection,
};
use super::{Shift, ShiftKind, SongMap, StemMap};

/// Fraction of a stem's peak RMS above which it counts as "present" (for
/// entrance/exit detection).
const ENTER_FRAC: f64 = 0.15;
/// Half-width (in envelope points) of the neighbourhood a local extremum must
/// dominate to count as a peak/valley.
const PEAK_NEIGHBORS: usize = 3;
/// A peak must reach at least this fraction of the stem's peak RMS.
const PEAK_MIN_FRAC: f64 = 0.4;
/// A valley must be at least this deep (1 - rms/max) to count.
const VALLEY_MIN_DEPTH: f64 = 0.4;

/// Build the [`SongMap`] from per-stem analyses plus the (optional) raw mix
/// sections.
///
/// The progression layer runs here: every stem is classified and given its
/// hit spans, and raw sections are snapped to the densest stem's downbeat
/// grid + grouped by label recurrence.
#[must_use]
pub fn build_song_map(mut stems: Vec<StemMap>, structure: Option<Vec<RawSection>>) -> SongMap {
    let sample_rate = stems.first().map_or(0, |s| s.sample_rate);
    let duration_seconds = stems
        .iter()
        .map(|s| s.duration_seconds)
        .fold(0.0f64, f64::max);
    let stem_count = stems.len();
    let shifts = derive_shifts(&stems);

    for stem in &mut stems {
        stem.class = classify_stem(&stem.energy.envelope);
        stem.hits = derive_hits(&stem.energy.envelope, stem.tempo.as_ref(), stem.class);
    }

    let (sections, structure_confidence) = match &structure {
        Some(raw) => snap_and_group_sections(raw, densest_downbeat_grid(&stems)),
        None => (Vec::new(), None),
    };

    SongMap {
        sample_rate,
        duration_seconds,
        stem_count,
        stems,
        shifts,
        sections,
        structure_confidence,
    }
}

/// Stable ordering key for a shift kind (used only to break time ties
/// deterministically).
fn kind_ord(kind: ShiftKind) -> u8 {
    match kind {
        ShiftKind::Entrance => 0,
        ShiftKind::EnergyPeak => 1,
        ShiftKind::EnergyValley => 2,
        ShiftKind::Silence => 3,
        ShiftKind::Exit => 4,
    }
}

/// Derive the fused, time-sorted shift list from per-stem envelopes + silences.
fn derive_shifts(stems: &[StemMap]) -> Vec<Shift> {
    let mut shifts: Vec<Shift> = Vec::new();

    for stem in stems {
        let env = &stem.energy.envelope;
        let max_rms = env.iter().map(|p| p.rms).fold(0.0f64, f64::max);

        if max_rms > 1e-9 && !env.is_empty() {
            let enter = ENTER_FRAC * max_rms;

            // Entrance: first crossing above the presence threshold, unless the
            // stem is already present at t=0.
            if let Some(first) = env.iter().position(|p| p.rms >= enter) {
                if first > 0 {
                    shifts.push(Shift {
                        t: env[first].t,
                        kind: ShiftKind::Entrance,
                        stem: Some(stem.name.clone()),
                        strength: (env[first].rms / max_rms).clamp(0.0, 1.0),
                    });
                }
            }

            // Exit: the point after the last present window, if the stem falls
            // silent before the end.
            if let Some(last) = env.iter().rposition(|p| p.rms >= enter) {
                if last + 1 < env.len() {
                    shifts.push(Shift {
                        t: env[last + 1].t,
                        kind: ShiftKind::Exit,
                        stem: Some(stem.name.clone()),
                        strength: (env[last].rms / max_rms).clamp(0.0, 1.0),
                    });
                }
            }

            // Local peaks / valleys, each dominating a ±PEAK_NEIGHBORS window.
            if env.len() > 2 * PEAK_NEIGHBORS {
                for i in PEAK_NEIGHBORS..(env.len() - PEAK_NEIGHBORS) {
                    let cur = env[i].rms;
                    let lo = i - PEAK_NEIGHBORS;
                    let hi = i + PEAK_NEIGHBORS;

                    let dominates_max = (lo..=hi).all(|j| env[j].rms <= cur);
                    let dominates_min = (lo..=hi).all(|j| env[j].rms >= cur);

                    // Strict against immediate neighbours to avoid plateau spam.
                    if dominates_max
                        && cur >= PEAK_MIN_FRAC * max_rms
                        && cur > env[i - 1].rms
                        && cur >= env[i + 1].rms
                    {
                        shifts.push(Shift {
                            t: env[i].t,
                            kind: ShiftKind::EnergyPeak,
                            stem: Some(stem.name.clone()),
                            strength: (cur / max_rms).clamp(0.0, 1.0),
                        });
                    } else if dominates_min && cur < env[i - 1].rms && cur <= env[i + 1].rms {
                        let depth = (1.0 - cur / max_rms).clamp(0.0, 1.0);
                        if depth > VALLEY_MIN_DEPTH {
                            shifts.push(Shift {
                                t: env[i].t,
                                kind: ShiftKind::EnergyValley,
                                stem: Some(stem.name.clone()),
                                strength: depth,
                            });
                        }
                    }
                }
            }
        }

        // Silence boundaries (independent of energy presence).
        for s in &stem.silences {
            let dur = (s.end - s.start).max(0.0);
            shifts.push(Shift {
                t: s.start,
                kind: ShiftKind::Silence,
                stem: Some(stem.name.clone()),
                strength: (dur / 4.0).clamp(0.2, 1.0),
            });
        }
    }

    shifts.sort_by(|a, b| {
        a.t.partial_cmp(&b.t)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| kind_ord(a.kind).cmp(&kind_ord(b.kind)))
    });
    shifts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::audio_analysis::{EnergyPoint, EnergySummary, Silence, Spectral};

    fn stem_with(name: &str, envelope: Vec<EnergyPoint>, silences: Vec<Silence>) -> StemMap {
        let duration = envelope.last().map_or(0.0, |p| p.t + 0.25);
        StemMap {
            name: name.to_string(),
            role: None,
            sample_rate: 44100,
            duration_seconds: duration,
            tempo: None,
            mood: None,
            energy: EnergySummary {
                window_seconds: 0.5,
                hop_seconds: 0.25,
                peak_dbfs: -6.0,
                rms_dbfs: -12.0,
                envelope,
            },
            silences,
            spectral: Spectral {
                centroid_hz: 0.0,
                flatness: 0.0,
                rolloff_hz: 0.0,
                low_freq_ratio: 0.0,
                high_freq_ratio: 0.0,
            },
            dynamic_range_db: 6.0,
            class: Default::default(),
            hits: Vec::new(),
        }
    }

    fn env(rms_values: &[f64]) -> Vec<EnergyPoint> {
        rms_values
            .iter()
            .enumerate()
            .map(|(i, &rms)| EnergyPoint {
                t: i as f64 * 0.25,
                rms,
            })
            .collect()
    }

    #[test]
    fn derives_entrance_peak_and_exit() {
        // Rise from silence → swell → fall back to silence.
        let e = env(&[0.0, 0.0, 0.1, 0.4, 0.7, 0.9, 0.7, 0.4, 0.1, 0.0, 0.0, 0.0]);
        let stem = stem_with("vocals", e, vec![]);
        let shifts = derive_shifts(&[stem]);

        assert!(
            shifts.iter().any(|s| s.kind == ShiftKind::Entrance),
            "expected an Entrance in {shifts:?}"
        );
        assert!(
            shifts.iter().any(|s| s.kind == ShiftKind::EnergyPeak),
            "expected an EnergyPeak in {shifts:?}"
        );
        assert!(
            shifts.iter().any(|s| s.kind == ShiftKind::Exit),
            "expected an Exit in {shifts:?}"
        );
        // The peak should sit at the maximum (t = 1.25s, index 5).
        let peak = shifts
            .iter()
            .find(|s| s.kind == ShiftKind::EnergyPeak)
            .unwrap();
        assert!((peak.t - 1.25).abs() < 1e-9, "peak at {}", peak.t);
    }

    #[test]
    fn constant_envelope_yields_no_energy_shifts() {
        let e = env(&[0.5; 12]);
        let stem = stem_with("pad", e, vec![]);
        let shifts = derive_shifts(&[stem]);
        assert!(
            !shifts
                .iter()
                .any(|s| matches!(s.kind, ShiftKind::EnergyPeak | ShiftKind::EnergyValley)),
            "a flat envelope must not spawn peaks/valleys: {shifts:?}"
        );
    }

    #[test]
    fn silence_segment_yields_a_silence_shift() {
        let e = env(&[0.5, 0.5, 0.0, 0.0, 0.5, 0.5]);
        let stem = stem_with(
            "drums",
            e,
            vec![Silence {
                start: 0.5,
                end: 1.0,
                avg_level_db: -80.0,
            }],
        );
        let shifts = derive_shifts(&[stem]);
        let sil = shifts
            .iter()
            .find(|s| s.kind == ShiftKind::Silence)
            .expect("expected a Silence shift");
        assert!((sil.t - 0.5).abs() < 1e-9);
    }

    #[test]
    fn build_song_map_reports_rate_duration_and_sorts_shifts() {
        let a = stem_with("a", env(&[0.0, 0.6, 0.9, 0.2, 0.0]), vec![]);
        let b = stem_with("b", env(&[0.0, 0.0, 0.3, 0.8, 0.9, 0.1]), vec![]);
        let map = build_song_map(vec![a, b], None);
        assert_eq!(map.sample_rate, 44100);
        assert_eq!(map.stem_count, 2);
        assert!(map.duration_seconds > 0.0);
        // Shifts are time-sorted.
        for w in map.shifts.windows(2) {
            assert!(w[0].t <= w[1].t, "shifts not sorted: {:?}", map.shifts);
        }
        // No mix → no sections, no structure confidence.
        assert!(map.sections.is_empty());
        assert!(map.structure_confidence.is_none());
    }

    #[test]
    fn build_song_map_classifies_and_derives_hits_per_stem() {
        // A constant pad and a sparse burst stem.
        let pad = stem_with("pad", env(&[0.5; 40]), vec![]);
        let mut burst_env = vec![0.0; 16];
        burst_env.extend(vec![0.9; 4]);
        burst_env.extend(vec![0.0; 20]);
        let hits_stem = stem_with("stabs", env(&burst_env), vec![]);

        let map = build_song_map(vec![pad, hits_stem], None);
        assert_eq!(
            map.stems[0].class,
            crate::service::audio_analysis::StemClass::Constant
        );
        assert!(!map.stems[0].hits.is_empty());
        assert_eq!(
            map.stems[1].class,
            crate::service::audio_analysis::StemClass::Sparse
        );
        assert_eq!(map.stems[1].hits.len(), 1);
    }

    #[test]
    fn build_song_map_attaches_snapped_sections() {
        use crate::service::audio_analysis::progression::RawSection;
        let a = stem_with("a", env(&[0.5; 40]), vec![]);
        let raw = vec![
            RawSection {
                start_s: 0.0,
                end_s: 5.2,
                label: "intro".to_string(),
                mean_energy: 0.4,
                confidence: 0.8,
            },
            RawSection {
                start_s: 5.2,
                end_s: 9.75,
                label: "verse".to_string(),
                mean_energy: 0.6,
                confidence: 0.9,
            },
        ];
        let map = build_song_map(vec![a], Some(raw));
        assert_eq!(map.sections.len(), 2);
        assert!(map.structure_confidence.is_some());
        assert_eq!(map.summary().section_count, 2);
    }
}
