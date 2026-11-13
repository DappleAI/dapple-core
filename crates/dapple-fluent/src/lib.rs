//! Fluent-inspired message resolution for dapple-prompt
//!
//! This crate provides:
//! - Fallback chain builders for locale and engine resolution
//! - Simple variable substitution with $variable syntax
//! - Deterministic emission of guide-shaped Fluent documents
//!   (`emit::emit_guide`) used by the guide-builder pipeline
//!
//! The resolution logic uses an "AND gateway" pattern where:
//! - Rust builds fallback chains from user context
//! - PostgreSQL narrows candidates with WHERE and picks best with ORDER BY
//! - Variable substitution happens after resolution

mod chain;
pub mod emit;
mod substitute;

pub use chain::{engine_chain, locale_chain};
pub use emit::{emit_guide, BriefPack, GuideFluent, InferenceProfile, NamedMessage, TaskMessage};
pub use substitute::{substitute, substitute_tracking};
