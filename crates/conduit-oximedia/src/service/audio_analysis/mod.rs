//! Audio-analysis service: multitrack WAV stems → an annotated "song map".
//!
//! This is the foundation slice of the music-driven fan-video authoring guide.
//! Given a set of per-instrument WAV **stems** (vocals / drums / bass / …), it
//! extracts per-stem *feel* signals (a windowed RMS energy envelope, silences,
//! spectral summary, dynamic range) plus tempo and mood, then fuses them into a
//! set of typed **shifts** — the musical/emotional turning points the downstream
//! guide hangs plot points off.
//!
//! Layering (mirrors [`crate::service::image`] / [`crate::service::av`]):
//! - [`decode`] — pure WAV → mono `f32` samples (via `hound`).
//! - [`analyze`] — per-stem feature extraction (`oximedia-mir` tempo/mood +
//!   `oximedia-analysis` silence/spectral/dynamic-range + a DIY RMS envelope).
//! - [`fuse`] — pure derivation of the [`SongMap`] and its [`Shift`] list.
//!
//! It is **pure local computation** — no external model, no API key, no enable
//! flag, no stub. The analysis always runs; the handler only needs a DB pool to
//! read the stem bytes and persist the resulting map.

mod analyze;
mod fuse;
mod progression;

use serde::{Deserialize, Serialize};

use crate::error::OximediaError;

/// A stem's raw bytes + metadata, as handed to the analyzer by the handler.
pub struct StemSource {
    /// Display name (e.g. `vocals`, `drums`); falls back to `stem N`.
    pub name: String,
    /// Optional semantic role (e.g. `vocals`/`drums`/`bass`/`other`). Used only
    /// to pick a "primary" stem for the summary tempo today; free-form.
    pub role: Option<String>,
    /// The stem's raw WAV bytes.
    pub bytes: Vec<u8>,
}

/// The annotated song map — the canonical artifact this service produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongMap {
    /// Sample rate of the analyzed stems (Hz). Stems are expected time-aligned
    /// and same-rate; this reports the first stem's rate.
    pub sample_rate: u32,
    /// Longest stem duration (seconds).
    pub duration_seconds: f64,
    /// Number of stems analyzed.
    pub stem_count: usize,
    /// Per-stem analysis.
    pub stems: Vec<StemMap>,
    /// Fused, time-sorted musical/emotional shifts derived across all stems.
    pub shifts: Vec<Shift>,
    /// Structural sections detected on the full mix (empty when no mix was
    /// provided). Boundaries are pre-snapped to the densest stem's downbeat
    /// grid, so operator correction is about labels/merging — never nudging.
    #[serde(default)]
    pub sections: Vec<SectionSpan>,
    /// Aggregate confidence of the section structure (duration-weighted mean),
    /// `None` when no mix was analyzed. Low values mean "treat as
    /// through-composed".
    #[serde(default)]
    pub structure_confidence: Option<f64>,
}

/// Per-stem analysis results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StemMap {
    /// Stem display name.
    pub name: String,
    /// Optional semantic role.
    pub role: Option<String>,
    /// Sample rate of this stem (Hz).
    pub sample_rate: u32,
    /// Stem duration (seconds).
    pub duration_seconds: f64,
    /// Tempo (from `oximedia-mir`); `None` if detection was inconclusive.
    pub tempo: Option<Tempo>,
    /// Mood (from `oximedia-mir`); `None` if detection was inconclusive.
    pub mood: Option<Mood>,
    /// Windowed RMS energy envelope + overall levels (the primary shift driver).
    pub energy: EnergySummary,
    /// Detected silence segments (≥ ~1s at the analyzer's fixed min-duration).
    pub silences: Vec<Silence>,
    /// Coarse spectral summary (from `oximedia-analysis`).
    pub spectral: Spectral,
    /// Dynamic range (dB), peak-to-RMS.
    pub dynamic_range_db: f64,
    /// Activity classification derived from the envelope (chunk budgets +
    /// prompt doctrine downstream).
    #[serde(default)]
    pub class: StemClass,
    /// The stem's "hits" — beat-snapped, silence-bounded activity spans (long
    /// spans subdivided on this stem's downbeat grid). The progression driver:
    /// each hit advances the stem's pattern clip one chunk.
    #[serde(default)]
    pub hits: Vec<HitSpan>,
}

/// Coarse per-stem activity class, computed from data already in the map
/// (presence ratio + span count). Drives chunk budgets and prompt doctrine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StemClass {
    /// Present nearly the whole song (pads / sustained beds).
    Constant,
    /// Present most of the time with gaps (loops / riffs). The default for
    /// maps persisted before classification existed.
    #[default]
    Rhythmic,
    /// Punctuating — long silences, few activity spans.
    Sparse,
}

