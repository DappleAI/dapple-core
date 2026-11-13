//! Build an editable OTIO timeline document from generated A/V pieces.
//!
//! This is the A/V analog of [`crate::service::image::compose_svg`]: it takes the
//! deterministic arrangement from a [`TimelineStrategy`] and serializes it as a
//! self-contained, hand-editable document — here an OpenTimelineIO (OTIO) JSON
//! timeline instead of an SVG. Each generated piece becomes a clip that
//! **references** its source media by URL/path (an OTIO `ExternalReference`); the
//! bytes are never read, decoded, or re-encoded. The operator opens the result in
//! a real NLE and finishes the cut.
//!
//! We use OxiMedia's `OtioExporter`, whose output is pure `serde_json` — none of
//! OxiMedia's codec/mux crates are touched at build or run time.

use std::path::PathBuf;

use oximedia_core::Rational;
use oximedia_timeline::fcpxml_export::{FcpxmlExportOptions, FcpxmlExporter};
use oximedia_timeline::otio_export::{OtioExportOptions, OtioExporter};
use oximedia_timeline::timeline_exporter::{EdlExportOptions, TimelineExporter};
use oximedia_timeline::types::Position;
use oximedia_timeline::{Clip, Marker, MediaSource, Timeline};

use super::strategy::{ClipKind, MeasuredClip, TimelineParams, TimelineStrategy};
use crate::error::OximediaError;

/// Output serialization of the composed timeline. OTIO/JSON is the portable
/// default; FCPXML and EDL are additional editor-native interchange documents.
/// AAF is deliberately *not* here — see [`TimelineFormat::is_known_unsupported`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineFormat {
    /// OpenTimelineIO JSON — the portable, editor-agnostic interchange document.
    Otio,
    /// Final Cut Pro XML (FCPXML v1.11) — resources + spine + markers.
    Fcpxml,
    /// CMX 3600 EDL — the venerable plain-text edit-decision list.
    Edl,
}

impl TimelineFormat {
    /// Resolve a format by name. `""`/`"otio"`/`"json"` → OTIO, `"fcpxml"`/
    /// `"fcpx"`/`"fcp"` → FCPXML, `"edl"`/`"cmx"`/`"cmx3600"` → EDL. `None` for
    /// an unknown name (the handler turns that into a 4xx). A *known-but-gated*
    /// name (see [`Self::is_known_unsupported`]) also returns `None`; the handler
    /// distinguishes the two to give an honest error.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "" | "otio" | "json" => Some(Self::Otio),
            "fcpxml" | "fcpx" | "fcp" => Some(Self::Fcpxml),
            "edl" | "cmx" | "cmx3600" => Some(Self::Edl),
            _ => None,
        }
    }

    /// Names OxiMedia 0.1.8 exposes an exporter *symbol* for but that we do NOT
    /// ship, so the handler can return an honest "not supported at oximedia
    /// 0.1.8" 4xx instead of a generic "unknown format".
    ///
    /// Only `aaf` today: AAF is a binary SMPTE ST 377-1 Structured-Storage
    /// container produced solely by a file-writing path in the immature
    /// `oximedia-aaf` helper crate (there is no string exporter, and its binary
    /// validity can't be verified by a unit test here). Shipping it would risk a
    /// broken `.aaf` file, so it is gated off per the "don't ship a broken file"
    /// rule.
    pub fn is_known_unsupported(name: &str) -> bool {
        matches!(name.trim().to_ascii_lowercase().as_str(), "aaf")
    }

    /// The mime type the produced asset is stamped with.
    pub fn mime(self) -> &'static str {
        match self {
            // OTIO is a JSON dialect; use the generic JSON mime so the existing
            // content endpoint / mime handling serves it without special-casing.
            Self::Otio => "application/json",
            // FCPXML is an XML document.
            Self::Fcpxml => "application/xml",
            // CMX 3600 EDL is plain text.
            Self::Edl => "text/plain",
        }
    }
}

