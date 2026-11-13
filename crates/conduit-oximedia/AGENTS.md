# Agent Instructions — conduit-oximedia

Opt-in media analysis + composition service (`oximedia` feature). Hosts
`POST /analyze-audio` (multitrack WAV stems → annotated song map) and
`POST /compose-av` (piece arrangement, audio mixdown, and the guide-compiled
composition-document path).

## The policy/mechanism boundary (this crate is ALL mechanism)

**Video-composition policy lives in GUIDE nushell tasks, not here.** A guide
compiles its genre — whatever windows, schedules, or moves it invents — into
the GENERIC `CompositionDoc` v3 (`service/av/composition.rs`: layers,
time-remap segments, placement keyframes, mattes, color spans, markers) and
submits it via `POST /compose-av` with `strategy: "document"`. This crate then
only:

1. **Validates** it (`validate_composition` — version/dimension/timing sanity,
   asset-existence checks handler-side; deliberately no semantic opinions),
2. **Persists** it as the renderer's manifest (`format: "render"` → a
   `document` asset for conduit-media-gateway's `POST /render-timeline`), or
3. **Exports** it as the operator's editable timeline
   (`compose_timeline_from_doc` → OTIO/FCPXML/EDL; layers→tracks,
   segments→clips, placement/matte/loop ride as `dapple:*` clip metadata).

**Adding a new video guide must require ZERO changes to this crate** (or any
backend crate). The precedent: the video-quilt's dot-window compiler used to
live here (`svg_dots.rs` + the `progression` strategy) and was deliberately
moved into the guide (`dapple-guides/.../video-quilt-render.nu`, using the
generic `svg shapes` flow command for geometry). Do not reintroduce a
genre-named strategy.

A genuinely new capability (something no combination of the generic document
primitives expresses) = extend `CompositionDoc` generically — a primitive any
genre could use, never a field named after one guide — and mirror the change
in conduit-media-gateway's `service/layered.rs`.

**Anti-patterns:**
- ❌ A strategy or request field named after a genre (dots, quilts, patches,
  stems…). Strategies here are generic arrangements only (`sequence`, the
  `wav` mix, `document`).
- ❌ Drifting the wire types: the `CompositionDoc` types in
  `service/av/composition.rs` are mirrored byte-for-byte in
  conduit-media-gateway's `service/layered.rs` — update both or neither
  (`doc_wire_roundtrip_matches_oximedia_shape` is the tripwire).
- ❌ Semantic validation of a document ("a layer should…"). Well-formed but
  silly renders as authored; the guide owns meaning.
- ❌ Reading shape geometry out of SVGs here or in the renderer. Geometry
  extraction is the generic `svg shapes` flow command; what shapes MEAN is
  guide policy; the renderer only rasterizes mattes.

## Note on the two serializations

`format: "render"` persists the validated document itself;
`format: otio/fcpxml/edl` exports `compose_timeline_from_doc` from the same
document — so the render manifest and the operator's timeline agree by
construction. Guides call `av compose --strategy document` twice with the same
piped doc.
