use std::env;

/// Configuration for the LLM inference service
#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// Shared key for internal auth (conduit must send this in X-Internal-Key header)
    pub internal_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: env::var("LLM_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("LLM_PORT")
                .unwrap_or_else(|_| "8084".to_string())
                .parse()
                .expect("LLM_PORT must be a valid port number"),
            internal_key: env::var("CONDUIT_INTERNAL_KEY").ok(),
        }
    }

    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Configuration for the mistral.rs GGUF backend
#[derive(Debug, Clone)]
pub struct MistralRsConfig {
    pub model_id: String,
    pub gguf_file: String,
    pub tok_model_id: String,
    pub hf_token: Option<String>,
}

const DEFAULT_GGUF_MODEL_ID: &str = "bartowski/Mistral-7B-Instruct-v0.3-GGUF";
const DEFAULT_GGUF_FILE: &str = "Mistral-7B-Instruct-v0.3-Q4_K_M.gguf";
const DEFAULT_TOK_MODEL_ID: &str = "mistralai/Mistral-7B-Instruct-v0.3";

impl Default for MistralRsConfig {
    fn default() -> Self {
        Self {
            model_id: DEFAULT_GGUF_MODEL_ID.to_string(),
            gguf_file: DEFAULT_GGUF_FILE.to_string(),
            tok_model_id: DEFAULT_TOK_MODEL_ID.to_string(),
            hf_token: None,
        }
    }
}

impl MistralRsConfig {
    pub fn from_env() -> Option<Self> {
        // Only create config if model ID is set
        std::env::var("MISTRALRS_MODEL_ID").ok()?;

        Some(Self {
            model_id: std::env::var("MISTRALRS_MODEL_ID")
                .unwrap_or_else(|_| DEFAULT_GGUF_MODEL_ID.to_string()),
            gguf_file: std::env::var("MISTRALRS_GGUF_FILE")
                .unwrap_or_else(|_| DEFAULT_GGUF_FILE.to_string()),
            tok_model_id: std::env::var("MISTRALRS_TOK_MODEL_ID")
                .unwrap_or_else(|_| DEFAULT_TOK_MODEL_ID.to_string()),
            hf_token: std::env::var("HF_TOKEN")
                .ok()
                .or_else(|| std::env::var("HUGGINGFACE_TOKEN").ok()),
        })
    }
}
