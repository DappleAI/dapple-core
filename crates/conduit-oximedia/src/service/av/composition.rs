//! The generic layered-composition document (version 3) — the wire contract
//! between guide-compiled compositions and the renderer.
//!
//! A guide task COMPILES its genre (whatever windows, schedules, or moves it
//! invents) into this document and submits it to `POST /compose-av` with
//! `strategy: "document"`. The backend only VALIDATES it, persists it for the
//! renderer (conduit-media-gateway's `POST /render-timeline`, which holds a
//! byte-for-byte serde mirror of these types), or exports it as an editable
//! OTIO/FCPXML/EDL timeline. No genre vocabulary exists here, and adding a new
//! video guide must never require touching this module.

use std::path::PathBuf;

use oximedia_core::Rational;
use oximedia_timeline::fcpxml_export::{FcpxmlExportOptions, FcpxmlExporter};
use oximedia_timeline::otio_export::{OtioExportOptions, OtioExporter};
use oximedia_timeline::timeline_exporter::{EdlExportOptions, TimelineExporter};
use oximedia_timeline::types::Position;
use oximedia_timeline::{Clip, Marker, MediaSource, Timeline};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::timeline::TimelineFormat;
use crate::error::OximediaError;

/// One play-window of a layer's source: at `at_s` on the timeline, play the
/// source from `source_in_s` for `duration_s`, then freeze on the last shown
/// frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SliceWindow {
    /// Timeline position (seconds).
    pub at_s: f64,
    /// In-point in the source (seconds).
    pub source_in_s: f64,
    /// Play length (seconds).
    pub duration_s: f64,
}

/// The full-mix audio source of the render.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AudioSource {
    pub asset_id: Uuid,
    /// Linear playback gain (1.0 = unity).
    pub gain: f64,
}

/// The renderer's input: a GENERIC layered composition (version 3), persisted
/// as a `document` asset by `POST /compose-av` with `strategy: "document"`.
/// Authored by a guide task; everything genre-specific is compiled away before
/// this document exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionDoc {
    /// Document schema version (3 = the generic composition shape).
    pub version: u32,
    pub name: String,
    pub fps_num: i32,
    pub fps_den: i32,
    pub width: u32,
    pub height: u32,
    /// Output duration (seconds); the outro still holds to it.
    pub duration_seconds: f64,
    pub audio: AudioSource,
    /// The hard cut from the composed layers to a held still.
    pub outro: OutroStill,
    /// Layers in ascending z order.
    pub layers: Vec<CompositionLayer>,
    #[serde(default)]
    pub color_spans: Vec<ColorSpan>,
    /// Timeline markers for the operator's editable export (the renderer
    /// ignores them). Plain time + label — no genre vocabulary.
    #[serde(default)]
    pub markers: Vec<TimelineMarkerSpec>,
}

/// Hard cut at `at_s` to a still image, held to the document's end.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OutroStill {
    pub at_s: f64,
    pub asset_id: Uuid,
}

/// One video layer of the composition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionLayer {
    /// Display label for logs/errors only.
    pub name: String,
    pub asset_id: Uuid,
    /// Loop the source to cover its span.
    #[serde(default)]
    pub loop_source: bool,
    /// Time-remap segments (play + freeze-hold). Empty ⇒ continuous playback.
    #[serde(default)]
    pub segments: Vec<SliceWindow>,
    /// Placement keyframes (step function of `t`). Empty ⇒ the full canvas.
    #[serde(default)]
    pub placement: Vec<PlacementKey>,
    #[serde(default)]
    pub matte: Option<Matte>,
}

/// The layer's rect from `at_s` onward (pixels, top-left origin).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlacementKey {
    pub at_s: f64,
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// A luma matte cut from a rasterizable asset, optionally inverted/drifting.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Matte {
    pub asset_id: Uuid,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub drift: Option<MatteDrift>,
}

/// Lissajous drift parameters for a matte.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MatteDrift {
    pub rate_x: f64,
    pub rate_y: f64,
    pub use_frac: f64,
}

