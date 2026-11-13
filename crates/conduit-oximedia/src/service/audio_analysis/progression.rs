//! Pure progression-layer derivations: section snapping/grouping, stem
//! classification, and per-stem hit extraction.
//!
//! Everything here is a deterministic function of data already in the map —
//! no audio access, no model. These are the temporal primitives the
//! multi-timelined-video ("video quilt") pipeline hangs on:
//!
//! - **Sections** detected on the mix are *snapped to the densest stem's
//!   downbeat grid* before an operator ever sees them, so Read-the-Song
//!   correction is about labels and merging, never millisecond nudging.
//! - **Hits** are a stem's silence-bounded activity spans (long spans
//!   subdivided on that stem's own downbeat grid, onsets beat-snapped). Each
//!   hit advances the stem's pattern clip one chunk downstream.
//! - **Classes** (`constant`/`rhythmic`/`sparse`) fall out of presence ratio +
//!   span count; they drive chunk budgets here and prompt doctrine downstream.

use super::{EnergyPoint, HitSpan, SectionSpan, StemClass, StemMap, Tempo};

/// Fraction of a stem's peak RMS above which a window counts as "present".
/// Matches `fuse::ENTER_FRAC` so hits agree with entrance/exit shifts.
const PRESENCE_FRAC: f64 = 0.15;
/// Activity gaps shorter than this merge into one span (seconds).
const MERGE_GAP_SECONDS: f64 = 0.75;
/// Activity spans shorter than this are dropped as noise (seconds).
const MIN_SPAN_SECONDS: f64 = 0.35;
/// Presence ratio at/above which a stem is `constant`.
const CONSTANT_RATIO: f64 = 0.85;
/// Presence ratio at/below which a stem is `sparse`.
const SPARSE_RATIO: f64 = 0.40;
/// Per-class target span length for downbeat subdivision (seconds).
const SPLIT_TARGET_CONSTANT: f64 = 30.0;
const SPLIT_TARGET_RHYTHMIC: f64 = 15.0;
/// Hard per-stem hit budget after subdivision.
const MAX_HITS_PER_STEM: usize = 24;
/// A section boundary snaps to a downbeat at most this far away (seconds).
const SECTION_SNAP_SECONDS: f64 = 2.0;
/// Sections shorter than this after snapping merge into their neighbour.
const MIN_SECTION_SECONDS: f64 = 1.0;
/// An onset snaps to a beat at most this far away (seconds).
const ONSET_SNAP_SECONDS: f64 = 0.35;

/// A raw (unsnapped, ungrouped) section straight off the segmenter.
#[derive(Debug, Clone)]
pub struct RawSection {
    pub start_s: f64,
    pub end_s: f64,
    pub label: String,
    pub mean_energy: f64,
    pub confidence: f64,
}

/// Pick the densest downbeat grid across stems — the grid section boundaries
/// snap to. Returns an empty slice when no stem has downbeats.
#[must_use]
pub fn densest_downbeat_grid(stems: &[StemMap]) -> &[f32] {
    stems
        .iter()
        .filter_map(|s| s.tempo.as_ref())
        .map(|t| t.downbeat_times.as_slice())
        .max_by_key(|d| d.len())
        .unwrap_or(&[])
}

/// Snap raw sections to the downbeat grid, drop degenerates, and group
/// same-label recurrences. Also returns the aggregate (duration-weighted)
/// structure confidence, `None` when there are no sections.
#[must_use]
pub fn snap_and_group_sections(
    raw: &[RawSection],
    downbeats: &[f32],
) -> (Vec<SectionSpan>, Option<f64>) {
    if raw.is_empty() {
        return (Vec::new(), None);
    }

    // Snap every boundary (starts; each end is the next start, so interior
    // boundaries stay watertight). The first start and last end are pinned.
    let mut starts: Vec<f64> = raw.iter().map(|s| s.start_s).collect();
    let last_end = raw.last().map_or(0.0, |s| s.end_s);
    for s in starts.iter_mut().skip(1) {
        *s = snap_to_grid(*s, downbeats, SECTION_SNAP_SECONDS);
    }

    // Rebuild contiguous sections; a section squeezed below the minimum after
    // snapping merges into its predecessor (its label is dropped).
    let mut sections: Vec<SectionSpan> = Vec::with_capacity(raw.len());
    for (i, r) in raw.iter().enumerate() {
        let start = starts[i];
        let end = if i + 1 < raw.len() {
            starts[i + 1]
        } else {
            last_end
        };
        if end - start < MIN_SECTION_SECONDS {
            // Degenerate after snapping: extend the previous section over it.
            if let Some(prev) = sections.last_mut() {
                prev.end_s = end.max(prev.end_s);
            }
            continue;
        }
        sections.push(SectionSpan {
            start_s: start,
            end_s: end,
            label: r.label.clone(),
            group: r.label.clone(),
            occurrence: 0, // assigned below
            mean_energy: r.mean_energy,
            confidence: r.confidence,
        });
    }

    // 1-based occurrence index within each same-label group.
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for s in &mut sections {
        let n = seen.entry(s.group.clone()).or_insert(0);
        *n += 1;
        s.occurrence = *n;
    }

    let total: f64 = sections.iter().map(|s| s.end_s - s.start_s).sum();
    let confidence = if total > 0.0 {
        Some(
            sections
                .iter()
                .map(|s| s.confidence * (s.end_s - s.start_s))
                .sum::<f64>()
                / total,
        )
    } else {
        None
    };

    (sections, confidence)
}

