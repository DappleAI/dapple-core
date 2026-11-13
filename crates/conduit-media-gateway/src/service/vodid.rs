//! Shared Twitch VOD identifier parsing.

use crate::error::MediaGatewayError;

/// Normalize a VOD URL or bare id into `(numeric_id, canonical_url)`.
///
/// Accepts `https://www.twitch.tv/videos/123`, `123`, or `v123`.
pub fn parse_vod(input: &str) -> Result<(String, String), MediaGatewayError> {
    let raw = input.trim();
    let id = if let Some(idx) = raw.find("/videos/") {
        raw[idx + "/videos/".len()..]
            .split(['/', '?', '&'])
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        raw.trim_start_matches('v').to_string()
    };

    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(MediaGatewayError::InvalidRequest(format!(
            "could not parse a Twitch VOD id from '{input}'"
        )));
    }

    let url = format!("https://www.twitch.tv/videos/{id}");
    Ok((id, url))
}
