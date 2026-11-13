//! Deterministic, no-model layout strategies for assembling generated pieces
//! onto a background canvas.
//!
//! This is the "where does each piece go" half of the generate-only image
//! pipeline. A model only ever produces net-new pieces (`generate`); the layout
//! here is pure, reproducible arithmetic over the pieces' **real content** (their
//! trimmed alpha bounding boxes, supplied as [`MeasuredPiece`]). Sizing/placing by
//! content — not the nominal frame — is the unlock: generated cutouts carry large
//! transparent margins, so packing by frame makes content look small and
//! over-spaced. Packing by content fills the canvas, and letting pieces overlap
//! their empty regions makes the field read packed rather than shelved.
//!
//! Determinism: the only "randomness" is a seeded splitmix64 jitter keyed by
//! `(seed, band, index)`. Same pieces + same `seed` ⇒ byte-identical layout, so a
//! re-render reproduces exactly (matches the inviolability/no-model principle).
//!
//! A [`LayoutStrategy`] is pluggable: `panorama` is the seek-and-find field the
//! packed-panorama POC needs; future strategies (grid/collage for "assemble
//! uploaded photos by theme") slot in behind the same trait with no change to the
//! compositor.

use serde::Deserialize;

/// A piece measured by its trimmed content (alpha) bounding box. Aspect comes
/// from the real drawn content, so callers need not tag shape — the optional
/// hints on the wire are forward-compat only and ignored by `panorama`.
#[derive(Debug, Clone, Copy)]
pub struct MeasuredPiece {
    /// Index into the caller's kept-piece list (the compositor pastes by this).
    pub index: usize,
    /// Trimmed content width in pixels.
    pub content_w: u32,
    /// Trimmed content height in pixels.
    pub content_h: u32,
}

/// A computed placement: paste piece `index` at top-left `(x, y)` scaled to
/// `(w, h)`. `w/h` preserves the piece's content aspect ratio. Returned in
/// paint order (far→near), so nearer/larger pieces composite on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub index: usize,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Deterministic knobs for a layout pass. All fractions are of the canvas
/// dimensions. Defaults are tuned starting points for the packed-panorama POC;
/// the guide tunes them per-render. `#[serde(default)]` means a request may send
/// only the keys it wants to override — absent keys fall back to [`Default`].
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct LayoutParams {
    /// Near (front/bottom) piece content height as a fraction of canvas height.
    pub near_height_frac: f32,
    /// Far (back/top) piece content height as a fraction of canvas height.
    pub far_height_frac: f32,
    /// Side margin kept clear on each edge, as a fraction of canvas width. This
    /// is the NEAR (bottom) band's margin; see `far_side_margin_frac`.
    pub side_margin_frac: f32,
    /// Side margin at the FAR (top/back) band, as a fraction of canvas width. The
    /// near (bottom) band uses `side_margin_frac`; bands between interpolate. A
    /// larger far margin narrows the top of the field so rows converge toward the
    /// centre with depth (a perspective funnel). Defaults to the `side_margin_frac`
    /// default (no taper) for backward-compatible behaviour; the guide opts in.
    pub far_side_margin_frac: f32,
    /// Number of depth bands (rows, far→near). Pieces are partitioned across
    /// these; band heights overlap so the field doesn't read as rigid rows.
    pub bands: u32,
    /// Horizontal jitter magnitude (anti-columnar), as a fraction of a piece's
    /// width. Breaks vertical alignment between bands.
    pub stagger_frac: f32,
    /// Vertical jitter within a band, as a fraction of the band's slot height.
    pub vertical_jitter_frac: f32,
    /// Allowed horizontal overlap between neighbours in a band, as a fraction of
    /// piece width (content bboxes overlap their transparent regions; packs tighter).
    pub overlap_frac: f32,
    /// Fraction of canvas height kept clear at the top for the backdrop horizon.
    pub keep_clear_top_frac: f32,
    /// Fraction of canvas height kept clear at the bottom (foreground rail).
    pub keep_clear_bottom_frac: f32,
    /// Deterministic jitter seed. `0` derives a stable seed from the piece count.
    pub seed: u64,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            near_height_frac: 0.28,
            far_height_frac: 0.18,
            side_margin_frac: 0.02,
            far_side_margin_frac: 0.02,
            bands: 5,
            // Calmer than the first cut: the geometry now spaces by real content
            // width and tiles the vertical zone with depth-graded slots, so heavy
            // jitter/overlap is no longer needed to look packed — and large values
            // only re-introduced the "forced on top of each other" stacking.
            stagger_frac: 0.22,
            vertical_jitter_frac: 0.10,
            overlap_frac: 0.06,
            keep_clear_top_frac: 0.15,
            keep_clear_bottom_frac: 0.03,
            seed: 0,
        }
    }
}