/// Classify a stem from its envelope: presence ratio → class.
#[must_use]
pub fn classify_stem(envelope: &[EnergyPoint]) -> StemClass {
    let max_rms = envelope.iter().map(|p| p.rms).fold(0.0f64, f64::max);
    if max_rms <= 1e-9 || envelope.is_empty() {
        return StemClass::Sparse;
    }
    let threshold = PRESENCE_FRAC * max_rms;
    let present = envelope.iter().filter(|p| p.rms >= threshold).count();
    let ratio = present as f64 / envelope.len() as f64;
    if ratio >= CONSTANT_RATIO {
        StemClass::Constant
    } else if ratio <= SPARSE_RATIO {
        StemClass::Sparse
    } else {
        StemClass::Rhythmic
    }
}

/// Derive a stem's hits: presence runs off the envelope → merge tiny gaps →
/// drop noise spans → subdivide long spans on the stem's downbeat grid (per
/// class budget) → snap onsets to the beat grid.
#[must_use]
pub fn derive_hits(
    envelope: &[EnergyPoint],
    tempo: Option<&Tempo>,
    class: StemClass,
) -> Vec<HitSpan> {
    let spans = activity_spans(envelope);
    let spans = merge_and_filter(spans);

    let downbeats: &[f32] = tempo.map_or(&[], |t| t.downbeat_times.as_slice());
    let beats: &[f32] = tempo.map_or(&[], |t| t.beat_times.as_slice());

    let target = match class {
        StemClass::Constant => Some(SPLIT_TARGET_CONSTANT),
        StemClass::Rhythmic => Some(SPLIT_TARGET_RHYTHMIC),
        StemClass::Sparse => None, // every span is one dramatic hit
    };

    let mut hits: Vec<HitSpan> = Vec::new();
    for span in spans {
        let mut pieces = match target {
            Some(t) => split_on_downbeats(span, downbeats, t),
            None => vec![span],
        };
        // Enforce the per-stem budget: stop splitting once full (the tail of
        // an over-budget span rides its last accepted piece).
        while hits.len() + pieces.len() > MAX_HITS_PER_STEM && pieces.len() > 1 {
            let last = pieces.pop().expect("len > 1");
            let prev = pieces.last_mut().expect("len >= 1");
            prev.end_s = last.end_s;
        }
        if hits.len() >= MAX_HITS_PER_STEM {
            // Budget exhausted: extend the final hit over the remainder.
            if let (Some(last_hit), Some(tail)) = (hits.last_mut(), pieces.last()) {
                last_hit.end_s = tail.end_s.max(last_hit.end_s);
            }
            continue;
        }
        hits.append(&mut pieces);
    }

    // Snap onsets to the beat grid (starts only; ends are hold boundaries).
    for h in &mut hits {
        let snapped = snap_to_grid(h.start_s, beats, ONSET_SNAP_SECONDS);
        if snapped < h.end_s {
            h.start_s = snapped;
        }
    }
    hits
}

