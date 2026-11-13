//! External-surface schema for `GET /api/dispatches/{id}/task_attempts`.
//!
//! Unlike most external schemas (which re-export the internal type verbatim),
//! the task-attempt wire format is a deliberate **projection**: the internal
//! `DispatchTaskAttempt` carries `context_metadata`, a control-plane grab-bag
//! whose shape depends on the event type (token metrics + author signals on
//! task-outcome events, but dapple-side directive identifiers on internal
//! `flow_directive` audit events). The customer surface exposes only the
//! author-emitted `signals` channel, so it serves
//! [`ExternalDispatchTaskAttempt`] (defined in `dapple-types`, shared with the
//! SDK), which replaces `context_metadata` with a projected `signals` field.

pub use dapple_data::model::dispatch_task_attempts::ExternalDispatchTaskAttempt;
