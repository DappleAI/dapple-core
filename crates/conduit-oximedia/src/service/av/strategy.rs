//! Deterministic, no-model arrangement strategies for laying generated audio and
//! video pieces onto a timeline.
//!
//! This is the "where does each clip go in time" half of the generate-only A/V
//! pipeline — the temporal analog of [`crate::service::image::layout`]. A model
//! only ever produces net-new pieces; the arrangement here is pure, reproducible
//! arithmetic over the pieces' **durations** (supplied as [`MeasuredClip`]). The
//! output is a set of [`ClipPlacement`]s (which track, and the in/out/timeline
//! frame positions) that [`super::timeline::compose_timeline`] turns into an
//! editable OTIO document.
//!
//! Determinism: a [`TimelineStrategy`] is a pure function of its inputs. `sequence`
//! (the default) is fully order-deterministic without any randomness; the `seed`
//! knob on [`TimelineParams`] is forward-compat for future strategies that jitter
//! (e.g. an "assemble b-roll" packer), mirroring `LayoutParams::seed`.

use serde::Deserialize;

/// Which track family a clip belongs on. Derived from the source asset's
/// `asset_kind` (`video`/`audio`) at the handler, so a caller doesn't tag it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    /// Lands on the video track.
    Video,
    /// Lands on the audio track.
    Audio,
}

/// A piece measured by its duration in timeline frames. Aspect/pixels are
/// irrelevant here — the timeline is a temporal arrangement, so the only
/// dimension that matters is how long the clip runs.
#[derive(Debug, Clone, Copy)]
pub struct MeasuredClip {
    /// Index into the caller's piece list (the composer references media by this).
    pub index: usize,
    /// Which track family this clip belongs on.
    pub kind: ClipKind,
    /// Clip length in timeline frames (`duration_seconds * fps`, rounded).
    pub duration_frames: i64,
}

/// A computed placement: put clip `index` on its `kind`'s track, using source
/// range `[source_in, source_out)` (the whole piece by default) at timeline
/// position `timeline_in`. All values are in frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipPlacement {
    pub index: usize,
    pub kind: ClipKind,
    /// Position on the track's timeline, in frames.
    pub timeline_in: i64,
    /// In-point in the source media, in frames (0 = clip start).
    pub source_in: i64,
    /// Out-point in the source media, in frames (`source_in + duration`).
    pub source_out: i64,
}

/// Deterministic knobs for a timeline arrangement. `#[serde(default)]` means a
/// request may send only the keys it wants to override; the all-zero [`Default`]
/// butts clips edge-to-edge with no jitter.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub struct TimelineParams {
    /// Frames of black/silence to leave between consecutive clips on a track.
    /// `0` (default) butts clips edge-to-edge (a hard cut / gapless narration).
    pub gap_frames: i64,
    /// Deterministic jitter seed. Ignored by `sequence` (which is inherently
    /// order-deterministic); reserved for future jittering strategies.
    pub seed: u64,
}

/// A pluggable arrangement of clips onto tracks over time. `Send + Sync` so a
/// boxed strategy can move onto the composer's `spawn_blocking` thread.
pub trait TimelineStrategy: Send + Sync {
    /// Compute placements for `clips`. Returned in the caller's input order.
    fn arrange(&self, clips: &[MeasuredClip], params: &TimelineParams) -> Vec<ClipPlacement>;
}

/// Resolve a strategy by name. `""`/`"sequence"` → the concatenating sequencer.
/// Returns `None` for an unknown name (the handler turns that into a 4xx).
pub fn strategy_for(name: &str) -> Option<Box<dyn TimelineStrategy>> {
    match name.trim() {
        "" | "sequence" => Some(Box::new(SequenceStrategy)),
        _ => None,
    }
}

/// The default arrangement: concatenate each track family end-to-end in input
/// order. Video pieces butt against one another on the video track (a hard-cut
/// assembly / rough cut); audio pieces butt against one another on the audio
/// track (gapless narration bed). Both tracks start at frame 0, so the audio bed
/// runs under the video from the top. `gap_frames` inserts a uniform gap between
/// neighbours on each track. Fully deterministic — same clips ⇒ same placements.
pub struct SequenceStrategy;