/// A musical/emotional turning point to drop onto the composed timeline as a
/// marker. This is decoupled from the audio-analysis
/// [`crate::service::audio_analysis::Shift`] (the handler maps a song map's
/// shifts into these) so `av::timeline` stays independent of the analysis
/// types — mirroring how [`TimelinePiece`] decouples it from asset rows. The
/// second→frame conversion is deferred to [`compose_timeline`], which owns the
/// timeline fps.
pub struct ShiftMarker {
    /// Time of the turning point, in seconds.
    pub t: f64,
    /// Marker label (from the shift's [`crate::service::audio_analysis::ShiftKind`],
    /// e.g. `energy_peak`).
    pub name: String,
    /// Human-readable note (stem + strength) shown in the NLE. A plain `String`,
    /// so it stays deterministic (unlike a `HashMap` of metadata, whose JSON key
    /// order is not guaranteed).
    pub comment: String,
}

/// One generated piece to place on the timeline.
pub struct TimelinePiece {
    /// Which track family (derived from the source asset's `asset_kind`).
    pub kind: ClipKind,
    /// Human-readable clip name (shown in the NLE).
    pub name: String,
    /// The reference the operator's NLE resolves — the asset content URL/path.
    /// Stored verbatim as the OTIO clip's `ExternalReference.target_url`.
    pub media_ref: String,
    /// Clip length in timeline frames.
    pub duration_frames: i64,
}

fn av_err(msg: impl std::fmt::Display) -> OximediaError {
    OximediaError::AvError(msg.to_string())
}

