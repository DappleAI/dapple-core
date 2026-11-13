//! Custom query-string extractor that supports internally-tagged enums with Vec fields.
//!
//! `axum::extract::Query` delegates to `serde_urlencoded`, which cannot deserialise
//! `#[serde(tag = "query")]` enums when variants contain `Vec` fields.  This module
//! provides [`QsQuery<T>`] which:
//!
//! 1. Parses the raw query string into a `serde_json::Value` (handling bracket notation,
//!    repeated keys, and JSON-encoded values).
//! 2. Deserialises `T` via `serde_json::from_value`, which handles internally-tagged
//!    enums correctly.
//!
//! No new crate dependencies are required — it uses `url::form_urlencoded` (already a dep)
//! and `serde_json`.

use aide::OperationInput;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::ops::Deref;

/// Extractor that deserialises query parameters via `serde_json`, supporting
/// internally-tagged enums (`#[serde(tag = "query")]`) and bracket-notation arrays
/// (`ids[0]=a&ids[1]=b`).
///
/// Drop-in replacement for `axum::extract::Query<T>`.
#[derive(Debug, Clone)]
pub struct QsQuery<T>(pub T);

impl<T> Deref for QsQuery<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> QsQuery<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

pub struct QsQueryRejection {
    message: String,
}

impl IntoResponse for QsQueryRejection {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, self.message).into_response()
    }
}

impl<T, S> FromRequestParts<S> for QsQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = QsQueryRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query_str = parts.uri.query().unwrap_or("");
        let value = parse_query_string(query_str);
        let t: T = serde_json::from_value(value).map_err(|e| QsQueryRejection {
            message: format!("Failed to deserialize query string: {e}"),
        })?;
        Ok(QsQuery(t))
    }
}

/// Empty impl — query schemas are documented via `QueryParamDocs`, not this trait.
impl<T> OperationInput for QsQuery<T> {}

/// Parse a raw query string into a `serde_json::Value`.
///
/// Handles:
/// - Bracket notation: `ids[0]=a&ids[1]=b` → `{"ids": ["a", "b"]}`
/// - Repeated keys: `ids=a&ids=b` → `{"ids": ["a", "b"]}`
/// - JSON-encoded values: `options={"limit":10}` → `{"options": {"limit": 10}}`
/// - Simple key-value pairs: `query=all` → `{"query": "all"}`
fn parse_query_string(query: &str) -> Value {
    // First pass: collect all raw key-value pairs, grouping bracket-notated keys.
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    // Separate bracket-indexed keys (e.g. `ids[0]`) from plain keys.
    // For bracket keys we track: base_name → BTreeMap<index, value>
    // Using BTreeMap so indices are sorted.
    let mut plain: HashMap<String, Vec<String>> = HashMap::new();
    let mut bracket: HashMap<String, std::collections::BTreeMap<usize, String>> = HashMap::new();

    for (key, val) in &pairs {
        if let Some(pos) = key.find('[') {
            // bracket notation: key[index]
            let base = &key[..pos];
            let idx_str = &key[pos + 1..key.len() - 1]; // strip [ and ]
            if let Ok(idx) = idx_str.parse::<usize>() {
                bracket
                    .entry(base.to_string())
                    .or_default()
                    .insert(idx, val.clone());
            } else {
                // Non-numeric bracket content — treat as plain key
                plain.entry(key.clone()).or_default().push(val.clone());
            }
        } else {
            plain.entry(key.clone()).or_default().push(val.clone());
        }
    }

    let mut map = Map::new();

    // Process bracket-notated arrays
    for (base, idx_map) in bracket {
        let arr: Vec<Value> = idx_map
            .into_values()
            .map(|v| try_parse_json_value(&v))
            .collect();
        map.insert(base, Value::Array(arr));
    }

    // Process plain keys
    for (key, vals) in plain {
        if vals.len() == 1 {
            // Single value — try to parse as JSON (for `options={"limit":10}`)
            let parsed = try_parse_json_value(&vals[0]);

            // If this key already exists from bracket parsing, merge as array
            if let Some(existing) = map.get_mut(&key) {
                if let Value::Array(arr) = existing {
                    arr.push(parsed);
                }
            } else {
                map.insert(key, parsed);
            }
        } else {
            // Multiple values for same key — treat as array (repeated-key notation)
            let arr: Vec<Value> = vals.iter().map(|v| try_parse_json_value(v)).collect();
            map.insert(key, Value::Array(arr));
        }
    }

    Value::Object(map)
}

/// Try to parse a string as JSON. If it fails, return it as a JSON string.
fn try_parse_json_value(s: &str) -> Value {
    // Try parsing as JSON first (handles objects, arrays, numbers, booleans, null)
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        // Only accept structured JSON (objects/arrays) or actual JSON primitives.
        // Bare strings like "all" should remain as strings, not parsed as JSON strings.
        match &v {
            Value::Object(_)
            | Value::Array(_)
            | Value::Number(_)
            | Value::Bool(_)
            | Value::Null => return v,
            Value::String(_) => {
                // serde_json parsed a quoted string like `"foo"` — return the inner string
                return v;
            }
        }
    }
    // Not valid JSON — return as string
    Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "query", rename_all = "snake_case")]
    enum TestQuery {
        All {
            options: Option<TestOptions>,
        },
        ByObjectIds {
            ids: Vec<String>,
            options: Option<TestOptions>,
        },
        BySlugs {
            slugs: Vec<String>,
            options: Option<TestOptions>,
        },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestOptions {
        limit: Option<usize>,
        page: Option<usize>,
    }

    #[test]
    fn test_all_variant() {
        let v = parse_query_string("query=all");
        let q: TestQuery = serde_json::from_value(v).unwrap();
        assert_eq!(q, TestQuery::All { options: None });
    }

    #[test]
    fn test_by_object_ids_bracket() {
        let v = parse_query_string("query=by_object_ids&ids[0]=aaa&ids[1]=bbb");
        let q: TestQuery = serde_json::from_value(v).unwrap();
        assert_eq!(
            q,
            TestQuery::ByObjectIds {
                ids: vec!["aaa".to_string(), "bbb".to_string()],
                options: None,
            }
        );
    }

    #[test]
    fn test_by_slugs_bracket() {
        let v = parse_query_string("query=by_slugs&slugs[0]=send-email&slugs[1]=review-doc");
        let q: TestQuery = serde_json::from_value(v).unwrap();
        assert_eq!(
            q,
            TestQuery::BySlugs {
                slugs: vec!["send-email".to_string(), "review-doc".to_string()],
                options: None,
            }
        );
    }

    #[test]
    fn test_with_json_options() {
        let v =
            parse_query_string(r#"query=by_object_ids&ids[0]=aaa&options={"limit":10,"page":1}"#);
        let q: TestQuery = serde_json::from_value(v).unwrap();
        assert_eq!(
            q,
            TestQuery::ByObjectIds {
                ids: vec!["aaa".to_string()],
                options: Some(TestOptions {
                    limit: Some(10),
                    page: Some(1),
                }),
            }
        );
    }

    #[test]
    fn test_empty_query() {
        let v = parse_query_string("");
        assert_eq!(v, Value::Object(Map::new()));
    }
}
