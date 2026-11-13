//! Deterministic multi-category moment detector (no LLM).
//!
//! Scores each transcript segment against six labeled cue lexicons — `educational`
//! (teaching/how-to), `informative` (context/lore), `exciting` (positive reactions),
//! `fails` (a discrete botch/death), `funny` (laughter), and `frustration`
//! (sustained irritation) — clusters consecutive scoring segments per kind into
//! candidate windows (bridging small gaps), attaches the game from the VOD chapters,
//! and returns candidates ranked by a saturating 0..1 score. All six come out of ONE
//! pass over the transcript (the expensive pass was the ASR upstream).
//!
//! The four reaction kinds (`exciting` / `fails` / `funny` / `frustration`) also fuse
//! the audio energy track, so a loud moment scores even when it's wordless (a scream,
//! a groan) — the **lexicon supplies the valence** that routes the spike to the right
//! kind; a pure energy spike with no cue words defaults to `exciting`.
//!
//! Each candidate also carries `neighbors`: nearby candidates of *other* kinds,
//! so the operator can approve a teaching moment together with the hype/fail right
//! around it and let the cut stage weave them. Lexicons + weights are starter sets
//! meant to be tuned against real results; a flan-T5 semantic pass is a later layer.

use serde::{Deserialize, Serialize};

use crate::service::transcribe::{EnergyTrack, TranscriptSegment};

/// One cue. `phrase` cues match as a lowercase substring; non-phrase (single-word)
/// cues match on word boundaries so `"always"` doesn't fire inside another word.
struct Cue {
    pat: &'static str,
    weight: f64,
    phrase: bool,
}

/// The kind of moment a candidate represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Teaching / how-to (second-person imperative + explanatory markers).
    Educational,
    /// Context / lore / definitions (expository, distinct from how-to teaching).
    Informative,
    /// Positive reactions — hype, clutch plays, big wins.
    Exciting,
    /// Negative reactions — deaths, blunders, throws (a discrete botch).
    Fails,
    /// Laughter / comedy — the streamer (or chat read aloud) cracking up.
    Funny,
    /// Sustained irritation — the streamer fighting the game/mechanics ("why does
    /// this keep happening"). Distinct from a discrete `fails`; there's learning in
    /// *where* the friction is.
    Frustration,
}

impl EventKind {
    /// The snake_case wire key (matches the serde rename) — used to build stable
    /// candidate ids and to tag clips. `pub` so the scene segmenter can read a
    /// beat's kind key when collecting a scene's content reactions.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Educational => "educational",
            Self::Informative => "informative",
            Self::Exciting => "exciting",
            Self::Fails => "fails",
            Self::Funny => "funny",
            Self::Frustration => "frustration",
        }
    }
}

/// Per-segment scored output for one kind: `(raw_weight, matched_cues)` per segment.
type ScoredSegments = Vec<(f64, Vec<String>)>;

/// A marker cue added to a segment's matched list when audio energy contributed.
const ENERGY_CUE: &str = "⚡energy";

/// Peak-relative energy at or above this fraction counts as a "spike" and
/// contributes to a reaction score; below it energy contributes nothing (so a
/// merely-audible segment doesn't become a candidate on loudness alone). Tunable.
const ENERGY_FLOOR: f64 = 0.5;

const EDUCATIONAL: &[Cue] = &[
    Cue {
        pat: "what you want to do",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "the trick is",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "the key is",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "pro tip",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "pay attention",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "make sure",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "you want to",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "you have to",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "you need to",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "the reason",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "remember to",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "the idea is",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "notice how",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "focus on",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "watch this",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "watch how",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "the way i",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "the way you",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "if you",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "so that",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "because",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "basically",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "always",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "never",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "avoid",
        weight: 1.0,
        phrase: false,
    },
    Cue {
        pat: "don't",
        weight: 1.0,
        phrase: false,
    },
];

const INFORMATIVE: &[Cue] = &[
    Cue {
        pat: "the difference between",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "the way this works",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "what this does",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "stands for",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "turns out",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "this is",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "there's a",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "means that",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "originally",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "back in",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "the story",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "the lore",
        weight: 2.0,
        phrase: true,
    },
];

const EXCITING: &[Cue] = &[
    Cue {
        pat: "let's go",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "oh my god",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "no way",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "clip that",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "did you see",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "are you kidding",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "so good",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "no no no",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "insane",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "clutch",
        weight: 3.0,
        phrase: false,
    },
    Cue {
        pat: "holy",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "gg",
        weight: 2.0,
        phrase: false,
    },
];

const FAILS: &[Cue] = &[
    Cue {
        pat: "why did i",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "i'm dead",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "i died",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "we died",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "my bad",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "messed up",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "oh no",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "so bad",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "unlucky",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "welp",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "rip",
        weight: 1.0,
        phrase: false,
    },
    Cue {
        pat: "worst",
        weight: 1.0,
        phrase: false,
    },
    Cue {
        pat: "whiffed",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "griefed",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "threw",
        weight: 1.0,
        phrase: false,
    },
    Cue {
        pat: "feed",
        weight: 1.0,
        phrase: false,
    },
];

