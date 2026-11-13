//! ONNX Runtime backend infrastructure for ML inference.
//!
//! This module provides a unified ONNX Runtime backend that handles:
//! - Execution provider detection (ROCm, CUDA, CoreML, CPU)
//! - Model loading from pre-converted ONNX files
//! - Thread-safe session management
//!
//! # Execution Provider Selection
//!
//! The execution provider is selected in the following order:
//! 1. `ONNX_EXECUTION_PROVIDER` environment variable (explicit override)
//! 2. Auto-detection based on available hardware (ROCm → CUDA → CoreML → CPU)
//!
//! # Model Path
//!
//! Models are loaded from the path specified by `ONNX_MODEL_PATH` environment
//! variable (defaults to `/models`). Each model should be in its own subdirectory
//! with a `model.onnx` file and optionally a `tokenizer.json`.
//!
//! # Threading
//!
//! Intra-op threads track the cgroup CPU quota via `num_cpus::get()`, which
//! reads it directly — `nproc` does NOT, and mistaking one for the other has
//! already cost one wrong diagnosis. See [`OnnxBackend::intra_op_threads`]
//! before re-opening any thread-count theory. `ONNX_INTRA_THREADS` overrides.
//!
//! # Twin file — patch both copies
//!
//! This module exists in two crates and is maintained BYTE-IDENTICAL:
//!
//! - `crates/conduit-onnx/src/service/onnx/mod.rs`
//! - `crates/dapple-prompt-conduit/src/service/onnx/mod.rs`
//!
//! conduit-onnx has no first-party path dependencies at all, so there is no
//! shared crate to hoist this into without inventing a dependency edge for one
//! module. Because the copies are byte-identical, `md5sum` on both paths is a
//! one-command drift check — keep it that way.

use ort::{
    execution_providers::{
        CUDAExecutionProvider, CoreMLExecutionProvider, ExecutionProvider, ROCmExecutionProvider,
    },
    session::{builder::GraphOptimizationLevel, Session},
    Error as OrtError,
};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur in ONNX Runtime operations
#[derive(Debug, Error)]
pub enum OnnxError {
    #[error("ONNX Runtime initialization failed: {0}")]
    InitializationError(String),

    #[error("Failed to load model from {path}: {reason}")]
    ModelLoadError { path: String, reason: String },

    #[error("Model inference failed: {0}")]
    InferenceError(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Model not found at {0}")]
    ModelNotFound(String),

    #[error("ORT error: {0}")]
    OrtError(#[from] OrtError),
}

/// Detected execution provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionProviderType {
    /// AMD ROCm (AMD GPUs)
    Rocm,
    /// NVIDIA CUDA
    Cuda,
    /// Apple CoreML (Apple Silicon)
    CoreMl,
    /// CPU fallback
    Cpu,
}

impl std::fmt::Display for ExecutionProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionProviderType::Rocm => write!(f, "ROCmExecutionProvider"),
            ExecutionProviderType::Cuda => write!(f, "CUDAExecutionProvider"),
            ExecutionProviderType::CoreMl => write!(f, "CoreMLExecutionProvider"),
            ExecutionProviderType::Cpu => write!(f, "CPUExecutionProvider"),
        }
    }
}

/// ONNX Runtime backend for model inference
///
/// This struct manages the ONNX Runtime environment and provides
/// methods for loading models with the appropriate execution provider.
#[derive(Clone)]
pub struct OnnxBackend {
    execution_provider: ExecutionProviderType,
    model_base_path: PathBuf,
}

impl OnnxBackend {
    /// Create a new ONNX backend with auto-detected execution provider
    pub fn new() -> Result<Self, OnnxError> {
        let ep = Self::detect_execution_provider();
        let model_base_path = Self::get_model_base_path();

        log::info!(
            "ONNX Runtime initialized with {} (model path: {})",
            ep,
            model_base_path.display()
        );

        Ok(Self {
            execution_provider: ep,
            model_base_path,
        })
    }

    /// Create a new ONNX backend with a specific execution provider
    pub fn with_provider(provider: ExecutionProviderType) -> Result<Self, OnnxError> {
        let model_base_path = Self::get_model_base_path();

        log::info!(
            "ONNX Runtime initialized with {} (model path: {})",
            provider,
            model_base_path.display()
        );

        Ok(Self {
            execution_provider: provider,
            model_base_path,
        })
    }

