use std::env;

/// Configuration for the ONNX inference service
#[derive(Debug, Clone)]
pub struct Config {
    /// Server host
    pub host: String,
    /// Server port
    pub port: u16,
    /// Shared key for internal auth (conduit must send this in X-Internal-Key header)
    pub internal_key: Option<String>,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            host: env::var("ONNX_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("ONNX_PORT")
                .unwrap_or_else(|_| "8083".to_string())
                .parse()
                .expect("ONNX_PORT must be a valid port number"),
            internal_key: env::var("CONDUIT_INTERNAL_KEY").ok(),
        }
    }

    /// Get the server address
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