impl Default for MatteDrift {
    fn default() -> Self {
        Self {
            rate_x: 0.11,
            rate_y: 0.13,
            use_frac: 0.8,
        }
    }
}

/// A time-windowed tone adjustment over the whole composition.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ColorSpan {
    pub start_s: f64,
    pub end_s: f64,
    #[serde(default)]
    pub temperature: f64,
    #[serde(default = "one")]
    pub saturation: f64,
    #[serde(default)]
    pub brightness: f64,
}

fn one() -> f64 {
    1.0
}

/// A timeline marker for the operator export: plain time + label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineMarkerSpec {
    pub at_s: f64,
    pub name: String,
    #[serde(default)]
    pub comment: String,
}

fn invalid(msg: impl std::fmt::Display) -> OximediaError {
    OximediaError::InvalidRequest(format!("composition document: {msg}"))
}

/// Every asset the document references (existence is checked handler-side).
#[must_use]
pub fn referenced_assets(doc: &CompositionDoc) -> Vec<(Uuid, &'static str)> {
    let mut ids = vec![(doc.audio.asset_id, "audio"), (doc.outro.asset_id, "outro")];
    for l in &doc.layers {
        ids.push((l.asset_id, "layer"));
        if let Some(m) = &l.matte {
            ids.push((m.asset_id, "matte"));
        }
    }
    ids
}

/// Generic sanity validation — version, dimensions, timing monotonicity.
/// Deliberately knows nothing about what the layers MEAN; a semantically
/// silly-but-well-formed document renders as authored.
pub fn validate_composition(doc: &CompositionDoc) -> Result<(), OximediaError> {
    if doc.version != 3 {
        return Err(invalid(format!(
            "version {} not supported (want 3)",
            doc.version
        )));
    }
    if doc.fps_num <= 0 || doc.fps_den <= 0 {
        return Err(invalid("fps_num and fps_den must be positive"));
    }
    if doc.width == 0 || doc.height == 0 {
        return Err(invalid("width and height must be positive"));
    }
    if !(doc.duration_seconds.is_finite() && doc.duration_seconds > 0.0) {
        return Err(invalid("duration_seconds must be positive and finite"));
    }
    if !(doc.outro.at_s.is_finite() && doc.outro.at_s >= 0.0) {
        return Err(invalid("outro.at_s must be non-negative and finite"));
    }
    if doc.layers.is_empty() {
        return Err(invalid("at least one layer is required"));
    }
    for (i, l) in doc.layers.iter().enumerate() {
        for (k, s) in l.segments.iter().enumerate() {
            if !(s.at_s.is_finite()
                && s.at_s >= 0.0
                && s.source_in_s.is_finite()
                && s.source_in_s >= 0.0
                && s.duration_s.is_finite()
                && s.duration_s > 0.0)
            {
                return Err(invalid(format!(
                    "layer[{i}] '{}' segment[{k}] has non-finite or non-positive timing",
                    l.name
                )));
            }
        }
        if l.segments.windows(2).any(|w| w[1].at_s < w[0].at_s) {
            return Err(invalid(format!(
                "layer[{i}] '{}' segments must be in ascending at_s order",
                l.name
            )));
        }
        for (k, p) in l.placement.iter().enumerate() {
            if !(p.at_s.is_finite() && p.at_s >= 0.0) || p.w == 0 || p.h == 0 {
                return Err(invalid(format!(
                    "layer[{i}] '{}' placement[{k}] has invalid timing or zero size",
                    l.name
                )));
            }
        }
        if l.placement.windows(2).any(|w| w[1].at_s < w[0].at_s) {
            return Err(invalid(format!(
                "layer[{i}] '{}' placement keys must be in ascending at_s order",
                l.name
            )));
        }
    }
    Ok(())
}

fn av_err(msg: impl std::fmt::Display) -> OximediaError {
    OximediaError::AvError(msg.to_string())
}

