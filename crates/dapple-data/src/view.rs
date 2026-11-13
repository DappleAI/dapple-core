//! Cross-handler view-building helpers.
//!
//! Some endpoints share fold logic across the internal and external surfaces
//! (e.g. brief-form / bindings). The internal surface cannot be imported by
//! the external surface (and vice versa) per the multi-surface AGENTS.md
//! anti-patterns, so the shared builders live here, against the data-layer
//! row types and `dapple-types` DTOs.

pub mod journey_bindings;
pub mod journey_brief_form;
