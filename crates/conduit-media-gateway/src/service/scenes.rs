//! Deterministic scene segmentation (no LLM).
//!
//! The detector ([`super::detect`]) produces cheap, over-eager *beats* — reaction
//! hits (funny / exciting / fails / frustration / educational / informative). Beats
//! are no longer the operator's curation unit; **scenes** are. A scene groups
//! one-or-more nearby beats into one coherent moment and is FRAMED by a core
//! game-state situation ([`CoreSituation`]): what the scene IS (a fight, a win, a
//! menu stretch), as opposed to a beat's reaction kind (why a moment got flagged).
//! Reaction beats are the CONTENTS that nest inside a frame — a self-inflicted
//! `fails` beat inside a `Triumph` scene (biff yourself while the poison finishes
//! the enemy) is exactly the juxtaposition that makes a great scene.
//!
//! Segmentation is beat-anchored and fully deterministic:
//!
//! 1. Sort beats by time; cluster beats within `merge_gap_secs` into a scene group,
//!    never crossing a chapter/game boundary. A lone beat is a one-beat scene.
//! 2. Frame each group from the situation signals over its window: a lexical scan
//!    of the window transcript against per-frame cue lexicons, fused with audio
//!    energy for `Combat` (loud + combat vocab). Frame precedence breaks ties
//!    (outcome markers > ongoing action > Setup). `Setup` only frames a SUSTAINED,
//!    quiet window — a transient menu flip mid-fight stays inside the Combat scene
//!    because the fight's window is short/loud and Combat outscores a gated Setup.
//! 3. Compute per-scene situational tags ("what else is going on"), the member
//!    beats' reaction kinds, a fuller transcript excerpt, the game/chapter, and a
//!    pitch priority = frame baseline + boost from contained beats (so a menu
//!    stretch packed with funny banter surfaces, and dead-air Setup sinks).
//!
//! The LLM story-weaver downstream only NAMES / cards / orders these scenes; the
//! separation into scenes is done here, deterministically.

use serde::{Deserialize, Serialize};

use crate::service::detect::{Candidate, Chapter, EventKind};
use crate::service::transcribe::{EnergyTrack, TranscriptSegment};

/// Peak-relative energy at or above this fraction is a "spike" (an action moment).
/// Mirrors `detect::ENERGY_FLOOR`; kept local so the two detectors tune independently.
const SPIKE_FLOOR: f64 = 0.5;

/// A core game-state situation that FRAMES a scene (what the scene IS). Distinct
/// from a beat's reaction [`EventKind`] (why a moment was flagged) — reaction beats
/// are contents that nest inside a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreSituation {
    /// A fight / action run (loud + combat vocab). Absorbs a mid-fight menu dip.
    Combat,
    /// Winning / hitting a goal (victory, clutch, level-up, achievement).
    Triumph,
    /// Dying / losing / a wipe (an OUTCOME frame; the blunder that caused it is a
    /// `fails` content beat, which can just as well sit inside a Triumph).
    Defeat,
    /// Finding / reacting to something new (secrets, lore, first-times).
    Discovery,
    /// Talking to viewers / banter between the action. The low-signal fallback.
    Downtime,
    /// Menus / loadouts / character-creation / shopping / settings. Low-priority,
    /// opt-in: only frames a sustained, quiet stretch; always pitched to the
    /// operator with a legible card, boosted when packed with content beats.
    Setup,
}

impl CoreSituation {
    /// All frames, in the fixed order used for stable iteration.
    pub const ALL: [CoreSituation; 6] = [
        CoreSituation::Combat,
        CoreSituation::Triumph,
        CoreSituation::Defeat,
        CoreSituation::Discovery,
        CoreSituation::Downtime,
        CoreSituation::Setup,
    ];