/// Assemble `pieces` into a timeline with `strategy`, drop `markers` (song-map
/// shifts) onto it, and serialize to `format`. Returns `(bytes, mime_type)`.
/// Pure and deterministic — same inputs ⇒ byte-identical output. No media bytes
/// are read; clips reference `media_ref`. An empty `markers` slice reproduces the
/// pre-marker behavior exactly.
#[allow(clippy::too_many_arguments)]
pub fn compose_timeline(
    name: &str,
    fps_num: i32,
    fps_den: i32,
    sample_rate: u32,
    pieces: &[TimelinePiece],
    markers: &[ShiftMarker],
    strategy: &dyn TimelineStrategy,
    params: &TimelineParams,
    format: TimelineFormat,
) -> Result<(Vec<u8>, &'static str), OximediaError> {
    if pieces.is_empty() {
        return Err(OximediaError::InvalidRequest(
            "compose-av requires at least one piece".to_string(),
        ));
    }

    // 1. Measure + arrange (pure temporal layout).
    let measured: Vec<MeasuredClip> = pieces
        .iter()
        .enumerate()
        .map(|(i, p)| MeasuredClip {
            index: i,
            kind: p.kind,
            duration_frames: p.duration_frames,
        })
        .collect();
    let placements = strategy.arrange(&measured, params);

    // 2. Build the timeline. One video + one audio track (the sequence strategy's
    //    two families); tracks are created up front so both always appear in the
    //    export even if a family has no clips.
    // `oximedia_core::Rational::new` takes i64; our fps is i32 (fits losslessly).
    let mut timeline = Timeline::new(
        name,
        Rational::new(i64::from(fps_num), i64::from(fps_den)),
        sample_rate,
    )
    .map_err(av_err)?;
    let video_track = timeline.add_video_track("V1").map_err(av_err)?;
    let audio_track = timeline.add_audio_track("A1").map_err(av_err)?;

    // 3. Materialize each placement as a clip referencing its source media.
    for placement in &placements {
        let piece = pieces
            .get(placement.index)
            .ok_or_else(|| OximediaError::Internal("placement index out of range".to_string()))?;

        let source = MediaSource::file(PathBuf::from(&piece.media_ref));
        let clip = Clip::new(
            piece.name.clone(),
            source,
            Position::new(placement.source_in),
            Position::new(placement.source_out),
            Position::new(placement.timeline_in),
        )
        .map_err(av_err)?;

        let track = match placement.kind {
            ClipKind::Video => video_track,
            ClipKind::Audio => audio_track,
        };
        timeline.add_clip(track, clip).map_err(av_err)?;
    }

    // 4. Drop song-map shifts onto the timeline as markers.
    //
    // We use OxiMedia's native marker API (`timeline.markers`, a
    // `MarkerCollection`) rather than post-hoc JSON injection: `OtioExporter`
    // reads `timeline.markers.markers()` into the OTIO `tracks.markers` array
    // (verified by the crate's own `test_otio_export_with_markers`) and
    // `FcpxmlExporter` emits `<marker>` elements from the same collection, so the
    // crate API round-trips cleanly through both exporters. EDL (CMX 3600) has no
    // marker concept and silently drops them — expected for that lossy format.
    // The OTIO stack (`tracks.markers`) is the whole-timeline home, which is the
    // natural anchor for a song-driven turning point. `add_marker` stable-sorts by
    // frame position, so ties preserve the caller's shift order — keeping output
    // deterministic.
    let fps_ratio = f64::from(fps_num) / f64::from(fps_den);
    for m in markers {
        let frame = (m.t * fps_ratio).round().max(0.0) as i64;
        let marker =
            Marker::new(Position::new(frame), m.name.clone()).with_comment(m.comment.clone());
        timeline.markers.add_marker(marker);
    }

    // 5. Serialize to the requested interchange format.
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
            // EDL timecodes need an integer display fps; round num/den.
            let display_fps = (fps_ratio.round().max(1.0)) as u32;
            let opts = EdlExportOptions {
                title: name.to_string(),
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
    use crate::service::av::strategy::strategy_for;
    use serde_json::Value;

    fn piece(kind: ClipKind, name: &str, url: &str, dur: i64) -> TimelinePiece {
        TimelinePiece {
            kind,
            name: name.to_string(),
            media_ref: url.to_string(),
            duration_frames: dur,
        }
    }

    fn shift(t: f64, name: &str) -> ShiftMarker {
        ShiftMarker {
            t,
            name: name.to_string(),
            comment: format!("{name} note"),
        }
    }

    #[test]
    fn format_parse_and_mime() {
        assert_eq!(TimelineFormat::parse("otio"), Some(TimelineFormat::Otio));
        assert_eq!(TimelineFormat::parse(""), Some(TimelineFormat::Otio));
        assert_eq!(TimelineFormat::parse("JSON"), Some(TimelineFormat::Otio));
        assert_eq!(
            TimelineFormat::parse("fcpxml"),
            Some(TimelineFormat::Fcpxml)
        );
        assert_eq!(TimelineFormat::parse("FCPX"), Some(TimelineFormat::Fcpxml));
        assert_eq!(TimelineFormat::parse("edl"), Some(TimelineFormat::Edl));
        assert_eq!(TimelineFormat::parse("cmx3600"), Some(TimelineFormat::Edl));
        assert_eq!(TimelineFormat::parse("mp4"), None);
        // AAF is a recognized-but-gated format, not an unknown one.
        assert_eq!(TimelineFormat::parse("aaf"), None);
        assert!(TimelineFormat::is_known_unsupported("aaf"));
        assert!(TimelineFormat::is_known_unsupported(" AAF "));
        assert!(!TimelineFormat::is_known_unsupported("mp4"));
        assert_eq!(TimelineFormat::Otio.mime(), "application/json");
        assert_eq!(TimelineFormat::Fcpxml.mime(), "application/xml");
        assert_eq!(TimelineFormat::Edl.mime(), "text/plain");
    }

    #[test]
    fn empty_pieces_is_a_bad_request() {
        let strat = strategy_for("sequence").unwrap();
        let err = compose_timeline(
            "empty",
            24,
            1,
            48000,
            &[],
            &[],
            strat.as_ref(),
            &TimelineParams::default(),
            TimelineFormat::Otio,
        )
        .unwrap_err();
        assert!(matches!(err, OximediaError::InvalidRequest(_)));
    }

    #[test]
    fn composes_valid_otio_with_two_tracks_and_media_refs() {
        let strat = strategy_for("sequence").unwrap();
        let pieces = vec![
            piece(
                ClipKind::Video,
                "shot 1",
                "https://api.dapple.x/api/assets/vid-1/content",
                120,
            ),
            piece(
                ClipKind::Audio,
                "narration",
                "https://api.dapple.x/api/assets/aud-1/content",
                240,
            ),
        ];
        let (bytes, mime) = compose_timeline(
            "My Reel",
            24,
            1,
            48000,
            &pieces,
            &[],
            strat.as_ref(),
            &TimelineParams::default(),
            TimelineFormat::Otio,
        )
        .unwrap();
        assert_eq!(mime, "application/json");

        // Re-parse the OTIO document and assert its shape.
        let doc: Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(doc["OTIO_SCHEMA"], "Timeline.1");
        assert_eq!(doc["name"], "My Reel");

        let tracks = doc["tracks"]["children"].as_array().expect("tracks array");
        assert_eq!(tracks.len(), 2, "one video + one audio track");

        let find_kind = |kind: &str| {
            tracks
                .iter()
                .find(|t| t["kind"] == kind)
                .unwrap_or_else(|| panic!("missing {kind} track"))
        };
        let video = find_kind("Video");
        let audio = find_kind("Audio");

        // The video track's first clip references our video asset URL verbatim.
        let vclip = &video["children"].as_array().unwrap()[0];
        assert_eq!(vclip["OTIO_SCHEMA"], "Clip.2");
        assert_eq!(vclip["name"], "shot 1");
        assert_eq!(
            vclip["media_reference"]["target_url"],
            "https://api.dapple.x/api/assets/vid-1/content"
        );
        assert_eq!(vclip["source_range"]["duration"]["value"], 120.0);

        let aclip = &audio["children"].as_array().unwrap()[0];
        assert_eq!(
            aclip["media_reference"]["target_url"],
            "https://api.dapple.x/api/assets/aud-1/content"
        );

        // With no shifts supplied, the OTIO stack carries an empty markers array
        // (pre-marker behavior unchanged).
        assert_eq!(
            doc["tracks"]["markers"].as_array().map(Vec::len),
            Some(0),
            "absent song map ⇒ no markers"
        );
    }

    #[test]
    fn shifts_become_otio_markers() {
        let strat = strategy_for("sequence").unwrap();
        let pieces = vec![piece(ClipKind::Audio, "bed", "u/bed", 480)];
        // 24 fps ⇒ frame = round(t * 24). 2.0s→48, 5.5s→132.
        let markers = vec![shift(2.0, "energy_peak"), shift(5.5, "silence")];
        let (bytes, _) = compose_timeline(
            "Marked",
            24,
            1,
            48000,
            &pieces,
            &markers,
            strat.as_ref(),
            &TimelineParams::default(),
            TimelineFormat::Otio,
        )
        .unwrap();

        let doc: Value = serde_json::from_slice(&bytes).expect("valid JSON");
        let mk = doc["tracks"]["markers"].as_array().expect("markers array");
        assert_eq!(mk.len(), 2, "one marker per shift");

        // add_marker sorts by frame, so order is (48, 132) here.
        assert_eq!(mk[0]["OTIO_SCHEMA"], "Marker.2");
        assert_eq!(mk[0]["name"], "energy_peak");
        assert_eq!(mk[0]["marked_range"]["start_time"]["value"], 48.0);
        assert_eq!(mk[1]["name"], "silence");
        assert_eq!(mk[1]["marked_range"]["start_time"]["value"], 132.0);
    }

    #[test]
    fn deterministic_byte_identical_with_markers() {
        let strat = strategy_for("sequence").unwrap();
        let pieces = vec![
            piece(ClipKind::Video, "a", "u/a", 100),
            piece(ClipKind::Video, "b", "u/b", 50),
        ];
        // Two shifts that land on the same frame exercise the stable-sort tie path.
        let markers = vec![
            shift(1.0, "entrance"),
            shift(1.0, "energy_peak"),
            shift(3.25, "exit"),
        ];
        let run = || {
            compose_timeline(
                "R",
                24,
                1,
                48000,
                &pieces,
                &markers,
                strat.as_ref(),
                &TimelineParams::default(),
                TimelineFormat::Otio,
            )
            .unwrap()
            .0
        };
        assert_eq!(run(), run(), "same inputs must give byte-identical OTIO");
    }

    #[test]
    fn deterministic_without_markers_matches_pre_marker_shape() {
        // Passing an empty markers slice reproduces the old behavior byte-for-byte
        // across runs.
        let strat = strategy_for("sequence").unwrap();
        let pieces = vec![piece(ClipKind::Video, "a", "u/a", 100)];
        let run = || {
            compose_timeline(
                "R",
                24,
                1,
                48000,
                &pieces,
                &[],
                strat.as_ref(),
                &TimelineParams::default(),
                TimelineFormat::Otio,
            )
            .unwrap()
            .0
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn fcpxml_export_emits_clip_and_marker() {
        let strat = strategy_for("sequence").unwrap();
        let pieces = vec![piece(
            ClipKind::Video,
            "shot 1",
            "https://api.dapple.x/api/assets/vid-1/content",
            120,
        )];
        let markers = vec![shift(1.0, "energy_peak")];
        let (bytes, mime) = compose_timeline(
            "My Reel",
            24,
            1,
            48000,
            &pieces,
            &markers,
            strat.as_ref(),
            &TimelineParams::default(),
            TimelineFormat::Fcpxml,
        )
        .unwrap();
        assert_eq!(mime, "application/xml");
        let xml = String::from_utf8(bytes).expect("fcpxml is utf-8");
        assert!(xml.contains("<fcpxml"), "missing fcpxml root: {xml}");
        assert!(xml.contains("</fcpxml>"));
        assert!(xml.contains("My Reel"), "missing project name");
        assert!(xml.contains("shot 1"), "missing clip");
        // FCPXML carries the marker (via the same MarkerCollection).
        assert!(xml.contains("energy_peak"), "missing marker: {xml}");
    }

    #[test]
    fn edl_export_emits_cmx3600_header_and_clip() {
        let strat = strategy_for("sequence").unwrap();
        let pieces = vec![piece(
            ClipKind::Video,
            "shot 1",
            "https://api.dapple.x/api/assets/vid-1/content",
            120,
        )];
        // EDL has no marker concept; supplying shifts must not break the export.
        let markers = vec![shift(1.0, "energy_peak")];
        let (bytes, mime) = compose_timeline(
            "My Reel",
            24,
            1,
            48000,
            &pieces,
            &markers,
            strat.as_ref(),
            &TimelineParams::default(),
            TimelineFormat::Edl,
        )
        .unwrap();
        assert_eq!(mime, "text/plain");
        let edl = String::from_utf8(bytes).expect("edl is utf-8");
        assert!(edl.starts_with("TITLE: My Reel"), "missing title: {edl}");
        assert!(edl.contains("FCM:"), "missing FCM line");
        assert!(
            edl.contains("FROM CLIP NAME: shot 1"),
            "missing clip event: {edl}"
        );
    }

    #[test]
    fn edl_and_fcpxml_are_deterministic() {
        let strat = strategy_for("sequence").unwrap();
        let pieces = vec![piece(ClipKind::Video, "a", "u/a", 100)];
        let markers = vec![shift(1.0, "entrance")];
        for fmt in [TimelineFormat::Edl, TimelineFormat::Fcpxml] {
            let run = || {
                compose_timeline(
                    "R",
                    24,
                    1,
                    48000,
                    &pieces,
                    &markers,
                    strat.as_ref(),
                    &TimelineParams::default(),
                    fmt,
                )
                .unwrap()
                .0
            };
            assert_eq!(run(), run(), "{fmt:?} must be byte-identical");
        }
    }
}
