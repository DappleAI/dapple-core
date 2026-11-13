//! Shared plumbing for the rath-rs-backed hosted-media backends.
//!
//! rath reaches hosted image/video/audio generation (and, later, text) through a
//! provider-agnostic client selected by a model URL. The backends share a few
//! concerns, kept here so each new modality (and a future Fal *text* backend)
//! reuses them without re-implementing the plumbing:
//!
//! 1. Building the model URL from an authored model slug, pinning the API-key
//!    env var per provider — `FAL_AUTH_KEY` for Fal image/video, `OPENAI_API_KEY`
//!    for OpenAI audio (rath resolves it via the `api_key_env` query param; the
//!    conduit env must carry the named variable at runtime).
//! 2. Mapping rath's [`RathError`] into [`ConduitError`].
//! 3. Resolving rath's URL-or-base64 result (`ImageData`/`VideoData`) into raw
//!    bytes — Fal returns a hosted URL for most models, so a `Url` result costs
//!    one extra HTTP GET. (OpenAI TTS returns raw bytes directly, so the audio
//!    path needs no resolver.)

use crate::error::ConduitError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rath::images::ImageData;
use rath::video::VideoData;
use rath::RathError;

/// Environment variable the conduit reads the Fal API key from.
///
/// rath's own default is `FAL_KEY`; we standardize on `FAL_AUTH_KEY` and force
/// it via the `api_key_env` query param on every model URL (see
/// [`fal_model_url`]). Each backend's `from_env` gates on the same variable, so
/// a backend only registers when the key rath will look up is actually present
/// (rath's `ModelUrl::parse` errors if `api_key_env` names an unset variable).
pub const FAL_API_KEY_ENV: &str = "FAL_AUTH_KEY";

/// True when [`FAL_API_KEY_ENV`] is set to a non-empty value.
pub fn fal_key_present() -> bool {
    std::env::var(FAL_API_KEY_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .is_some()
}

/// Environment variable the conduit reads the OpenAI API key from.
///
/// The OpenAI audio (TTS/STT) path resolves its key the same way the Fal path
/// does — via the `api_key_env` query param — but names `OPENAI_API_KEY` (the
/// same variable the gpt-image image backend already uses). The rath TTS backend
/// gates registration on this being present.
pub const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";

/// True when [`OPENAI_API_KEY_ENV`] is set to a non-empty value.
pub fn openai_key_present() -> bool {
    std::env::var(OPENAI_API_KEY_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .is_some()
}

/// Build a rath model URL from an authored `model` slug, pinning the API-key env
/// var to `default_key_env`.
///
/// - A bare slug (`fal-ai/flux/schnell`, `gpt-4o-mini-tts`) →
///   `{default_scheme}:///{slug}?api_key_env={default_key_env}`.
/// - A model that already carries a `scheme:///` (an author pointing at another
///   rath provider, or a custom `base_url`) is used verbatim, with `api_key_env`
///   appended when absent — so rath still resolves a key without us
///   second-guessing the rest of the URL.
fn model_url(model: &str, default_scheme: &str, default_key_env: &str) -> String {
    let model = model.trim();
    let base = if model.contains(":///") {
        model.to_string()
    } else {
        format!("{default_scheme}:///{}", model.trim_start_matches('/'))
    };
    if base.contains("api_key_env=") {
        base
    } else {
        let sep = if base.contains('?') { '&' } else { '?' };
        format!("{base}{sep}api_key_env={default_key_env}")
    }
}

/// Build a Fal model URL, pinning the API-key env var to `FAL_AUTH_KEY`.
pub fn fal_model_url(model: &str) -> String {
    model_url(model, "fal", FAL_API_KEY_ENV)
}

/// Build an OpenAI model URL, pinning the API-key env var to `OPENAI_API_KEY`.
/// Used by the audio (TTS) backend — e.g. `gpt-4o-mini-tts` →
/// `openai:///gpt-4o-mini-tts?api_key_env=OPENAI_API_KEY`.
pub fn openai_model_url(model: &str) -> String {
    model_url(model, "openai", OPENAI_API_KEY_ENV)
}

/// Map a rath error into a conduit image error.
pub fn image_err(e: RathError) -> ConduitError {
    ConduitError::ImageError(format!("rath image error: {e}"))
}

/// Map a rath error into a conduit video error.
pub fn video_err(e: RathError) -> ConduitError {
    ConduitError::VideoError(format!("rath video error: {e}"))
}

/// Map a rath error into a conduit TTS error.
pub fn tts_err(e: RathError) -> ConduitError {
    ConduitError::TtsError(format!("rath tts error: {e}"))
}

/// Resolve a rath image result into raw bytes, fetching the URL when the
/// provider returned one.
pub async fn image_bytes(
    http: &reqwest::Client,
    data: &ImageData,
) -> Result<Vec<u8>, ConduitError> {
    match data {
        ImageData::Url { url } => download(http, url).await.map_err(ConduitError::ImageError),
        ImageData::Base64 { data, .. } => decode_base64(data).map_err(ConduitError::ImageError),
    }
}

/// Resolve a rath video result into `(bytes, mime_type)`, fetching the URL when
/// the provider returned one (mime defaults to `video/mp4`).
pub async fn video_bytes(
    http: &reqwest::Client,
    data: &VideoData,
) -> Result<(Vec<u8>, String), ConduitError> {
    match data {
        VideoData::Url { url } => {
            let bytes = download(http, url)
                .await
                .map_err(ConduitError::VideoError)?;
            Ok((bytes, "video/mp4".to_string()))
        }
        VideoData::Base64 { data, mime_type } => {
            let bytes = decode_base64(data).map_err(ConduitError::VideoError)?;
            Ok((bytes, mime_type.clone()))
        }
    }
}

/// GET `url` and return its non-empty body bytes. Fal result URLs are
/// pre-signed, so no auth header is sent.
async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to fetch result url: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("result url returned {status} for {url}"));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("failed to read result bytes: {e}"))?
        .to_vec();
    if bytes.is_empty() {
        return Err(format!("result url {url} returned 0 bytes"));
    }
    Ok(bytes)
}

