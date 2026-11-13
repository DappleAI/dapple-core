//! The `guide search` command for semantic search over guide content
//!
//! Calls the conduit /search endpoint and returns structured results as a Nu table.

use crate::commands::prompt_send::{get_conduit_url, get_dapple_metadata};
use crate::conduit_client::{ConduitClient, SearchRequest};
use nu_engine::command_prelude::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct GuideSearch;

impl Command for GuideSearch {
    fn name(&self) -> &str {
        "guide search"
    }

    fn signature(&self) -> Signature {
        Signature::build("guide search")
            .input_output_types(vec![
                (Type::Nothing, Type::table()),
                (Type::Nothing, Type::list(Type::record())),
            ])
            .required(
                "query",
                SyntaxShape::String,
                "Natural language search query",
            )
            .named(
                "tags",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Audience tag filter (default: [\"universal\"])",
                Some('t'),
            )
            .named(
                "locale",
                SyntaxShape::String,
                "Locale filter (default: \"en\")",
                Some('l'),
            )
            .named(
                "limit",
                SyntaxShape::Int,
                "Maximum number of results (default: 20, max: 100)",
                Some('n'),
            )
            .named(
                "threshold",
                SyntaxShape::Number,
                "Minimum similarity score (default: 0.0)",
                None,
            )
            .named(
                "guide-ids",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "Scope search to specific guide UUIDs",
                None,
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Search guide content by semantic similarity."
    }

    fn extra_description(&self) -> &str {
        r#"Performs pgvector semantic search over guide content embeddings via Conduit.

Returns a table with columns:
  block_id, guide_id, chapter_id, guide_title, chapter_title,
  directive_type, chunk_text, similarity, metadata

Environment:
  DAPPLE_PROMPT_CONDUIT_URL or DAPPLE_METADATA.conduit_url must be set.

Examples:
  # Basic search
  guide search "authentication" --limit 5

  # Filter by similarity
  guide search "workflow" | where similarity > 0.3

  # Search within specific guides
  guide search "setup" --guide-ids ["550e8400-e29b-41d4-a716-446655440000"]
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["guide", "search", "semantic", "pgvector", "embedding"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Search guide content",
                example: r#"guide search "authentication" --limit 5"#,
                result: None,
            },
            Example {
                description: "Search and filter by similarity",
                example: r#"guide search "workflow" | where similarity > 0.3 | select guide_title chapter_title similarity"#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;

        // Required: query string
        let query: String = call.req(engine_state, stack, 0)?;

        if query.trim().is_empty() {
            return Err(ShellError::GenericError {
                error: "Empty search query".into(),
                msg: "Query must not be empty".into(),
                span: Some(span),
                help: Some("Provide a non-empty search query string".into()),
                inner: vec![],
            });
        }

        // Optional flags
        let tags: Option<Vec<Value>> = call.get_flag(engine_state, stack, "tags")?;
        let locale: Option<String> = call.get_flag(engine_state, stack, "locale")?;
        let limit: Option<i64> = call.get_flag(engine_state, stack, "limit")?;
        let threshold: Option<Value> = call.get_flag(engine_state, stack, "threshold")?;
        let guide_id_values: Option<Vec<Value>> =
            call.get_flag(engine_state, stack, "guide-ids")?;

        // Parse tags
        let tags: Vec<String> = tags
            .map(|vals| {
                vals.into_iter()
                    .map(|v| v.coerce_into_string().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();

        // Parse threshold
        let threshold: Option<f64> = threshold.map(|v| match v {
            Value::Float { val, .. } => val,
            Value::Int { val, .. } => val as f64,
            _ => 0.0,
        });

        // Parse guide_ids as UUIDs
        let guide_ids: Vec<Uuid> = if let Some(vals) = guide_id_values {
            let mut ids = Vec::with_capacity(vals.len());
            for v in vals {
                let s = v
                    .coerce_into_string()
                    .map_err(|_| ShellError::GenericError {
                        error: "Invalid guide ID".into(),
                        msg: "Expected a UUID string".into(),
                        span: Some(span),
                        help: None,
                        inner: vec![],
                    })?;
                let id = Uuid::parse_str(&s).map_err(|e| ShellError::GenericError {
                    error: format!("Invalid UUID: {}", s),
                    msg: e.to_string(),
                    span: Some(span),
                    help: Some("Guide IDs must be valid UUIDs".into()),
                    inner: vec![],
                })?;
                ids.push(id);
            }
            ids
        } else {
            Vec::new()
        };

        // Get conduit URL from metadata or environment
        let metadata = get_dapple_metadata(engine_state, stack);
        let conduit_url = get_conduit_url(&metadata);

        // Build request
        let request = SearchRequest {
            q: query.clone(),
            tags,
            locale: locale.unwrap_or_default(),
            limit: limit.map(|l| l as i32),
            threshold,
            guide_ids,
        };

        // Execute search via conduit
        let client = ConduitClient::new(&conduit_url);
        let response = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(client.search(request))
        });

        match response {
            Ok(search_response) => {
                let records: Vec<Value> = search_response
                    .results
                    .into_iter()
                    .map(|item| {
                        // Extract guide_title and chapter_title from metadata if present
                        let guide_title = item
                            .metadata
                            .get("guide_title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let chapter_title = item
                            .metadata
                            .get("chapter_title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        Value::record(
                            record! {
                                "block_id" => Value::string(item.block_id.to_string(), span),
                                "guide_id" => Value::string(item.guide_id.to_string(), span),
                                "chapter_id" => Value::string(item.chapter_id.to_string(), span),
                                "guide_title" => Value::string(guide_title, span),
                                "chapter_title" => Value::string(chapter_title, span),
                                "directive_type" => Value::string(item.directive_type, span),
                                "chunk_text" => Value::string(item.chunk_text, span),
                                "similarity" => Value::float(item.similarity, span),
                                "metadata" => json_to_nu_value(&item.metadata, span),
                            },
                            span,
                        )
                    })
                    .collect();

                Ok(Value::list(records, span).into_pipeline_data())
            }
            Err(e) => Err(ShellError::GenericError {
                error: format!("Guide search failed: {}", e),
                msg: "Search request to conduit failed".into(),
                span: Some(span),
                help: Some("Ensure DAPPLE_PROMPT_CONDUIT_URL is set and conduit is running".into()),
                inner: vec![],
            }),
        }
    }
}

/// Convert a serde_json::Value to a nu Value
fn json_to_nu_value(json: &serde_json::Value, span: Span) -> Value {
    match json {
        serde_json::Value::Null => Value::nothing(span),
        serde_json::Value::Bool(b) => Value::bool(*b, span),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::int(i, span)
            } else if let Some(f) = n.as_f64() {
                Value::float(f, span)
            } else {
                Value::string(n.to_string(), span)
            }
        }
        serde_json::Value::String(s) => Value::string(s.clone(), span),
        serde_json::Value::Array(arr) => {
            let vals: Vec<Value> = arr.iter().map(|v| json_to_nu_value(v, span)).collect();
            Value::list(vals, span)
        }
        serde_json::Value::Object(obj) => {
            let mut rec = Record::new();
            for (k, v) in obj {
                rec.push(k.clone(), json_to_nu_value(v, span));
            }
            Value::record(rec, span)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_name() {
        let cmd = GuideSearch;
        assert_eq!(cmd.name(), "guide search");
    }

    #[test]
    fn test_command_description() {
        let cmd = GuideSearch;
        assert!(!cmd.description().is_empty());
    }

    #[test]
    fn test_json_to_nu_value_null() {
        let span = Span::test_data();
        let result = json_to_nu_value(&serde_json::Value::Null, span);
        assert!(matches!(result, Value::Nothing { .. }));
    }

    #[test]
    fn test_json_to_nu_value_string() {
        let span = Span::test_data();
        let result = json_to_nu_value(&serde_json::json!("hello"), span);
        assert_eq!(result.coerce_into_string().unwrap(), "hello");
    }

    #[test]
    fn test_json_to_nu_value_object() {
        let span = Span::test_data();
        let json = serde_json::json!({"key": "value", "num": 42});
        let result = json_to_nu_value(&json, span);
        assert!(matches!(result, Value::Record { .. }));
    }

    #[test]
    fn test_json_to_nu_value_array() {
        let span = Span::test_data();
        let json = serde_json::json!([1, 2, 3]);
        let result = json_to_nu_value(&json, span);
        assert!(matches!(result, Value::List { .. }));
    }
}
