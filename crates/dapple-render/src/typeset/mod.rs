//! The Typst backend — markdown in, PDF bytes out.
//!
//! (Named `typeset` rather than `typst` so `use typst::…` paths inside the
//! crate unambiguously refer to the external engine crate.)
//!
//! Pipeline: [`transform`] turns markdown into Typst markup (two passes:
//! anchor-id collection, then emission), [`brand`] wraps it in the Dapple
//! preamble (page furniture, heading rules, callouts, colophon), [`world`]
//! provides the hermetic compilation environment (embedded fonts, virtual
//! files, no filesystem/network), and [`compile`] runs the engine and exports
//! the PDF.

pub mod brand;
pub mod compile;
pub mod escape;
pub mod transform;
pub mod world;