impl StemClass {
    /// Stable snake_case label (matches the serde wire form).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Rhythmic => "rhythmic",
            Self::Sparse => "sparse",
        }
    }
}

/// One silence-bounded, beat-snapped activity span of a stem — a "hit".
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HitSpan {
    /// Onset (seconds), snapped to the stem's beat grid when one exists.
    pub start_s: f64,
    /// End (seconds).
    pub end_s: f64,
}

/// One structural section of the full mix, boundary-snapped to the densest
/// stem's downbeat grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionSpan {
    /// Start time (seconds), downbeat-snapped.
    pub start_s: f64,
    /// End time (seconds), downbeat-snapped.
    pub end_s: f64,
    /// Structural label (`intro`/`verse`/`pre-chorus`/`chorus`/`bridge`/
    /// `outro`/`unknown`).
    pub label: String,
    /// Same-label recurrence group key (equal to `label`); sections sharing a
    /// group recur musically and share story/art direction downstream.
    pub group: String,
    /// 1-based occurrence index within the group.
    pub occurrence: u32,
    /// Mean energy of the section (normalised, from the segmenter).
    pub mean_energy: f64,
    /// Segmenter confidence (0.0–1.0).
    pub confidence: f64,
}

/// Tempo detection result (subset of `oximedia-mir`'s `TempoResult` + beat grid).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tempo {
    /// Beats per minute.
    pub bpm: f32,
    /// Detection confidence (0.0–1.0).
    pub confidence: f32,
    /// Tempo stability (0.0–1.0, higher = steadier).
    pub stability: f32,
    /// Beat times (seconds).
    pub beat_times: Vec<f32>,
    /// Downbeat (bar-start) times (seconds).
    pub downbeat_times: Vec<f32>,
}

/// Mood detection result (Russell valence/arousal + top label).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mood {
    /// Valence (negative → positive, -1.0..1.0).
    pub valence: f32,
    /// Arousal (calm → energetic, 0.0..1.0).
    pub arousal: f32,
    /// Emotional intensity (0.0..1.0).
    pub intensity: f32,
    /// Highest-scoring mood label, if any.
    pub top: Option<String>,
}

/// One point of the windowed energy envelope.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnergyPoint {
    /// Window centre time (seconds).
    pub t: f64,
    /// Window RMS (linear, 0.0..~1.0).
    pub rms: f64,
}

/// Windowed RMS energy envelope + overall levels for a stem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergySummary {
    /// Analysis window length (seconds).
    pub window_seconds: f64,
    /// Hop between windows (seconds).
    pub hop_seconds: f64,
    /// Peak level (dBFS).
    pub peak_dbfs: f64,
    /// Overall RMS level (dBFS).
    pub rms_dbfs: f64,
    /// The envelope, in time order.
    pub envelope: Vec<EnergyPoint>,
}

/// A silence segment (seconds).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Silence {
    /// Start time (seconds).
    pub start: f64,
    /// End time (seconds).
    pub end: f64,
    /// Average level during the segment (dB).
    pub avg_level_db: f64,
}

/// Coarse spectral summary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Spectral {
    /// Spectral centroid (Hz) — perceived "brightness".
    pub centroid_hz: f64,
    /// Spectral flatness (0.0 tonal … 1.0 noise-like).
    pub flatness: f64,
    /// Spectral rolloff (Hz), 95% energy threshold.
    pub rolloff_hz: f64,
    /// Low-frequency energy ratio (0.0..1.0).
    pub low_freq_ratio: f64,
    /// High-frequency energy ratio (0.0..1.0).
    pub high_freq_ratio: f64,
}

/// A typed musical/emotional turning point on the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shift {
    /// Time (seconds).
    pub t: f64,
    /// What kind of turning point this is.
    pub kind: ShiftKind,
    /// The stem that produced it (`None` = fused/global).
    pub stem: Option<String>,
    /// Relative salience (0.0..1.0).
    pub strength: f64,
}

/// Kinds of shift the fuse layer derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftKind {
    /// A local energy maximum (a swell / hit).
    EnergyPeak,
    /// A local energy minimum (a drop / breakdown).
    EnergyValley,
    /// The start of a silence segment.
    Silence,
    /// A stem's first entrance (energy rises from near-nothing).
    Entrance,
    /// A stem's exit (energy falls to near-nothing and stays there).
    Exit,
}