    /// The snake_case wire key (matches the serde rename).
    pub const fn key(self) -> &'static str {
        match self {
            Self::Combat => "combat",
            Self::Triumph => "triumph",
            Self::Defeat => "defeat",
            Self::Discovery => "discovery",
            Self::Downtime => "downtime",
            Self::Setup => "setup",
        }
    }

    /// Baseline pitch priority before the contained-beat boost. Action frames start
    /// high, Discovery/Downtime medium, Setup low (opt-in).
    fn baseline_priority(self) -> f64 {
        match self {
            Self::Combat | Self::Triumph => 1.0,
            Self::Defeat => 0.9,
            Self::Discovery => 0.6,
            Self::Downtime => 0.4,
            Self::Setup => 0.15,
        }
    }

    /// Tie-break rank when two frames score equally (lower wins): outcome markers
    /// beat ongoing action beat exploration beat Setup beat the Downtime fallback.
    fn precedence(self) -> u8 {
        match self {
            Self::Triumph => 0,
            Self::Defeat => 1,
            Self::Combat => 2,
            Self::Discovery => 3,
            Self::Setup => 4,
            Self::Downtime => 5,
        }
    }

    /// The per-frame cue lexicon.
    fn lexicon(self) -> &'static [SitCue] {
        match self {
            Self::Combat => COMBAT,
            Self::Triumph => TRIUMPH,
            Self::Defeat => DEFEAT,
            Self::Discovery => DISCOVERY,
            Self::Downtime => DOWNTIME,
            Self::Setup => SETUP,
        }
    }

    /// Fallback frame for a scene whose window carries no situation vocab at all,
    /// mapped from its strongest member beat's reaction kind.
    fn from_kind(kind: EventKind) -> Self {
        match kind {
            // Hype / struggling-with-the-game read as action.
            EventKind::Exciting | EventKind::Frustration => Self::Combat,
            EventKind::Fails => Self::Defeat,
            // Laughter / teaching / lore with no situation vocab = him talking.
            EventKind::Funny | EventKind::Educational | EventKind::Informative => Self::Downtime,
        }
    }
}

/// One situation cue. `phrase` cues match as a lowercase substring; single-word
/// cues match on word boundaries. Mirrors `detect::Cue`.
struct SitCue {
    pat: &'static str,
    weight: f64,
    phrase: bool,
}

const COMBAT: &[SitCue] = &[
    SitCue {
        pat: "boss",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "reload",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "reloading",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "shield",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "he's low",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "they're low",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "low health",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "behind you",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "on your left",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "on your right",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "push",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "pushing",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "cover",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "get him",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "get them",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "kill",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "headshot",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "heal",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "ultimate",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "enemy",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "enemies",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "incoming",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "engage",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "fight",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "attack",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "dodge",
        weight: 1.0,
        phrase: false,
    },
];

const TRIUMPH: &[SitCue] = &[
    SitCue {
        pat: "we won",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "victory",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "we did it",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "clutch",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "that's the win",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "got him",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "got them",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "let's go",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "ace",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "first try",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "level up",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "leveled up",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "unlocked",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "achievement",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "finally beat",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "cleared it",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "new record",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "easy",
        weight: 1.0,
        phrase: false,
    },
];

const DEFEAT: &[SitCue] = &[
    SitCue {
        pat: "i died",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "we died",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "we lost",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "defeat",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "he got me",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "they got me",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "wasted",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "respawn",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "wiped",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "we wipe",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "game over",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "back to the checkpoint",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "lost that one",
        weight: 2.0,
        phrase: true,
    },
];

const DISCOVERY: &[SitCue] = &[
    SitCue {
        pat: "what is that",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "what's this",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "no way",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "found it",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "found a",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "secret",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "first time",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "look at this",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "check this out",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "over here",
        weight: 1.0,
        phrase: true,
    },
    SitCue {
        pat: "never seen",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "new area",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "hidden",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "easter egg",
        weight: 3.0,
        phrase: true,
    },
];

