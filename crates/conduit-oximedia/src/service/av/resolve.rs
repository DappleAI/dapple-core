//! Resolve a conversation's **relative-chain** placement into absolute clip start
//! times (seconds).
//!
//! Timing lives here, not in the delivery block. Each clip hooks to a *prior*
//! clip's END with a signed offset; walking the chain resolves every clip's
//! absolute start. **Overlaps are allowed and are the point** (people talking
//! over each other). A clip with no relation is a *root* at 0.0s. Offsets are
//! relative — each clip is placed from wherever its anchor actually landed — so
//! the uneven real clip lengths ripple through and there is no global clock.
//!
//! The engine vocabulary is deliberately generic: `consecutive` (follows) vs
//! `interrupting` (overlaps). What those *mean* in a given guide (a "muddle" that
//! rides on top vs a floor-taking interrupt, how hard it mutters down) is the
//! author's; the engine only needs the timing.

/// How a clip relates to the clip it hooks to. Both anchor to the anchor clip's
/// END; the mode only flips the sign convention so an author expresses timing
/// naturally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RelateMode {
    /// Follows the anchor: `start = anchor_end + offset` (offset ≥ 0 = a gap; a
    /// small negative offset is the ordinary rough-handoff overlap).
    #[default]
    Consecutive,
    /// Cuts in over the anchor: `start = anchor_end − offset` (offset > 0 = how
    /// many seconds before the anchor ends it barges in).
    Interrupting,
}

impl RelateMode {
    /// Parse a wire label; anything other than `interrupting` is `consecutive`.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("interrupting") {
            Self::Interrupting
        } else {
            Self::Consecutive
        }
    }
}

/// One clip's relation to a prior clip. `to` indexes the anchor clip and MUST be
/// earlier (`to < this clip's index`); a clip with no relation is a root at 0.0s.
#[derive(Debug, Clone, Copy)]
pub struct Relate {
    /// Index of the anchor clip in the piece list.
    pub to: usize,
    /// Timing relation to the anchor.
    pub mode: RelateMode,
    /// Seconds, interpreted per `mode`.
    pub offset_seconds: f64,
}

/// Resolve absolute start times (seconds) for the clips from their relations and
/// their real (decoded) durations. Walks the chain in list order — each clip's
/// anchor must already be resolved (`to < index`). Overlaps allowed; starts are
/// clamped to ≥ 0. Errors on a forward/self reference or a length mismatch.
pub fn resolve_starts(
    relates: &[Option<Relate>],
    durations_seconds: &[f64],
) -> Result<Vec<f64>, String> {
    let n = durations_seconds.len();
    if relates.len() != n {
        return Err(format!(
            "resolve_starts: {} relations for {n} clips",
            relates.len()
        ));
    }
    let mut starts = vec![0.0f64; n];
    for i in 0..n {
        let Some(r) = relates[i] else {
            continue; // root: stays 0.0
        };
        if r.to >= i {
            return Err(format!(
                "clip {i} relates to clip {} which is not earlier in the list \
                 (a clip may only hook to a prior clip)",
                r.to
            ));
        }
        let anchor_end = starts[r.to] + durations_seconds[r.to].max(0.0);
        let start = match r.mode {
            RelateMode::Consecutive => anchor_end + r.offset_seconds,
            RelateMode::Interrupting => anchor_end - r.offset_seconds,
        };
        starts[i] = start.max(0.0);
    }
    Ok(starts)
}

/// How long the interrupted (anchor) voice keeps sounding AFTER a barge-in before it
/// yields the floor — the brief window where BOTH voices overlap, which is what makes
/// an interruption sound real (people collide for a beat, then one drops) rather than
/// a clean hard cut at the barge-in. Clamped to the anchor's natural end.
const INTERRUPT_OVERLAP_SECS: f64 = 0.3;

