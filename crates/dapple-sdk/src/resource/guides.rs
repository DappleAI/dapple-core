//! Client for the `/api/guides/` endpoints.
//!
//! Supports CRUD operations, tarball-based ingest, PDF rendering,
//! guide card queries, and favorites.

use crate::types::{
    AddFavoriteGuideResponse, CheckFavoriteGuideResponse, CreateGuideSchema, CreateGuideSetSchema,
    DeleteQueryParams, FilterOptions, GetGuideCardSetSchema, GetGuideSetSchema, Guide,
    GuideCardCarouselItem, IngestGuideResponse, IngestParams, ListFavoriteGuidesResponse,
    RemoveFavoriteGuideResponse,
};
use crate::{ClientCore, SdkError};
use reqwest::multipart::{Form, Part};
use uuid::Uuid;

/// Re-export of the shared upsert outcome — same enum returned by
/// `ExternalGuidesClient::upsert_from_dir`.
#[cfg(feature = "tarball")]
pub use crate::resource::upsert::UpsertOutcome;

/// Client for guide operations.
pub struct GuidesClient<'a> {
    client: &'a ClientCore,
}

impl<'a> GuidesClient<'a> {
    pub(crate) fn new(client: &'a ClientCore) -> Self {
        Self { client }
    }

    // ========================================================================
    // Standard CRUD
    // ========================================================================

    /// Get a single guide by object ID.
    pub async fn get(&self, id: Uuid) -> Result<Guide, SdkError> {
        self.client.get_one(&format!("api/guides/{id}")).await
    }

    /// List all guides.
    pub async fn list(&self, options: Option<FilterOptions>) -> Result<Vec<Guide>, SdkError> {
        let query = GetGuideSetSchema::All {
            scope: None,
            environment: None,
            options,
        };
        self.client.get_many("api/guides/", &query).await
    }