const DOWNTIME: &[SitCue] = &[
    SitCue {
        pat: "chat",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "thanks for the",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "thank you for the",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "welcome back",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "subscribe",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "subscriber",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "donation",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "you guys",
        weight: 1.0,
        phrase: true,
    },
    SitCue {
        pat: "how's everyone",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "good morning",
        weight: 1.0,
        phrase: true,
    },
    SitCue {
        pat: "by the way",
        weight: 1.0,
        phrase: true,
    },
    SitCue {
        pat: "real quick",
        weight: 1.0,
        phrase: true,
    },
];

const SETUP: &[SitCue] = &[
    SitCue {
        pat: "loadout",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "load out",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "inventory",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "equip",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "the menu",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "settings",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "sensitivity",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "character creation",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "skill tree",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "talent tree",
        weight: 3.0,
        phrase: true,
    },
    SitCue {
        pat: "respec",
        weight: 3.0,
        phrase: false,
    },
    SitCue {
        pat: "the build",
        weight: 1.0,
        phrase: true,
    },
    SitCue {
        pat: "options",
        weight: 1.0,
        phrase: false,
    },
    SitCue {
        pat: "shop",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "the store",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "customize",
        weight: 2.0,
        phrase: false,
    },
    SitCue {
        pat: "gear up",
        weight: 2.0,
        phrase: true,
    },
    SitCue {
        pat: "stash",
        weight: 2.0,
        phrase: false,
    },
];

/// A detected scene (the operator's curation unit).
#[derive(Clone, Debug, Serialize)]
pub struct Scene {
    /// Stable id (`scene-<start_centisecond>`), so a curated scene round-trips.
    pub id: String,
    /// The core game-state situation this scene is framed by.
    pub frame: CoreSituation,
    pub start: f64,
    pub end: f64,
    /// Pitch priority: frame baseline + contained-beat boost. Higher = surface it.
    pub priority: f64,
    /// Situations present in the window ("what else is going on"), frame first,
    /// then others by signal strength — the `+ battle` / `+ victory` tags.
    pub situation_tags: Vec<String>,
    /// Distinct reaction kinds of the member beats (the contents), sorted.
    pub beat_kinds: Vec<String>,
    /// The member beats grouped into this scene (nearest-in-time, contents).
    pub beats: Vec<Candidate>,
    /// A fuller transcript of the scene window (bounded), for the operator / LLM.
    pub transcript: String,
    /// The game/chapter title at the scene midpoint.
    pub game: Option<String>,
    /// Aggregate matched situation cues (why this frame), deduped.
    pub cues: Vec<String>,
}

/// Scene-segmentation tuning knobs.
pub struct SceneParams {
    /// Group beats whose gap is within this many seconds into one scene.
    pub merge_gap_secs: f64,
    /// `Setup` only frames a window at least this long (seconds).
    pub setup_min_secs: f64,
    /// `Setup` only frames a window whose MEAN peak-relative energy is at or below
    /// this (menus are quiet). A loud window never frames Setup.
    pub setup_max_energy: f64,
    /// Weight applied to a peak-relative energy spike when scoring `Combat`.
    pub energy_weight: f64,
    /// Cap the scene-window transcript to this many characters.
    pub transcript_max_chars: usize,
    /// Max priority the contained beats can add on top of the frame baseline.
    pub beat_boost_cap: f64,
}

impl Default for SceneParams {
    fn default() -> Self {
        Self {
            merge_gap_secs: 25.0,
            setup_min_secs: 25.0,
            setup_max_energy: 0.35,
            energy_weight: 2.0,
            transcript_max_chars: 600,
            beat_boost_cap: 0.6,
        }
    }
}

