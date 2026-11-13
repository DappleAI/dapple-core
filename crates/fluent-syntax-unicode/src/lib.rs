//! Workspace-local fork of [`fluent-syntax`](https://crates.io/crates/fluent-syntax)
//! v0.11.1 with a Unicode-aware identifier scanner. See this crate's
//! `README.md` for the diff against upstream.
pub mod ast;
pub mod parser;
pub mod serializer;
