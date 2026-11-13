//! Audio + video "generate → compose" service.
//!
//! The A/V analog of the generate-only image pipeline: it takes already-generated
//! audio/video **piece assets** and assembles them deterministically. Placement —
//! where each clip sits in time — is resolved once ([`resolve`] for the
//! overlapping conversation chain, [`strategy`] for the non-overlapping document
//! sequencer); two back-halves consume it, mirroring image's SVG-doc vs PNG-raster
//! split:
//!
//! - **Timeline document** ([`timeline::compose_timeline`]): an editable
//!   OpenTimelineIO/FCPXML/EDL file that **references** each piece by URL/path and
//!   never touches the media (the inviolability principle) — an operator finishes
//!   the cut in a real NLE. Pure `serde_json` over OxiMedia's interchange layer; no
//!   codec/mux crate.
//! - **Mixed render** ([`render::AudioMixCompositor`]): the overlapping
//!   multi-voice **conversation** deliverable. Here we DO read the clip samples and
//!   sum them (WAV in → one WAV out via `hound`); the overlap ("talking over each
//!   other") is just summed PCM with per-clip gain + short edge fades — no exotic
//!   DSP, still no codec crate. A `VideoCompositor` slots onto the same seam later.
//! - **Composition document** ([`composition`]): the GENERIC layered
//!   composition (version 3) a GUIDE task compiles its genre into — layers,
//!   time-remap segments, placement keyframes, mattes, color spans. `POST
//!   /compose-av` with `strategy: "document"` validates it, persists it for
//!   conduit-media-gateway's `POST /render-timeline`, or exports it as the
//!   operator's editable timeline. All genre policy lives guide-side; adding a
//!   new video guide never touches this crate.

mod composition;
mod render;
mod resolve;
mod strategy;
mod timeline;

pub use composition::{
    compose_timeline_from_doc, referenced_assets, validate_composition, AudioSource, ColorSpan,
    CompositionDoc, CompositionLayer, Matte, MatteDrift, OutroStill, PlacementKey, SliceWindow,
    TimelineMarkerSpec,
};
pub use render::{AudioMixCompositor, Compositor, MixInput};
pub use resolve::{interrupt_caps, resolve_starts, Relate, RelateMode};
pub use strategy::{
    strategy_for, ClipKind, ClipPlacement, MeasuredClip, TimelineParams, TimelineStrategy,
};
pub use timeline::{compose_timeline, ShiftMarker, TimelineFormat, TimelinePiece};
