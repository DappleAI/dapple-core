//! Resource client modules for the Dapple Prompt API.
//!
//! Each module provides a typed client for a specific API resource,
//! with ergonomic methods that handle query serialization, envelope
//! unwrapping, and authentication.

pub mod api_tokens;
pub mod assets;
pub mod brief_groups;
pub mod brief_packs;
pub mod brief_units;
pub mod briefs;
pub mod categories;
pub mod deprecations;
pub mod directives;
pub mod dispatch_intent_bindings;
pub mod dispatch_phases;
pub mod dispatch_task_attempts;
pub mod dispatches;
pub mod drafts;
#[cfg(feature = "external")]
pub mod external_dispatch_intent_bindings;
#[cfg(feature = "external")]
pub mod external_dispatches;
#[cfg(feature = "external")]
pub mod external_guides;
#[cfg(feature = "external")]
pub mod external_journeys;
#[cfg(feature = "external")]
pub mod external_playbooks;
#[cfg(feature = "external")]
pub mod external_value_slots;
pub mod fluent_messages;
pub mod guides;
pub mod hats;
pub mod inference_profiles;
pub mod journeys;
pub mod playbooks;
pub mod prompts;
pub mod stages;
pub mod steps;
pub mod tasks;
#[cfg(feature = "tarball")]
pub mod upsert;
pub mod user_details;
pub mod value_slots;
pub mod versioned_objects;