const FRUSTRATION: &[Cue] = &[
    Cue {
        pat: "why does this",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "so frustrating",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "so annoying",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "give me a break",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "you gotta be kidding",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "are you serious",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "why won't",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "every time",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "this is stupid",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "come on",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "of course",
        weight: 1.0,
        phrase: true,
    },
    Cue {
        pat: "seriously",
        weight: 1.0,
        phrase: false,
    },
    Cue {
        pat: "ugh",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "unfair",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "broken",
        weight: 1.0,
        phrase: false,
    },
    Cue {
        pat: "ridiculous",
        weight: 2.0,
        phrase: false,
    },
];

const FUNNY: &[Cue] = &[
    Cue {
        pat: "[laughter]",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "haha",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "cracking up",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "dying laughing",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "so funny",
        weight: 3.0,
        phrase: true,
    },
    Cue {
        pat: "hilarious",
        weight: 3.0,
        phrase: false,
    },
    Cue {
        pat: "lmao",
        weight: 3.0,
        phrase: false,
    },
    Cue {
        pat: "lmfao",
        weight: 3.0,
        phrase: false,
    },
    Cue {
        pat: "lol",
        weight: 2.0,
        phrase: false,
    },
    Cue {
        pat: "i'm dying",
        weight: 2.0,
        phrase: true,
    },
    Cue {
        pat: "i can't",
        weight: 1.0,
        phrase: true,
    },
];

/// A VOD chapter (game/segment) used to label + optionally scope candidates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub title: String,
}

/// A nearby candidate of a *different* kind — the material for weaving a teaching
/// moment together with the hype/fail right around it.
#[derive(Clone, Debug, Serialize)]
pub struct Neighbor {
    /// The referenced candidate's stable id (join key for weaving).
    pub id: String,
    pub kind: EventKind,
    pub start: f64,
    pub end: f64,
    /// Seconds between the two windows (0 when they overlap).
    pub gap_secs: f64,
}

/// A detected candidate (reviewable before cutting).
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    /// Stable id (`<kind>-<start_centisecond>`), so a curated beat round-trips to
    /// the assembler and neighbors can be joined across re-detects.
    pub id: String,
    pub kind: EventKind,
    pub start: f64,
    pub end: f64,
    /// Saturating 0..1 score (`raw / (raw + K)`).
    pub score: f64,
    pub snippet: String,
    pub cues: Vec<String>,
    pub game: Option<String>,
    /// Nearby candidates of other kinds (for weaving), nearest first.
    pub neighbors: Vec<Neighbor>,
}

/// Detection tuning knobs.
pub struct DetectParams {
    /// Drop candidates below this normalized score (default 0.0 = keep all; the
    /// operator filters the table).
    pub min_score: f64,
    /// Bridge consecutive scoring segments up to this gap (seconds) into one
    /// candidate window.
    pub merge_gap_secs: f64,
    /// Optional case-insensitive game/chapter-title filter.
    pub game_filter: Option<String>,
    /// Restrict detection to these kinds (`None` = all kinds).
    pub kinds: Option<Vec<EventKind>>,
    /// Weight applied to a normalized energy spike when scoring reaction kinds.
    pub energy_weight: f64,
    /// Window (seconds) within which a candidate of another kind is surfaced as a
    /// `neighbor` for weaving. `0` disables neighbor computation.
    pub neighbor_window_secs: f64,
    /// Extend the clip start back to the onset of the preceding audio-energy spike, up to
    /// this many seconds (`0` disables). Captures the moment itself (the loud play) rather
    /// than only the streamer's callout/reaction around it. Applied to every kind.
    pub preroll_max_secs: f64,
    /// Extend the clip end forward through the trailing audio-energy spike, up to this many
    /// seconds (`0` disables). Holds a fight/dive to its resolution instead of cutting the
    /// instant the streamer stops narrating. Applied to every kind.
    pub postroll_max_secs: f64,
}

impl Default for DetectParams {
    fn default() -> Self {
        Self {
            min_score: 0.0,
            merge_gap_secs: 8.0,
            game_filter: None,
            kinds: None,
            energy_weight: 2.0,
            neighbor_window_secs: 30.0,
            preroll_max_secs: 12.0,
            postroll_max_secs: 12.0,
        }
    }
}

