//! Shared WAV decode/encode (via `hound`), used by both `audio_analysis` (decode
//! stems for analysis) and `av` (decode clips + encode the mixed conversation).
//!
//! Decode handles integer (8/16/24/32-bit) and float sample formats and downmixes
//! any channel count to mono by averaging; it reads the real `WavSpec` rather than
//! assuming the TTS-path 44-byte-header shortcut. Encode writes mono 16-bit PCM —
//! the widely-playable form for the mixed-conversation deliverable.

use std::borrow::Cow;
use std::io::Cursor;

/// Decoded mono audio.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Mono samples, normalized to roughly -1.0..1.0.
    pub samples: Vec<f32>,
    /// Sample rate (Hz).
    pub sample_rate: u32,
}

impl DecodedAudio {
    /// Duration in seconds (rate-independent — safe to compare/chain across clips
    /// of different sample rates).
    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }
}

/// Amplitude below which a sample counts as silence for edge-trimming. TTS clips
/// carry near-digital-silence padding at the head/tail; ~0.02 (≈ −34 dBFS) clears
/// that without eating real speech onsets.
const SILENCE_THRESHOLD: f32 = 0.02;

/// Audio kept on each retained edge after trimming, so a hard onset/offset isn't
/// clipped (≈ 25 ms).
const TRIM_PAD_SECONDS: f64 = 0.025;

/// Trim leading and trailing near-silence from a clip so downstream timing offsets
/// act on real speech boundaries, not the TTS padding.
///
/// Only the head and tail are trimmed — interior pauses (a beat mid-line) are left
/// intact. A short pad ([`TRIM_PAD_SECONDS`]) is kept on each retained edge so
/// speech onsets/offsets aren't clipped. A clip with no sample above
/// [`SILENCE_THRESHOLD`] (fully silent) is returned **unchanged**, so no clip ever
/// becomes zero-length (which would break chain resolution / render).
#[must_use]
pub fn trim_edge_silence(audio: &DecodedAudio) -> DecodedAudio {
    let s = &audio.samples;
    let Some(first) = s.iter().position(|v| v.abs() > SILENCE_THRESHOLD) else {
        return audio.clone(); // fully silent — leave intact
    };
    // A loud sample exists (`first`), so `rposition` also finds one.
    let last = s
        .iter()
        .rposition(|v| v.abs() > SILENCE_THRESHOLD)
        .unwrap_or(s.len() - 1);

    let pad = (TRIM_PAD_SECONDS * f64::from(audio.sample_rate)).round() as usize;
    let start = first.saturating_sub(pad);
    let end = (last + pad + 1).min(s.len()); // exclusive
    DecodedAudio {
        samples: s[start..end].to_vec(),
        sample_rate: audio.sample_rate,
    }
}

/// Normalize a RIFF/WAVE buffer so its `data` sub-chunk length is a
/// spec-compliant multiple of the frame size, clamped to the bytes actually
/// present.
///
/// OpenAI's TTS `wav` output can declare a `data` length that is not a whole
/// multiple of the frame size — a streamed-wav placeholder (`0xFFFFFFFF`, which
/// is odd) or an odd trailing byte — which `hound` rejects in `WavReader::new`
/// with "data chunk length is not a multiple of sample size", before reading any
/// samples. This walks the chunks (honoring the RIFF odd-length pad byte); when
/// the declared `data` length disagrees with a frame-aligned view of the bytes
/// present, it truncates to the real data end and rewrites the `data` + RIFF
/// size fields. Buffers that already parse are returned borrowed, unchanged;
/// anything not recognizably RIFF/WAVE is passed through so hound emits its own
/// error.
fn sanitize_wav(bytes: &[u8]) -> Cow<'_, [u8]> {
    // Must look like RIFF/WAVE; otherwise let hound produce the error.
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Cow::Borrowed(bytes);
    }

    // Walk sub-chunks for `fmt ` (block align) and `data` (body start + declared len).
    let mut pos = 12usize;
    let mut block_align: Option<usize> = None;
    let mut data: Option<(usize, usize)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            // WAVEFORMAT: block align is a u16 at byte offset 12 of the fmt body.
            block_align = Some(u16::from_le_bytes([bytes[body + 12], bytes[body + 13]]) as usize);
        }
        if id == b"data" {
            data = Some((body, len));
            break;
        }
        // Advance past this chunk body plus the RIFF word-alignment pad byte.
        match body.checked_add(len + (len & 1)) {
            Some(next) => pos = next,
            None => break,
        }
    }

    let (Some(block_align), Some((data_start, declared))) = (block_align, data) else {
        return Cow::Borrowed(bytes);
    };
    if block_align == 0 {
        return Cow::Borrowed(bytes);
    }

    let available = bytes.len().saturating_sub(data_start);
    let effective = declared.min(available);
    let aligned = effective - (effective % block_align);
    if aligned == declared {
        // Already frame-aligned and present — hound will accept it as-is.
        return Cow::Borrowed(bytes);
    }

    // Rewrite: truncate to the real data end, patch the `data` and RIFF size fields.
    let mut out = bytes[..data_start + aligned].to_vec();
    let data_size_off = data_start - 4;
    out[data_size_off..data_size_off + 4].copy_from_slice(&(aligned as u32).to_le_bytes());
    let riff_size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Cow::Owned(out)
}