/// Group beats into scenes and frame each one. Beats must be the detector's output
/// for the same transcript/energy/chapters; the result is in chronological order.
pub fn segment(
    segments: &[TranscriptSegment],
    energy: &EnergyTrack,
    chapters: &[Chapter],
    beats: &[Candidate],
    params: &SceneParams,
) -> Vec<Scene> {
    if beats.is_empty() {
        return Vec::new();
    }
    let energy_peak = energy.rms.iter().copied().fold(0.0_f32, f32::max);

    // Sort beats by start (detect() returns them score-ordered).
    let mut ordered: Vec<&Candidate> = beats.iter().collect();
    ordered.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Cluster into groups by time gap, never crossing a chapter boundary.
    let mut groups: Vec<Vec<&Candidate>> = vec![vec![ordered[0]]];
    for cand in &ordered[1..] {
        let cur = groups.last().expect("at least one group");
        let prev_end = cur.iter().map(|c| c.end).fold(f64::MIN, f64::max);
        let gap = cand.start - prev_end;
        let same_chapter = chapter_title(chapters, group_midpoint(cur))
            == chapter_title(chapters, f64::midpoint(cand.start, cand.end));
        if gap <= params.merge_gap_secs && same_chapter {
            groups.last_mut().expect("group").push(cand);
        } else {
            groups.push(vec![cand]);
        }
    }

    groups
        .into_iter()
        .map(|group| build_scene(&group, segments, energy, energy_peak, chapters, params))
        .collect()
}

/// Shape a beat group into a framed [`Scene`].
fn build_scene(
    group: &[&Candidate],
    segments: &[TranscriptSegment],
    energy: &EnergyTrack,
    energy_peak: f32,
    chapters: &[Chapter],
    params: &SceneParams,
) -> Scene {
    let start = group.iter().map(|c| c.start).fold(f64::MAX, f64::min);
    let end = group.iter().map(|c| c.end).fold(f64::MIN, f64::max);
    let duration = end - start;

    let window_text = window_transcript(segments, start, end);
    let energy_max = window_energy(energy, energy_peak, start, end, EnergyStat::Max);
    let energy_mean = window_energy(energy, energy_peak, start, end, EnergyStat::Mean);

    // Strongest member beat by score, for the no-vocab fallback frame.
    let strongest_kind = group
        .iter()
        .max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|c| c.kind);

    let (frame, situation_tags, cues) = classify_frame(
        &window_text,
        energy_max,
        energy_mean,
        duration,
        strongest_kind,
        params,
    );

    let mut beat_kinds: Vec<String> = group.iter().map(|c| c.kind.key().to_string()).collect();
    beat_kinds.sort_unstable();
    beat_kinds.dedup();

    let beat_score_sum: f64 = group.iter().map(|c| c.score).sum();
    let priority = frame.baseline_priority() + (beat_score_sum * 0.3).min(params.beat_boost_cap);

    Scene {
        id: scene_id(start),
        frame,
        start,
        end,
        priority,
        situation_tags,
        beat_kinds,
        beats: group.iter().map(|c| (*c).clone()).collect(),
        transcript: truncate_text(&window_text, params.transcript_max_chars),
        game: chapter_title(chapters, f64::midpoint(start, end)),
        cues,
    }
}