/// Score → cluster (per kind) → attach neighbors → rank the transcript into
/// candidate windows across all requested kinds.
pub fn detect(
    segments: &[TranscriptSegment],
    energy: &EnergyTrack,
    chapters: &[Chapter],
    params: &DetectParams,
) -> Vec<Candidate> {
    let energy_peak = energy.rms.iter().copied().fold(0.0_f32, f32::max);

    // One pass over the transcript: score every segment for every kind. The four
    // reaction kinds (exciting/fails/funny/frustration) also fuse the audio energy
    // spike, routed by valence: the spike amplifies whichever reaction kind has the
    // strongest lexical signal, and a wordless spike (no reaction words at all)
    // defaults to `exciting`.
    let mut edu: ScoredSegments = Vec::with_capacity(segments.len());
    let mut info: ScoredSegments = Vec::with_capacity(segments.len());
    let mut exc: ScoredSegments = Vec::with_capacity(segments.len());
    let mut fail: ScoredSegments = Vec::with_capacity(segments.len());
    let mut funny: ScoredSegments = Vec::with_capacity(segments.len());
    let mut frust: ScoredSegments = Vec::with_capacity(segments.len());

    for seg in segments {
        edu.push(score_segment(&seg.text, EDUCATIONAL));
        info.push(score_segment(&seg.text, INFORMATIVE));

        let mut se = score_segment(&seg.text, EXCITING);
        let mut sf = score_segment(&seg.text, FAILS);
        let mut sfun = score_segment(&seg.text, FUNNY);
        let mut sfr = score_segment(&seg.text, FRUSTRATION);

        if params.energy_weight > 0.0 {
            let e = segment_energy(energy, energy_peak, seg.start, seg.end);
            if e > 0.0 {
                let boost = params.energy_weight * e;
                // Route the spike to the strongest-lexical reaction kind. Index 0
                // (exciting) is the default, so ties and a wordless spike fall to it.
                let scores = [se.0, sf.0, sfun.0, sfr.0];
                let mut best = 0usize;
                for (i, &v) in scores.iter().enumerate().skip(1) {
                    if v > scores[best] {
                        best = i;
                    }
                }
                let target = match best {
                    1 => &mut sf,
                    2 => &mut sfun,
                    3 => &mut sfr,
                    _ => &mut se,
                };
                target.0 += boost;
                target.1.push(ENERGY_CUE.to_string());
            }
        }
        exc.push(se);
        fail.push(sf);
        funny.push(sfun);
        frust.push(sfr);
    }

    let per_kind: [(EventKind, ScoredSegments); 6] = [
        (EventKind::Educational, edu),
        (EventKind::Informative, info),
        (EventKind::Exciting, exc),
        (EventKind::Fails, fail),
        (EventKind::Funny, funny),
        (EventKind::Frustration, frust),
    ];

    let mut candidates: Vec<Candidate> = Vec::new();
    for (kind, scored) in &per_kind {
        if let Some(kinds) = &params.kinds {
            if !kinds.contains(kind) {
                continue;
            }
        }
        candidates.extend(build_candidates(
            *kind,
            segments,
            scored,
            energy,
            energy_peak,
            chapters,
            params,
        ));
    }

    populate_neighbors(&mut candidates, params.neighbor_window_secs);

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

/// Cluster this kind's hot segments into candidate windows and shape each into a
/// `Candidate` (game-labeled, snippet-truncated, score-floored).
fn build_candidates(
    kind: EventKind,
    segments: &[TranscriptSegment],
    scored: &[(f64, Vec<String>)],
    energy: &EnergyTrack,
    energy_peak: f32,
    chapters: &[Chapter],
    params: &DetectParams,
) -> Vec<Candidate> {
    let hot: Vec<usize> = scored
        .iter()
        .enumerate()
        .filter(|(_, (raw, _))| *raw > 0.0)
        .map(|(i, _)| i)
        .collect();
    if hot.is_empty() {
        return Vec::new();
    }

    // Cluster hot segments whose gap is within merge_gap_secs.
    let mut clusters: Vec<Vec<usize>> = vec![vec![hot[0]]];
    for &idx in &hot[1..] {
        let prev = *clusters.last().and_then(|c| c.last()).unwrap_or(&idx);
        let gap = segments[idx].start - segments[prev].end;
        if gap <= params.merge_gap_secs {
            clusters.last_mut().expect("at least one cluster").push(idx);
        } else {
            clusters.push(vec![idx]);
        }
    }

    let game_filter = params.game_filter.as_ref().map(|g| g.to_lowercase());

    let mut out: Vec<Candidate> = Vec::new();
    for cluster in clusters {
        let first = cluster[0];
        let last = *cluster.last().expect("non-empty cluster");
        let mut start = segments[first].start;
        let mut end = segments[last].end;

        // Clip windows are anchored on speech timestamps (Whisper runs only where he's
        // talking), but a gameplay recap needs the PLAY in frame, not just the narration.
        // The play lands as an audio-energy spike; the words land before it (a callout) or
        // after it (a reaction). Reach the start back to the onset of the preceding spike
        // and the end forward through the trailing spike, so the whole loud moment (the
        // fight, the dive) is captured. Applied to EVERY kind — a calm "kill credit" or a
        // teaching callout is narrated over a play too — and energy-gated + bounded, so a
        // quiet lead-in/tail or a caption-only transcript is left untouched.
        if params.preroll_max_secs > 0.0 {
            start = preroll_onset(energy, energy_peak, start, params.preroll_max_secs);
        }
        if params.postroll_max_secs > 0.0 {
            end = postroll_offset(energy, energy_peak, end, params.postroll_max_secs);
        }

        let raw_sum: f64 = cluster.iter().map(|&k| scored[k].0).sum();
        let score = raw_sum / (raw_sum + 3.0);
        if score < params.min_score {
            continue;
        }

        let game = chapter_for(chapters, f64::midpoint(start, end));
        if let Some(filter) = &game_filter {
            let keep = game
                .as_deref()
                .is_some_and(|g| g.to_lowercase().contains(filter));
            if !keep {
                continue;
            }
        }

        let snippet_full = (first..=last)
            .map(|k| segments[k].text.trim())
            .collect::<Vec<_>>()
            .join(" ");
        let snippet = truncate_snippet(&snippet_full, 240);

        let mut cues: Vec<String> = cluster.iter().flat_map(|&k| scored[k].1.clone()).collect();
        cues.sort();
        cues.dedup();

        out.push(Candidate {
            id: candidate_id(kind, start),
            kind,
            start,
            end,
            score,
            snippet,
            cues,
            game,
            neighbors: Vec::new(),
        });
    }
    out
}

/// Deterministic candidate id: `<kind>-<start_centisecond>`. Stable across
/// re-detects of the same window, so a curated beat round-trips to the assembler.
fn candidate_id(kind: EventKind, start: f64) -> String {
    format!("{}-{}", kind.key(), (start * 100.0).round() as i64)
}

/// Attach, to each candidate, the candidates of *other* kinds whose window falls
/// within `window` seconds — nearest first. This is the "exciting/fail part right
/// around the teaching" surfaced for weaving.
fn populate_neighbors(candidates: &mut [Candidate], window: f64) {
    if window <= 0.0 || candidates.len() < 2 {
        return;
    }
    let snap: Vec<(String, EventKind, f64, f64)> = candidates
        .iter()
        .map(|c| (c.id.clone(), c.kind, c.start, c.end))
        .collect();

    for (i, cand) in candidates.iter_mut().enumerate() {
        let mut nbrs: Vec<Neighbor> = Vec::new();
        for (j, (id, kind, start, end)) in snap.iter().enumerate() {
            if i == j || *kind == cand.kind {
                continue;
            }
            let gap = window_gap(cand.start, cand.end, *start, *end);
            if gap <= window {
                nbrs.push(Neighbor {
                    id: id.clone(),
                    kind: *kind,
                    start: *start,
                    end: *end,
                    gap_secs: gap,
                });
            }
        }
        nbrs.sort_by(|a, b| {
            a.gap_secs
                .partial_cmp(&b.gap_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        cand.neighbors = nbrs;
    }
}

/// An externally-supplied beat to merge into the detector's candidates before
/// scene grouping. The lexicons only hear *reactions*; a caller that has read the
/// transcript another way (e.g. the guide mining commentary against the operator's
/// primer) hands what it found here as plain timestamped spans. Generic mechanism:
/// the gateway never knows *why* a span matters — the caller's `label` carries that.
#[derive(Clone, Debug, Deserialize)]
pub struct ExtraBeat {
    pub start: f64,
    pub end: f64,
    /// Reaction kind to file the beat under (default `informative`).
    #[serde(default)]
    pub kind: Option<EventKind>,
    /// Normalized 0..1 score (default 0.65 — surfaced, but below a strong reaction hit).
    #[serde(default)]
    pub score: Option<f64>,
    /// Short caller-side "why this matters" note; lands in `cues` as `"context: <label>"`.
    #[serde(default)]
    pub label: Option<String>,
    /// Verbatim quote for the beat; when empty it is rebuilt from the transcript span.
    #[serde(default)]
    pub snippet: Option<String>,
}

/// The cue marking a merged [`ExtraBeat`] (readers can tell caller-mined beats apart).
pub const EXTRA_BEAT_CUE: &str = "context";

/// Default score for an [`ExtraBeat`] that doesn't bring its own.
const EXTRA_BEAT_SCORE: f64 = 0.65;

/// Merge caller-supplied beats into the detected candidates: build a [`Candidate`]
/// per span (snippet from the transcript when not provided, game from the chapters),
/// skip spans whose midpoint already sits inside a same-kind candidate (same moment,
/// and the detected one carries richer cues), then recompute neighbors and re-rank.
pub fn merge_extra_beats(
    candidates: &mut Vec<Candidate>,
    extra: &[ExtraBeat],
    segments: &[TranscriptSegment],
    chapters: &[Chapter],
    neighbor_window_secs: f64,
) {
    if extra.is_empty() {
        return;
    }
    for xb in extra {
        if xb.start < 0.0 || xb.end <= xb.start {
            continue;
        }
        let kind = xb.kind.unwrap_or(EventKind::Informative);
        let mid = f64::midpoint(xb.start, xb.end);
        if candidates
            .iter()
            .any(|c| c.kind == kind && mid >= c.start && mid <= c.end)
        {
            continue;
        }
        let snippet = match xb.snippet.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() => truncate_snippet(s, 240),
            _ => {
                let joined = segments
                    .iter()
                    .filter(|s| s.end > xb.start && s.start < xb.end)
                    .map(|s| s.text.trim())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                truncate_snippet(&joined, 240)
            }
        };
        let cue = match xb.label.as_deref().map(str::trim) {
            Some(l) if !l.is_empty() => format!("{EXTRA_BEAT_CUE}: {l}"),
            _ => EXTRA_BEAT_CUE.to_string(),
        };
        candidates.push(Candidate {
            // `x` keeps the id disjoint from the detector's `<kind>-<centis>` space.
            id: format!("{}-x{}", kind.key(), (xb.start * 100.0).round() as i64),
            kind,
            start: xb.start,
            end: xb.end,
            score: xb.score.unwrap_or(EXTRA_BEAT_SCORE).clamp(0.0, 1.0),
            snippet,
            cues: vec![cue],
            game: chapter_for(chapters, mid),
            neighbors: Vec::new(),
        });
    }
    populate_neighbors(candidates, neighbor_window_secs);
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Gap between two `[start,end]` windows; `0` when they touch or overlap.
fn window_gap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    if a1 < b0 {
        b0 - a1
    } else if b1 < a0 {
        a0 - b1
    } else {
        0.0
    }
}

/// Peak-relative energy over `[start,end]` seconds, floored to `ENERGY_FLOOR`.
/// Returns 0 when there's no energy track or the segment isn't a spike.
fn segment_energy(energy: &EnergyTrack, peak: f32, start: f64, end: f64) -> f64 {
    if energy.rms.is_empty() || energy.hz <= 0.0 || peak <= 0.0 {
        return 0.0;
    }
    let i0 = (start * energy.hz).floor().max(0.0) as usize;
    if i0 >= energy.rms.len() {
        return 0.0;
    }
    let i1 = ((end * energy.hz).ceil() as usize)
        .max(i0 + 1)
        .min(energy.rms.len());
    let seg_peak = energy.rms[i0..i1].iter().copied().fold(0.0_f32, f32::max);
    let norm = f64::from((seg_peak / peak).clamp(0.0, 1.0));
    if norm >= ENERGY_FLOOR {
        norm
    } else {
        0.0
    }
}

/// Onset time of the nearest audio-energy spike at or before `reaction_start`, bounded
/// to `max_secs` of look-back. Finds the closest spike bucket (peak-relative RMS ≥
/// [`ENERGY_FLOOR`]) not after the reaction, then walks back through its contiguous run
/// to the run's start and returns that time — the moment the loud play began. Quiet
/// buckets *between* the play and the reaction are skipped (the streamer often goes
/// silent, then talks). Returns `reaction_start` unchanged when there's no spike in the
/// window or no energy track, so quiet lead-ins and caption-only transcripts are safe.
fn preroll_onset(energy: &EnergyTrack, peak: f32, reaction_start: f64, max_secs: f64) -> f64 {
    if energy.rms.is_empty() || energy.hz <= 0.0 || peak <= 0.0 || max_secs <= 0.0 {
        return reaction_start;
    }
    let norm_at = |i: usize| f64::from((energy.rms[i] / peak).clamp(0.0, 1.0));
    let last = energy.rms.len() as isize - 1;
    let start_idx = ((reaction_start * energy.hz).floor() as isize).min(last);
    let floor_idx = (((reaction_start - max_secs) * energy.hz).floor() as isize).max(0);
    if start_idx < floor_idx {
        return reaction_start;
    }

    // Nearest spike bucket at or before the reaction, within the budget.
    let mut i = start_idx;
    while i >= floor_idx && norm_at(i as usize) < ENERGY_FLOOR {
        i -= 1;
    }
    if i < floor_idx {
        return reaction_start; // no spike in the look-back window
    }
    // Walk back through the contiguous spike to its onset.
    let mut onset = i;
    while onset > floor_idx && norm_at((onset - 1) as usize) >= ENERGY_FLOOR {
        onset -= 1;
    }
    (onset as f64 / energy.hz).min(reaction_start)
}

/// End time of the audio-energy spike at or after `reaction_end`, bounded to `max_secs` of
/// look-ahead. Mirror of [`preroll_onset`]: finds the nearest spike bucket (peak-relative
/// RMS ≥ [`ENERGY_FLOOR`]) not before the end, then walks forward through its contiguous run
/// to the run's end and returns that time — the moment the loud play finished. Quiet buckets
/// *between* the narration and the trailing action are skipped (the streamer calls a play,
/// then it resolves). Returns `reaction_end` unchanged when there's no spike in the window or
/// no energy track, so quiet tails and caption-only transcripts are safe.
fn postroll_offset(energy: &EnergyTrack, peak: f32, reaction_end: f64, max_secs: f64) -> f64 {
    if energy.rms.is_empty() || energy.hz <= 0.0 || peak <= 0.0 || max_secs <= 0.0 {
        return reaction_end;
    }
    let norm_at = |i: usize| f64::from((energy.rms[i] / peak).clamp(0.0, 1.0));
    let last = energy.rms.len() as isize - 1;
    let end_idx = ((reaction_end * energy.hz).ceil() as isize).max(0);
    if end_idx > last {
        return reaction_end;
    }
    let ceil_idx = (((reaction_end + max_secs) * energy.hz).ceil() as isize).min(last);

    // Nearest spike bucket at or after the end, within the budget.
    let mut i = end_idx;
    while i <= ceil_idx && norm_at(i as usize) < ENERGY_FLOOR {
        i += 1;
    }
    if i > ceil_idx {
        return reaction_end; // no spike in the look-ahead window
    }
    // Walk forward through the contiguous spike to its offset.
    let mut offset = i;
    while offset < ceil_idx && norm_at((offset + 1) as usize) >= ENERGY_FLOOR {
        offset += 1;
    }
    // Include the final loud bucket's span (+1 bucket); never earlier than the narration end.
    (((offset + 1) as f64) / energy.hz).max(reaction_end)
}

/// Sum the weights of every cue in `lexicon` matched in `text`, returning
/// `(raw, matched)`.
fn score_segment(text: &str, lexicon: &[Cue]) -> (f64, Vec<String>) {
    let lower = text.to_lowercase();
    let padded = format!(" {lower} ");
    let mut raw = 0.0;
    let mut matched = Vec::new();
    for cue in lexicon {
        let hit = if cue.phrase {
            lower.contains(cue.pat)
        } else {
            padded.contains(&format!(" {} ", cue.pat))
        };
        if hit {
            raw += cue.weight;
            matched.push(cue.pat.to_string());
        }
    }
    (raw, matched)
}

/// The title of the chapter covering time `t` (open-ended chapters have `end <= 0`).
fn chapter_for(chapters: &[Chapter], t: f64) -> Option<String> {
    chapters
        .iter()
        .find(|c| t >= c.start && (c.end <= 0.0 || t < c.end))
        .map(|c| c.title.clone())
        .filter(|title| !title.is_empty())
}

fn truncate_snippet(s: &str, max_chars: usize) -> String {
    let normalized = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut out: String = normalized.chars().take(max_chars).collect();
    if let Some(idx) = out.rfind(' ') {
        out.truncate(idx);
    }
    out.push('…');
    out
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

    /// A flat energy track at `hz = 1` where second `t` has RMS `rms[t]`.
    fn energy(rms: &[f32]) -> EnergyTrack {
        EnergyTrack {
            hz: 1.0,
            rms: rms.to_vec(),
        }
    }

    #[test]
    fn candidates_carry_stable_ids() {
        let segments = vec![seg(12.0, 15.0, "the trick is to dodge left")];
        let a = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        let b = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        assert_eq!(a.len(), 1);
        assert!(!a[0].id.is_empty());
        assert!(a[0].id.starts_with("educational-"));
        // Deterministic: same input → same id, so a curated beat round-trips.
        assert_eq!(a[0].id, b[0].id);
    }

    #[test]
    fn reaction_preroll_reaches_back_to_energy_spike() {
        // The fails reaction fires on the words at t=10, but the loud play (energy
        // spike) was at t=5..7. The clip should reach back to the spike onset so the
        // moment is in frame, not just the "oh no".
        let segments = vec![seg(10.0, 12.0, "oh no i died")];
        let rms = [
            50.0, 50.0, 50.0, 50.0, 50.0, 1000.0, 1000.0, 1000.0, 50.0, 50.0, 50.0, 50.0,
        ];
        let cands = detect(&segments, &energy(&rms), &[], &DetectParams::default());
        let fail = cands
            .iter()
            .find(|c| c.kind == EventKind::Fails)
            .expect("a fails candidate");
        assert_eq!(
            fail.start, 5.0,
            "clip start reached back to the spike onset"
        );
    }

    #[test]
    fn reaction_without_spike_is_unchanged() {
        // No energy track (or no preceding spike) → the window is untouched.
        let segments = vec![seg(10.0, 12.0, "oh no i died")];
        let cands = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        let fail = cands
            .iter()
            .find(|c| c.kind == EventKind::Fails)
            .expect("a fails candidate");
        assert_eq!(fail.start, 10.0, "no spike -> window unchanged");
    }

    #[test]
    fn neighbor_id_joins_back_to_a_candidate() {
        let segments = vec![
            seg(0.0, 3.0, "the key is to bait the dodge"),
            seg(8.0, 10.0, "let's go that was clutch"),
        ];
        let cands = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        let ids: std::collections::HashSet<&str> = cands.iter().map(|c| c.id.as_str()).collect();
        for c in &cands {
            for n in &c.neighbors {
                assert!(
                    ids.contains(n.id.as_str()),
                    "neighbor id must join a candidate"
                );
            }
        }
    }

    #[test]
    fn buckets_by_kind() {
        let segments = vec![
            seg(0.0, 3.0, "the trick is to always dodge left"),
            seg(10.0, 13.0, "this is originally from the old patch"),
        ];
        let cands = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        // One educational, one informative.
        assert!(cands.iter().any(|c| c.kind == EventKind::Educational));
        assert!(cands.iter().any(|c| c.kind == EventKind::Informative));
        assert!(!cands.iter().any(|c| c.kind == EventKind::Exciting));
    }

    #[test]
    fn laughter_buckets_as_funny() {
        // Whisper transcribes laughter as "haha"/"[laughter]" → funny, not exciting.
        let segments = vec![seg(0.0, 2.0, "hahaha oh man that was so funny")];
        let cands = detect(
            &segments,
            &energy(&[1000.0, 1000.0]),
            &[],
            &DetectParams::default(),
        );
        assert!(cands.iter().any(|c| c.kind == EventKind::Funny));
        // The energy spike is routed to funny (strongest reaction lexical), not exciting.
        assert!(!cands.iter().any(|c| c.kind == EventKind::Exciting));
        let funny = cands.iter().find(|c| c.kind == EventKind::Funny).unwrap();
        assert!(funny.cues.iter().any(|c| c == ENERGY_CUE));
    }

    #[test]
    fn frustration_is_its_own_kind() {
        let segments = vec![seg(
            0.0,
            3.0,
            "why does this keep happening it's so frustrating",
        )];
        let cands = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        assert!(cands.iter().any(|c| c.kind == EventKind::Frustration));
        // A discrete death is still a fail, not frustration.
        let died = detect(
            &[seg(0.0, 3.0, "oh no i died")],
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        assert!(died.iter().any(|c| c.kind == EventKind::Fails));
        assert!(!died.iter().any(|c| c.kind == EventKind::Frustration));
    }

    #[test]
    fn wordless_energy_spike_defaults_to_exciting() {
        // A segment with no cue words but a loud energy spike over its second.
        let segments = vec![seg(0.0, 1.0, "aaaaah")];
        let cands = detect(&segments, &energy(&[1000.0]), &[], &DetectParams::default());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].kind, EventKind::Exciting);
        assert!(cands[0].cues.iter().any(|c| c == ENERGY_CUE));
    }

    #[test]
    fn energy_spike_over_fail_cues_buckets_as_fails() {
        // Same loud spike, but the words are negative-valence → fails, not exciting.
        let segments = vec![seg(0.0, 1.0, "oh no i died so bad")];
        let cands = detect(&segments, &energy(&[1000.0]), &[], &DetectParams::default());
        assert!(cands.iter().any(|c| c.kind == EventKind::Fails));
        assert!(!cands.iter().any(|c| c.kind == EventKind::Exciting));
        let fail = cands.iter().find(|c| c.kind == EventKind::Fails).unwrap();
        assert!(fail.cues.iter().any(|c| c == ENERGY_CUE));
    }

    #[test]
    fn quiet_energy_does_not_create_a_reaction_candidate() {
        // Energy well below the floor relative to the track peak → no candidate.
        let segments = vec![seg(0.0, 1.0, "just walking around")];
        let cands = detect(
            &segments,
            &energy(&[100.0, 1000.0]),
            &[],
            &DetectParams::default(),
        );
        assert!(cands.is_empty());
    }

    #[test]
    fn teaching_and_hype_are_mutual_neighbors() {
        let segments = vec![
            seg(0.0, 3.0, "the key is to bait the dodge"),
            seg(8.0, 10.0, "let's go that was clutch"),
        ];
        let cands = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        let teach = cands
            .iter()
            .find(|c| c.kind == EventKind::Educational)
            .expect("educational candidate");
        assert!(teach
            .neighbors
            .iter()
            .any(|n| n.kind == EventKind::Exciting));
        let hype = cands
            .iter()
            .find(|c| c.kind == EventKind::Exciting)
            .expect("exciting candidate");
        assert!(hype
            .neighbors
            .iter()
            .any(|n| n.kind == EventKind::Educational));
    }

    #[test]
    fn kinds_filter_restricts_output() {
        let segments = vec![
            seg(0.0, 3.0, "the trick is to dodge"),
            seg(8.0, 10.0, "let's go clutch"),
        ];
        let params = DetectParams {
            kinds: Some(vec![EventKind::Exciting]),
            ..DetectParams::default()
        };
        let cands = detect(&segments, &EnergyTrack::default(), &[], &params);
        assert!(!cands.is_empty());
        assert!(cands.iter().all(|c| c.kind == EventKind::Exciting));
    }

    #[test]
    fn preroll_applies_to_nonreaction_kinds() {
        // A calm teaching callout at t=10, narrated over a play whose energy spike ran
        // t=5..7. Educational is NOT a reaction kind, but the clip must still reach back to
        // the play — the fix is preroll on EVERY kind, not just reactions (a "kill credit"
        // read like this is why complaints #1/#2 showed the talk and not the play).
        let segments = vec![seg(10.0, 12.0, "the trick is to always dodge left")];
        let rms = [
            50.0, 50.0, 50.0, 50.0, 50.0, 1000.0, 1000.0, 1000.0, 50.0, 50.0, 50.0, 50.0,
        ];
        let cands = detect(&segments, &energy(&rms), &[], &DetectParams::default());
        let edu = cands
            .iter()
            .find(|c| c.kind == EventKind::Educational)
            .expect("an educational candidate");
        assert_eq!(
            edu.start, 5.0,
            "educational clip reached back to the play's spike onset"
        );
    }

    #[test]
    fn postroll_extends_end_through_trailing_action() {
        // He calls a dive at t=2..4, then the fight itself (energy spike) runs t=5..9 AFTER
        // he stops talking. The clip end must reach forward through the loud action, not
        // stop at t=4 (complaints #3/#4: fights cut out the instant he goes quiet).
        let segments = vec![seg(2.0, 4.0, "the trick is to tower dive here")];
        let rms = [
            50.0, 50.0, 50.0, 50.0, 50.0, 1000.0, 1000.0, 1000.0, 1000.0, 50.0, 50.0,
        ];
        let cands = detect(&segments, &energy(&rms), &[], &DetectParams::default());
        let edu = cands
            .iter()
            .find(|c| c.kind == EventKind::Educational)
            .expect("an educational candidate");
        assert!(
            edu.end >= 9.0,
            "clip end reached forward through the trailing action (got {})",
            edu.end
        );
    }

    #[test]
    fn postroll_offset_edge_cases() {
        // hz = 1: spike buckets are 1.0 (>= ENERGY_FLOOR 0.5), quiet buckets 0.1.
        let track = energy(&[0.1, 0.1, 1.0, 1.0, 0.1]);
        // From a narration end at t=1 with a quiet gap, reach forward past the gap to the
        // end of the loud run [2,4) -> 4.0.
        assert_eq!(
            postroll_offset(&track, 1.0, 1.0, 5.0),
            4.0,
            "skips the quiet gap and holds to the end of the loud run"
        );
        // Bounded: the spike at t=2 is beyond a 1s look-ahead from t=0 -> unchanged.
        assert_eq!(
            postroll_offset(&track, 1.0, 0.0, 1.0),
            0.0,
            "spike beyond the budget -> end unchanged"
        );
        // No loud run ahead -> unchanged.
        let quiet = energy(&[1.0, 0.1, 0.1, 0.1]);
        assert_eq!(
            postroll_offset(&quiet, 1.0, 1.0, 5.0),
            1.0,
            "no trailing spike -> end unchanged"
        );
        // No energy track -> unchanged (caption-only transcripts are safe).
        assert_eq!(postroll_offset(&EnergyTrack::default(), 0.0, 3.0, 5.0), 3.0);
    }

    #[test]
    fn extra_beats_merge_with_snippet_and_neighbors() {
        let segments = vec![
            seg(10.0, 14.0, "in the red corner we have the champion"),
            seg(14.5, 18.0, "facing the rookie with a perfect record"),
            seg(40.0, 43.0, "the trick is to dodge left"),
        ];
        let mut cands = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        assert_eq!(cands.len(), 1, "only the teaching line scores lexically");
        let extra = vec![ExtraBeat {
            start: 10.0,
            end: 18.0,
            kind: None,
            score: None,
            label: Some("fighter introductions".to_string()),
            snippet: None,
        }];
        merge_extra_beats(&mut cands, &extra, &segments, &[], 60.0);
        assert_eq!(cands.len(), 2);
        let mined = cands
            .iter()
            .find(|c| c.kind == EventKind::Informative)
            .expect("mined beat filed as informative");
        assert_eq!(mined.id, "informative-x1000");
        assert!(
            mined.snippet.contains("red corner"),
            "snippet rebuilt from the span"
        );
        assert_eq!(
            mined.cues,
            vec!["context: fighter introductions".to_string()]
        );
        // Neighbors recomputed across the merged set (mined ↔ educational, 60s window).
        assert!(mined
            .neighbors
            .iter()
            .any(|n| n.kind == EventKind::Educational));
        let edu = cands
            .iter()
            .find(|c| c.kind == EventKind::Educational)
            .expect("detected beat kept");
        assert!(edu.neighbors.iter().any(|n| n.id == "informative-x1000"));
    }

    #[test]
    fn extra_beats_skip_invalid_and_same_kind_overlap() {
        let segments = vec![seg(
            10.0,
            14.0,
            "turns out this dagger scales with dexterity",
        )];
        let mut cands = detect(
            &segments,
            &EnergyTrack::default(),
            &[],
            &DetectParams::default(),
        );
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].kind, EventKind::Informative);
        let (start, end) = (cands[0].start, cands[0].end);
        let extra = vec![
            // Midpoint inside the detected informative window → same moment, skipped.
            ExtraBeat {
                start,
                end,
                kind: None,
                score: None,
                label: None,
                snippet: None,
            },
            // Degenerate span → skipped.
            ExtraBeat {
                start: 50.0,
                end: 50.0,
                kind: None,
                score: None,
                label: None,
                snippet: None,
            },
        ];
        merge_extra_beats(&mut cands, &extra, &segments, &[], 30.0);
        assert_eq!(cands.len(), 1, "nothing merged");
    }
}