/// A pluggable arrangement of pieces onto a canvas. `Send + Sync` so a boxed
/// strategy can move onto the compositor's `spawn_blocking` thread.
pub trait LayoutStrategy: Send + Sync {
    /// Compute placements for `pieces` on a `canvas` of `(width, height)` px.
    fn arrange(
        &self,
        canvas: (u32, u32),
        pieces: &[MeasuredPiece],
        params: &LayoutParams,
    ) -> Vec<Placement>;
}

/// Resolve a strategy by name. `""`/`"panorama"` → the seek-and-find packer.
/// Returns `None` for an unknown name (the handler turns that into a 4xx).
pub fn strategy_for(name: &str) -> Option<Box<dyn LayoutStrategy>> {
    match name.trim() {
        "" | "panorama" => Some(Box::new(PanoramaStrategy)),
        _ => None,
    }
}

/// splitmix64-derived unit float in `[0, 1)`, keyed deterministically.
fn hash_unit(seed: u64, a: u64, b: u64) -> f32 {
    let mut z = seed
        .wrapping_add(a.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(b.wrapping_mul(0xD1B5_4A32_D192_ED03))
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // top 53 bits → [0, 1)
    (z >> 11) as f32 / (1u64 << 53) as f32
}

/// Signed jitter in `(-1, 1)`.
fn hash_signed(seed: u64, a: u64, b: u64) -> f32 {
    hash_unit(seed, a, b) * 2.0 - 1.0
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Content aspect (w/h) of a measured piece; `1.0` for a degenerate zero-height box.
fn aspect_of(p: &MeasuredPiece) -> f32 {
    if p.content_h > 0 {
        p.content_w as f32 / p.content_h as f32
    } else {
        1.0
    }
}

/// Seek-and-find field. Two ideas keep pieces from blocking one another:
///
/// **Vertically**, the usable zone is carved into depth-graded *slots* that
/// **tile** it — near (bottom) slots are taller than far (top) slots (ratio
/// `near_height_frac / far_height_frac`), and together they sum to the zone
/// height. Each piece is sized to its slot (× a small [`VFILL`] bleed for an
/// organic, un-shelved feel), so adjacent rows overlap only slightly and a near
/// piece can never bury a far one. This is the fix for the old model, which sized
/// pieces as a fraction of the *full canvas* and then crammed three full-height
/// rows into a half-height zone — guaranteeing vertical stacking.
///
/// **Horizontally**, each row is packed by real content *width*: pieces are
/// justified across the row's usable width with an even edge-gap, shrunk to fit if
/// the row is overfull, and the anti-columnar jitter is **bounded by that gap** so
/// it can stagger pieces without ever shoving them into each other.
///
/// The usable width tapers with depth (`far_side_margin_frac` at the top →
/// `side_margin_frac` at the bottom) for a perspective funnel. Piece counts are
/// distributed by each row's **capacity** (`usable_w / (band_h · avg_aspect)`), so
/// the short far rows take more (small) pieces and the tall near rows fewer (big)
/// ones — real depth, and ~equal fullness so shrink-to-fit can't invert the depth
/// gradient (a back row reading bigger than a front row). Pieces are ordered by
/// content **aspect** so the widest friezes take the nearest bands (their figures
/// fill the band height, so they belong up front). A lone piece in a multi-row
/// layout gets a seeded horizontal offset (so singletons in different rows don't
/// column up); the single piece in a single-row layout is centred.
pub struct PanoramaStrategy;

/// Each piece is sized to its depth slot times this factor. >1 gives a small,
/// deliberate vertical bleed between adjacent rows so the field reads organic
/// rather than shelved; kept low so rows never heavily stack.
const VFILL: f32 = 1.08;

impl LayoutStrategy for PanoramaStrategy {
    fn arrange(
        &self,
        canvas: (u32, u32),
        pieces: &[MeasuredPiece],
        params: &LayoutParams,
    ) -> Vec<Placement> {
        let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
        let n = pieces.len();
        if n == 0 || cw < 1.0 || ch < 1.0 {
            return Vec::new();
        }

        let seed = if params.seed != 0 {
            params.seed
        } else {
            (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1234_5678
        };

        // Adapt the depth-row count to the piece count so no row ends up a lone
        // piece pinned against one side. At most ~`TARGET_PER_BAND` pieces per row,
        // capped by the requested `bands` and by `n`. Round UP: a small board (e.g.
        // 6 moments with `bands = 3`) should still use the requested rows instead of
        // collapsing to 2 (`round(6/2.5) = 2`), which piled the crowd into one wide
        // bottom row. `ceil(6/2.5) = 3` spreads it over the full depth.
        const TARGET_PER_BAND: f32 = 2.5;
        let bands = ((n as f32 / TARGET_PER_BAND).ceil() as usize)
            .max(1)
            .min(params.bands.max(1) as usize)
            .min(n);

        let near_margin_frac = params.side_margin_frac.clamp(0.0, 0.45);
        let far_margin_frac = params
            .far_side_margin_frac
            .clamp(0.0, 0.45)
            .max(near_margin_frac);
        let near_frac = params.near_height_frac.clamp(0.02, 1.0);
        let far_frac = params.far_height_frac.clamp(0.02, 1.0);
        let overlap = params.overlap_frac.clamp(0.0, 0.6);

        let top = params.keep_clear_top_frac.clamp(0.0, 0.9) * ch;
        let bottom = ch - params.keep_clear_bottom_frac.clamp(0.0, 0.9) * ch;
        let zone_h = (bottom - top).max(1.0);

        // --- Per-band vertical geometry: depth-graded slots that TILE the zone.
        // Slot weights run far→near as 1 → (near/far), so near rows are taller and
        // the slots sum to `zone_h` (they tile it instead of each being an
        // independent fraction of the whole canvas). `h_px = slot * VFILL`, capped
        // at `near_frac * ch` so a few-piece layout doesn't blow pieces up huge.
        let depth_ratio = (near_frac / far_frac).max(1.0);
        let weights: Vec<f32> = (0..bands)
            .map(|b| {
                let t = if bands == 1 {
                    1.0
                } else {
                    b as f32 / (bands as f32 - 1.0)
                };
                lerp(1.0, depth_ratio, t)
            })
            .collect();
        let wsum: f32 = weights.iter().sum::<f32>().max(f32::EPSILON);
        let slots: Vec<f32> = weights.iter().map(|w| zone_h * w / wsum).collect();
        let h_cap = (near_frac * ch).max(1.0);
        let band_h: Vec<f32> = slots
            .iter()
            .map(|s| (s * VFILL).min(h_cap).max(1.0))
            .collect();
        // Band centre-y from cumulative slot tops.
        let mut band_cy = Vec::with_capacity(bands);
        {
            let mut acc = top;
            for &s in &slots {
                band_cy.push(acc + s * 0.5);
                acc += s;
            }
        }

        // --- Per-band horizontal geometry: depth-tapered margin / usable width.
        let mut band_margin = Vec::with_capacity(bands);
        let mut band_usable = Vec::with_capacity(bands);
        for b in 0..bands {
            let t = if bands == 1 {
                1.0
            } else {
                b as f32 / (bands as f32 - 1.0)
            };
            let m = (lerp(far_margin_frac, near_margin_frac, t) * cw).round();
            band_margin.push(m);
            band_usable.push((cw - 2.0 * m).max(1.0));
        }

        // Order pieces by content aspect so the WIDEST cutouts (single-row friezes,
        // whose figures fill the whole band height) take the NEAREST/front bands, and
        // the squarest cutouts (multi-row knots, smaller figures) the FAR/back bands. A
        // wide frieze dropped into a back band otherwise shows figures larger than a
        // square cutout in front — the "back bigger than front" report. We sort a local
        // index list and keep each piece's caller `index` (the compositor pastes by
        // `Placement.index`), so the composite output is unchanged.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&i, &j| {
            aspect_of(&pieces[i])
                .partial_cmp(&aspect_of(&pieces[j]))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(i.cmp(&j))
        });
        let sorted: Vec<MeasuredPiece> = order.iter().map(|&i| pieces[i]).collect();

        // Distribute piece counts by each row's CAPACITY (≈ how many average-width
        // pieces fit: usable_w / (band_h · avg_aspect)), not raw usable width. Capacity
        // is larger for the FAR (short) rows, so they take MORE small pieces while the
        // NEAR (tall) rows take FEWER big ones — the real depth cue (few big foreground
        // groups, many small background ones). And because counts track capacity, every
        // row is ~equally full, so shrink-to-fit (if it fires) scales every row by the
        // SAME factor and can never drag a front row's height below a back row's ("back
        // bigger than front"). Pieces fill the rows as contiguous far→near ranges from
        // `sorted`.
        let avg_aspect = (sorted.iter().map(aspect_of).sum::<f32>() / n as f32).max(0.1);
        let capacity: Vec<f32> = (0..bands)
            .map(|b| band_usable[b] / (band_h[b] * avg_aspect).max(1.0))
            .collect();
        let counts = distribute_counts(n, &capacity);

        let mut out = Vec::with_capacity(n);
        let mut start = 0usize;

        for b in 0..bands {
            let count = counts[b];
            if count == 0 {
                continue;
            }
            let band = &sorted[start..start + count];
            start += count;

            let mut h_px = band_h[b];
            let margin = band_margin[b];
            let usable_w = band_usable[b];

            // Per-piece widths at this band height (content aspect preserved).
            let aspect: Vec<f32> = band.iter().map(aspect_of).collect();
            let mut widths: Vec<f32> = aspect.iter().map(|a| (h_px * a).max(1.0)).collect();

            // Shrink-to-fit: if the row can't fit even at the max allowed overlap,
            // scale the whole row (height + widths) down so it does. This keeps
            // content aspect and guarantees within-row pieces never collide.
            let sum_w: f32 = widths.iter().sum();
            let avg_w = sum_w / count as f32;
            let min_footprint = sum_w - (count.saturating_sub(1) as f32) * overlap * avg_w;
            if min_footprint > usable_w && min_footprint > 0.0 {
                let s = usable_w / min_footprint;
                h_px *= s;
                for w in widths.iter_mut() {
                    *w *= s;
                }
            }
            let sum_w: f32 = widths.iter().sum();
            let avg_w = sum_w / count as f32;

            // Even edge-gap so the row is justified across the usable width: first
            // piece's left edge at the left margin, last piece's right edge at the
            // right margin, equal gaps between. `g < 0` means a controlled overlap
            // (only reachable after shrink-to-fit clamps it).
            let g = if count > 1 {
                (usable_w - sum_w) / (count as f32 - 1.0)
            } else {
                0.0
            };

            let mut centers = Vec::with_capacity(count);
            if count == 1 {
                // A lone piece: centre it in a single-row layout, but in a
                // multi-row layout give it a seeded horizontal offset so lone
                // pieces in different rows don't stack into a column.
                let frac = if bands == 1 {
                    0.5
                } else {
                    (0.5 + 0.28 * hash_signed(seed, b as u64, 7)).clamp(0.18, 0.82)
                };
                centers.push(margin + usable_w * frac);
            } else {
                let mut cursor = margin;
                for &w in &widths {
                    centers.push(cursor + w * 0.5);
                    cursor += w + g;
                }
            }

            let cy = band_cy[b];
            // Horizontal jitter is bounded by 40% of the gap so two neighbours can
            // each drift toward one another and still leave a positive gap — the
            // stagger never re-creates the overlap the geometry just removed.
            let jx_cap = (0.4 * g).max(0.0).min(params.stagger_frac * avg_w);
            // Vertical jitter is a small fraction of the slot so a piece can't ride
            // up into the next row or the cleared sky band.
            let jy_mag = params.vertical_jitter_frac * slots[b] * 0.5;

            for (i, &w) in widths.iter().enumerate() {
                let jx = hash_signed(seed, b as u64, i as u64) * jx_cap;
                let jy = hash_signed(seed, 0xB17, (b * 4096 + i) as u64) * jy_mag;

                let x = centers[i] + jx - w / 2.0;
                let y = cy - h_px / 2.0 + jy;

                // Clamp x inside the canvas, but clamp y inside the usable ZONE
                // (`[top, bottom]`), not the full canvas. Anchoring to `ch - h_px` let
                // an oversized near piece ride all the way to the bottom edge, piling
                // the near row against the frame; holding it to `bottom - h_px` rests it
                // on the zone's lower bound instead, and the `top` floor keeps far-row
                // pieces out of the reserved sky. `lo_y`/`hi_y` are ordered so a piece
                // taller than the zone still stays on-canvas (degenerate but safe).
                let max_x = (cw - w).max(0.0);
                let lo_y = top.min((ch - h_px).max(0.0));
                let hi_y = (bottom - h_px).max(lo_y);
                let x = x.clamp(0.0, max_x);
                let y = y.clamp(lo_y, hi_y);

                out.push(Placement {
                    index: band[i].index,
                    x: x.round() as u32,
                    y: y.round() as u32,
                    w: w.round().max(1.0) as u32,
                    h: h_px.round().max(1.0) as u32,
                });
            }
        }

        out
    }
}

/// Distribute `n` items across rows in proportion to `weights` (each row's usable
/// width), using the largest-remainder method. Deterministic: floors first, then
/// hands out the leftover to the largest fractional parts (ties broken by row
/// index, so equal weights reproduce the old even split with the extra going to
/// the earliest/far rows). Sums to exactly `n`.
fn distribute_counts(n: usize, weights: &[f32]) -> Vec<usize> {
    let b = weights.len();
    if b == 0 {
        return Vec::new();
    }
    let wsum: f32 = weights.iter().sum();
    if !wsum.is_finite() || wsum <= 0.0 {
        // Degenerate: even split.
        let base = n / b;
        let extra = n % b;
        return (0..b).map(|i| base + usize::from(i < extra)).collect();
    }
    let mut counts = vec![0usize; b];
    let mut fracs: Vec<(f32, usize)> = Vec::with_capacity(b);
    let mut assigned = 0usize;
    for (i, &w) in weights.iter().enumerate() {
        let exact = n as f32 * w / wsum;
        let fl = exact.floor();
        counts[i] = fl as usize;
        assigned += counts[i];
        fracs.push((exact - fl, i));
    }
    let mut rem = n.saturating_sub(assigned);
    fracs.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut k = 0usize;
    while rem > 0 && b > 0 {
        counts[fracs[k % b].1] += 1;
        rem -= 1;
        k += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(index: usize) -> MeasuredPiece {
        MeasuredPiece {
            index,
            content_w: 100,
            content_h: 100,
        }
    }

    fn params() -> LayoutParams {
        LayoutParams {
            seed: 42,
            ..Default::default()
        }
    }

    #[test]
    fn places_every_piece_once() {
        let pieces: Vec<_> = (0..17).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        assert_eq!(out.len(), 17);
        let mut seen: Vec<usize> = out.iter().map(|p| p.index).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..17).collect::<Vec<_>>());
    }

    #[test]
    fn near_pieces_are_larger_than_far() {
        // Band 0 is far (top); the last band is near (bottom). With a depth
        // gradient the near band's pieces must be taller than the far band's.
        let pieces: Vec<_> = (0..10).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let far_h = out.first().unwrap().h;
        let near_h = out.last().unwrap().h;
        assert!(
            near_h > far_h,
            "near piece h={near_h} should exceed far piece h={far_h}"
        );
    }

    #[test]
    fn pieces_are_large_enough() {
        // With the zone-relative depth slots the default field still produces
        // sizeable heroes: on a 2048-tall canvas (default keep-clear) the far
        // (smallest) row clears ~320px and the near (largest) ~500px.
        let pieces: Vec<_> = (0..10).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let min_h = out.iter().map(|p| p.h).min().unwrap();
        let max_h = out.iter().map(|p| p.h).max().unwrap();
        assert!(min_h >= 320, "far pieces h={min_h} too small");
        assert!(max_h >= 500, "near pieces h={max_h} too small");
    }

    #[test]
    fn same_row_pieces_do_not_overlap() {
        // The whole point of width-aware packing: pieces sharing a depth row (same
        // height) must not overlap horizontally — left-to-right, each piece's right
        // edge stops before the next piece's left edge (modulo rounding).
        let pieces: Vec<_> = (0..15).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let mut rows: std::collections::BTreeMap<u32, Vec<Placement>> = Default::default();
        for p in &out {
            rows.entry(p.h).or_default().push(*p);
        }
        for row in rows.values_mut() {
            row.sort_by_key(|p| p.x);
            for w in row.windows(2) {
                assert!(
                    w[0].x + w[0].w <= w[1].x + 8,
                    "same-row overlap: {:?} runs into {:?}",
                    w[0],
                    w[1]
                );
            }
        }
    }

    #[test]
    fn adjacent_rows_do_not_stack() {
        // Depth rows tile the zone, so vertically-adjacent rows overlap only
        // slightly — never the "forced on top of each other" stacking. Assert the
        // overlap between consecutive rows stays under half the smaller row height.
        let pieces: Vec<_> = (0..15).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let mut by_h: std::collections::BTreeMap<u32, Vec<Placement>> = Default::default();
        for p in &out {
            by_h.entry(p.h).or_default().push(*p);
        }
        // (top, bottom, h) per row, sorted top→bottom (far→near).
        let mut rows: Vec<(f32, f32, u32)> = by_h
            .values()
            .map(|ps| {
                let t = ps.iter().map(|p| p.y).min().unwrap() as f32;
                let bt = ps.iter().map(|p| p.y + p.h).max().unwrap() as f32;
                (t, bt, ps[0].h)
            })
            .collect();
        rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for w in rows.windows(2) {
            let (_t0, b0, h0) = w[0];
            let (t1, _b1, h1) = w[1];
            let overlap = (b0 - t1).max(0.0);
            let min_h = h0.min(h1) as f32;
            assert!(
                overlap < 0.5 * min_h,
                "rows stack: overlap {overlap} >= 0.5 * {min_h}"
            );
        }
    }

    #[test]
    fn spans_full_width_no_centre_cluster() {
        // The field must reach near both side margins, not huddle in the centre.
        let pieces: Vec<_> = (0..12).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let min_x = out.iter().map(|p| p.x).min().unwrap();
        let max_right = out.iter().map(|p| p.x + p.w).max().unwrap();
        assert!(
            min_x < 300,
            "left edge should be near the margin, got {min_x}"
        );
        assert!(
            max_right > 3072 - 300,
            "right edge should reach near the far margin, got {max_right}"
        );
    }

    #[test]
    fn far_band_converges_with_taper() {
        // With a far-side margin wider than the near-side margin, the top (far)
        // row must be inset on BOTH sides relative to the bottom (near) row —
        // the perspective funnel. Far/near bands are identified by height (far is
        // smallest, near largest, since height lerps far→near with depth).
        let pieces: Vec<_> = (0..12).map(square).collect();
        let p = LayoutParams {
            seed: 42,
            side_margin_frac: 0.02,
            far_side_margin_frac: 0.20,
            ..Default::default()
        };
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &p);
        let min_h = out.iter().map(|pl| pl.h).min().unwrap();
        let max_h = out.iter().map(|pl| pl.h).max().unwrap();
        assert!(
            min_h < max_h,
            "need a depth gradient to identify far vs near band"
        );

        let far_min_x = out
            .iter()
            .filter(|pl| pl.h == min_h)
            .map(|pl| pl.x)
            .min()
            .unwrap();
        let far_max_right = out
            .iter()
            .filter(|pl| pl.h == min_h)
            .map(|pl| pl.x + pl.w)
            .max()
            .unwrap();
        let near_min_x = out
            .iter()
            .filter(|pl| pl.h == max_h)
            .map(|pl| pl.x)
            .min()
            .unwrap();
        let near_max_right = out
            .iter()
            .filter(|pl| pl.h == max_h)
            .map(|pl| pl.x + pl.w)
            .max()
            .unwrap();

        assert!(
            far_min_x > near_min_x,
            "far row left edge {far_min_x} should be inset past near row {near_min_x}"
        );
        assert!(
            far_max_right < near_max_right,
            "far row right edge {far_max_right} should fall short of near row {near_max_right}"
        );
    }

    #[test]
    fn no_taper_by_default() {
        // Default params (far == near margin) must NOT taper: the far (top) row
        // still reaches near both side margins, identical to pre-taper behaviour.
        let pieces: Vec<_> = (0..12).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let min_h = out.iter().map(|pl| pl.h).min().unwrap();
        let far_min_x = out
            .iter()
            .filter(|pl| pl.h == min_h)
            .map(|pl| pl.x)
            .min()
            .unwrap();
        let far_max_right = out
            .iter()
            .filter(|pl| pl.h == min_h)
            .map(|pl| pl.x + pl.w)
            .max()
            .unwrap();
        assert!(
            far_min_x < 400,
            "far row should reach the left margin without taper, got {far_min_x}"
        );
        assert!(
            far_max_right > 3072 - 400,
            "far row should reach the right margin without taper, got {far_max_right}"
        );
    }

    #[test]
    fn single_piece_is_centred() {
        // Regression: a lone piece used to be pinned to the left margin. It must
        // now land near the canvas centre, never against an edge.
        let out = PanoramaStrategy.arrange((3072, 2048), &[square(0)], &params());
        assert_eq!(out.len(), 1);
        let pl = out[0];
        let centre = pl.x as f32 + pl.w as f32 / 2.0;
        let cw = 3072.0;
        assert!(
            (centre - cw / 2.0).abs() < 0.15 * cw,
            "single piece centre {centre} should be near canvas centre {}",
            cw / 2.0
        );
    }

    #[test]
    fn few_pieces_fill_full_width() {
        // The screenshot's case: ~4 pieces. The old packer stacked them in a
        // left column with the right side blank. The field must now reach both
        // side margins.
        let pieces: Vec<_> = (0..4).map(square).collect();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let min_x = out.iter().map(|p| p.x).min().unwrap();
        let max_right = out.iter().map(|p| p.x + p.w).max().unwrap();
        assert!(
            min_x < 300,
            "left edge should reach the margin, got {min_x}"
        );
        assert!(
            max_right > 3072 - 300,
            "right edge should reach the far margin, got {max_right}"
        );
    }

    #[test]
    fn centres_span_most_of_width() {
        // For small piece counts the spread of piece centres must cover more than
        // half the canvas — catches both a left column and a centre column.
        for n in [4usize, 5, 6, 8] {
            let pieces: Vec<_> = (0..n).map(square).collect();
            let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
            let centres: Vec<f32> = out.iter().map(|p| p.x as f32 + p.w as f32 / 2.0).collect();
            let min_c = centres.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_c = centres.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            assert!(
                max_c - min_c > 0.5 * 3072.0,
                "n={n}: centre spread {} should exceed half the width",
                max_c - min_c
            );
        }
    }

    #[test]
    fn no_piece_floats_in_sky() {
        // With the tightened vertical jitter no piece should ride up into the
        // reserved horizon/sky band near the top of the canvas.
        let p = params();
        let floor = (p.keep_clear_top_frac * 2048.0 * 0.6) as u32;
        for n in [4usize, 6, 12] {
            let pieces: Vec<_> = (0..n).map(square).collect();
            let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &p);
            for pl in &out {
                assert!(
                    pl.y >= floor,
                    "n={n}: piece y={} floats into the sky band (floor={floor})",
                    pl.y
                );
            }
        }
    }

    #[test]
    fn keeps_top_band_clear_and_stays_in_canvas() {
        let pieces: Vec<_> = (0..12).map(square).collect();
        let p = params();
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &p);
        let top_clear = (p.keep_clear_top_frac * 2048.0) as u32;
        for pl in &out {
            // Allow vertical jitter to nudge a far piece slightly into the band,
            // but it must not start at the very top of the canvas.
            assert!(pl.y + pl.h <= 2048, "piece must stay inside the canvas");
            assert!(pl.x + pl.w <= 3072, "piece must stay inside the canvas");
            assert!(
                pl.y as f32 > top_clear as f32 * 0.3,
                "no piece should sit in the cleared horizon band: y={}",
                pl.y
            );
        }
    }

    #[test]
    fn deterministic_for_a_fixed_seed() {
        let pieces: Vec<_> = (0..9).map(square).collect();
        let a = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        let b = PanoramaStrategy.arrange((3072, 2048), &pieces, &params());
        assert_eq!(a, b, "same pieces + same seed must give identical layout");
    }

    #[test]
    fn empty_input_yields_no_placements() {
        let out = PanoramaStrategy.arrange((3072, 2048), &[], &params());
        assert!(out.is_empty());
    }

    #[test]
    fn strategy_for_resolves_panorama_and_rejects_unknown() {
        assert!(strategy_for("panorama").is_some());
        assert!(strategy_for("").is_some());
        assert!(strategy_for("grid").is_none());
    }

    #[test]
    fn guide_operating_point_spreads_without_stacking() {
        // Mirror the packed-panorama render call: figures pulled into the bottom
        // half (keep-clear-top 0.5), perspective funnel (side 0.04 → far-side 0.20),
        // 3 depth rows, a realistic mix of square + wide cluster cutouts. Assert the
        // field spreads across the width, keeps pieces in the bottom zone + canvas,
        // and never stacks rows on top of each other.
        let mk = |index: usize, wide: bool| MeasuredPiece {
            index,
            content_w: if wide { 1500 } else { 1000 },
            content_h: 1000,
        };
        let pieces: Vec<_> = (0..8)
            .map(|i| mk(i, i % 3 == 0)) // a few wide ones, like the board's shots
            .collect();
        let p = LayoutParams {
            seed: 7,
            keep_clear_top_frac: 0.5,
            side_margin_frac: 0.04,
            far_side_margin_frac: 0.20,
            bands: 3,
            ..Default::default()
        };
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &p);
        assert_eq!(out.len(), 8);

        // Stays inside the canvas, and out of the top (backdrop) half.
        for pl in &out {
            assert!(
                pl.x + pl.w <= 3072 && pl.y + pl.h <= 2048,
                "escaped canvas: {pl:?}"
            );
            assert!(
                pl.y as f32 > 0.35 * 2048.0,
                "piece rode into the backdrop half: {pl:?}"
            );
        }

        // Reaches across the width (the near/bottom row is full-bleed).
        let min_x = out.iter().map(|p| p.x).min().unwrap();
        let max_right = out.iter().map(|p| p.x + p.w).max().unwrap();
        assert!(
            min_x < 400 && max_right > 3072 - 400,
            "field doesn't span the width"
        );

        // No same-row horizontal collisions.
        let mut rows: std::collections::BTreeMap<u32, Vec<Placement>> = Default::default();
        for pl in &out {
            rows.entry(pl.h).or_default().push(*pl);
        }
        for row in rows.values_mut() {
            row.sort_by_key(|p| p.x);
            for w in row.windows(2) {
                assert!(
                    w[0].x + w[0].w <= w[1].x + 8,
                    "same-row overlap at guide op-point"
                );
            }
        }
    }

    #[test]
    fn wide_pieces_get_wider_placements() {
        // A 2:1 content box must produce w ≈ 2*h at its band.
        let wide = MeasuredPiece {
            index: 0,
            content_w: 200,
            content_h: 100,
        };
        let out = PanoramaStrategy.arrange((3072, 2048), &[wide], &params());
        let pl = out[0];
        let ratio = pl.w as f32 / pl.h as f32;
        assert!(
            (ratio - 2.0).abs() < 0.2,
            "wide piece aspect preserved, got {ratio}"
        );
    }

    #[test]
    fn six_moments_spread_across_three_rows_in_zone() {
        // Regression for the bottom-clumping fix at the guide's operating point
        // (keep-clear-top 0.5, bands 3). A 6-moment board must spread over all THREE
        // depth rows, not collapse to 2 (the old `round(6/2.5) = 2`) and pile the
        // crowd into one wide bottom row. And the zone clamp must hold: no piece rides
        // past the usable lower bound to the very bottom edge of the canvas.
        let pieces: Vec<_> = (0..6).map(square).collect();
        let p = LayoutParams {
            seed: 7,
            keep_clear_top_frac: 0.5,
            keep_clear_bottom_frac: 0.05,
            side_margin_frac: 0.04,
            far_side_margin_frac: 0.20,
            bands: 3,
            ..Default::default()
        };
        let ch = 2048.0_f32;
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &p);
        assert_eq!(out.len(), 6);

        // Three distinct depth rows (heights are depth-graded, one per band).
        let mut heights: Vec<u32> = out.iter().map(|pl| pl.h).collect();
        heights.sort_unstable();
        heights.dedup();
        assert!(
            heights.len() >= 3,
            "6 moments should occupy 3 depth rows, got heights {heights:?}"
        );

        // Zone clamp holds: nothing rides past the usable lower bound (no bottom pile-up).
        let bottom = ch - p.keep_clear_bottom_frac * ch;
        for pl in &out {
            assert!(
                (pl.y + pl.h) as f32 <= bottom + 2.0,
                "piece {pl:?} rides past the zone bottom {bottom}"
            );
        }
    }

    #[test]
    fn front_rows_are_never_smaller_than_back_rows() {
        // "Back bigger than front" guard. At the crowded guide operating point the
        // near/bottom band must never render SHORTER than the far/top band. The old
        // width-proportional distribution piled the most pieces into the wide near
        // row, which then shrank-to-fit below the far row; capacity-based counts keep
        // every row ~equally full so shrink-to-fit scales rows uniformly and preserves
        // the depth gradient.
        for n in [8usize, 12, 16, 20] {
            let pieces: Vec<_> = (0..n).map(square).collect();
            let p = LayoutParams {
                seed: 7,
                keep_clear_top_frac: 0.5,
                keep_clear_bottom_frac: 0.05,
                side_margin_frac: 0.04,
                far_side_margin_frac: 0.20,
                bands: 3,
                ..Default::default()
            };
            let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &p);
            // The far band is the highest on the canvas (smallest y); the near band the
            // lowest (largest y). Their pieces' heights must keep near >= far.
            let far_h = out.iter().min_by_key(|pl| pl.y).unwrap().h;
            let near_h = out.iter().max_by_key(|pl| pl.y).unwrap().h;
            assert!(
                near_h >= far_h,
                "n={n}: near/front h={near_h} must be >= far/back h={far_h}"
            );
        }
    }

    #[test]
    fn wider_cutouts_take_the_nearer_bands() {
        // Figure-size parity: a wide frieze's figures fill the band height, so wides
        // belong in the near/front bands. With a square+wide mix, the near (tallest)
        // band's mean aspect must exceed the far (shortest) band's.
        let mk = |index: usize, wide: bool| MeasuredPiece {
            index,
            content_w: if wide { 1800 } else { 900 },
            content_h: 900,
        };
        let pieces: Vec<_> = (0..12).map(|i| mk(i, i % 2 == 0)).collect();
        let p = LayoutParams {
            seed: 7,
            keep_clear_top_frac: 0.5,
            keep_clear_bottom_frac: 0.05,
            side_margin_frac: 0.04,
            far_side_margin_frac: 0.20,
            bands: 3,
            ..Default::default()
        };
        let out = PanoramaStrategy.arrange((3072, 2048), &pieces, &p);
        let min_h = out.iter().map(|pl| pl.h).min().unwrap();
        let max_h = out.iter().map(|pl| pl.h).max().unwrap();
        assert!(min_h < max_h, "need a depth gradient to tell far from near");
        let mean_aspect = |h: u32| {
            let v: Vec<f32> = out
                .iter()
                .filter(|pl| pl.h == h)
                .map(|pl| pl.w as f32 / pl.h as f32)
                .collect();
            v.iter().sum::<f32>() / v.len().max(1) as f32
        };
        assert!(
            mean_aspect(max_h) > mean_aspect(min_h),
            "near band mean aspect {} should exceed far band {}",
            mean_aspect(max_h),
            mean_aspect(min_h)
        );
    }
}
