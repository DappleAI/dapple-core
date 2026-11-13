//! Shell-outs to the bundled `yt-dlp` and `ffmpeg` binaries.
//!
//! These are runtime binaries baked into the service's Docker image (never cargo
//! deps). `yt-dlp` handles Twitch HLS download + metadata; `ffmpeg` handles audio
//! extraction and (later, Stage E) clip cutting. All external-process spawning
//! lives here in Rust — the flow nushell layer cannot shell out.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio::process::Command;

use crate::error::MediaGatewayError;

/// A VOD chapter (game/segment), as reported by `yt-dlp --dump-single-json`.
#[derive(Clone, Debug, Serialize)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// Lightweight probe of a VOD: numeric duration + chapters. `yt-dlp` reports a
/// numeric `duration` (seconds) that is more convenient than Helix's `"3h2m1s"`
/// string, and it is the only source of Twitch chapter data (Helix has none).
#[derive(Clone, Debug, Serialize)]
pub struct VodProbe {
    pub duration_secs: f64,
    pub title: String,
    pub chapters: Vec<Chapter>,
}

/// Run `program args...`, returning captured stdout. A non-zero exit becomes a
/// `MediaTool` error carrying the tail of stderr (operator-visible).
async fn run(program: &str, args: &[&str]) -> Result<String, MediaGatewayError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| MediaGatewayError::MediaTool(format!("failed to spawn {program}: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = stderr.lines().collect();
        let start = lines.len().saturating_sub(8);
        let tail = lines[start..].join("\n");
        return Err(MediaGatewayError::MediaTool(format!(
            "{program} exited with {}: {tail}",
            output.status
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Probe a media file's duration in seconds via `ffprobe` (bundled alongside ffmpeg in
/// the Docker runtime). Used to size the operator's card-audio track so the intro cards
/// can advance through it one card-length at a time (see `assemble::card_audio_offset`).
pub async fn probe_duration(path: &Path) -> Result<f64, MediaGatewayError> {
    let p = path_str(path)?;
    let out = run(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nokey=1:noprint_wrappers=1",
            p,
        ],
    )
    .await?;
    out.trim().parse::<f64>().map_err(|e| {
        MediaGatewayError::MediaTool(format!("ffprobe duration parse ({}): {e}", out.trim()))
    })
}

/// Bounded yt-dlp retry flags shared by the download paths — ride out transient
/// Twitch-CDN and file-access hiccups (`--file-access-retries` directly covers the
/// fragment `.part` rename that fails in the wild). BOUNDED on purpose: `run` has no
/// wall-clock timeout, so an `infinite` here could hang the handler forever on a
/// genuinely dead fragment.
const YTDLP_RETRY_FLAGS: &[&str] = &[
    "--retries",
    "10",
    "--fragment-retries",
    "20",
    "--file-access-retries",
    "10",
];

/// How many times a whole yt-dlp download is re-attempted from a clean slate before
/// giving up. Complements `YTDLP_RETRY_FLAGS` (which retry *within* one yt-dlp run):
/// this catches a whole-run transient failure (CDN reset, extractor hiccup). Bounded.
const DOWNLOAD_ATTEMPTS: u32 = 3;

/// Fixed backoff between whole-download retry attempts.
const DOWNLOAD_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);

/// Run a yt-dlp download into `dest_dir`, retrying a transient whole-run failure up to
/// `DOWNLOAD_ATTEMPTS` times. `prefix` is the output-template stem (`"source."` /
/// `"clip."`); any partial artifacts a failed attempt left are cleared before the next
/// so each retry starts clean. On success returns the produced file — the first
/// `dest_dir` entry whose name starts with `prefix` and is not a `.part`/`.ytdl` temp.
async fn run_download_with_retries(
    args: &[&str],
    dest_dir: &Path,
    prefix: &str,
    what: &str,
) -> Result<PathBuf, MediaGatewayError> {
    let mut last_err: Option<MediaGatewayError> = None;
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match run("yt-dlp", args).await {
            Ok(_) => match find_produced(dest_dir, prefix).await? {
                Some(path) => return Ok(path),
                None => {
                    last_err = Some(MediaGatewayError::MediaTool(format!(
                        "yt-dlp produced no {what} file"
                    )));
                }
            },
            Err(e) => {
                log::warn!(
                    "yt-dlp {what} download attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed: {e}"
                );
                last_err = Some(e);
            }
        }
        // Clear this attempt's partials so the next retry starts clean.
        clear_download_partials(dest_dir, prefix).await;
        if attempt < DOWNLOAD_ATTEMPTS {
            tokio::time::sleep(DOWNLOAD_RETRY_BACKOFF).await;
        }
    }
    Err(last_err
        .unwrap_or_else(|| MediaGatewayError::MediaTool(format!("yt-dlp {what} download failed"))))
}

/// Find the final produced file in `dest_dir`: the first entry whose name starts with
/// `prefix` (`"source."` / `"clip."`) and is NOT a fragment/resume temp (`.part*` /
/// `.ytdl`). Excluding those never returns a half-written fragment.
async fn find_produced(
    dest_dir: &Path,
    prefix: &str,
) -> Result<Option<PathBuf>, MediaGatewayError> {
    let mut rd = tokio::fs::read_dir(dest_dir)
        .await
        .map_err(|e| MediaGatewayError::Internal(format!("scratch read_dir: {e}")))?;
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|e| MediaGatewayError::Internal(format!("scratch read_dir: {e}")))?
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && !name.contains(".part") && !name.ends_with(".ytdl") {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// Best-effort removal of every artifact in `dest_dir` whose name starts with `prefix`
/// (the produced file plus any `.part`/fragment/`.ytdl` temps a failed attempt left), so
/// the next download retry starts from a clean slate. Errors are ignored.
async fn clear_download_partials(dest_dir: &Path, prefix: &str) {
    let Ok(mut rd) = tokio::fs::read_dir(dest_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// Probe a VOD's duration + chapters via `yt-dlp --dump-single-json`.
pub async fn probe(url: &str) -> Result<VodProbe, MediaGatewayError> {
    let json = run(
        "yt-dlp",
        &["--dump-single-json", "--no-warnings", "--no-playlist", url],
    )
    .await?;

    let value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| MediaGatewayError::MediaTool(format!("yt-dlp json parse: {e}")))?;

    let duration_secs = value
        .get("duration")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let chapters = value
        .get("chapters")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(Chapter {
                        start: c.get("start_time")?.as_f64()?,
                        end: c
                            .get("end_time")
                            .and_then(serde_json::Value::as_f64)
                            .unwrap_or(0.0),
                        title: c
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(VodProbe {
        duration_secs,
        title,
        chapters,
    })
}

/// Pass-1 download: fetch the audio-only rendition (tiny) into `dest_dir` and
/// return the produced file path. Falls back to the worst video rendition when a
/// stream has no dedicated audio track (audio is extracted downstream by ffmpeg).
pub async fn download_audio(url: &str, dest_dir: &Path) -> Result<PathBuf, MediaGatewayError> {
    let template = dest_dir.join("source.%(ext)s");
    let template = template
        .to_str()
        .ok_or_else(|| MediaGatewayError::Internal("non-utf8 scratch path".to_string()))?;

    let mut args: Vec<&str> = vec![
        "-f",
        "ba/audio_only/worst",
        "--no-playlist",
        "--no-warnings",
    ];
    args.extend_from_slice(YTDLP_RETRY_FLAGS);
    args.extend_from_slice(&["-o", template, url]);

    run_download_with_retries(&args, dest_dir, "source.", "source audio").await
}

/// Best-effort caption fetch: ask yt-dlp for an English subtitle track (skipping
/// the video) into `dest_dir`. Returns the produced `.vtt`/`.srt` path, or `None`
/// when the VOD has no caption track (the common Twitch case — not an error).
pub async fn download_subs(
    url: &str,
    dest_dir: &Path,
) -> Result<Option<PathBuf>, MediaGatewayError> {
    let template = dest_dir.join("subs.%(ext)s");
    let template = template
        .to_str()
        .ok_or_else(|| MediaGatewayError::Internal("non-utf8 scratch path".to_string()))?;

    // Missing subs make yt-dlp exit non-zero — that is expected, so ignore it and
    // decide purely on whether a subtitle file was written.
    let _ = run(
        "yt-dlp",
        &[
            "--skip-download",
            "--write-subs",
            "--sub-langs",
            "en.*",
            "--sub-format",
            "vtt/srt",
            "--no-warnings",
            "-o",
            template,
            url,
        ],
    )
    .await;

    let mut rd = tokio::fs::read_dir(dest_dir)
        .await
        .map_err(|e| MediaGatewayError::Internal(format!("scratch read_dir: {e}")))?;
    while let Some(entry) = rd
        .next_entry()
        .await
        .map_err(|e| MediaGatewayError::Internal(format!("scratch read_dir: {e}")))?
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("subs.") && (name.ends_with(".vtt") || name.ends_with(".srt")) {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// Pass-2 ranged download: fetch ONLY the `[start, end]` window at up to `height`
/// pixels tall, frame-accurately cut and remuxed to mp4, into `dest_dir`. Returns
/// the produced clip path. This is the only place the actual video bytes are
/// pulled, and only for approved highlight ranges.
pub async fn download_clip(
    url: &str,
    start: f64,
    end: f64,
    height: u32,
    dest_dir: &Path,
) -> Result<PathBuf, MediaGatewayError> {
    let template = dest_dir.join("clip.%(ext)s");
    let template = template
        .to_str()
        .ok_or_else(|| MediaGatewayError::Internal("non-utf8 scratch path".to_string()))?;
    let section = format!("*{start}-{end}");
    let format = if height >= 100_000 {
        "bv*+ba/b".to_string()
    } else {
        format!("bv*[height<={height}]+ba/b[height<={height}]/b")
    };

    let mut args: Vec<&str> = vec![
        "-f",
        &format,
        "--download-sections",
        &section,
        "--force-keyframes-at-cuts",
        "--merge-output-format",
        "mp4",
        "--no-playlist",
        "--no-warnings",
    ];
    args.extend_from_slice(YTDLP_RETRY_FLAGS);
    args.extend_from_slice(&["-o", template, url]);

    run_download_with_retries(&args, dest_dir, "clip.", "clip").await
}

/// Transcode `input` to a 16 kHz mono 16-bit PCM WAV at `out` (whisper's expected
/// input shape).
pub async fn to_wav_16k_mono(input: &Path, out: &Path) -> Result<(), MediaGatewayError> {
    let input = input
        .to_str()
        .ok_or_else(|| MediaGatewayError::Internal("non-utf8 input path".to_string()))?;
    let out_s = out
        .to_str()
        .ok_or_else(|| MediaGatewayError::Internal("non-utf8 output path".to_string()))?;

    run(
        "ffmpeg",
        &[
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            input,
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            out_s,
        ],
    )
    .await?;
    Ok(())
}

/// Alpha of the black scrim laid over a still-frame card background: the footage reads
/// through at roughly `1 - CARD_FRAME_SCRIM_ALPHA`, dimmed enough that the white card
/// text stays legible over bright gameplay frames. Tunable.
const CARD_FRAME_SCRIM_ALPHA: f64 = 0.5;

/// Render a short **text intro card** (`text_file`'s contents, centered in white),
/// `dur_secs` long, with a stereo audio track (the operator's uploaded `audio` when
/// supplied, else silent) — normalized to the same `width`x`height`@`fps`/H.264/AAC
/// shape as the clips so the segments concat cleanly. The background is a solid
/// `color_hex` (`0xRRGGBB`), or — when `bg_frame` is `Some` — a still frame from the
/// footage dimmed by a black scrim so the white text stays legible. When `logo` is
/// `Some`, the PNG is scaled and composited bottom-center (the DappleAi credit).
/// `text_file` is used via drawtext `textfile` (renders literal newlines) to sidestep
/// drawtext escaping.
#[allow(clippy::too_many_arguments)]
pub async fn render_card(
    text_file: &Path,
    color_hex: &str,
    width: u32,
    height: u32,
    fps: u32,
    dur_secs: f64,
    font_file: &str,
    logo: Option<&Path>,
    audio: Option<&Path>,
    // Seek this many seconds into `audio` before laying it under the card (input-level
    // `-ss`). `0.0` plays from the head (today's behavior). Lets the reel advance through
    // one card-audio track one card-length at a time — see `assemble::card_audio_offset`.
    audio_offset_secs: f64,
    bg_frame: Option<&Path>,
    // Line count of the (already-wrapped) caption in `text_file`, used only to place the
    // optional timestamp line just below the vertically-centered caption block.
    caption_lines: usize,
    // When `Some`, a small "overly exact" clip-start timestamp is drawn centered directly
    // under the caption (its own `textfile`, so colons need no escaping).
    timestamp_file: Option<&Path>,
    out: &Path,
) -> Result<(), MediaGatewayError> {
    let text_file = path_str(text_file)?;
    let out_s = path_str(out)?;
    let fontsize = (height / 16).max(20);
    let line_spacing = 16u32;
    let drawtext = format!(
        "drawtext=fontfile={font_file}:textfile={text_file}:fontcolor=white:fontsize={fontsize}:line_spacing={line_spacing}:x=(w-text_w)/2:y=(h-text_h)/2"
    );
    // Optional timestamp drawtext, placed just under the centered caption block. The caption
    // block (height `cap_h`) is centered, so its bottom edge is `(h+cap_h)/2`; drop the
    // timestamp `gap` px below that, independently horizontally centered.
    let timestamp_path = match timestamp_file {
        Some(p) => Some(path_str(p)?.to_string()),
        None => None,
    };
    let ts_drawtext = timestamp_path.as_ref().map(|tp| {
        let ts_fontsize = (fontsize * 3 / 5).max(20);
        let cap_lines = caption_lines.max(1) as u32;
        let cap_h = cap_lines * fontsize + cap_lines.saturating_sub(1) * line_spacing;
        let gap = (fontsize / 2).max(1);
        format!(
            "drawtext=fontfile={font_file}:textfile={tp}:fontcolor=0xB8C0CC:fontsize={ts_fontsize}:x=(w-text_w)/2:y=(h+{cap_h})/2+{gap}"
        )
    });
    let dur = format!("{dur_secs}");
    let fps_s = fps.to_string();
    let anull = "anullsrc=channel_layout=stereo:sample_rate=48000";
    // The operator's uploaded audio, if any, played under the card. A real track is
    // padded with `apad` so a clip shorter than the card still fills the full duration,
    // and `-t` (below) trims a longer one — so the card keeps exactly one AAC stereo
    // 48 kHz stream of `dur_secs`, matching the clips for `-c copy` concat.
    let audio_path = match audio {
        Some(p) => Some(path_str(p)?.to_string()),
        None => None,
    };
    let frame_path = match bg_frame {
        Some(p) => Some(path_str(p)?.to_string()),
        None => None,
    };

    // Inputs in a fixed order, tracked by index for the filtergraph maps.
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
    ];

    // Input 0 (background): a dimmed still frame from the footage, else a solid color.
    let mut next = 0u32;
    let bg_idx = next;
    next += 1;
    match &frame_path {
        Some(p) => args.extend([
            "-loop".into(),
            "1".into(),
            "-t".into(),
            dur.clone(),
            "-i".into(),
            p.clone(),
        ]),
        None => {
            let color = format!("color=c={color_hex}:s={width}x{height}:r={fps}:d={dur_secs}");
            args.extend(["-f".into(), "lavfi".into(), "-i".into(), color]);
        }
    }

    // Optional logo input.
    let logo_idx = if let Some(logo) = logo {
        let logo_s = path_str(logo)?.to_string();
        args.extend(["-loop".into(), "1".into(), "-i".into(), logo_s]);
        let li = next;
        next += 1;
        Some(li)
    } else {
        None
    };

    // Audio input (uploaded track, else a silent stereo source). A positive
    // `audio_offset_secs` seeks into the track first (`-ss` before `-i`), so successive
    // cards play successive slices of one song.
    let audio_idx = next;
    match &audio_path {
        Some(p) if audio_offset_secs > 0.0 => args.extend([
            "-ss".into(),
            format!("{audio_offset_secs}"),
            "-i".into(),
            p.clone(),
        ]),
        Some(p) => args.extend(["-i".into(), p.clone()]),
        None => args.extend(["-f".into(), "lavfi".into(), "-i".into(), anull.into()]),
    }

    // Video filtergraph: dim the frame (or pass the color) -> text -> optional logo.
    let mut chains: Vec<String> = Vec::new();
    match &frame_path {
        Some(_) => chains.push(format!(
            "[{bg_idx}:v]scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},setsar=1,drawbox=x=0:y=0:w={width}:h={height}:color=black@{CARD_FRAME_SCRIM_ALPHA}:t=fill[base]"
        )),
        None => chains.push(format!("[{bg_idx}:v]setsar=1[base]")),
    }
    match &ts_drawtext {
        Some(ts) => {
            chains.push(format!("[base]{drawtext}[cap]"));
            chains.push(format!("[cap]{ts}[txt]"));
        }
        None => chains.push(format!("[base]{drawtext}[txt]")),
    }
    let vmap = if let Some(li) = logo_idx {
        let logo_w = ((f64::from(width) * 0.16) as u32).max(1);
        let margin = (height / 18).max(1);
        chains.push(format!("[{li}:v]scale={logo_w}:-1[logo]"));
        chains.push(format!(
            "[txt][logo]overlay=x=(W-w)/2:y=H-h-{margin},format=yuv420p[v]"
        ));
        "[v]".to_string()
    } else {
        chains.push("[txt]format=yuv420p[v]".to_string());
        "[v]".to_string()
    };
    // Audio: pad an uploaded track to the card length; a silent source is trimmed by `-t`.
    let amap = if audio_path.is_some() {
        chains.push(format!("[{audio_idx}:a]apad[a]"));
        "[a]".to_string()
    } else {
        format!("{audio_idx}:a")
    };

    let filter = chains.join(";");
    args.extend([
        "-filter_complex".into(),
        filter,
        "-map".into(),
        vmap,
        "-map".into(),
        amap,
        "-t".into(),
        dur,
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-preset".into(),
        "veryfast".into(),
        "-r".into(),
        fps_s,
        "-c:a".into(),
        "aac".into(),
        "-ar".into(),
        "48000".into(),
        "-ac".into(),
        "2".into(),
        out_s.to_string(),
    ]);

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run("ffmpeg", &arg_refs).await?;
    Ok(())
}

/// Grab a single still image from a clip for a card background: the FIRST frame
/// (`last = false`) or the LAST frame (`last = true`). Best-effort at the call site.
pub async fn extract_frame(input: &Path, last: bool, out: &Path) -> Result<(), MediaGatewayError> {
    let in_s = path_str(input)?;
    let out_s = path_str(out)?;
    let args: Vec<&str> = if last {
        // Seek to the final second and overwrite the output until the last decoded frame
        // remains (`-frames:v 1` after `-sseof` would grab the FIRST frame of the tail).
        vec![
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-sseof",
            "-1",
            "-i",
            in_s,
            "-update",
            "1",
            "-q:v",
            "2",
            out_s,
        ]
    } else {
        vec![
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            in_s,
            "-frames:v",
            "1",
            "-q:v",
            "2",
            out_s,
        ]
    };
    run("ffmpeg", &args).await?;
    Ok(())
}

/// Run ffmpeg with `args`, discarding stdout. Thin public wrapper over the
/// private `run` so sibling modules (the progression renderer) reuse the same
/// spawn + stderr-tail error shaping.
pub async fn run_ffmpeg(args: &[&str]) -> Result<(), MediaGatewayError> {
    run("ffmpeg", args).await.map(|_| ())
}

/// Re-encode `input` to a uniform `width`x`height`@`fps` H.264/yuv420p + AAC stereo
/// 48 kHz shape (scaled to fit + letterboxed), so it concats cleanly with the cards
/// and the other clips. Preserves the clip's own audio (the streamer's).
pub async fn normalize_video(
    input: &Path,
    width: u32,
    height: u32,
    fps: u32,
    out: &Path,
) -> Result<(), MediaGatewayError> {
    let input = path_str(input)?;
    let out_s = path_str(out)?;
    let vf = format!(
        "scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps={fps}"
    );

    run(
        "ffmpeg",
        &[
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            input,
            "-vf",
            &vf,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-c:a",
            "aac",
            "-ar",
            "48000",
            "-ac",
            "2",
            out_s,
        ],
    )
    .await?;
    Ok(())
}

/// Concatenate `parts` (all sharing codec params, e.g. cards + normalized clips) into
/// `out` via the ffmpeg concat demuxer with stream copy (no re-encode). `list_file`
/// is written with one `file '<path>'` line per part.
pub async fn concat_mp4(
    parts: &[PathBuf],
    list_file: &Path,
    out: &Path,
) -> Result<(), MediaGatewayError> {
    let mut list = String::new();
    for p in parts {
        let s = path_str(p)?;
        // The concat demuxer quotes paths with single quotes; escape any embedded one.
        list.push_str(&format!("file '{}'\n", s.replace('\'', "'\\''")));
    }
    tokio::fs::write(list_file, list)
        .await
        .map_err(|e| MediaGatewayError::Assemble(format!("write concat list: {e}")))?;

    let list_s = path_str(list_file)?;
    let out_s = path_str(out)?;
    run(
        "ffmpeg",
        &[
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            list_s,
            "-c",
            "copy",
            out_s,
        ],
    )
    .await?;
    Ok(())
}

/// Build the `-filter_complex` for a cross-dissolve reel: chain an `xfade`
/// (`transition=dissolve`) on the video and an `acrossfade` on the audio across every
/// consecutive pair of the `durations`-long segments, overlapping each seam by a
/// dissolve. Returns `(filter_complex, video_label, audio_label)`, or `None` when there
/// are fewer than two segments (nothing to dissolve).
///
/// Each seam's dissolve is clamped to half of each adjacent segment
/// (`min(transition, d_prev/2, d_cur/2)`) so a short clip or card can't be over-run by a
/// transition longer than itself; the running offset then stays strictly inside the
/// accumulated stream. Pure (no I/O) so the offset math is unit-tested; `xfade_reel`
/// feeds it probed real durations.
fn build_xfade_filter(durations: &[f64], transition: f64) -> Option<(String, String, String)> {
    if durations.len() < 2 || transition <= 0.0 {
        return None;
    }
    let mut clauses: Vec<String> = Vec::new();
    let mut acc_v = "[0:v]".to_string();
    let mut acc_a = "[0:a]".to_string();
    let mut acc_dur = durations[0];
    for i in 1..durations.len() {
        let seam = transition
            .min(durations[i - 1] * 0.5)
            .min(durations[i] * 0.5)
            .max(0.0);
        let offset = (acc_dur - seam).max(0.0);
        let vl = format!("[vx{i}]");
        let al = format!("[ax{i}]");
        clauses.push(format!(
            "{acc_v}[{i}:v]xfade=transition=dissolve:duration={seam:.3}:offset={offset:.3}{vl}"
        ));
        clauses.push(format!("{acc_a}[{i}:a]acrossfade=d={seam:.3}{al}"));
        acc_v = vl;
        acc_a = al;
        acc_dur += durations[i] - seam;
    }
    Some((clauses.join(";"), acc_v, acc_a))
}

/// Maximum simultaneous inputs per xfade ffmpeg pass. A chained xfade filtergraph
/// demuxes and decodes EVERY input eagerly, buffering later segments' raw frames
/// until their seam is reached — memory scales with total reel length, and a
/// reel-sized part count OOMs the pod (observed: 4Gi limit blown by a ~24-part
/// recap). Folding in levels of at most this many inputs bounds the spike; each
/// frame is re-encoded once per level (veryfast/720p — fine for a recap reel).
const MAX_XFADE_INPUTS: usize = 6;

/// Part counts of each fold level, first entry = the input count, last entry
/// `<= max` (the final pass). Pure so the convergence is unit-tested; `xfade_reel`
/// walks the same arithmetic. `max < 2` cannot converge and is a programming error.
fn fold_level_sizes(n: usize, max: usize) -> Vec<usize> {
    debug_assert!(max >= 2, "fold requires max >= 2 to converge");
    let mut sizes = vec![n];
    let mut n = n;
    while n > max {
        n = n.div_ceil(max);
        sizes.push(n);
    }
    sizes
}

/// Concatenate `parts` (uniform-shape cards + normalized clips) into `out` with a
/// cross-dissolve at every seam: video `xfade=dissolve` + audio `acrossfade`, each
/// `transition_secs` long (per-seam clamped). Re-encodes (a filtergraph can't stream-copy
/// like [`concat_mp4`]), so use this only when a transition is wanted; `transition_secs
/// <= 0` or a single part should route to [`concat_mp4`] instead. Durations are probed so
/// the overlap offsets are exact.
///
/// More than [`MAX_XFADE_INPUTS`] parts fold hierarchically: consecutive chunks of at
/// most `MAX_XFADE_INPUTS` render to intermediates (written next to `out`, i.e. inside
/// the job scratch dir the caller cleans up), which become the next level's parts. The
/// seam between two chunks becomes a seam of the next level's pass over the two
/// intermediates — the same dissolve, exact offsets (every level re-probes real
/// durations) — while memory stays bounded by one chunk's inputs.
pub async fn xfade_reel(
    parts: &[PathBuf],
    transition_secs: f64,
    out: &Path,
) -> Result<(), MediaGatewayError> {
    let levels = fold_level_sizes(parts.len(), MAX_XFADE_INPUTS);
    log::info!(
        "xfade reel: {} parts, {} pass level(s) ({:?})",
        parts.len(),
        levels.len(),
        levels
    );
    let mut level: Vec<PathBuf> = parts.to_vec();
    let mut lvl = 0usize;
    while level.len() > MAX_XFADE_INPUTS {
        lvl += 1;
        let mut next: Vec<PathBuf> = Vec::with_capacity(level.len().div_ceil(MAX_XFADE_INPUTS));
        for (j, chunk) in level.chunks(MAX_XFADE_INPUTS).enumerate() {
            let inter = out.with_extension(format!("lvl{lvl}-{j}.mp4"));
            log::info!(
                "xfade level {lvl}: chunk {}/{} ({} inputs)",
                j + 1,
                level.len().div_ceil(MAX_XFADE_INPUTS),
                chunk.len()
            );
            xfade_pass(chunk, transition_secs, &inter).await?;
            next.push(inter);
        }
        level = next;
    }
    xfade_pass(&level, transition_secs, out).await
}

/// One bounded xfade render: dissolve `parts` (at most [`MAX_XFADE_INPUTS`] of them —
/// callers go through [`xfade_reel`], which folds larger sets) into `out` with a single
/// ffmpeg invocation; 0/1 part falls back to a plain [`concat_mp4`] join.
async fn xfade_pass(
    parts: &[PathBuf],
    transition_secs: f64,
    out: &Path,
) -> Result<(), MediaGatewayError> {
    let mut durations = Vec::with_capacity(parts.len());
    for p in parts {
        durations.push(probe_duration(p).await?);
    }
    let Some((filter, v_label, a_label)) = build_xfade_filter(&durations, transition_secs) else {
        // Nothing to dissolve (0/1 part) — fall back to a plain join.
        let list = out.with_extension("concat.txt");
        return concat_mp4(parts, &list, out).await;
    };

    let out_s = path_str(out)?;
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
    ];
    for p in parts {
        args.push("-i".into());
        args.push(path_str(p)?.to_string());
    }
    args.push("-filter_complex".into());
    args.push(filter);
    args.push("-map".into());
    args.push(v_label);
    args.push("-map".into());
    args.push(a_label);
    args.extend(
        [
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-preset", "veryfast", "-c:a", "aac", "-ar",
            "48000", "-ac", "2", out_s,
        ]
        .iter()
        .map(|s| (*s).to_string()),
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run("ffmpeg", &arg_refs).await?;
    Ok(())
}

fn path_str(p: &Path) -> Result<&str, MediaGatewayError> {
    p.to_str()
        .ok_or_else(|| MediaGatewayError::Internal("non-utf8 path".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xfade_filter_none_below_two_segments() {
        assert!(build_xfade_filter(&[], 0.4).is_none());
        assert!(build_xfade_filter(&[3.0], 0.4).is_none());
        assert!(build_xfade_filter(&[3.0, 2.0], 0.0).is_none());
    }

    #[test]
    fn xfade_filter_chains_offsets_and_labels() {
        // Three segments (card 2.5s, clip 6s, card 2.5s), dissolve 0.4s.
        let (fc, v, a) = build_xfade_filter(&[2.5, 6.0, 2.5], 0.4).unwrap();
        assert_eq!(v, "[vx2]");
        assert_eq!(a, "[ax2]");
        // First seam: offset = 2.5 - 0.4 = 2.1; second: acc = 2.5+6-0.4 = 8.1, offset 7.7.
        assert!(fc.contains("[0:v][1:v]xfade=transition=dissolve:duration=0.400:offset=2.100[vx1]"));
        assert!(fc.contains("[vx1][2:v]xfade=transition=dissolve:duration=0.400:offset=7.700[vx2]"));
        assert!(fc.contains("[0:a][1:a]acrossfade=d=0.400[ax1]"));
        assert!(fc.contains("[ax1][2:a]acrossfade=d=0.400[ax2]"));
    }

    #[test]
    fn xfade_seam_clamped_to_short_segment() {
        // A 0.5s clip between two cards: the dissolve into/out of it clamps to 0.25s
        // (half the short segment), never the full 0.4s, so it can't be over-run.
        let (fc, _, _) = build_xfade_filter(&[2.5, 0.5, 2.5], 0.4).unwrap();
        assert!(fc.contains("duration=0.250:offset=2.250[vx1]"));
        // Next seam clamps to the short segment's half too (its tail side).
        assert!(fc.contains("duration=0.250"));
    }

    #[test]
    fn fold_within_bound_is_single_pass() {
        assert_eq!(fold_level_sizes(1, 6), vec![1]);
        assert_eq!(fold_level_sizes(6, 6), vec![6]);
    }

    #[test]
    fn fold_two_levels_for_a_typical_recap() {
        // 12 beats -> 24 parts: 4 chunks of 6, then one final 4-input pass.
        assert_eq!(fold_level_sizes(24, 6), vec![24, 4]);
        // A trailing singleton chunk (7 -> [6, 1]) still converges.
        assert_eq!(fold_level_sizes(7, 6), vec![7, 2]);
    }

    #[test]
    fn fold_converges_for_deep_reels() {
        // 108 parts (54 beats): 108 -> 18 -> 3.
        assert_eq!(fold_level_sizes(108, 6), vec![108, 18, 3]);
        // Every count up to a few hundred terminates with a final level <= max.
        for n in 1..=300 {
            let sizes = fold_level_sizes(n, 6);
            assert_eq!(sizes[0], n);
            assert!(*sizes.last().unwrap() <= 6);
            assert!(sizes.windows(2).all(|w| w[1] < w[0]));
        }
    }
}
