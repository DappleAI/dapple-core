//! External-surface guides client (`external` feature gated).
//!
//! Mirrors a narrow slice of [`crate::resource::guides::GuidesClient`] —
//! only the methods applicable to the external surface
//! (`external-api.dapple.x`):
//!
//! - `list(options)` — `GET /api/guides` (paginated; scoped to the caller's
//!   namespace).
//! - `ingest(bytes)` — `POST /api/guides/ingest` (multipart tarball;
//!   server stamps the caller's namespace + `external` scope on every
//!   produced object_path).
//! - `ingest_from_dir(dir)` — convenience that builds the tarball
//!   in-memory before uploading. Requires the `tarball` feature.
//! - `ingest_version(path_id, bytes, backfill)` — `PATCH /api/guides/{path_id}/ingest`
//!   (multipart tarball; replaces the named guide's content in place,
//!   namespace-gated so cross-customer PATCH attempts return 404). Pass
//!   `backfill = true` to flip the server into `ForceRepair` mode
//!   (`?backfill=true`), re-materializing every derived entity even when
//!   the content hash is unchanged.
//! - `ingest_version_from_dir(path_id, dir, backfill)` — directory
//!   companion. Requires the `tarball` feature.
//! - `upsert_from_dir(dir, backfill)` — smart helper for idempotent ingest.
//!   Reads `title` from the directory's `guide.toml`, derives the slug
//!   locally (mirroring `rev.slugify`), pages through the caller's guides
//!   looking for a match, and either PATCHes the existing path
//!   (threading `backfill`) or POSTs a new guide. Returns [`UpsertOutcome`]
//!   so callers can tell which branch they hit. Requires the `tarball`
//!   feature.
//!
//! Internal-only endpoints (canonical sync, render, cards, favorites,
//! shared content, etc.) are absent by construction. There's no compile-
//! time path from `ExternalDappleClient::guides()` to any internal-only
//! method — the type-level separation is the point.

use crate::types::{FilterOptions, Guide, IngestGuideResponse};
use crate::{ClientCore, SdkError};
use reqwest::multipart::{Form, Part};
use uuid::Uuid;