impl TimelineStrategy for SequenceStrategy {
    fn arrange(&self, clips: &[MeasuredClip], params: &TimelineParams) -> Vec<ClipPlacement> {
        let gap = params.gap_frames.max(0);
        // Independent cursors per track family so audio and video both start at 0
        // and advance only within their own track.
        let mut video_cursor: i64 = 0;
        let mut audio_cursor: i64 = 0;

        let mut out = Vec::with_capacity(clips.len());
        for clip in clips {
            let dur = clip.duration_frames.max(0);
            let cursor = match clip.kind {
                ClipKind::Video => &mut video_cursor,
                ClipKind::Audio => &mut audio_cursor,
            };
            let timeline_in = *cursor;
            out.push(ClipPlacement {
                index: clip.index,
                kind: clip.kind,
                timeline_in,
                source_in: 0,
                source_out: dur,
            });
            *cursor = timeline_in + dur + gap;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(index: usize, kind: ClipKind, dur: i64) -> MeasuredClip {
        MeasuredClip {
            index,
            kind,
            duration_frames: dur,
        }
    }

    #[test]
    fn strategy_for_resolves_sequence_and_rejects_unknown() {
        assert!(strategy_for("sequence").is_some());
        assert!(strategy_for("").is_some());
        assert!(strategy_for("grid").is_none());
    }

    #[test]
    fn video_clips_concatenate_end_to_end() {
        let clips = vec![
            clip(0, ClipKind::Video, 100),
            clip(1, ClipKind::Video, 50),
            clip(2, ClipKind::Video, 25),
        ];
        let out = SequenceStrategy.arrange(&clips, &TimelineParams::default());
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].timeline_in, 0);
        assert_eq!(out[1].timeline_in, 100);
        assert_eq!(out[2].timeline_in, 150);
        // Source range covers the whole piece.
        assert_eq!((out[0].source_in, out[0].source_out), (0, 100));
    }

    #[test]
    fn audio_and_video_have_independent_cursors_both_from_zero() {
        // Interleaved kinds in the input; each track family concatenates on its
        // own, and both start at frame 0 (audio bed runs under the video).
        let clips = vec![
            clip(0, ClipKind::Video, 100),
            clip(1, ClipKind::Audio, 40),
            clip(2, ClipKind::Video, 60),
            clip(3, ClipKind::Audio, 30),
        ];
        let out = SequenceStrategy.arrange(&clips, &TimelineParams::default());
        let v0 = out.iter().find(|p| p.index == 0).unwrap();
        let a1 = out.iter().find(|p| p.index == 1).unwrap();
        let v2 = out.iter().find(|p| p.index == 2).unwrap();
        let a3 = out.iter().find(|p| p.index == 3).unwrap();
        assert_eq!(v0.timeline_in, 0);
        assert_eq!(a1.timeline_in, 0); // audio starts at 0, not after the video
        assert_eq!(v2.timeline_in, 100); // second video after the first
        assert_eq!(a3.timeline_in, 40); // second audio after the first
    }

    #[test]
    fn gap_frames_inserts_uniform_gap() {
        let clips = vec![clip(0, ClipKind::Video, 100), clip(1, ClipKind::Video, 50)];
        let params = TimelineParams {
            gap_frames: 12,
            ..Default::default()
        };
        let out = SequenceStrategy.arrange(&clips, &params);
        assert_eq!(out[0].timeline_in, 0);
        assert_eq!(out[1].timeline_in, 112); // 100 + 12 gap
    }

    #[test]
    fn deterministic_for_fixed_input() {
        let clips = vec![clip(0, ClipKind::Video, 100), clip(1, ClipKind::Audio, 240)];
        let a = SequenceStrategy.arrange(&clips, &TimelineParams::default());
        let b = SequenceStrategy.arrange(&clips, &TimelineParams::default());
        assert_eq!(a, b);
    }

    #[test]
    fn empty_input_yields_no_placements() {
        let out = SequenceStrategy.arrange(&[], &TimelineParams::default());
        assert!(out.is_empty());
    }
}