/// Contiguous presence runs (rms ≥ PRESENCE_FRAC × max) as raw spans.
fn activity_spans(envelope: &[EnergyPoint]) -> Vec<HitSpan> {
    let max_rms = envelope.iter().map(|p| p.rms).fold(0.0f64, f64::max);
    if max_rms <= 1e-9 {
        return Vec::new();
    }
    let threshold = PRESENCE_FRAC * max_rms;

    let mut spans = Vec::new();
    let mut open: Option<f64> = None;
    for (i, p) in envelope.iter().enumerate() {
        let present = p.rms >= threshold;
        match (present, open) {
            (true, None) => open = Some(p.t),
            (false, Some(start)) => {
                spans.push(HitSpan {
                    start_s: start,
                    end_s: p.t,
                });
                open = None;
            }
            _ => {}
        }
        if present && i == envelope.len() - 1 {
            if let Some(start) = open.take() {
                spans.push(HitSpan {
                    start_s: start,
                    end_s: p.t,
                });
            }
        }
    }
    spans
}

/// Merge spans separated by gaps under MERGE_GAP_SECONDS; drop spans under
/// MIN_SPAN_SECONDS.
fn merge_and_filter(spans: Vec<HitSpan>) -> Vec<HitSpan> {
    let mut merged: Vec<HitSpan> = Vec::with_capacity(spans.len());
    for s in spans {
        match merged.last_mut() {
            Some(prev) if s.start_s - prev.end_s < MERGE_GAP_SECONDS => {
                prev.end_s = s.end_s.max(prev.end_s);
            }
            _ => merged.push(s),
        }
    }
    merged
        .into_iter()
        .filter(|s| s.end_s - s.start_s >= MIN_SPAN_SECONDS)
        .collect()
}

