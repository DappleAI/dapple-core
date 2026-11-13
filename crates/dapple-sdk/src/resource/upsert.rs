//! Shared helpers for the `upsert_from_dir` smart-ingest helper exposed on
//! both [`crate::resource::guides::GuidesClient`] and
//! [`crate::resource::external_guides::ExternalGuidesClient`].
//!
//! The two surfaces share the same upsert ergonomics: read `guide.toml`'s
//! title, derive the slug locally (mirroring SQL `rev.slugify` byte-for-byte
//! — same ASCII-only stripping that
//! `crates/dapple-data/src/slugify.rs` does), page through the caller's
//! guides looking for a slug match, then dispatch to PATCH or POST. Having
//! one implementation here means consumers (acme-sdk, dapple-suite,
//! dapple-ui) get the same behavior on either surface without each having
//! to roll their own resolution logic.
//!
//! The `tarball` feature gates everything here because the only callers
//! (`upsert_from_dir`) need to build a tarball from a directory.

#![cfg(feature = "tarball")]

use crate::SdkError;
use crate::types::IngestGuideResponse;

/// Outcome of a smart upsert. Both surfaces return the same enum so a
/// consumer can switch on the branch without surface-specific imports.
///
/// The wrapped [`IngestGuideResponse`] is the raw wire response from the
/// server (POST returns `{ status, guide, derived_objects }`; PATCH returns
/// the same plus an `outcome: "refreshed" | "new_version"` discriminator,
/// reachable via `response.extra`).
#[derive(Debug)]
pub enum UpsertOutcome {
    /// Server-side `POST /api/guides/ingest` ran — no existing guide matched
    /// the derived slug in the caller's namespace.
    Created(IngestGuideResponse),
    /// Server-side `PATCH /api/guides/{path_id}/ingest` ran — an existing
    /// guide matched the derived slug. The wire response's `outcome` field
    /// (in `extra`) distinguishes "refreshed" (same content hash, derived
    /// entities re-materialized in place) from "new_version" (new content
    /// hash, a new versioned_object created under the same path).
    Updated(IngestGuideResponse),
}

/// Read the top-level `title` field out of a guide's `guide.toml`. Uses a
/// minimal `toml::Value` lookup to avoid pulling `dapple-parser` into the
/// SDK's dependency closure.
pub(crate) fn read_guide_title(path: &std::path::Path) -> Result<String, SdkError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        SdkError::Config(format!(
            "failed to read guide.toml at {}: {}",
            path.display(),
            e
        ))
    })?;
    let parsed: toml::Value = toml::from_str(&content)
        .map_err(|e| SdkError::Config(format!("invalid guide.toml: {e}")))?;
    parsed
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| SdkError::Config("guide.toml is missing the `title` field".to_string()))
}