    /// List guides by object IDs.
    pub async fn list_by_ids(
        &self,
        ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Guide>, SdkError> {
        let query = GetGuideSetSchema::ByObjectIds { ids, options };
        self.client.get_many("api/guides/", &query).await
    }

    /// List guides by slug names.
    pub async fn list_by_slugs(
        &self,
        slugs: Vec<String>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Guide>, SdkError> {
        let query = GetGuideSetSchema::BySlugs {
            slugs,
            scope: None,
            environment: None,
            options,
        };
        self.client.get_many("api/guides/", &query).await
    }

    /// List guides by path object IDs.
    pub async fn list_by_path_ids(
        &self,
        path_ids: Vec<Uuid>,
        options: Option<FilterOptions>,
    ) -> Result<Vec<Guide>, SdkError> {
        let query = GetGuideSetSchema::ByPathIds { path_ids, options };
        self.client.get_many("api/guides/", &query).await
    }

    /// Get a single guide by slug. Returns the first match.
    pub async fn get_by_slug(&self, slug: &str) -> Result<Guide, SdkError> {
        let results = self.list_by_slugs(vec![slug.to_string()], None).await?;
        results.into_iter().next().ok_or_else(|| SdkError::Api {
            status: 404,
            message: format!("guide with slug '{slug}' not found"),
        })
    }

    /// Create one or more guides (direct schema creation, not ingest).
    pub async fn create(&self, objs: Vec<CreateGuideSchema>) -> Result<Vec<Guide>, SdkError> {
        let body = CreateGuideSetSchema { objs };
        self.client.post_many("api/guides/", &body).await
    }

    /// Delete (deprecate) a single guide by object ID.
    pub async fn delete_one(&self, id: Uuid) -> Result<Guide, SdkError> {
        self.client.delete_one(&format!("api/guides/{id}")).await
    }

    /// Deprecate a single guide by path object ID with level and reason.
    pub async fn deprecate_one(
        &self,
        id: Uuid,
        level: Option<String>,
        reason: Option<String>,
    ) -> Result<Guide, SdkError> {
        let params = DeleteQueryParams { level, reason };
        let results: Vec<Guide> = self
            .client
            .delete_many(&format!("api/guides/{id}"), &params)
            .await?;
        results.into_iter().next().ok_or_else(|| SdkError::Api {
            status: 404,
            message: format!("guide '{id}' not returned in deprecation response"),
        })
    }

    // ========================================================================
    // Ingest
    // ========================================================================

    /// Ingest a guide from a pre-built tarball (`.tar.gz` bytes).
    pub async fn ingest(
        &self,
        tarball_bytes: Vec<u8>,
        params: IngestParams,
    ) -> Result<IngestGuideResponse, SdkError> {
        let path = build_ingest_path("api/guides/ingest", &params);
        let part = Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .map_err(|e| SdkError::Config(format!("invalid mime type: {e}")))?;
        let form = Form::new().part("file", part);
        self.client.post_multipart(&path, form).await
    }

    /// Ingest a guide from a local directory by creating a tarball in memory.
    #[cfg(feature = "tarball")]
    pub async fn ingest_from_dir(
        &self,
        dir: &std::path::Path,
        params: IngestParams,
    ) -> Result<IngestGuideResponse, SdkError> {
        let tarball_bytes = create_tarball_from_dir(dir)?;
        self.ingest(tarball_bytes, params).await
    }

    /// Ingest a new version of an existing guide from a tarball (PATCH).
    pub async fn ingest_version(
        &self,
        path_id: Uuid,
        tarball_bytes: Vec<u8>,
        params: IngestParams,
    ) -> Result<IngestGuideResponse, SdkError> {
        let path = build_ingest_path(&format!("api/guides/{path_id}/ingest"), &params);
        let part = Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .map_err(|e| SdkError::Config(format!("invalid mime type: {e}")))?;
        let form = Form::new().part("file", part);
        self.client.patch_multipart(&path, form).await
    }

    /// Smart idempotent ingest. Reads `guide.toml` from `dir` to discover
    /// the title, derives the slug client-side (mirroring `rev.slugify`),
    /// pages through guides looking for a match by slug, and then either:
    /// * `PATCH /api/guides/{path_id}/ingest` if a match was found
    ///   ([`UpsertOutcome::Updated`]), or
    /// * `POST /api/guides/ingest` if no match was found
    ///   ([`UpsertOutcome::Created`]).
    ///
    /// Mirrors `ExternalGuidesClient::upsert_from_dir` so callers writing
    /// against either surface get the same ergonomics. Consumers that
    /// previously rolled their own "look up by slug → POST or PATCH"
    /// resolution (notably `dapple-suite`'s `sync_guide_from_dir`) should
    /// migrate to this helper.
    ///
    /// Pages 100 guides at a time, capped at
    /// [`UPSERT_MAX_PAGES`](Self::UPSERT_MAX_PAGES) (6,400 guides).
    /// Requires the `tarball` feature.
    #[cfg(feature = "tarball")]
    pub async fn upsert_from_dir(
        &self,
        dir: &std::path::Path,
        params: IngestParams,
    ) -> Result<UpsertOutcome, SdkError> {
        let guide_toml_path = dir.join("guide.toml");
        let title = crate::resource::upsert::read_guide_title(&guide_toml_path)?;
        // Look up by the source_path-basename slug (the dir = slug rule + migration
        // 73), NOT slugify(title): a title-derived slug misses the existing guide,
        // falls through to POST, and trips the server's title-UNIQUE. Prefer the
        // `?source_path` override the request will actually send; else guide.toml's.
        let source_path = params
            .source_path
            .clone()
            .or_else(|| crate::resource::upsert::read_guide_source_path(&guide_toml_path));
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

        let tarball_bytes = create_tarball_from_dir(dir)?;

        match found {
            Some(g) => {
                let response = self
                    .ingest_version(g.path_id, tarball_bytes, params)
                    .await?;
                Ok(UpsertOutcome::Updated(response))
            }
            None => {
                let response = self.ingest(tarball_bytes, params).await?;
                Ok(UpsertOutcome::Created(response))
            }
        }
    }

    /// Maximum number of pages [`upsert_from_dir`](Self::upsert_from_dir)
    /// will walk before giving up. Combined with the helper's 100-per-page
    /// step, this caps the lookup at 6,400 guides.
    #[cfg(feature = "tarball")]
    pub const UPSERT_MAX_PAGES: usize = 64;

    /// Page size [`upsert_from_dir`](Self::upsert_from_dir) uses when
    /// walking guides looking for a slug match.
    #[cfg(feature = "tarball")]
    pub const UPSERT_PAGE_SIZE: usize = 100;

    // ========================================================================
    // Render
    // ========================================================================

    /// Render a guide as a PDF. Returns the raw PDF bytes.
    pub async fn render_pdf(
        &self,
        guide_id: Uuid,
        profile: &str,
    ) -> Result<bytes::Bytes, SdkError> {
        #[derive(serde::Serialize)]
        struct RenderBody<'a> {
            profile: &'a str,
        }
        let body = RenderBody { profile };
        self.client
            .post_bytes(&format!("api/guides/{guide_id}/render"), &body)
            .await
    }

    // ========================================================================
    // Cards
    // ========================================================================

    /// List all guide cards.
    pub async fn cards(
        &self,
        options: Option<FilterOptions>,
    ) -> Result<Vec<GuideCardCarouselItem>, SdkError> {
        let query = GetGuideCardSetSchema::All {
            scope: None,
            environment: None,
            options,
        };
        self.client.get_many("api/guides/cards", &query).await
    }

    /// List guide cards filtered by category.
    pub async fn cards_by_category(
        &self,
        category: &str,
        options: Option<FilterOptions>,
    ) -> Result<Vec<GuideCardCarouselItem>, SdkError> {
        let query = GetGuideCardSetSchema::ByCategory {
            category: category.to_string(),
            scope: None,
            environment: None,
            options,
        };
        self.client.get_many("api/guides/cards", &query).await
    }

    // ========================================================================
    // Favorites
    // ========================================================================

    /// Add a guide to favorites.
    pub async fn favorite(&self, id: Uuid) -> Result<AddFavoriteGuideResponse, SdkError> {
        self.client
            .post_one(&format!("api/guides/{id}/favorite"), &())
            .await
    }

    /// Remove a guide from favorites.
    pub async fn unfavorite(&self, id: Uuid) -> Result<RemoveFavoriteGuideResponse, SdkError> {
        self.client
            .delete_one(&format!("api/guides/{id}/favorite"))
            .await
    }

    /// Check if a guide is a favorite.
    pub async fn is_favorite(&self, id: Uuid) -> Result<CheckFavoriteGuideResponse, SdkError> {
        self.client
            .get_one(&format!("api/guides/{id}/favorite"))
            .await
    }

    /// List all favorite guides.
    pub async fn favorites(&self) -> Result<ListFavoriteGuidesResponse, SdkError> {
        self.client.get_one("api/guides/favorites").await
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Build the ingest endpoint path with optional query parameters.
fn build_ingest_path(base: &str, params: &IngestParams) -> String {
    let mut query_parts = Vec::new();
    if params.dry_run {
        query_parts.push("dry_run=true".to_string());
    }
    if let Some(ref sp) = params.source_path {
        let encoded: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("source_path", sp)
            .finish();
        query_parts.push(encoded);
    }
    if params.backfill {
        query_parts.push("backfill=true".to_string());
    }
    if query_parts.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", query_parts.join("&"))
    }
}

/// Create a `.tar.gz` archive from a directory.
///
/// Shared with `ExternalGuidesClient::ingest_from_dir` under the
/// `external` + `tarball` features.
#[cfg(feature = "tarball")]
pub(crate) fn create_tarball_from_dir(dir: &std::path::Path) -> Result<Vec<u8>, SdkError> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let buf = Vec::new();
    let encoder = GzEncoder::new(buf, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    archive.append_dir_all(".", dir)?;
    archive.finish()?;
    let encoder = archive.into_inner()?;
    let compressed = encoder.finish()?;
    Ok(compressed)
}