/// Decide a scene's frame from its window: lexical situation scores (Combat fused
/// with an energy spike), Setup gated to sustained-quiet windows, precedence-broken
/// ties, and a member-beat-kind fallback when the window carries no situation vocab.
/// Returns `(frame, situation_tags, matched_cues)`.
fn classify_frame(
    window_text: &str,
    energy_max: f64,
    energy_mean: f64,
    duration: f64,
    strongest_kind: Option<EventKind>,
    params: &SceneParams,
) -> (CoreSituation, Vec<String>, Vec<String>) {
    let setup_qualifies =
        duration >= params.setup_min_secs && energy_mean <= params.setup_max_energy;

    // (situation, effective_score_for_framing, lexical_raw, matched_cues)
    let mut scored: Vec<(CoreSituation, f64, f64, Vec<String>)> = Vec::with_capacity(6);
    for sit in CoreSituation::ALL {
        let (mut raw, matched) = score_text(window_text, sit.lexicon());
        let lexical_raw = raw;
        // Combat is fused with an energy spike ONLY when combat vocab is present
        // (a wordless loud laugh is not a fight); mirrors detect's lexicon-routed
        // energy fusion.
        if sit == CoreSituation::Combat && lexical_raw > 0.0 && energy_max >= SPIKE_FLOOR {
            raw += params.energy_weight * energy_max;
        }
        // Suppress Setup from framing unless it is a sustained, quiet window.
        let effective = if sit == CoreSituation::Setup && !setup_qualifies {
            0.0
        } else {
            raw
        };
        scored.push((sit, effective, lexical_raw, matched));
    }

    // Frame = max effective score, precedence-broken; fallback if nothing scored.
    let frame = scored
        .iter()
        .filter(|(_, eff, _, _)| *eff > 0.0)
        .max_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.precedence().cmp(&a.0.precedence()))
        })
        .map(|(sit, _, _, _)| *sit)
        .unwrap_or_else(|| {
            // No situation vocab at all: a loud window is action, else map from the
            // strongest member beat's reaction kind (defaults to Downtime).
            if energy_max >= SPIKE_FLOOR {
                CoreSituation::Combat
            } else {
                strongest_kind.map_or(CoreSituation::Downtime, CoreSituation::from_kind)
            }
        });

    // Tags = situations with lexical signal, frame first, then by score desc.
    // Drop Downtime (the fallback, not informative) and gated-out Setup as tags.
    let mut tag_pool: Vec<(CoreSituation, f64)> = scored
        .iter()
        .filter(|(sit, _, lex, _)| {
            *lex > 0.0
                && *sit != CoreSituation::Downtime
                && (*sit != CoreSituation::Setup || setup_qualifies)
        })
        .map(|(sit, _, lex, _)| (*sit, *lex))
        .collect();
    tag_pool.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.precedence().cmp(&b.0.precedence()))
    });
    let mut situation_tags: Vec<String> = Vec::new();
    situation_tags.push(frame.key().to_string());
    for (sit, _) in tag_pool {
        let k = sit.key().to_string();
        if !situation_tags.contains(&k) {
            situation_tags.push(k);
        }
    }

    let mut cues: Vec<String> = scored.into_iter().flat_map(|(_, _, _, m)| m).collect();
    cues.sort();
    cues.dedup();

    (frame, situation_tags, cues)
}