/// Decode WAV bytes to mono `f32` samples.
///
/// Returns a human-readable error string on any decode failure (bad header,
/// unsupported format, truncated data) so the caller can prepend context.
pub fn decode_wav(bytes: &[u8]) -> Result<DecodedAudio, String> {
    if bytes.is_empty() {
        return Err("empty WAV (0 bytes)".to_string());
    }

    // OpenAI's TTS `wav` output can declare a `data` chunk length that is not a
    // clean multiple of the frame size (a streamed-wav placeholder, or an odd
    // trailing byte). Normalize before handing to hound, which otherwise rejects
    // such files outright. Clean WAVs pass through borrowed, unchanged.
    let bytes = sanitize_wav(bytes);
    let reader = hound::WavReader::new(Cursor::new(bytes.as_ref()))
        .map_err(|e| format!("not a valid WAV: {e}"))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let sample_rate = spec.sample_rate;

    // Read interleaved samples as f32 in [-1, 1].
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            let mut r = reader;
            r.samples::<f32>()
                .collect::<Result<Vec<f32>, _>>()
                .map_err(|e| format!("failed reading float samples: {e}"))?
        }
        hound::SampleFormat::Int => {
            // `samples::<i32>()` sign-extends 8/16/24/32-bit ints; normalize by
            // the format's full-scale magnitude.
            let bits = spec.bits_per_sample.max(1);
            let full_scale = 2f32.powi(i32::from(bits) - 1);
            let mut r = reader;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / full_scale))
                .collect::<Result<Vec<f32>, _>>()
                .map_err(|e| format!("failed reading int samples: {e}"))?
        }
    };

    if interleaved.is_empty() {
        return Err("WAV contained no samples".to_string());
    }

    // Downmix to mono by averaging channels within each frame.
    let samples = if channels <= 1 {
        interleaved
    } else {
        let frames = interleaved.len() / channels;
        let mut mono = Vec::with_capacity(frames);
        for frame in interleaved.chunks_exact(channels) {
            let sum: f32 = frame.iter().sum();
            mono.push(sum / channels as f32);
        }
        mono
    };

    Ok(DecodedAudio {
        samples,
        sample_rate,
    })
}

