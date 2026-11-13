//! Environment loading utilities.

use std::path::PathBuf;

/// Load env vars: ~/.dapple.env first, then .env
pub fn load_env() {
    load_env_with_fallback(".env");
}

/// Load env vars with custom fallback file.
pub fn load_env_with_fallback(fallback: &str) {
    // 1. Load ~/.dapple.env first
    if let Some(home) = home_dir() {
        let dapple_env = home.join(".dapple.env");
        if dapple_env.exists() {
            let _ = dotenvy::from_path(&dapple_env);
        }
    }

    // 2. Load fallback, then .env
    if fallback != ".env" {
        let _ = dotenvy::from_filename(fallback).or_else(|_| dotenvy::dotenv());
    } else {
        let _ = dotenvy::dotenv();
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}