/// Decode a base64 payload, tolerating a `data:...;base64,` URI prefix. The
/// base64 alphabet has no comma, so splitting on the last `,` only ever strips a
/// data-URI header.
fn decode_base64(data: &str) -> Result<Vec<u8>, String> {
    let payload = data.rsplit_once(',').map(|(_, b)| b).unwrap_or(data).trim();
    let bytes = STANDARD
        .decode(payload)
        .map_err(|e| format!("failed to base64-decode result: {e}"))?;
    if bytes.is_empty() {
        return Err("base64 result decoded to 0 bytes".to_string());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_slug_gets_fal_scheme_and_key_env() {
        assert_eq!(
            fal_model_url("fal-ai/flux/schnell"),
            "fal:///fal-ai/flux/schnell?api_key_env=FAL_AUTH_KEY"
        );
        // Leading slashes trimmed; whitespace ignored.
        assert_eq!(
            fal_model_url("  /fal-ai/ltx-video "),
            "fal:///fal-ai/ltx-video?api_key_env=FAL_AUTH_KEY"
        );
    }

    #[test]
    fn full_url_is_preserved_and_key_env_appended() {
        assert_eq!(
            fal_model_url("fal:///fal-ai/flux/schnell"),
            "fal:///fal-ai/flux/schnell?api_key_env=FAL_AUTH_KEY"
        );
        // Existing query → `&` separator.
        assert_eq!(
            fal_model_url("fal:///fal-ai/flux?base_url=https://x/y"),
            "fal:///fal-ai/flux?base_url=https://x/y&api_key_env=FAL_AUTH_KEY"
        );
        // Author already pinned api_key_env → left untouched (no duplicate).
        assert_eq!(
            fal_model_url("fal:///m?api_key_env=CUSTOM_KEY"),
            "fal:///m?api_key_env=CUSTOM_KEY"
        );
    }

    #[test]
    fn openai_model_url_gets_scheme_and_openai_key_env() {
        assert_eq!(
            openai_model_url("gpt-4o-mini-tts"),
            "openai:///gpt-4o-mini-tts?api_key_env=OPENAI_API_KEY"
        );
        // Full URL preserved; api_key_env appended.
        assert_eq!(
            openai_model_url("openai:///tts-1"),
            "openai:///tts-1?api_key_env=OPENAI_API_KEY"
        );
        // Author already pinned api_key_env → untouched.
        assert_eq!(
            openai_model_url("openai:///tts-1?api_key_env=CUSTOM"),
            "openai:///tts-1?api_key_env=CUSTOM"
        );
    }

    #[test]
    fn decode_base64_handles_bare_and_data_uri() {
        // "hi" base64.
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi");
        assert_eq!(decode_base64("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert!(decode_base64("").is_err());
    }
}
