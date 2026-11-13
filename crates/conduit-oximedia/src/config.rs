use std::env;

/// Configuration for the conduit-oximedia service.
#[derive(Debug, Clone)]
pub struct Config {
    /// Server host.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// Shared key for internal auth (callers send it in `X-Internal-Key`).
    /// When unset, auth is disabled (dev mode).
    pub internal_key: Option<String>,
}

impl Config {
    /// Load configuration from the environment.
    pub fn from_env() -> Self {
        Self {
            host: env::var("OXIMEDIA_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("OXIMEDIA_PORT")
                .unwrap_or_else(|_| "8085".to_string())
                .parse()
                .expect("OXIMEDIA_PORT must be a valid port number"),
            internal_key: env::var("CONDUIT_INTERNAL_KEY").ok(),
        }
    }

    /// Server bind address.
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
