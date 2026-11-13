//! Twitch Helix client — VOD validation + authoritative metadata.
//!
//! Helix is **required**: `TwitchClient::from_config` fails loud when the app
//! credentials are absent. Helix has no chapter data (that comes from yt-dlp) and
//! its `duration` is a `"3h2m1s"` string; the numeric duration + chapters come
//! from [`super::media::probe`]. Helix is used to validate the VOD exists and for
//! the authoritative title/user.

use serde::Serialize;
use twitch_api::helix::videos::{GetVideosRequest, Video};
use twitch_api::twitch_oauth2::{AppAccessToken, ClientId, ClientSecret};
use twitch_api::types::VideoId;
use twitch_api::HelixClient;

use crate::config::Config;
use crate::error::MediaGatewayError;

/// Authoritative VOD metadata from Helix.
#[derive(Clone, Debug, Serialize)]
pub struct VodMetadata {
    pub id: String,
    pub title: String,
    /// The VOD's own description text as the streamer wrote it. Part of the
    /// "meta content that comes from the service" threaded into the editor
    /// context as a value-slot binding. Often empty on Twitch.
    pub description: String,
    pub user_login: String,
    pub user_name: String,
    /// Raw Helix duration string (e.g. `"3h20m30s"`). The numeric duration used
    /// by the pipeline comes from yt-dlp's probe instead.
    pub duration_raw: String,
    pub url: String,
}

/// Helix client holding the app credentials. Cheap to construct; the app access
/// token is fetched per call (low volume, and tokens are single-use here).
pub struct TwitchClient {
    http: reqwest::Client,
    client_id: ClientId,
    client_secret: ClientSecret,
}

impl TwitchClient {
    /// Build from config, failing loud when the Twitch app credentials are unset.
    pub fn from_config(config: &Config) -> Result<Self, MediaGatewayError> {
        let client_id = config
            .twitch_client_id
            .clone()
            .ok_or_else(|| MediaGatewayError::Config("TWITCH_CLIENT_ID is not set".to_string()))?;
        let client_secret = config.twitch_client_secret.clone().ok_or_else(|| {
            MediaGatewayError::Config("TWITCH_CLIENT_SECRET is not set".to_string())
        })?;
        Ok(Self {
            http: reqwest::Client::new(),
            client_id: ClientId::new(client_id),
            client_secret: ClientSecret::new(client_secret),
        })
    }

    /// Fetch a fresh app access token (client-credentials flow).
    async fn token(&self) -> Result<AppAccessToken, MediaGatewayError> {
        AppAccessToken::get_app_access_token(
            &self.http,
            self.client_id.clone(),
            self.client_secret.clone(),
            vec![],
        )
        .await
        .map_err(|e| MediaGatewayError::Twitch(format!("app access token: {e}")))
    }

    /// Validate + fetch a single VOD by numeric id. `NotFound` when Helix returns
    /// no video for the id.
    pub async fn get_video(&self, video_id: &str) -> Result<VodMetadata, MediaGatewayError> {
        let token = self.token().await?;
        let helix: HelixClient<reqwest::Client> = HelixClient::with_client(self.http.clone());
        let request = GetVideosRequest::ids(vec![VideoId::new(video_id.to_string())]);
        let response = helix
            .req_get(request, &token)
            .await
            .map_err(|e| MediaGatewayError::Twitch(format!("get videos: {e}")))?;

        let video: Video = response.data.into_iter().next().ok_or_else(|| {
            MediaGatewayError::NotFound(format!("Twitch VOD {video_id} not found"))
        })?;

        Ok(VodMetadata {
            id: video.id.to_string(),
            title: video.title,
            description: video.description,
            user_login: video.user_login.to_string(),
            user_name: video.user_name.to_string(),
            duration_raw: video.duration,
            url: video.url,
        })
    }
}