/// Serialize any composition document as the operator's editable timeline
/// (OTIO/FCPXML/EDL). Purely structural: one video track per layer (segments
/// become clips; a segment-less layer documents as one clip over the composed
/// span), an `outro` track for the held still, one audio track, and the
/// document's markers. Layer placement/matte/loop ride as `dapple:*` clip
/// metadata (JSON strings) so nothing is lost on the way to the NLE. Pure and
/// deterministic.
pub fn compose_timeline_from_doc(
    doc: &CompositionDoc,
    media_base: &str,
    sample_rate: u32,
    format: TimelineFormat,
) -> Result<(Vec<u8>, &'static str), OximediaError> {
    let fps_ratio = f64::from(doc.fps_num) / f64::from(doc.fps_den);
    let frames = |t: f64| -> i64 { (t * fps_ratio).round().max(0.0) as i64 };
    let base = media_base.trim_end_matches('/');
    let media_ref = |id: Uuid| format!("{base}/api/assets/{id}/content");

    let mut timeline = Timeline::new(
        &doc.name,
        Rational::new(i64::from(doc.fps_num), i64::from(doc.fps_den)),
        sample_rate,
    )
    .map_err(av_err)?;

    // The composed span: layers run up to the outro cut.
    let span_s = doc.outro.at_s.min(doc.duration_seconds);

    for layer in &doc.layers {
        let track = timeline
            .add_video_track(layer.name.clone())
            .map_err(av_err)?;
        // Structural metadata the NLE would otherwise lose — stamped on the
        // layer's first clip.
        let mut extra: Vec<(String, String)> = Vec::new();
        if layer.loop_source {
            extra.push(("dapple:loop".to_string(), "true".to_string()));
        }
        if !layer.placement.is_empty() {
            let json = serde_json::to_string(&layer.placement)
                .map_err(|e| av_err(format!("placement metadata: {e}")))?;
            extra.push(("dapple:placement".to_string(), json));
        }
        if let Some(m) = &layer.matte {
            let json =
                serde_json::to_string(m).map_err(|e| av_err(format!("matte metadata: {e}")))?;
            extra.push(("dapple:matte".to_string(), json));
        }

        if layer.segments.is_empty() {
            // Continuous layer: one clip covering the composed span.
            let mut clip = Clip::new(
                layer.name.clone(),
                MediaSource::file(PathBuf::from(media_ref(layer.asset_id))),
                Position::new(0),
                Position::new(frames(span_s).max(1)),
                Position::new(0),
            )
            .map_err(av_err)?;
            for (k, v) in extra {
                clip.metadata.insert(k, v);
            }
            timeline.add_clip(track, clip).map_err(av_err)?;
        } else {
            for (k, slice) in layer.segments.iter().enumerate() {
                let source_in = frames(slice.source_in_s);
                let source_out = frames(slice.source_in_s + slice.duration_s).max(source_in + 1);
                let mut clip = Clip::new(
                    format!("{} {}", layer.name, k + 1),
                    MediaSource::file(PathBuf::from(media_ref(layer.asset_id))),
                    Position::new(source_in),
                    Position::new(source_out),
                    Position::new(frames(slice.at_s)),
                )
                .map_err(av_err)?;
                if k == 0 {
                    for (mk, mv) in std::mem::take(&mut extra) {
                        clip.metadata.insert(mk, mv);
                    }
                }
                timeline.add_clip(track, clip).map_err(av_err)?;
            }
        }
    }

    // The outro still (hard cut, held to the end of the document).
    {
        let track = timeline.add_video_track("outro").map_err(av_err)?;
        let hold = (doc.duration_seconds - doc.outro.at_s).max(0.0);
        let clip = Clip::new(
            "outro".to_string(),
            MediaSource::file(PathBuf::from(media_ref(doc.outro.asset_id))),
            Position::new(0),
            Position::new(frames(hold).max(1)),
            Position::new(frames(doc.outro.at_s)),
        )
        .map_err(av_err)?;
        timeline.add_clip(track, clip).map_err(av_err)?;
    }

    // The audio source, 0 to the end.
    {
        let track = timeline.add_audio_track("A1").map_err(av_err)?;
        let clip = Clip::new(
            "audio".to_string(),
            MediaSource::file(PathBuf::from(media_ref(doc.audio.asset_id))),
            Position::new(0),
            Position::new(frames(doc.duration_seconds).max(1)),
            Position::new(0),
        )
        .map_err(av_err)?;
        timeline.add_clip(track, clip).map_err(av_err)?;
    }

    for m in &doc.markers {
        let mut marker = Marker::new(Position::new(frames(m.at_s)), m.name.clone());
        if !m.comment.is_empty() {
            marker = marker.with_comment(m.comment.clone());
        }
        timeline.markers.add_marker(marker);
    }

    let bytes = match format {
        TimelineFormat::Otio => OtioExporter::export(&timeline, &OtioExportOptions::default())
            .map_err(av_err)?
            .into_bytes(),
        TimelineFormat::Fcpxml => {
            FcpxmlExporter::export(&timeline, &FcpxmlExportOptions::default())
                .map_err(av_err)?
                .into_bytes()
        }
        TimelineFormat::Edl => {
            let display_fps = (fps_ratio.round().max(1.0)) as u32;
            let opts = EdlExportOptions {
                title: doc.name.clone(),
                frame_rate: display_fps,
                ..Default::default()
            };
            TimelineExporter::new()
                .to_edl(&timeline, &opts)
                .map_err(av_err)?
                .into_bytes()
        }
    };
    Ok((bytes, format.mime()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn doc() -> CompositionDoc {
        CompositionDoc {
            version: 3,
            name: "Quilt".to_string(),
            fps_num: 24,
            fps_den: 1,
            width: 1280,
            height: 720,
            duration_seconds: 60.0,
            audio: AudioSource {
                asset_id: Uuid::from_u128(10),
                gain: 1.0,
            },
            outro: OutroStill {
                at_s: 55.0,
                asset_id: Uuid::from_u128(12),
            },
            layers: vec![
                CompositionLayer {
                    name: "pads".to_string(),
                    asset_id: Uuid::from_u128(1),
                    loop_source: false,
                    segments: vec![
                        SliceWindow {
                            at_s: 2.0,
                            source_in_s: 0.0,
                            duration_s: 4.0,
                        },
                        SliceWindow {
                            at_s: 40.0,
                            source_in_s: 4.0,
                            duration_s: 4.0,
                        },
                    ],
                    placement: vec![
                        PlacementKey {
                            at_s: 0.0,
                            x: 173,
                            y: 277,
                            w: 294,
                            h: 166,
                        },
                        PlacementKey {
                            at_s: 20.0,
                            x: 739,
                            y: 236,
                            w: 442,
                            h: 248,
                        },
                    ],
                    matte: None,
                },
                CompositionLayer {
                    name: "foreground".to_string(),
                    asset_id: Uuid::from_u128(13),
                    loop_source: true,
                    segments: Vec::new(),
                    placement: Vec::new(),
                    matte: Some(Matte {
                        asset_id: Uuid::from_u128(11),
                        invert: true,
                        drift: Some(MatteDrift::default()),
                    }),
                },
            ],
            color_spans: vec![ColorSpan {
                start_s: 10.0,
                end_s: 20.0,
                temperature: 0.5,
                saturation: 1.2,
                brightness: 0.1,
            }],
            markers: vec![TimelineMarkerSpec {
                at_s: 20.0,
                name: "chorus 1".to_string(),
                comment: "song section".to_string(),
            }],
        }
    }

    // ── wire shape ───────────────────────────────────────────────────────────

    #[test]
    fn doc_wire_shape_is_pinned() {
        // The media-gateway renderer holds a serde mirror of these types; this
        // pins the field names it deserializes. Do not rename fields.
        let json = serde_json::to_value(doc()).unwrap();
        for key in [
            "version",
            "name",
            "fps_num",
            "fps_den",
            "width",
            "height",
            "duration_seconds",
            "audio",
            "outro",
            "layers",
            "color_spans",
        ] {
            assert!(json.get(key).is_some(), "missing top-level key {key}");
        }
        let layer = &json["layers"][0];
        for key in ["name", "asset_id", "loop_source", "segments", "placement"] {
            assert!(layer.get(key).is_some(), "missing layer key {key}");
        }
        assert_eq!(json["layers"][0]["placement"][0]["at_s"], 0.0);
        assert_eq!(json["outro"]["at_s"], 55.0);

        // A doc WITHOUT markers still parses (the field is additive).
        let mut bare = json.clone();
        bare.as_object_mut().unwrap().remove("markers");
        let back: CompositionDoc = serde_json::from_value(bare).unwrap();
        assert!(back.markers.is_empty());
    }

    // ── validation ───────────────────────────────────────────────────────────

    #[test]
    fn validates_a_well_formed_doc() {
        assert!(validate_composition(&doc()).is_ok());
    }

    #[test]
    fn rejects_wrong_version_and_empty_layers() {
        let mut d = doc();
        d.version = 2;
        assert!(validate_composition(&d).is_err());

        let mut d = doc();
        d.layers.clear();
        assert!(validate_composition(&d).is_err());
    }

    #[test]
    fn rejects_unsorted_or_degenerate_timing() {
        let mut d = doc();
        d.layers[0].segments.reverse();
        assert!(validate_composition(&d).is_err());

        let mut d = doc();
        d.layers[0].placement[0].w = 0;
        assert!(validate_composition(&d).is_err());

        let mut d = doc();
        d.layers[0].segments[0].duration_s = 0.0;
        assert!(validate_composition(&d).is_err());

        let mut d = doc();
        d.duration_seconds = f64::NAN;
        assert!(validate_composition(&d).is_err());
    }

    #[test]
    fn collects_referenced_assets() {
        let ids: Vec<Uuid> = referenced_assets(&doc())
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        assert!(ids.contains(&Uuid::from_u128(10))); // audio
        assert!(ids.contains(&Uuid::from_u128(12))); // outro
        assert!(ids.contains(&Uuid::from_u128(1))); // layer
        assert!(ids.contains(&Uuid::from_u128(11))); // matte
    }

    // ── exporter ─────────────────────────────────────────────────────────────

    #[test]
    fn otio_exports_one_track_per_layer_plus_outro_and_audio() {
        let (bytes, mime) =
            compose_timeline_from_doc(&doc(), "", 48_000, TimelineFormat::Otio).unwrap();
        assert_eq!(mime, "application/json");
        let otio: Value = serde_json::from_slice(&bytes).expect("valid JSON");
        let tracks = otio["tracks"]["children"].as_array().expect("tracks");
        // 2 layers + outro + audio.
        assert_eq!(tracks.len(), 4, "tracks: {tracks:?}");

        let clips_of = |t: &Value| -> Vec<Value> {
            t["children"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["OTIO_SCHEMA"] == "Clip.2")
                .cloned()
                .collect()
        };

        // The segmented layer carries one clip per segment, placement stamped
        // on the first.
        let pads = tracks.iter().find(|t| t["name"] == "pads").expect("pads");
        let clips = clips_of(pads);
        assert_eq!(clips.len(), 2);
        let placement = clips[0]["metadata"]["dapple:placement"]
            .as_str()
            .expect("placement metadata");
        assert!(placement.contains("\"at_s\":0.0") || placement.contains("\"at_s\":0"));

        // The continuous layer is one clip with loop + matte metadata.
        let fg = tracks
            .iter()
            .find(|t| t["name"] == "foreground")
            .expect("fg");
        let fclips = clips_of(fg);
        assert_eq!(fclips.len(), 1);
        assert_eq!(fclips[0]["metadata"]["dapple:loop"], "true");
        assert!(fclips[0]["metadata"]["dapple:matte"].as_str().is_some());

        // Outro + audio + markers.
        assert!(tracks.iter().any(|t| t["name"] == "outro"));
        let audio = tracks.iter().find(|t| t["kind"] == "Audio").expect("audio");
        assert_eq!(clips_of(audio).len(), 1);
        let markers = otio["tracks"]["markers"].as_array().expect("markers");
        assert_eq!(markers.len(), 1);
    }

    #[test]
    fn export_is_deterministic() {
        let a = compose_timeline_from_doc(&doc(), "", 48_000, TimelineFormat::Otio)
            .unwrap()
            .0;
        let b = compose_timeline_from_doc(&doc(), "", 48_000, TimelineFormat::Otio)
            .unwrap()
            .0;
        assert_eq!(a, b);
    }
}