/// Encode mono `f32` samples (in ~[-1, 1]) as 16-bit PCM WAV bytes.
pub fn encode_wav_i16_mono(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w =
            hound::WavWriter::new(&mut buf, spec).map_err(|e| format!("wav writer: {e}"))?;
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            w.write_sample(v).map_err(|e| format!("wav write: {e}"))?;
        }
        w.finalize().map_err(|e| format!("wav finalize: {e}"))?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a mono i16 WAV to bytes for round-trip testing.
    pub(crate) fn write_wav_i16_mono(samples: &[i16], sample_rate: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut buf, spec).expect("writer");
            for &s in samples {
                w.write_sample(s).expect("write");
            }
            w.finalize().expect("finalize");
        }
        buf.into_inner()
    }

    /// Write an interleaved-stereo f32 WAV to bytes.
    fn write_wav_f32_stereo(frames: &[(f32, f32)], sample_rate: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut buf, spec).expect("writer");
            for &(l, r) in frames {
                w.write_sample(l).expect("write l");
                w.write_sample(r).expect("write r");
            }
            w.finalize().expect("finalize");
        }
        buf.into_inner()
    }

    #[test]
    fn decodes_mono_i16() {
        let src = vec![0i16, 16384, -16384, 32767, -32768, 0];
        let bytes = write_wav_i16_mono(&src, 44100);
        let decoded = decode_wav(&bytes).expect("decode");
        assert_eq!(decoded.sample_rate, 44100);
        assert_eq!(decoded.samples.len(), src.len());
        // 16384 / 32768 = 0.5
        assert!((decoded.samples[1] - 0.5).abs() < 1e-3);
        assert!((decoded.samples[2] + 0.5).abs() < 1e-3);
        // Full-scale positive is ~+1.0.
        assert!((decoded.samples[3] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn decodes_and_downmixes_stereo_f32() {
        // Left = 1.0, right = 0.0 → mono average = 0.5.
        let frames = vec![(1.0f32, 0.0f32); 8];
        let bytes = write_wav_f32_stereo(&frames, 48000);
        let decoded = decode_wav(&bytes).expect("decode");
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.samples.len(), frames.len()); // halved from interleaved
        for s in &decoded.samples {
            assert!((s - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn duration_seconds_is_rate_independent() {
        let a = DecodedAudio {
            samples: vec![0.0; 24_000],
            sample_rate: 24_000,
        };
        assert!((a.duration_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn encode_then_decode_roundtrips() {
        let samples: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.01).sin() * 0.5).collect();
        let bytes = encode_wav_i16_mono(&samples, 24_000).expect("encode");
        let decoded = decode_wav(&bytes).expect("decode");
        assert_eq!(decoded.sample_rate, 24_000);
        assert_eq!(decoded.samples.len(), samples.len());
        // 16-bit quantization error is small.
        for (a, b) in samples.iter().zip(decoded.samples.iter()) {
            assert!((a - b).abs() < 1e-3);
        }
    }

    #[test]
    fn rejects_empty_bytes() {
        assert!(decode_wav(&[]).is_err());
    }

    #[test]
    fn rejects_non_wav() {
        assert!(decode_wav(b"this is not a wav file at all").is_err());
    }

    #[test]
    fn sanitize_repairs_placeholder_data_size() {
        // Valid mono i16 WAV, then clobber the `data` size field to the
        // streamed-wav placeholder 0xFFFFFFFF (odd → hound rejects it),
        // mimicking OpenAI's TTS `wav` output.
        let src: Vec<i16> = (0..64).map(|i| (i * 100) as i16).collect();
        let mut bytes = write_wav_i16_mono(&src, 24_000);
        // Canonical hound layout: `data` chunk header at 36, size field at 40.
        assert_eq!(&bytes[36..40], b"data");
        bytes[40..44].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());

        // Raw corrupted bytes fail hound directly...
        assert!(hound::WavReader::new(Cursor::new(&bytes[..])).is_err());
        // ...but decode_wav (with sanitize) recovers every sample.
        let decoded = decode_wav(&bytes).expect("sanitized decode");
        assert_eq!(decoded.sample_rate, 24_000);
        assert_eq!(decoded.samples.len(), src.len());
    }

    #[test]
    fn sanitize_trims_odd_trailing_byte() {
        let src: Vec<i16> = (0..32).map(|i| i as i16).collect();
        let mut bytes = write_wav_i16_mono(&src, 24_000);
        assert_eq!(&bytes[36..40], b"data");
        // Declare an odd data length (len*2 + 1) and append a stray byte.
        let odd_len = (src.len() * 2 + 1) as u32;
        bytes[40..44].copy_from_slice(&odd_len.to_le_bytes());
        bytes.push(0x7F);

        assert!(hound::WavReader::new(Cursor::new(&bytes[..])).is_err());
        let decoded = decode_wav(&bytes).expect("sanitized decode");
        // The odd tail is dropped; whole frames survive.
        assert_eq!(decoded.samples.len(), src.len());
    }

    #[test]
    fn sanitize_leaves_valid_wav_borrowed() {
        // A spec-compliant WAV must not be copied or mutated.
        let src = vec![0i16, 4096, -4096, 8192, -8192, 0];
        let bytes = write_wav_i16_mono(&src, 24_000);
        assert!(matches!(sanitize_wav(&bytes), Cow::Borrowed(_)));
    }

    #[test]
    fn trim_removes_leading_and_trailing_silence() {
        // rate 1000 → pad = round(0.025 * 1000) = 25 samples.
        let mut samples = vec![0.0f32; 100]; // leading silence
        samples.extend(vec![0.5f32; 200]); // speech [100, 300)
        samples.extend(vec![0.0f32; 100]); // trailing silence
        let out = trim_edge_silence(&DecodedAudio {
            samples,
            sample_rate: 1000,
        });
        // first loud = 100, last loud = 299, pad 25 → [75, 325) = 250 samples.
        assert_eq!(out.samples.len(), 250);
        // The 25-sample head pad is still silence; speech starts right after it.
        assert_eq!(out.samples[0], 0.0);
        assert!((out.samples[25] - 0.5).abs() < 1e-6);
        // Trailing pad silence is kept too.
        assert_eq!(out.samples[out.samples.len() - 1], 0.0);
    }

    #[test]
    fn trim_preserves_interior_silence() {
        // No edge silence, but a gap in the middle must survive (only edges trim).
        let mut samples = vec![0.5f32; 50];
        samples.extend(vec![0.0f32; 30]); // interior pause [50, 80)
        samples.extend(vec![0.5f32; 50]);
        let out = trim_edge_silence(&DecodedAudio {
            samples,
            sample_rate: 1000,
        });
        assert_eq!(out.samples.len(), 130, "nothing to trim at the edges");
        assert_eq!(out.samples[60], 0.0, "interior pause preserved");
    }

    #[test]
    fn trim_all_silent_is_unchanged() {
        let out = trim_edge_silence(&DecodedAudio {
            samples: vec![0.0f32; 100],
            sample_rate: 1000,
        });
        assert_eq!(out.samples.len(), 100, "fully-silent clip left intact");
    }
}
