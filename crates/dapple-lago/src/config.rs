/// Configuration for connecting to the Lago API
#[derive(Debug, Clone)]
pub struct LagoConfig {
    pub api_url: String,
    pub api_key: String,
}

impl LagoConfig {
    /// Load from environment variables. Returns None if LAGO_API_URL is not set.
    /// Follows the same pattern as DAPPLE_PROMPT_CONDUIT_URL — silent degradation when unconfigured.
    pub fn from_env() -> Option<Self> {
        let api_url = std::env::var("LAGO_API_URL").ok()?;
        let api_key = std::env::var("LAGO_API_KEY").ok()?;
        Some(Self { api_url, api_key })
    }

    /// Load test configuration. Uses `LAGO_TEST_API_KEY` to target an isolated
    /// test organization, keeping the primary org clean for manual testing.
    ///
    /// Falls back to `LAGO_API_KEY` if the test key isn't set.
    pub fn from_env_test() -> Option<Self> {
        let api_url = std::env::var("LAGO_API_URL").ok()?;
        let api_key = std::env::var("LAGO_TEST_API_KEY")
            .or_else(|_| std::env::var("LAGO_API_KEY"))
            .ok()?;
        Some(Self { api_url, api_key })
    }
}
