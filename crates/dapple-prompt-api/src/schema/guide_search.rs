use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Query parameters for the guide search endpoint.
///
/// Search uses Conduit's embedding service to encode the query text,
/// then runs pgvector similarity search with audience/locale filtering.
#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct GuideSearchParams {
    /// Natural language search query.
    pub q: String,
    /// Audience tags to filter by (e.g., ["product", "universal"]).
    /// Defaults to ["universal"] if omitted.
    pub tags: Option<Vec<String>>,
    /// Locale tag for filtering (e.g., "en", "es", "pt-br").
    /// Defaults to "en" if omitted.
    pub locale: Option<String>,
    /// Maximum number of results. Clamped to 1..100.
    /// Defaults to 20 if omitted.
    pub limit: Option<i32>,
    /// Scope search to specific guide IDs (for agent RAG use cases).
    /// When omitted, searches all guides.
    pub guide_ids: Option<Vec<Uuid>>,
    /// Minimum similarity score threshold (0.0 to 1.0).
    /// Results below this score are excluded. Defaults to 0.0.
    pub threshold: Option<f64>,
}

impl GuideSearchParams {
    /// Resolve defaults and clamp values.
    pub fn resolve(&self) -> ResolvedSearchParams {
        ResolvedSearchParams {
            q: self.q.clone(),
            tags: self
                .tags
                .clone()
                .unwrap_or_else(|| vec!["universal".to_string()]),
            locale: self.locale.clone().unwrap_or_else(|| "en".to_string()),
            limit: self.limit.unwrap_or(20).clamp(1, 100),
            guide_ids: self.guide_ids.clone(),
            threshold: self.threshold.unwrap_or(0.0),
        }
    }
}

/// Resolved search parameters with defaults applied.
pub struct ResolvedSearchParams {
    pub q: String,
    pub tags: Vec<String>,
    pub locale: String,
    pub limit: i32,
    pub guide_ids: Option<Vec<Uuid>>,
    pub threshold: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_defaults() {
        let params = GuideSearchParams {
            q: "test query".into(),
            tags: None,
            locale: None,
            limit: None,
            guide_ids: None,
            threshold: None,
        };
        let resolved = params.resolve();
        assert_eq!(resolved.tags, vec!["universal".to_string()]);
        assert_eq!(resolved.locale, "en");
        assert_eq!(resolved.limit, 20);
        assert_eq!(resolved.threshold, 0.0);
    }

    #[test]
    fn test_resolve_limit_clamping() {
        let params = GuideSearchParams {
            q: "test".into(),
            tags: None,
            locale: None,
            limit: Some(500),
            guide_ids: None,
            threshold: None,
        };
        assert_eq!(params.resolve().limit, 100);

        let params2 = GuideSearchParams {
            q: "test".into(),
            tags: None,
            locale: None,
            limit: Some(-5),
            guide_ids: None,
            threshold: None,
        };
        assert_eq!(params2.resolve().limit, 1);
    }

    #[test]
    fn test_resolve_with_explicit_values() {
        let params = GuideSearchParams {
            q: "my search".into(),
            tags: Some(vec!["product".into(), "agent".into()]),
            locale: Some("es".into()),
            limit: Some(50),
            guide_ids: None,
            threshold: Some(0.5),
        };
        let resolved = params.resolve();
        assert_eq!(resolved.q, "my search");
        assert_eq!(resolved.tags, vec!["product", "agent"]);
        assert_eq!(resolved.locale, "es");
        assert_eq!(resolved.limit, 50);
        assert_eq!(resolved.threshold, 0.5);
    }
}
