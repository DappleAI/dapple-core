# Agent Instructions — conduit-media-gateway

Opt-in media pipeline service (`media-gateway` feature; Docker-only deploy —
ffmpeg/yt-dlp/whisper are baked into the image). Hosts the VOD recap pipeline
(`vod *`) and the generic composition renderer (`POST /render-timeline`).

## The policy/mechanism boundary (read this before adding a feature)

**Renderers in this crate are GENERIC MECHANISMS. Genre policy lives in GUIDE
nushell tasks.** The composition renderer (`service/layered.rs`) consumes a
[`CompositionDoc`] (version 3, compiled by a guide task and validated/persisted
by conduit-oximedia's `POST /compose-av` `strategy: "document"`) that speaks
plain NLE vocabulary — layers, time-remap segments with freeze-holds, step
placement keyframes, luma mattes (optionally inverted/drifting), time-windowed
color spans, a hard cut to a held still, one audio track. It knows nothing
about songs, stems, dots, reveals, or any guide.

This rule exists because it was broken twice: the renderer once spoke the
video-quilt guide's vocabulary (`PatternTrack { stem, dot }`, reveal, mask),
and after that was compiled away, the compiler itself (dot-window parsing +
the quilt schedule) still lived in backend Rust — so a new video genre still
meant new backend code. Both halves now live in the guide
(`dapple-guides/.../video-quilt-render.nu`, using the generic `svg shapes`
flow command for geometry). Adding a new video guide requires zero backend
changes.

**Anti-patterns:**
- ❌ Adding a guide/genre concept to `CompositionDoc` or any renderer (a field
  named after what one guide displays, a schedule computed from one guide's
  domain). If a visual idea can't be expressed with the existing primitives,
  extend the primitives GENERICALLY (a new keyframe channel, a new matte mode,
  a new segment behavior) so any guide can compile to them — and mirror the
  wire types with conduit-oximedia's `service/av/composition.rs` (update both
  or neither).
- ❌ Parsing semantic geometry out of assets here. `svg_mask.rs` only
  RASTERIZES an SVG into a matte; geometry extraction is the generic
  `svg shapes` flow command, and what shapes MEAN is guide policy.
- ❌ Accepting the retired version-2 "progression manifest". `render-timeline`
  rejects it with guidance (re-run `av compose`); don't add a silent
  upconverter that would misrender.

## Known constraints

- `build_composition_filter` emits ONE un-folded eager ffmpeg filtergraph.
  The VOD recap path's `MAX_XFADE_INPUTS` folding does NOT cover it; hardening
  it is tracked follow-up work. `MAX_RENDER_SECONDS` (240s) is the blast-radius
  cap in the meantime.
- Multi-keyframe placement scales a layer ONCE to the largest visited rect and
  centers it on smaller rects (scale is not animatable per-frame); position is
  a piecewise `if(lt(t,..))` overlay expression, so keyframes add expression
  length, not filtergraph nodes.