/// Split a span at the downbeats nearest its equal partitions so no piece is
/// (much) longer than `target_len`. No downbeats inside the span → no split.
fn split_on_downbeats(span: HitSpan, downbeats: &[f32], target_len: f64) -> Vec<HitSpan> {
    let dur = span.end_s - span.start_s;
    let pieces = (dur / target_len).ceil().max(1.0) as usize;
    if pieces <= 1 {
        return vec![span];
    }

    // Candidate cut points: downbeats strictly inside the span.
    let inside: Vec<f64> = downbeats
        .iter()
        .map(|&d| f64::from(d))
        .filter(|&d| d > span.start_s + f64::EPSILON && d < span.end_s - f64::EPSILON)
        .collect();
    if inside.is_empty() {
        return vec![span];
    }

    // For each equal-partition time, pick the nearest unused inside-downbeat.
    let mut cuts: Vec<f64> = Vec::new();
    for k in 1..pieces {
        let ideal = span.start_s + dur * (k as f64) / (pieces as f64);
        let nearest = inside
            .iter()
            .copied()
            .min_by(|a, b| {
                (a - ideal)
                    .abs()
                    .partial_cmp(&(b - ideal).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("inside non-empty");
        if !cuts.iter().any(|c| (c - nearest).abs() < f64::EPSILON) {
            cuts.push(nearest);
        }
    }
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut cur = span.start_s;
    for c in cuts {
        out.push(HitSpan {
            start_s: cur,
            end_s: c,
        });
        cur = c;
    }
    out.push(HitSpan {
        start_s: cur,
        end_s: span.end_s,
    });
    out
}

/// Snap `t` to the nearest grid point within `tolerance`; unchanged otherwise
/// (or when the grid is empty).
fn snap_to_grid(t: f64, grid: &[f32], tolerance: f64) -> f64 {
    grid.iter()
        .map(|&g| f64::from(g))
        .min_by(|a, b| {
            (a - t)
                .abs()
                .partial_cmp(&(b - t).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .filter(|g| (g - t).abs() <= tolerance)
        .unwrap_or(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(rms: &[f64]) -> Vec<EnergyPoint> {
        rms.iter()
            .enumerate()
            .map(|(i, &r)| EnergyPoint {
                t: i as f64 * 0.25,
                rms: r,
            })
            .collect()
    }

    fn tempo(beats: Vec<f32>, downbeats: Vec<f32>) -> Tempo {
        Tempo {
            bpm: 120.0,
            confidence: 0.9,
            stability: 0.9,
            beat_times: beats,
            downbeat_times: downbeats,
        }
    }

    fn raw(start: f64, end: f64, label: &str, conf: f64) -> RawSection {
        RawSection {
            start_s: start,
            end_s: end,
            label: label.to_string(),
            mean_energy: 0.5,
            confidence: conf,
        }
    }

    // ── snapping + grouping ──────────────────────────────────────────────────

    #[test]
    fn sections_snap_to_downbeats_and_stay_contiguous() {
        let raw_sections = vec![
            raw(0.0, 10.3, "intro", 0.8),
            raw(10.3, 30.6, "verse", 0.7),
            raw(30.6, 50.0, "chorus", 0.9),
        ];
        // Downbeats every 2s.
        let downbeats: Vec<f32> = (0..26).map(|i| i as f32 * 2.0).collect();
        let (sections, conf) = snap_and_group_sections(&raw_sections, &downbeats);

        assert_eq!(sections.len(), 3);
        assert!(
            (sections[0].start_s - 0.0).abs() < 1e-9,
            "first start pinned"
        );
        assert!((sections[0].end_s - 10.0).abs() < 1e-9, "10.3 → 10.0");
        assert!((sections[1].start_s - 10.0).abs() < 1e-9, "contiguous");
        assert!((sections[1].end_s - 30.0).abs() < 1e-9, "30.6 → 30.0"); // wait: nearest downbeat to 30.6 is 30.0
        assert!((sections[2].end_s - 50.0).abs() < 1e-9, "last end pinned");
        assert!(conf.is_some());
    }

    #[test]
    fn same_label_sections_group_with_occurrences() {
        let raw_sections = vec![
            raw(0.0, 10.0, "verse", 0.8),
            raw(10.0, 20.0, "chorus", 0.8),
            raw(20.0, 30.0, "verse", 0.8),
            raw(30.0, 40.0, "chorus", 0.8),
        ];
        let (sections, _) = snap_and_group_sections(&raw_sections, &[]);
        assert_eq!(sections[0].group, "verse");
        assert_eq!(sections[0].occurrence, 1);
        assert_eq!(sections[2].group, "verse");
        assert_eq!(sections[2].occurrence, 2);
        assert_eq!(sections[3].group, "chorus");
        assert_eq!(sections[3].occurrence, 2);
    }

    #[test]
    fn degenerate_section_after_snapping_merges_into_predecessor() {
        // The middle section is 0.5s wide — under the minimum — and must fold
        // into its predecessor without breaking contiguity.
        let raw_sections = vec![
            raw(0.0, 10.0, "verse", 0.8),
            raw(10.0, 10.5, "unknown", 0.2),
            raw(10.5, 20.0, "chorus", 0.8),
        ];
        let (sections, _) = snap_and_group_sections(&raw_sections, &[]);
        assert_eq!(sections.len(), 2);
        assert!((sections[0].end_s - 10.5).abs() < 1e-9);
        assert!((sections[1].start_s - 10.5).abs() < 1e-9);
    }

    #[test]
    fn no_sections_no_confidence() {
        let (sections, conf) = snap_and_group_sections(&[], &[]);
        assert!(sections.is_empty());
        assert!(conf.is_none());
    }

    #[test]
    fn structure_confidence_is_duration_weighted() {
        let raw_sections = vec![
            raw(0.0, 30.0, "verse", 1.0),  // 30s at 1.0
            raw(30.0, 40.0, "outro", 0.0), // 10s at 0.0
        ];
        let (_, conf) = snap_and_group_sections(&raw_sections, &[]);
        assert!((conf.unwrap() - 0.75).abs() < 1e-9);
    }

    // ── classification ───────────────────────────────────────────────────────

    #[test]
    fn classifies_constant_rhythmic_sparse() {
        // Always present.
        assert_eq!(classify_stem(&env(&[0.5; 40])), StemClass::Constant);
        // Present ~60% of the time.
        let mut half = vec![0.5; 24];
        half.extend(vec![0.0; 16]);
        assert_eq!(classify_stem(&env(&half)), StemClass::Rhythmic);
        // Present ~15% of the time.
        let mut sparse = vec![0.9; 6];
        sparse.extend(vec![0.0; 34]);
        assert_eq!(classify_stem(&env(&sparse)), StemClass::Sparse);
        // Dead stem.
        assert_eq!(classify_stem(&env(&[0.0; 10])), StemClass::Sparse);
    }

    // ── hits ─────────────────────────────────────────────────────────────────

    #[test]
    fn sparse_stem_hits_are_its_activity_spans() {
        // Three bursts separated by long silences (0.25s per point).
        let mut rms = vec![0.0; 8];
        rms.extend(vec![0.9; 4]); // burst 1: 2.0–3.0
        rms.extend(vec![0.0; 12]);
        rms.extend(vec![0.8; 4]); // burst 2: 6.0–7.0
        rms.extend(vec![0.0; 12]);
        rms.extend(vec![0.7; 4]); // burst 3: 10.0–11.0
        rms.extend(vec![0.0; 4]);
        let hits = derive_hits(&env(&rms), None, StemClass::Sparse);
        assert_eq!(hits.len(), 3, "hits: {hits:?}");
        assert!((hits[0].start_s - 2.0).abs() < 0.3);
        assert!((hits[1].start_s - 6.0).abs() < 0.3);
        assert!((hits[2].start_s - 10.0).abs() < 0.3);
        for h in &hits {
            assert!(h.end_s > h.start_s);
        }
    }

    #[test]
    fn tiny_gaps_merge_and_noise_spans_drop() {
        // Two presence runs separated by a 0.5s gap (< merge threshold) plus a
        // lone 0.25s blip later (< min span).
        let mut rms = vec![0.9; 8]; // 0.0–2.0
        rms.extend(vec![0.0; 2]); // 2.0–2.5 gap (merges)
        rms.extend(vec![0.9; 8]); // 2.5–4.5
        rms.extend(vec![0.0; 12]);
        rms.push(0.9); // lone blip (drops)
        rms.extend(vec![0.0; 8]);
        let hits = derive_hits(&env(&rms), None, StemClass::Sparse);
        assert_eq!(hits.len(), 1, "hits: {hits:?}");
    }

    #[test]
    fn constant_stem_long_span_subdivides_on_downbeats() {
        // 90s of constant presence at 0.25s/point = 360 points.
        let rms = vec![0.6; 360];
        // Downbeats every 2 seconds.
        let downbeats: Vec<f32> = (0..46).map(|i| i as f32 * 2.0).collect();
        let hits = derive_hits(
            &env(&rms),
            Some(&tempo(vec![], downbeats)),
            StemClass::Constant,
        );
        // ~90s / 30s target = 3 pieces, each cut on a downbeat.
        assert_eq!(hits.len(), 3, "hits: {hits:?}");
        for h in &hits[1..] {
            assert!(
                (h.start_s / 2.0 - (h.start_s / 2.0).round()).abs() < 1e-6,
                "cut {} not on a downbeat",
                h.start_s
            );
        }
        // Watertight coverage.
        for w in hits.windows(2) {
            assert!((w[0].end_s - w[1].start_s).abs() < 1e-9);
        }
    }

    #[test]
    fn no_downbeats_means_no_subdivision() {
        let rms = vec![0.6; 360]; // 90s constant
        let hits = derive_hits(&env(&rms), None, StemClass::Constant);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn hit_budget_is_enforced() {
        // 6 minutes of constant presence with downbeats every second would
        // want 24+ pieces at the rhythmic 15s target; the budget caps it.
        let rms = vec![0.6; 1440]; // 360s
        let downbeats: Vec<f32> = (0..360).map(|i| i as f32).collect();
        let hits = derive_hits(
            &env(&rms),
            Some(&tempo(vec![], downbeats)),
            StemClass::Rhythmic,
        );
        assert!(hits.len() <= MAX_HITS_PER_STEM, "{} hits", hits.len());
        // Coverage still reaches the end.
        assert!((hits.last().unwrap().end_s - 359.75).abs() < 1.0);
    }

    #[test]
    fn onsets_snap_to_beats() {
        // A burst starting at 2.125s with beats on the whole seconds.
        let mut rms = vec![0.0; 9]; // 0.0–2.0 (first present point t=2.25)
        rms.extend(vec![0.9; 8]);
        rms.extend(vec![0.0; 8]);
        let beats: Vec<f32> = (0..8).map(|i| i as f32).collect();
        let hits = derive_hits(&env(&rms), Some(&tempo(beats, vec![])), StemClass::Sparse);
        assert_eq!(hits.len(), 1);
        assert!(
            (hits[0].start_s - 2.0).abs() < 1e-9,
            "onset {} should snap to 2.0",
            hits[0].start_s
        );
    }

    #[test]
    fn snap_respects_tolerance() {
        assert_eq!(snap_to_grid(5.0, &[], 2.0), 5.0);
        assert!((snap_to_grid(5.4, &[4.0, 6.0], 2.0) - 6.0).abs() < 1e-9);
        // Nearest grid point is 1.6s away — outside a 0.35 tolerance.
        assert!((snap_to_grid(5.4, &[4.0, 7.0], 0.35) - 5.4).abs() < 1e-9);
    }
}
