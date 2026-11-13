use super::{SynthesizeRequest, SynthesizeResult, TtsBackend};
use crate::error::OnnxServiceError;
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;

/// Stub TTS backend for testing
/// Produces deterministic WAV audio (sine wave tone) based on input hash
pub struct StubTtsBackend {
    sample_rate: u32,
}

impl StubTtsBackend {
    pub fn new() -> Self {
        Self { sample_rate: 24000 }
    }

    /// Generate a deterministic sine wave tone based on text hash
    fn generate_audio(&self, text: &str) -> Vec<u8> {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();

        // Use hash to determine frequency (300-600 Hz range)
        let frequency = 300.0 + (hash % 300) as f32;

        // Duration based on text length (roughly 0.1 seconds per word, min 0.5s, max 3s)
        let word_count = text.split_whitespace().count().max(1);
        let duration_seconds = (word_count as f32 * 0.1).clamp(0.5, 3.0);
        let num_samples = (self.sample_rate as f32 * duration_seconds) as usize;

        // Generate sine wave samples
        let samples: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f32 / self.sample_rate as f32;
                // Add slight amplitude envelope (fade in/out)
                let envelope = if t < 0.05 {
                    t / 0.05
                } else if t > duration_seconds - 0.05 {
                    (duration_seconds - t) / 0.05
                } else {
                    1.0
                };
                (2.0 * std::f32::consts::PI * frequency * t).sin() * 0.3 * envelope
            })
            .collect();

        // Encode as WAV
        let mut wav_bytes = Vec::new();
        write_pcm_as_wav(&mut wav_bytes, &samples, self.sample_rate).unwrap();
        wav_bytes
    }
}

impl Default for StubTtsBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TtsBackend for StubTtsBackend {
    async fn synthesize(
        &self,
        request: &SynthesizeRequest,
    ) -> Result<SynthesizeResult, OnnxServiceError> {
        log::debug!("Stub TTS synthesizing: '{}'", request.text);

        let audio_bytes = self.generate_audio(&request.text);

        Ok(SynthesizeResult::new(
            audio_bytes,
            self.sample_rate,
            format!("[STUB TTS] {}", request.text),
        ))
    }

    fn name(&self) -> &str {
        "stub"
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// Write PCM samples as WAV format
fn write_pcm_as_wav<W: Write>(w: &mut W, samples: &[f32], sample_rate: u32) -> std::io::Result<()> {
    let len = 12u32; // header
    let len = len + 24u32; // fmt
    let len = len + samples.len() as u32 * 2 + 8; // data
    let n_channels = 1u16;
    let bytes_per_second = sample_rate * 2 * n_channels as u32;

    w.write_all(b"RIFF")?;
    w.write_all(&(len - 8).to_le_bytes())?;
    w.write_all(b"WAVE")?;

    // Format block
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&n_channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&bytes_per_second.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // bytes per sample
    w.write_all(&16u16.to_le_bytes())?; // bits per sample

    // Data block
    w.write_all(b"data")?;
    w.write_all(&(samples.len() as u32 * 2).to_le_bytes())?;
    for sample in samples.iter() {
        let i16_sample = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
        w.write_all(&i16_sample.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_deterministic() {
        let backend = StubTtsBackend::new();

        let request = SynthesizeRequest::new("Hello world");
        let result1 = backend.synthesize(&request).await.unwrap();
        let result2 = backend.synthesize(&request).await.unwrap();

        // Same input should produce same audio
        assert_eq!(result1.audio_bytes, result2.audio_bytes);
    }

    #[tokio::test]
    async fn test_stub_different_inputs() {
        let backend = StubTtsBackend::new();

        let result1 = backend
            .synthesize(&SynthesizeRequest::new("Hello"))
            .await
            .unwrap();
        let result2 = backend
            .synthesize(&SynthesizeRequest::new("Goodbye"))
            .await
            .unwrap();

        // Different inputs should produce different audio (different frequencies)
        assert_ne!(result1.audio_bytes, result2.audio_bytes);
    }

    #[tokio::test]
    async fn test_stub_valid_wav() {
        let backend = StubTtsBackend::new();

        let result = backend
            .synthesize(&SynthesizeRequest::new("Test"))
            .await
            .unwrap();

        // Check WAV header
        assert!(result.audio_bytes.len() > 44);
        assert_eq!(&result.audio_bytes[0..4], b"RIFF");
        assert_eq!(&result.audio_bytes[8..12], b"WAVE");
        assert_eq!(&result.audio_bytes[12..16], b"fmt ");
    }

    #[tokio::test]
    async fn test_stub_sample_rate() {
        let backend = StubTtsBackend::new();
        assert_eq!(backend.sample_rate(), 24000);
    }

    #[tokio::test]
    async fn test_stub_duration_scales_with_text() {
        let backend = StubTtsBackend::new();

        let short = backend
            .synthesize(&SynthesizeRequest::new("Hi"))
            .await
            .unwrap();
        let long = backend
            .synthesize(&SynthesizeRequest::new(
                "This is a much longer sentence with many more words",
            ))
            .await
            .unwrap();

        // Longer text should produce more audio
        assert!(long.audio_bytes.len() > short.audio_bytes.len());
        assert!(long.duration_seconds > short.duration_seconds);
    }
}