/// Customer-side guides client. Reached via
/// [`crate::ExternalDappleClient::guides`].
pub struct ExternalGuidesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> ExternalGuidesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    /// GET /api/guides — list the customer's guides, scoped to the caller's
    /// namespace at `scope = 'external'` server-side. Paginated via
    /// `FilterOptions` (`limit`, `page`) on the flat wire shape that the
    /// server's `ListExternalGuidesQuery` accepts.
    ///
    /// Pass `None` to take the default page (page=1, limit=50). To walk a
    /// namespace larger than one page, increment `page` until the response
    /// is empty.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Guide>, SdkError> {
        #[derive(serde::Serialize, Default)]
        struct Query {
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<usize>,
            #[serde(skip_serializing_if = "Option::is_none")]
            page: Option<usize>,
        }
        let q = match options {
            Some(o) => Query {
                limit: o.limit,
                page: o.page,
            },
            None => Query::default(),
        };
        self.client.get_many("api/guides", &q).await
    }

    /// Ingest a guide from a pre-built tarball (`.tar.gz` bytes).
    ///
    /// The server materializes the guide into the caller's namespace at
    /// `scope = 'external'` via the per-transaction GUC pattern; this
    /// SDK doesn't need to specify the namespace explicitly — it's
    /// derived from the JWT.
    pub async fn ingest(&self, tarball_bytes: Vec<u8>) -> Result<IngestGuideResponse, SdkError> {
        let part = Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .map_err(|e| SdkError::Config(format!("invalid mime type: {e}")))?;
        let form = Form::new().part("file", part);
        self.client.post_multipart("api/guides/ingest", form).await
    }

    /// Ingest a guide from a local directory by building a tarball in
    /// memory first. Requires the `tarball` feature.
    #[cfg(feature = "tarball")]
    pub async fn ingest_from_dir(
        &self,
        dir: &std::path::Path,
    ) -> Result<IngestGuideResponse, SdkError> {
        let tarball_bytes = crate::resource::guides::create_tarball_from_dir(dir)?;
        self.ingest(tarball_bytes).await
    }

    /// PATCH /api/guides/{path_id}/ingest — re-ingest an existing guide.
    ///
    /// `backfill` maps to the server's `?backfill=true` query flag. When
    /// `true` the ingest runs in `ForceRepair` mode: every derived entity
    /// (tasks, prompts, inference profiles, briefs/groups/packs) is
    /// re-materialized as a fresh version even when its content hash is
    /// unchanged. Use it to repair downstream state that drifted or that
    /// was produced before a materializer fix landed. Leave it `false` for
    /// the ordinary content-hash-honoring re-ingest (REUSE unchanged
    /// entities, PATCH only what actually changed).
    ///
    /// The server is namespace-gated: a foreign or unknown `path_id`
    /// returns 404 `RESOURCE_NOT_FOUND` with no existence leak. The
    /// response body discriminates the two server-side outcomes via an
    /// `outcome` field on `IngestGuideResponse.extra`:
    /// * `"refreshed"` — the tarball's content hash already existed for
    ///   this path; derived entities were re-materialized in place
    ///   (HTTP 200). With `backfill = true` the derived entities are
    ///   re-versioned even on this branch.
    /// * `"new_version"` — the content hash was new; a new
    ///   versioned_object was created under the same `path_id`
    ///   (HTTP 201).
    pub async fn ingest_version(
        &self,
        path_id: Uuid,
        tarball_bytes: Vec<u8>,
        backfill: bool,
    ) -> Result<IngestGuideResponse, SdkError> {
        let part = Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .map_err(|e| SdkError::Config(format!("invalid mime type: {e}")))?;
        let form = Form::new().part("file", part);
        // `patch_multipart` appends `path` to the base URL verbatim, so a
        // trailing query string rides along untouched.
        let path = if backfill {
            format!("api/guides/{path_id}/ingest?backfill=true")
        } else {
            format!("api/guides/{path_id}/ingest")
        };
        self.client.patch_multipart(&path, form).await
    }

    /// PATCH companion that builds the tarball in-memory from a local
    /// directory. `backfill` is forwarded to
    /// [`ingest_version`](Self::ingest_version). Requires the `tarball`
    /// feature.
    #[cfg(feature = "tarball")]
    pub async fn ingest_version_from_dir(
        &self,
        path_id: Uuid,
        dir: &std::path::Path,
        backfill: bool,
    ) -> Result<IngestGuideResponse, SdkError> {
        let tarball_bytes = crate::resource::guides::create_tarball_from_dir(dir)?;
        self.ingest_version(path_id, tarball_bytes, backfill).await
    }

    /// Smart idempotent ingest. Reads `guide.toml` from `dir` to discover
    /// the title, derives the slug client-side (mirroring `rev.slugify`),
    /// pages through the caller's guides looking for a match by slug, and
    /// then either:
    /// * `PATCH /api/guides/{path_id}/ingest` if a match was found
    ///   ([`UpsertOutcome::Updated`]), or
    /// * `POST /api/guides/ingest` if no match was found
    ///   ([`UpsertOutcome::Created`]).
    ///
    /// `backfill` is threaded into the PATCH (Updated) branch only — see
    /// [`ingest_version`](Self::ingest_version) for its `ForceRepair`
    /// semantics. The first-ingest POST (Created) branch ignores it: there
    /// is no prior version to repair.
    ///
    /// Pages 100 guides at a time, capped at
    /// [`UPSERT_MAX_PAGES`](Self::UPSERT_MAX_PAGES) (6,400 guides) — beyond
    /// that the helper returns a `Config` error to avoid runaway walks.
    /// Customers approaching that scale need a server-side by-slug lookup
    /// endpoint instead.
    ///
    /// Requires the `tarball` feature.
    #[cfg(feature = "tarball")]
    pub async fn upsert_from_dir(
        &self,
        dir: &std::path::Path,
        backfill: bool,
    ) -> Result<UpsertOutcome, SdkError> {
        let guide_toml_path = dir.join("guide.toml");
        let title = crate::resource::upsert::read_guide_title(&guide_toml_path)?;
        // Look up by the source_path-basename slug (the dir = slug rule), NOT
        // slugify(title). External callers can't pass a `?source_path` override, so
        // the source_path comes only from guide.toml.
        let source_path = crate::resource::upsert::read_guide_source_path(&guide_toml_path);
        let target_slug = crate::resource::upsert::lookup_slug(source_path.as_deref(), &title);

        // Walk pages until match or exhausted.
        let mut found: Option<Guide> = None;
        for page in 1..=Self::UPSERT_MAX_PAGES {
            let listed = self
                .list(Some(FilterOptions {
                    page: Some(page),
                    limit: Some(Self::UPSERT_PAGE_SIZE),
                }))
                .await?;
            if listed.is_empty() {
                break;
            }
            if let Some(hit) = listed.into_iter().find(|g| g.slug == target_slug) {
                found = Some(hit);
                break;
            }
        }

        let tarball_bytes = crate::resource::guides::create_tarball_from_dir(dir)?;

        match found {
            Some(g) => {
                let response = self
                    .ingest_version(g.path_id, tarball_bytes, backfill)
                    .await?;
                Ok(UpsertOutcome::Updated(response))
            }
            None => {
                // First ingest (POST): no prior version exists to repair, so
                // `backfill` is irrelevant on this branch.
                let response = self.ingest(tarball_bytes).await?;
                Ok(UpsertOutcome::Created(response))
            }
        }
    }

    /// Maximum number of pages [`upsert_from_dir`](Self::upsert_from_dir)
    /// will walk before giving up. Combined with the helper's 100-per-page
    /// step, this caps the lookup at 6,400 guides per namespace.
    pub const UPSERT_MAX_PAGES: usize = 64;

    /// Page size [`upsert_from_dir`](Self::upsert_from_dir) uses when
    /// walking the caller's guides looking for a slug match.
    pub const UPSERT_PAGE_SIZE: usize = 100;
}

/// Re-export of the shared upsert outcome. Both
/// `ExternalGuidesClient::upsert_from_dir` and the internal
/// `GuidesClient::upsert_from_dir` return the same shape so consumers can
/// switch on the branch without surface-specific imports.
#[cfg(feature = "tarball")]
pub use crate::resource::upsert::UpsertOutcome;