/// Sum the weights of every cue in `lexicon` matched in `text`, returning
/// `(raw, matched)`. Mirrors `detect::score_segment`.
fn score_text(text: &str, lexicon: &[SitCue]) -> (f64, Vec<String>) {
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

/// Which reduction to apply over a window's energy buckets.
enum EnergyStat {
    Max,
    Mean,
}

/// Peak-relative energy over `[start,end]` seconds (0..1), reduced by `stat`.
/// Returns 0 when there's no energy track.
fn window_energy(energy: &EnergyTrack, peak: f32, start: f64, end: f64, stat: EnergyStat) -> f64 {
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
    let slice = &energy.rms[i0..i1];
    let value = match stat {
        EnergyStat::Max => slice.iter().copied().fold(0.0_f32, f32::max),
        EnergyStat::Mean => {
            if slice.is_empty() {
                0.0
            } else {
                slice.iter().copied().sum::<f32>() / slice.len() as f32
            }
        }
    };
    f64::from((value / peak).clamp(0.0, 1.0))
}

/// Join the text of every segment overlapping `[start,end]`.
fn window_transcript(segments: &[TranscriptSegment], start: f64, end: f64) -> String {
    segments
        .iter()
        .filter(|s| s.end > start && s.start < end)
        .map(|s| s.text.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Midpoint time of a beat group (min start .. max end).
fn group_midpoint(group: &[&Candidate]) -> f64 {
    let start = group.iter().map(|c| c.start).fold(f64::MAX, f64::min);
    let end = group.iter().map(|c| c.end).fold(f64::MIN, f64::max);
    f64::midpoint(start, end)
}

/// The title of the chapter covering time `t` (open-ended chapters have `end <= 0`).
/// Local copy of `detect::chapter_for`.
fn chapter_title(chapters: &[Chapter], t: f64) -> Option<String> {
    chapters
        .iter()
        .find(|c| t >= c.start && (c.end <= 0.0 || t < c.end))
        .map(|c| c.title.clone())
        .filter(|title| !title.is_empty())
}

/// Deterministic scene id: `scene-<start_centisecond>`.
fn scene_id(start: f64) -> String {
    format!("scene-{}", (start * 100.0).round() as i64)
}

/// Whitespace-collapse and truncate to `max_chars` on a word boundary. Local copy
/// of `detect::truncate_snippet`.
fn truncate_text(s: &str, max_chars: usize) -> String {
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

    fn energy(hz: f64, rms: &[f32]) -> EnergyTrack {
        EnergyTrack {
            hz,
            rms: rms.to_vec(),
        }
    }

    /// A minimal beat with the given kind/window/score.
    fn beat(id: &str, kind: EventKind, start: f64, end: f64, score: f64) -> Candidate {
        Candidate {
            id: id.to_string(),
            kind,
            start,
            end,
            score,
            snippet: String::new(),
            cues: Vec::new(),
            game: None,
            neighbors: Vec::new(),
        }
    }

    #[test]
    fn lone_beat_becomes_one_beat_scene() {
        let segs = vec![seg(10.0, 13.0, "let's go we won that clutch")];
        let beats = vec![beat("exciting-1000", EventKind::Exciting, 10.0, 13.0, 0.5)];
        let scenes = segment(
            &segs,
            &EnergyTrack::default(),
            &[],
            &beats,
            &SceneParams::default(),
        );
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].beats.len(), 1);
        // Victory vocab present → Triumph frame.
        assert_eq!(scenes[0].frame, CoreSituation::Triumph);
    }

    #[test]
    fn nearby_beats_group_far_beats_split() {
        let segs = vec![
            seg(10.0, 12.0, "get him he's low behind you"),
            seg(20.0, 22.0, "reload reload enemy incoming"),
            seg(400.0, 402.0, "thanks for the sub chat"),
        ];
        let beats = vec![
            beat("exciting-1000", EventKind::Exciting, 10.0, 12.0, 0.5),
            beat("exciting-2000", EventKind::Exciting, 20.0, 22.0, 0.5),
            beat("funny-40000", EventKind::Funny, 400.0, 402.0, 0.4),
        ];
        let scenes = segment(
            &segs,
            &EnergyTrack::default(),
            &[],
            &beats,
            &SceneParams::default(),
        );
        // First two beats within 25s → one scene; the third 380s later → its own.
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].beats.len(), 2);
        assert_eq!(scenes[1].beats.len(), 1);
    }

    #[test]
    fn menu_dip_mid_fight_stays_combat() {
        // A short, LOUD window with combat vocab plus a fleeting menu word. Setup is
        // gated out (short + loud) so the fight stays a Combat scene; the menu is
        // just content, not its own frame.
        let segs = vec![seg(
            10.0,
            14.0,
            "get him he's low reload behind you quick open the menu heal push",
        )];
        let beats = vec![beat("exciting-1000", EventKind::Exciting, 10.0, 14.0, 0.7)];
        // Quiet up to t=10, then a loud spike across the fight window [10,14].
        let mut rms = vec![0.0_f32; 10];
        rms.extend(std::iter::repeat_n(1000.0_f32, 6));
        let scenes = segment(
            &segs,
            &energy(1.0, &rms),
            &[],
            &beats,
            &SceneParams::default(),
        );
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].frame, CoreSituation::Combat);
    }

    #[test]
    fn sustained_quiet_menu_becomes_setup() {
        // A long (>= setup_min_secs), quiet window full of menu vocab and a couple of
        // funny banter beats → a Setup scene, boosted above bare Setup by the beats.
        let segs = vec![
            seg(
                100.0,
                130.0,
                "okay let's open the inventory and check the loadout",
            ),
            seg(
                130.0,
                160.0,
                "gonna respec the skill tree and equip this gear haha",
            ),
        ];
        let beats = vec![
            beat("funny-10500", EventKind::Funny, 105.0, 108.0, 0.4),
            beat("funny-12800", EventKind::Funny, 128.0, 131.0, 0.5),
        ];
        // The menu window is quiet RELATIVE TO THE TRACK PEAK: a loud spike earlier
        // (t=5, the action before the menu) sets the peak so [105,131] normalizes to
        // ~0.1, well below setup_max_energy.
        let mut rms: Vec<f32> = vec![100.0; 170];
        rms[5] = 1000.0;
        let scenes = segment(
            &segs,
            &energy(1.0, &rms),
            &[],
            &beats,
            &SceneParams::default(),
        );
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].frame, CoreSituation::Setup);
        // Setup baseline is 0.15; two funny beats (0.9 sum * 0.3 = 0.27) boost it.
        assert!(scenes[0].priority > CoreSituation::Setup.baseline_priority());
        assert!(scenes[0].beat_kinds.contains(&"funny".to_string()));
    }

    #[test]
    fn chapter_change_forces_a_boundary() {
        // Two beats only 5s apart (within merge_gap) but in different chapters.
        let segs = vec![
            seg(10.0, 12.0, "get him reload"),
            seg(17.0, 19.0, "check this out found a secret"),
        ];
        let beats = vec![
            beat("exciting-1000", EventKind::Exciting, 10.0, 12.0, 0.5),
            beat("informative-1700", EventKind::Informative, 17.0, 19.0, 0.5),
        ];
        let chapters = vec![
            Chapter {
                start: 0.0,
                end: 15.0,
                title: "Game A".into(),
            },
            Chapter {
                start: 15.0,
                end: 0.0,
                title: "Game B".into(),
            },
        ];
        let scenes = segment(
            &segs,
            &EnergyTrack::default(),
            &chapters,
            &beats,
            &SceneParams::default(),
        );
        assert_eq!(scenes.len(), 2, "chapter change splits the scene");
        assert_eq!(scenes[0].game.as_deref(), Some("Game A"));
        assert_eq!(scenes[1].game.as_deref(), Some("Game B"));
    }

    #[test]
    fn fail_inside_triumph_frames_as_triumph_with_fail_content() {
        // The sword-in-the-head-while-the-poison-wins scene: a fails beat, but the
        // window declares victory. Frame = Triumph (outcome), fails nests as content.
        let segs = vec![seg(
            10.0,
            14.0,
            "oh no i hit myself but wait we won that's the win let's go",
        )];
        let beats = vec![beat("fails-1000", EventKind::Fails, 10.0, 14.0, 0.6)];
        let scenes = segment(
            &segs,
            &EnergyTrack::default(),
            &[],
            &beats,
            &SceneParams::default(),
        );
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].frame, CoreSituation::Triumph);
        assert!(scenes[0].beat_kinds.contains(&"fails".to_string()));
        assert!(scenes[0].situation_tags.contains(&"triumph".to_string()));
    }

    #[test]
    fn setup_priority_below_combat() {
        assert!(
            CoreSituation::Setup.baseline_priority() < CoreSituation::Combat.baseline_priority()
        );
    }

    #[test]
    fn empty_beats_no_scenes() {
        let scenes = segment(
            &[],
            &EnergyTrack::default(),
            &[],
            &[],
            &SceneParams::default(),
        );
        assert!(scenes.is_empty());
    }
}