    /// Get the base path for ONNX models
    fn get_model_base_path() -> PathBuf {
        std::env::var("ONNX_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/models"))
    }

    /// Intra-op thread count for every ONNX session this backend builds.
    ///
    /// # `num_cpus::get()` is ALREADY cgroup-quota-aware — do not re-diagnose this
    ///
    /// A 2026-08-02 investigation into a conduit-onnx OOM/probe incident wrongly
    /// concluded that ONNX was spawning 32 intra-op threads onto a 2-CPU quota,
    /// on the strength of `nproc` printing 32 inside the pod. It is not. The two
    /// disagree because they answer different questions:
    ///
    /// ```text
    /// nproc / Cpus_allowed_list  = 32                 <- affinity mask only
    /// /sys/fs/cgroup/cpu.max     = 200000 100000      <- quota: 2 CPUs
    /// num_cpus::get()            = 2                  <- min(affinity, quota)
    /// ```
    ///
    /// `num_cpus` reads the quota itself (`src/linux.rs` `cgroups_num_cpus()`,
    /// parsing cgroup-v2 `cpu.max` and v1 `cpu.cfs_quota_us`), so the thread
    /// count has always tracked the manifest's CPU limit. A hand-rolled quota
    /// reader here would be a branch that can never differ from the line below
    /// while looking like it does. **Measure before re-opening this** — the
    /// startup log line emitted here is the instrument; `nproc` is not.
    ///
    /// `ONNX_INTRA_THREADS` overrides, for tuning without a rebuild. Applies to
    /// EVERY session — embedding, MLM, T5 and TTS all build through here.
    fn intra_op_threads() -> usize {
        let cores = num_cpus::get().max(1);
        let env_override = std::env::var("ONNX_INTRA_THREADS").ok();
        let (threads, source) = Self::resolve_intra_op_threads(env_override.as_deref(), cores);

        log::info!(
            "ONNX intra-op threads: {threads} (from {source}; num_cpus::get() = {cores}, \
             which already honours the cgroup CPU quota)"
        );
        threads
    }

    /// Pure resolution of the thread count, split out so the policy is testable
    /// without touching the environment. Returns the count and the name of the
    /// source that decided it (for the log line).
    fn resolve_intra_op_threads(env_override: Option<&str>, cores: usize) -> (usize, &'static str) {
        let cores = cores.max(1);

        // The override is deliberately NOT clamped to `cores`: being able to
        // set it ABOVE the quota is how you test an oversubscription theory
        // against the running pod instead of inferring one from `nproc`.
        match env_override
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
        {
            Some(n) => (n, "ONNX_INTRA_THREADS"),
            None => (cores, "num_cpus"),
        }
    }

    /// Detect the best available execution provider
    fn detect_execution_provider() -> ExecutionProviderType {
        // Check environment variable first
        if let Ok(ep) = std::env::var("ONNX_EXECUTION_PROVIDER") {
            return match ep.as_str() {
                "ROCmExecutionProvider" => ExecutionProviderType::Rocm,
                "CUDAExecutionProvider" => ExecutionProviderType::Cuda,
                "CoreMLExecutionProvider" => ExecutionProviderType::CoreMl,
                _ => ExecutionProviderType::Cpu,
            };
        }

        // Auto-detect based on available providers
        // Try ROCm first (AMD GPUs)
        if ROCmExecutionProvider::default()
            .is_available()
            .unwrap_or(false)
        {
            log::info!("ROCm execution provider detected");
            return ExecutionProviderType::Rocm;
        }

        // Try CUDA (NVIDIA GPUs)
        if CUDAExecutionProvider::default()
            .is_available()
            .unwrap_or(false)
        {
            log::info!("CUDA execution provider detected");
            return ExecutionProviderType::Cuda;
        }

        // Try CoreML (Apple Silicon)
        if CoreMLExecutionProvider::default()
            .is_available()
            .unwrap_or(false)
        {
            log::info!("CoreML execution provider detected");
            return ExecutionProviderType::CoreMl;
        }

        log::info!("No GPU acceleration available, using CPU");
        ExecutionProviderType::Cpu
    }

    /// Get the current execution provider type
    pub fn execution_provider(&self) -> ExecutionProviderType {
        self.execution_provider
    }

    /// Get the model base path
    pub fn model_base_path(&self) -> &Path {
        &self.model_base_path
    }

    /// Load an ONNX model from the given subdirectory
    ///
    /// The model is expected to be at `{model_base_path}/{model_name}/model.onnx`
    pub fn load_model(&self, model_name: &str) -> Result<Session, OnnxError> {
        let model_path = self.model_base_path.join(model_name).join("model.onnx");

        if !model_path.exists() {
            return Err(OnnxError::ModelNotFound(model_path.display().to_string()));
        }

        self.load_model_from_path(&model_path)
    }

    /// Load an ONNX model from a specific path
    pub fn load_model_from_path(&self, model_path: &Path) -> Result<Session, OnnxError> {
        log::info!(
            "Loading ONNX model from {} with {}",
            model_path.display(),
            self.execution_provider
        );

        let session = self.build_session(model_path)?;

        log::info!("Model loaded successfully");
        Ok(session)
    }

    /// Build an ONNX session with the configured execution provider
    fn build_session(&self, model_path: &Path) -> Result<Session, OnnxError> {
        let mut builder = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(Self::intra_op_threads())?;

        // Add the appropriate execution provider
        builder = match self.execution_provider {
            ExecutionProviderType::Rocm => {
                builder.with_execution_providers([ROCmExecutionProvider::default().build()])?
            }
            ExecutionProviderType::Cuda => {
                builder.with_execution_providers([CUDAExecutionProvider::default().build()])?
            }
            ExecutionProviderType::CoreMl => {
                builder.with_execution_providers([CoreMLExecutionProvider::default().build()])?
            }
            ExecutionProviderType::Cpu => builder,
        };

        builder
            .commit_from_file(model_path)
            .map_err(|e| OnnxError::ModelLoadError {
                path: model_path.display().to_string(),
                reason: e.to_string(),
            })
    }

    /// Get the path to a tokenizer for a model
    ///
    /// Returns the path to `{model_base_path}/{model_name}/tokenizer.json`
    pub fn tokenizer_path(&self, model_name: &str) -> PathBuf {
        self.model_base_path.join(model_name).join("tokenizer.json")
    }

    /// Check if a model exists
    pub fn model_exists(&self, model_name: &str) -> bool {
        self.model_base_path
            .join(model_name)
            .join("model.onnx")
            .exists()
    }
}