/// Read the optional top-level `source_path` field out of a guide's `guide.toml`.
/// Returns `None` when the field is absent (or the file can't be read/parsed —
/// callers read [`read_guide_title`] first, so by here the toml is valid). The
/// upsert lookup slug derives from the source_path directory basename, so this is
/// how `upsert_from_dir` discovers the source_path the server will store the slug
/// from (when no `?source_path` override is supplied).
pub(crate) fn read_guide_source_path(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: toml::Value = toml::from_str(&content).ok()?;
    parsed
        .get("source_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Derive the guide-upsert lookup slug exactly the way the server does: the
/// `source_path` directory basename run through [`slugify`], falling back to the
/// title when no source_path is available. Mirrors `insert_guides.sqlc`
/// (`slugify(COALESCE(NULLIF(regexp_replace(source_path, '^.*/', ''), ''), title))`)
/// and migration 73 byte-for-byte, so the client's POST-or-PATCH lookup matches
/// the stored `guide_path_details.slug`. `basename` strips everything through the
/// last `/`; a slashless path passes through unchanged; an empty/`/`-terminated
/// path falls back to the title.
///
/// This replaced a bare `slugify(title)` lookup: once the server slug rule became
/// "directory basename", a title-derived lookup missed the existing guide, fell
/// through to POST, and tripped the server's title-UNIQUE.
pub(crate) fn lookup_slug(source_path: Option<&str>, title: &str) -> String {
    let basename = source_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.rsplit('/').next())
        .filter(|s| !s.is_empty());
    slugify(basename.unwrap_or(title))
}

/// Port of `rev.slugify` (see
/// `crates/dapple-data/migrations/00000000000042_unicode_slugify.sql`
/// and the Rust mirror at `crates/dapple-data/src/slugify.rs`).
///
/// **Unicode-aware:** Accented Latin characters and other Unicode letters
/// are preserved. SQL strips characters outside `[[:alnum:]\s_-]`, which
/// in a UTF-8 database matches Unicode letters and digits; `is_alphanumeric`
/// here covers the same set. The SDK upsert helper looks up by slug (via
/// [`lookup_slug`], which slugifies the source_path basename), so this function
/// and SQL's `rev.slugify` must agree byte-for-byte — a divergence misses the
/// existing row, falls through to POST, and trips the server's name-UNIQUE.
pub(crate) fn slugify(input: &str) -> String {
    let lowered = input.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_ascii_whitespace() || *c == '_' || *c == '-')
        .collect();

    let mut out = String::with_capacity(cleaned.len());
    let mut prev_dash = false;
    for c in cleaned.chars() {
        if c.is_ascii_whitespace() {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.push(c);
            prev_dash = c == '-';
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::{lookup_slug, slugify};

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn slugify_matches_rev_slugify_fixtures() {
        // Mirror the cases the SQL function handles. Each pair was
        // verified against `SELECT rev.slugify($1)` in psql.
        let cases = &[
            ("What If New Menu Item", "what-if-new-menu-item"),
            ("Essay Helper Pack", "essay-helper-pack"),
            ("Stuart's Librarian Guide", "stuarts-librarian-guide"),
            ("   leading and trailing   ", "leading-and-trailing"),
            ("multiple   spaces", "multiple-spaces"),
            ("UPPER_CASE-mixed", "upper_case-mixed"),
            ("punctuation!?.,;:", "punctuation"),
            ("number-9 of 10", "number-9-of-10"),
        ];
        for (input, expected) in cases {
            assert_eq!(slugify(input), *expected, "input={:?}", input);
        }
    }

    /// Accented Latin characters are preserved (Unicode-aware slugify).
    /// Must match `dapple_data::slugify` and `rev.slugify` byte-for-byte.
    #[test]
    fn slugify_preserves_accented_latin() {
        let cases = &[
            ("Como Lida com Alergênios", "como-lida-com-alergênios"),
            ("E se contratasse um padeiro", "e-se-contratasse-um-padeiro"),
            (
                "Correspondência aproximada de linhas de fatura",
                "correspondência-aproximada-de-linhas-de-fatura",
            ),
            (
                "Menu para um evento específico",
                "menu-para-um-evento-específico",
            ),
            (
                "Onde está a adivinhar vs a medir",
                "onde-está-a-adivinhar-vs-a-medir",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(slugify(input), *expected, "input={:?}", input);
        }
    }

    #[test]
    fn slugify_collapses_dashes_only_via_whitespace() {
        // The SQL function does NOT collapse adjacent literal dashes.
        // The Rust port matches that — collapsing happens only through
        // the whitespace → '-' substitution path.
        assert_eq!(slugify("foo--bar"), "foo--bar");
    }

    // ---- lookup_slug: the source_path-basename lookup key (dir = slug) --------

    #[test]
    fn lookup_slug_uses_source_path_basename_not_title() {
        // The regression case: title and folder differ; the lookup must follow
        // the folder (basename), matching the server's stored slug.
        assert_eq!(
            lookup_slug(
                Some("public/dapple/learn-from-gaming-vod"),
                "Dapple Learns From a Gaming VOD"
            ),
            "learn-from-gaming-vod"
        );
    }

    #[test]
    fn lookup_slug_slashless_source_path_passes_through() {
        assert_eq!(
            lookup_slug(Some("learn-from-gaming-vod"), "Whatever Title"),
            "learn-from-gaming-vod"
        );
    }

    #[test]
    fn lookup_slug_slugifies_the_basename() {
        // A basename with caps/spaces is still slugified (mirrors the server's
        // slugify(basename)).
        assert_eq!(
            lookup_slug(Some("public/dapple/My Guide"), "Ignored"),
            "my-guide"
        );
    }

    #[test]
    fn lookup_slug_falls_back_to_title() {
        // None / empty / whitespace-only / trailing-slash source_path → the
        // COALESCE(..., title) fallback, exactly like insert_guides.sqlc.
        assert_eq!(lookup_slug(None, "Essay Helper"), "essay-helper");
        assert_eq!(lookup_slug(Some(""), "Essay Helper"), "essay-helper");
        assert_eq!(lookup_slug(Some("   "), "Essay Helper"), "essay-helper");
        assert_eq!(lookup_slug(Some("foo/"), "Essay Helper"), "essay-helper");
    }
}
