use std::env;
use std::path::PathBuf;

/// Configuration for the conduit-media-gateway service.
///
/// `Debug` is implemented by hand to redact secrets (`internal_key`,
/// `twitch_client_secret`) so the startup log line never prints them.
#[derive(Clone)]
pub struct Config {
    /// Server host (`MEDIA_GATEWAY_HOST`, default `0.0.0.0`).
    pub host: String,
    /// Server port (`MEDIA_GATEWAY_PORT`, default `8086`).
    pub port: u16,
    /// Shared key for internal auth (callers send it in `X-Internal-Key`).
    /// When unset, auth is disabled (dev mode). From `CONDUIT_INTERNAL_KEY`.
    pub internal_key: Option<String>,
    /// Twitch app client id (`TWITCH_CLIENT_ID`) for Helix metadata/validation.
    pub twitch_client_id: Option<String>,
    /// Twitch app client secret (`TWITCH_CLIENT_SECRET`). Required at job time —
    /// the pipeline fails loud when this or the id is absent.
    pub twitch_client_secret: Option<String>,
    /// whisper.cpp model to load (`WHISPER_MODEL`, default `base.en`). May be a
    /// bare ggml model name or a path; resolved by the transcription stage.
    pub whisper_model: String,
    /// Scratch directory for VOD downloads + intermediate media
    /// (`MEDIA_GATEWAY_SCRATCH_DIR`, default a temp subdir). In-cluster this is an
    /// emptyDir mount; the full VOD lives here and is never persisted as an asset.
    pub scratch_dir: PathBuf,
}

impl Config {
    /// Load configuration from the environment.
    pub fn from_env() -> Self {
        let non_empty = |k: &str| env::var(k).ok().filter(|s| !s.is_empty());
        Self {
            host: env::var("MEDIA_GATEWAY_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("MEDIA_GATEWAY_PORT")
                .unwrap_or_else(|_| "8086".to_string())
                .parse()
                .expect("MEDIA_GATEWAY_PORT must be a valid port number"),
            internal_key: non_empty("CONDUIT_INTERNAL_KEY"),
            twitch_client_id: non_empty("TWITCH_CLIENT_ID"),
            twitch_client_secret: non_empty("TWITCH_CLIENT_SECRET"),
            whisper_model: env::var("WHISPER_MODEL").unwrap_or_else(|_| "base.en".to_string()),
            scratch_dir: non_empty("MEDIA_GATEWAY_SCRATCH_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| env::temp_dir().join("conduit-media-gateway")),
        }
    }

    /// Server bind address.
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |o: &Option<String>| o.as_ref().map(|_| "<set>");
        f.debug_struct("Config")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("internal_key", &redact(&self.internal_key))
            .field("twitch_client_id", &self.twitch_client_id)
            .field("twitch_client_secret", &redact(&self.twitch_client_secret))
            .field("whisper_model", &self.whisper_model)
            .field("scratch_dir", &self.scratch_dir)
            .finish()
    }
}