impl Default for OnnxBackend {
    fn default() -> Self {
        Self::new().expect("Failed to initialize ONNX Runtime")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_provider_display() {
        assert_eq!(
            ExecutionProviderType::Rocm.to_string(),
            "ROCmExecutionProvider"
        );
        assert_eq!(
            ExecutionProviderType::Cuda.to_string(),
            "CUDAExecutionProvider"
        );
        assert_eq!(
            ExecutionProviderType::CoreMl.to_string(),
            "CoreMLExecutionProvider"
        );
        assert_eq!(
            ExecutionProviderType::Cpu.to_string(),
            "CPUExecutionProvider"
        );
    }

    #[test]
    fn resolve_intra_op_threads_defaults_to_num_cpus() {
        assert_eq!(
            OnnxBackend::resolve_intra_op_threads(None, 2),
            (2, "num_cpus")
        );
    }

    #[test]
    fn resolve_intra_op_threads_prefers_the_env_override() {
        assert_eq!(
            OnnxBackend::resolve_intra_op_threads(Some(" 4 "), 2),
            (4, "ONNX_INTRA_THREADS")
        );
    }

    #[test]
    fn resolve_intra_op_threads_ignores_a_junk_or_zero_override() {
        for bad in ["", "0", "-1", "lots"] {
            assert_eq!(
                OnnxBackend::resolve_intra_op_threads(Some(bad), 2),
                (2, "num_cpus"),
                "override {bad:?}"
            );
        }
    }

    /// The override must be able to exceed the quota — that is how an
    /// oversubscription theory gets TESTED against the running pod rather than
    /// inferred from `nproc`. See `intra_op_threads`.
    #[test]
    fn resolve_intra_op_threads_lets_the_override_exceed_the_quota() {
        assert_eq!(OnnxBackend::resolve_intra_op_threads(Some("32"), 2).0, 32);
    }

    #[test]
    fn resolve_intra_op_threads_never_resolves_to_zero() {
        assert_eq!(OnnxBackend::resolve_intra_op_threads(None, 0).0, 1);
    }

    #[test]
    fn test_model_path_construction() {
        std::env::set_var("ONNX_MODEL_PATH", "/test/models");
        let backend = OnnxBackend::with_provider(ExecutionProviderType::Cpu).unwrap();

        assert_eq!(
            backend.tokenizer_path("modernbert-embed"),
            PathBuf::from("/test/models/modernbert-embed/tokenizer.json")
        );
        std::env::remove_var("ONNX_MODEL_PATH");
    }
}