impl ShiftKind {
    /// Stable snake_case label — the name a shift is given when it is placed on
    /// a composed timeline as a marker (matches the `serde` `rename_all`
    /// wire form, but is spelled out here so a marker name never depends on
    /// serialization). Deterministic.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::EnergyPeak => "energy_peak",
            Self::EnergyValley => "energy_valley",
            Self::Silence => "silence",
            Self::Entrance => "entrance",
            Self::Exit => "exit",
        }
    }
}

/// A small at-a-glance summary of a [`SongMap`], returned in the HTTP response
/// so callers don't have to fetch + parse the full map just to sanity-check it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongMapSummary {
    /// Longest stem duration (seconds).
    pub duration_seconds: f64,
    /// Number of stems.
    pub stem_count: usize,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Number of derived shifts.
    pub shift_count: usize,
    /// Representative tempo (BPM) — prefers a `drums` stem, else the first stem
    /// with a tempo reading.
    pub tempo_bpm: Option<f32>,
    /// Number of detected mix sections (0 when no mix was provided).
    #[serde(default)]
    pub section_count: usize,
    /// Aggregate section-structure confidence (see [`SongMap`]).
    #[serde(default)]
    pub structure_confidence: Option<f64>,
    /// Per-stem class + hit count, in stem order — what the Read-the-Song pane
    /// and the design prompts feed on.
    #[serde(default)]
    pub stems: Vec<StemSummary>,
}

/// Per-stem entry of the [`SongMapSummary`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StemSummary {
    /// Stem display name.
    pub name: String,
    /// Activity class label (`constant`/`rhythmic`/`sparse`).
    pub class: String,
    /// Number of derived hits (progression steps).
    pub hit_count: usize,
}

impl SongMap {
    /// Build the at-a-glance summary.
    #[must_use]
    pub fn summary(&self) -> SongMapSummary {
        // Prefer a percussive stem's tempo (isolated drums track most reliably),
        // else the first stem that has a tempo.
        let tempo_bpm = self
            .stems
            .iter()
            .find(|s| s.role.as_deref() == Some("drums"))
            .and_then(|s| s.tempo.as_ref())
            .or_else(|| self.stems.iter().find_map(|s| s.tempo.as_ref()))
            .map(|t| t.bpm);

        SongMapSummary {
            duration_seconds: self.duration_seconds,
            stem_count: self.stem_count,
            sample_rate: self.sample_rate,
            shift_count: self.shifts.len(),
            tempo_bpm,
            section_count: self.sections.len(),
            structure_confidence: self.structure_confidence,
            stems: self
                .stems
                .iter()
                .map(|s| StemSummary {
                    name: s.name.clone(),
                    class: s.class.label().to_string(),
                    hit_count: s.hits.len(),
                })
                .collect(),
        }
    }
}

/// Decode, analyze, and fuse a set of stems into a [`SongMap`].
///
/// `mix` is the optional mastered full mix: when present it is segmented into
/// structural sections (downbeat-snapped) that ride the map; the mix is never
/// added as a stem.
///
/// Synchronous + CPU-bound (FFTs, autocorrelation) — call from a
/// `spawn_blocking` context, as the handler does.
pub fn analyze_stems(
    sources: Vec<StemSource>,
    mix: Option<StemSource>,
) -> Result<SongMap, OximediaError> {
    if sources.is_empty() {
        return Err(OximediaError::InvalidRequest(
            "analyze-audio requires at least one stem".to_string(),
        ));
    }

    let mut stems = Vec::with_capacity(sources.len());
    for (i, src) in sources.into_iter().enumerate() {
        let label = if src.name.is_empty() {
            format!("stem {}", i + 1)
        } else {
            src.name.clone()
        };

        let decoded = crate::service::wav::decode_wav(&src.bytes)
            .map_err(|e| OximediaError::AudioAnalysisError(format!("stem '{label}': {e}")))?;

        let stem = analyze::analyze_stem(&label, src.role, &decoded)
            .map_err(|e| OximediaError::AudioAnalysisError(format!("stem '{label}': {e}")))?;

        stems.push(stem);
    }

    // Segment the full mix into sections, if provided.
    let structure = match mix {
        Some(src) => {
            let decoded = crate::service::wav::decode_wav(&src.bytes)
                .map_err(|e| OximediaError::AudioAnalysisError(format!("mix: {e}")))?;
            Some(
                analyze::segment_mix(&decoded)
                    .map_err(|e| OximediaError::AudioAnalysisError(format!("mix: {e}")))?,
            )
        }
        None => None,
    };

    Ok(fuse::build_song_map(stems, structure))
}