/// The absolute time (seconds) at which each clip's playout must **stop** because a
/// later clip cuts in over it — `None` if it plays to its natural end.
///
/// This is what makes a floor-taking interruption sound real: the interrupted
/// (anchor) clip yields the floor shortly after the interrupter barges in, instead of
/// both voices plowing on to their natural ends. The anchor keeps sounding for a short
/// collision window ([`INTERRUPT_OVERLAP_SECS`]) past the barge-in — both voices
/// overlap in that window — then stops. Only an `Interrupting` relation truncates, and
/// only when the barge-in lands *strictly inside* the anchor's natural span
/// (`anchor_start < interrupter_start < anchor_end`). A `Consecutive` overlap — the
/// backchannel "talk-over" / agreement that doesn't take the floor — never truncates:
/// both voices finish. If several clips interrupt one anchor, the **earliest** barge-in
/// sets the cut.
///
/// Truncation is render-only: `resolve_starts` still walks the chain on natural
/// durations, so later turns anchor off natural ends and no gap opens where a clip
/// was cut. Call this AFTER `resolve_starts` with the same `relates`/`durations`
/// and the resolved `starts`.
#[must_use]
pub fn interrupt_caps(
    relates: &[Option<Relate>],
    starts: &[f64],
    durations_seconds: &[f64],
) -> Vec<Option<f64>> {
    let n = starts.len();
    let mut caps: Vec<Option<f64>> = vec![None; n];
    for i in 0..n {
        let Some(r) = relates.get(i).copied().flatten() else {
            continue;
        };
        if r.mode != RelateMode::Interrupting || r.to >= n {
            continue;
        }
        let anchor_start = starts[r.to];
        let anchor_end = anchor_start + durations_seconds[r.to].max(0.0);
        let cut = starts[i];
        // Only a barge-in strictly inside the anchor's natural span truncates it:
        // a cut at/after the natural end changes nothing; the `> anchor_start` guard
        // keeps a huge offset (already clamped to 0.0 by resolve_starts) from
        // zeroing the anchor out. The anchor rides INTERRUPT_OVERLAP_SECS past the
        // barge-in (the collision window) before yielding, clamped to its natural end.
        if cut > anchor_start && cut < anchor_end {
            let cap_at = (cut + INTERRUPT_OVERLAP_SECS).min(anchor_end);
            caps[r.to] = Some(caps[r.to].map_or(cap_at, |prev: f64| prev.min(cap_at)));
        }
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consecutive(to: usize, offset: f64) -> Option<Relate> {
        Some(Relate {
            to,
            mode: RelateMode::Consecutive,
            offset_seconds: offset,
        })
    }
    fn interrupting(to: usize, offset: f64) -> Option<Relate> {
        Some(Relate {
            to,
            mode: RelateMode::Interrupting,
            offset_seconds: offset,
        })
    }

    #[test]
    fn root_starts_at_zero() {
        let starts = resolve_starts(&[None], &[2.0]).unwrap();
        assert_eq!(starts, vec![0.0]);
    }

    #[test]
    fn consecutive_butts_at_anchor_end() {
        // A(2s) then B consecutive with 0 offset → B at 2.0.
        let starts = resolve_starts(&[None, consecutive(0, 0.0)], &[2.0, 1.0]).unwrap();
        assert_eq!(starts, vec![0.0, 2.0]);
    }

    #[test]
    fn consecutive_gap_pushes_later() {
        let starts = resolve_starts(&[None, consecutive(0, 0.5)], &[2.0, 1.0]).unwrap();
        assert_eq!(starts, vec![0.0, 2.5]);
    }

    #[test]
    fn interrupting_overlaps_before_anchor_end() {
        // A(2s), B interrupts 0.5s before A ends → B at 1.5 (overlaps A's tail).
        let starts = resolve_starts(&[None, interrupting(0, 0.5)], &[2.0, 1.0]).unwrap();
        assert_eq!(starts, vec![0.0, 1.5]);
        assert!(starts[1] < 2.0, "interrupt starts before the anchor ends");
    }

    #[test]
    fn chain_ripples_relative_to_actual_landings() {
        // A(2) → B interrupts A 0.5 before end (1.5) → C consecutive after B (B len 1 → ends 2.5).
        let durs = [2.0, 1.0, 1.0];
        let relates = [None, interrupting(0, 0.5), consecutive(1, 0.0)];
        let starts = resolve_starts(&relates, &durs).unwrap();
        assert_eq!(starts[0], 0.0);
        assert_eq!(starts[1], 1.5);
        assert_eq!(starts[2], 2.5); // 1.5 + 1.0
    }

    #[test]
    fn multiple_reactions_stack_off_one_anchor() {
        // B and C both hook to A's end, staggered → a pile-on.
        let durs = [2.0, 0.4, 0.4];
        let relates = [None, interrupting(0, 0.6), interrupting(0, 0.3)];
        let starts = resolve_starts(&relates, &durs).unwrap();
        assert_eq!(starts[1], 1.4);
        assert_eq!(starts[2], 1.7);
        assert_ne!(starts[1], starts[2], "staggered, not synchronized");
    }

    #[test]
    fn start_clamped_to_zero() {
        // Interrupt offset larger than anchor end → clamps to 0, not negative.
        let starts = resolve_starts(&[None, interrupting(0, 5.0)], &[2.0, 1.0]).unwrap();
        assert_eq!(starts[1], 0.0);
    }

    #[test]
    fn forward_reference_is_rejected() {
        assert!(resolve_starts(&[consecutive(1, 0.0), None], &[1.0, 1.0]).is_err());
        // self-reference
        assert!(resolve_starts(&[None, consecutive(1, 0.0)], &[1.0, 1.0]).is_err());
    }

    #[test]
    fn relate_mode_parse() {
        assert_eq!(RelateMode::parse("interrupting"), RelateMode::Interrupting);
        assert_eq!(RelateMode::parse("INTERRUPTING"), RelateMode::Interrupting);
        assert_eq!(RelateMode::parse("consecutive"), RelateMode::Consecutive);
        assert_eq!(RelateMode::parse("whatever"), RelateMode::Consecutive);
    }

    #[test]
    fn interrupt_caps_truncates_the_anchor_after_the_collision_window() {
        // A(2s), B interrupts 0.5s before A ends → B starts at 1.5. A rides the 0.3s
        // collision window past the barge-in (both audible in [1.5, 1.8]) then yields,
        // so A is capped at 1.8 — strictly AFTER the barge-in, not a hard cut at it.
        let durs = [2.0, 1.0];
        let relates = [None, interrupting(0, 0.5)];
        let starts = resolve_starts(&relates, &durs).unwrap();
        let caps = interrupt_caps(&relates, &starts, &durs);
        assert_eq!(caps, vec![Some(1.8), None]);
        assert!(
            caps[0].unwrap() > starts[1],
            "the cap is past the barge-in — a real overlap window, not a hard cut"
        );
    }

    #[test]
    fn interrupt_caps_collision_window_clamps_to_natural_end() {
        // A(2s), B interrupts only 0.1s before A ends → B at 1.9. The 0.3s window
        // would reach 2.2, past A's natural end, so the cap clamps to 2.0 (A plays
        // essentially full; a barge-in near the end doesn't chop meaningfully).
        let durs = [2.0, 1.0];
        let relates = [None, interrupting(0, 0.1)];
        let starts = resolve_starts(&relates, &durs).unwrap();
        let caps = interrupt_caps(&relates, &starts, &durs);
        assert_eq!(caps, vec![Some(2.0), None]);
    }

    #[test]
    fn interrupt_caps_ignores_consecutive_overlaps() {
        // A "talk-over": B rides on top via a negative consecutive offset. Both play
        // full — the anchor is NOT truncated.
        let durs = [2.0, 1.0];
        let relates = [None, consecutive(0, -0.5)];
        let starts = resolve_starts(&relates, &durs).unwrap();
        assert!(
            starts[1] < 2.0,
            "negative consecutive offset overlaps the tail"
        );
        let caps = interrupt_caps(&relates, &starts, &durs);
        assert_eq!(
            caps,
            vec![None, None],
            "consecutive overlaps never truncate"
        );
    }

    #[test]
    fn interrupt_caps_earliest_barge_in_wins() {
        // Two interrupts on one anchor barge in at 1.4 and 1.7. The earliest (1.4)
        // sets the cut: its 0.3s collision window lands the cap at 1.7. (The later
        // barge-in at 1.7 would cap at min(2.0, 2.0)=2.0, so the earliest wins.)
        let durs = [2.0, 0.4, 0.4];
        let relates = [None, interrupting(0, 0.6), interrupting(0, 0.3)];
        let starts = resolve_starts(&relates, &durs).unwrap();
        let caps = interrupt_caps(&relates, &starts, &durs);
        assert_eq!(caps[0], Some(1.7), "earliest barge-in sets the cut");
    }

    #[test]
    fn interrupt_caps_no_truncation_past_natural_end() {
        // A tiny interrupt offset makes B start AFTER A's natural end (a gap, oddly
        // typed as interrupting) → nothing to cut.
        let durs = [2.0, 1.0];
        let relates = [None, interrupting(0, -0.5)]; // start = 2.0 - (-0.5) = 2.5
        let starts = resolve_starts(&relates, &durs).unwrap();
        assert!(starts[1] >= 2.0);
        let caps = interrupt_caps(&relates, &starts, &durs);
        assert_eq!(caps, vec![None, None]);
    }
}
