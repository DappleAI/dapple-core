use std::env;

/// Configuration for the conduit service
#[derive(Debug, Clone)]
pub struct Config {
    /// Database connection URL
    pub database_url: String,
    /// Server host
    pub host: String,
    /// Server port
    pub port: u16,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DAPPLE_PROMPT_DATABASE_URL")
                .expect("DAPPLE_PROMPT_DATABASE_URL must be set"),
            host: env::var("CONDUIT_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("CONDUIT_PORT")
                .unwrap_or_else(|_| "8081".to_string())
                .parse()
                .expect("CONDUIT_PORT must be a valid port number"),
        }
    }

    /// Get the server address
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
