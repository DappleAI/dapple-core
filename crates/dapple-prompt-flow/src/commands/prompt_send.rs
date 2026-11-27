//! The `prompt send` command for executing prompts via conduit
//!
//! This module also exports shared types and helper functions used by
//! summarize, madlib, and synth commands.

use crate::conduit_client::{ConduitClient, ConduitClientError, PromptRequest, PromptResult};
use nu_engine::command_prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone)]
pub struct PromptSend;

// ============================================================================
// Public types for shared use by other commands
// ============================================================================

/// Prompt info with id and engine_body (for TTS/summarization config)
/// This mirrors the PromptInfo from dispatch_runner.rs for deserialization
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PromptInfo {
    pub id: Uuid,
    pub description: Option<String>,
    pub engine_body: Option<String>,
    #[serde(default)]
    pub maker_keys: Vec<String>,
}

/// Metadata passed via DAPPLE_METADATA environment variable
/// Public for use by summarize, madlib, synth commands
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DappleMetadata {
    #[serde(default)]
    pub attempt_id: Uuid,
    #[serde(default)]
    pub playbook_task_id: Uuid,
    pub conduit_url: Option<String>,
    /// Mapping of prompt names to their object_ids (legacy, deprecated)
    #[serde(default)]
    pub prompt_mappings: HashMap<String, Uuid>,
    /// Mapping of prompt names to their full info (object_id, description, engine_body, maker_keys)
    #[serde(default)]
    pub prompt_data: HashMap<String, PromptInfo>,
    /// API URL for commands that need HTTP access to dapple-prompt-api.
    /// Populated from DAPPLE_PROMPT_API_URL env var by dispatch_runner.
    #[serde(default)]
    pub api_url: Option<String>,
    /// RAG context for guide content retrieval, passed through to Conduit.
    /// When present, Conduit queries pgvector and injects retrieved chunks as system context.
    #[serde(default)]
    pub rag_context: Option<serde_json::Value>,
    /// Dispatch ID of the currently-executing dispatch. Phase F: forwarded
    /// to Conduit so it can fetch task context from api and inject it as a
    /// system message. Optional because some synthetic / test metadata
    /// records do not carry dispatch context.
    #[serde(default)]
    pub dispatch_id: Option<Uuid>,
    /// Dispatch-task ID (i.e. the `dispatch_tasks.id` from the DB, not the
    /// source Task entity id). Pairs with `dispatch_id` so Conduit can
    /// resolve the api's task_context endpoint.
    #[serde(default)]
    pub task_id: Option<Uuid>,
}

// ============================================================================
// Public helper functions for shared use by other commands
// ============================================================================

/// Get DAPPLE_METADATA from the environment
pub fn get_dapple_metadata(engine_state: &EngineState, stack: &Stack) -> DappleMetadata {
    // Try to get DAPPLE_METADATA from environment
    if let Some((_name, meta_value)) =
        stack.get_env_var_insensitive(engine_state, "DAPPLE_METADATA")
    {
        // Convert nu Value to JSON
        let meta_json = value_to_json(meta_value);
        match serde_json::from_value::<DappleMetadata>(meta_json.clone()) {
            Ok(metadata) => return metadata,
            Err(e) => {
                log::error!("Failed to deserialize DAPPLE_METADATA: {}", e);
            }
        }
    }

    // Return default metadata if not found (for testing)
    DappleMetadata {
        attempt_id: Uuid::nil(),
        playbook_task_id: Uuid::nil(),
        conduit_url: None,
        api_url: None,
        prompt_mappings: HashMap::new(),
        prompt_data: HashMap::new(),
        rag_context: None,
        dispatch_id: None,
        task_id: None,
    }
}

/// Get the API URL from metadata or environment
pub fn get_api_url(metadata: &DappleMetadata) -> Option<String> {
    metadata
        .api_url
        .clone()
        .or_else(|| std::env::var("DAPPLE_PROMPT_API_URL").ok())
}

/// Get the conduit URL from metadata or environment
pub fn get_conduit_url(metadata: &DappleMetadata) -> String {
    metadata
        .conduit_url
        .clone()
        .or_else(|| std::env::var("DAPPLE_PROMPT_CONDUIT_URL").ok())
        .expect(
            "DAPPLE_PROMPT_CONDUIT_URL must be set (either in metadata.conduit_url or environment)",
        )
}

/// Get the conduit-oximedia media service URL from the environment.
///
/// Returns `Err` when unset — the OxiMedia media service is opt-in, reachable
/// only when `DAPPLE_PROMPT_CONDUIT_MEDIA_URL` is configured. Callers (the
/// `av compose` / `audio analyze` commands) turn the error into a shaped command
/// error.
pub fn get_media_url() -> Result<String, String> {
    std::env::var("DAPPLE_PROMPT_CONDUIT_MEDIA_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "media service not configured (set DAPPLE_PROMPT_CONDUIT_MEDIA_URL to the conduit-oximedia URL)"
                .to_string()
        })
}

/// Get the conduit-media-gateway service URL from the environment.
///
/// Returns `Err` when unset — the media gateway is opt-in, reachable only when
/// `DAPPLE_MEDIA_GATEWAY_URL` is configured. The `twitch`/`vod` commands turn the
/// error into a shaped command error.
pub fn get_media_gateway_url() -> Result<String, String> {
    std::env::var("DAPPLE_MEDIA_GATEWAY_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "media gateway not configured (set DAPPLE_MEDIA_GATEWAY_URL to the conduit-media-gateway URL)"
                .to_string()
        })
}

/// Validate that a prompt has the expected maker_key (legacy) or lineage_key
/// Returns an error with a helpful message if validation fails
///
/// This function supports both old and new taxonomy:
/// - Old: maker_key like "tts", "summarization", "mlm"
/// - New: lineage_key like "vits", "flan-t5", "modernbert"
///
/// For backwards compatibility, we check both the maker_keys field and map
/// legacy maker keys to their corresponding lineage keys.
#[allow(clippy::result_large_err)]
pub fn validate_prompt_maker_key(
    prompt_name: &str,
    prompt_info: &PromptInfo,
    expected_maker_key: &str,
    span: Span,
) -> Result<(), ShellError> {
    // Check if the expected key is in the maker_keys list
    if prompt_info
        .maker_keys
        .contains(&expected_maker_key.to_string())
    {
        return Ok(());
    }

    // Also check for lineage key equivalents (new taxonomy)
    // Map legacy maker keys to lineage keys
    let lineage_equivalents = match expected_maker_key {
        "tts" => vec!["vits"],
        "summarization" => vec!["flan-t5"],
        "mlm" => vec!["modernbert"],
        _ => vec![],
    };

    for lineage in &lineage_equivalents {
        if prompt_info.maker_keys.contains(&lineage.to_string()) {
            return Ok(());
        }
    }

    // Build error message based on what maker_keys the prompt has
    let hint = if prompt_info.maker_keys.is_empty() {
        format!(
            "The prompt '{}' has no maker_key associations. Ensure it has model-specific bodies configured.",
            prompt_name
        )
    } else {
        let maker_key_display = prompt_info.maker_keys.join(", ");
        let command_hint = match prompt_info.maker_keys.first().map(|s| s.as_str()) {
            Some("tts") | Some("vits") => "Use `synth` for TTS prompts",
            Some("summarization") | Some("flan-t5") => "Use `summarize` for summarization prompts",
            Some("mlm") | Some("modernbert") => "Use `madlib` for MLM prompts",
            _ => "Use `prompt send` for general prompts",
        };
        format!(
            "Prompt '{}' uses lineage='{}', but this command requires lineage='{}'. {}.",
            prompt_name, maker_key_display, expected_maker_key, command_hint
        )
    };

    Err(ShellError::GenericError {
        error: format!(
            "Prompt '{}' is not a {} prompt",
            prompt_name, expected_maker_key
        ),
        msg: hint,
        span: Some(span),
        help: Some("Choose a prompt with the correct lineage.".into()),
        inner: vec![],
    })
}

/// Validate that a prompt has the expected lineage_key (new taxonomy)
/// Returns an error with a helpful message if validation fails
///
/// This is the new taxonomy version that checks lineage keys directly.
#[allow(clippy::result_large_err)]
pub fn validate_prompt_lineage_key(
    prompt_name: &str,
    prompt_info: &PromptInfo,
    expected_lineage_key: &str,
    span: Span,
) -> Result<(), ShellError> {
    // Check if the expected lineage key is in the maker_keys list
    // (maker_keys will contain lineage_keys after migration)
    if prompt_info
        .maker_keys
        .contains(&expected_lineage_key.to_string())
    {
        return Ok(());
    }

    // Also check legacy equivalents for backwards compatibility
    let legacy_equivalents = match expected_lineage_key {
        "vits" => vec!["tts"],
        "flan-t5" => vec!["summarization"],
        "modernbert" => vec!["mlm"],
        _ => vec![],
    };

    for legacy in &legacy_equivalents {
        if prompt_info.maker_keys.contains(&legacy.to_string()) {
            return Ok(());
        }
    }

    // Build error message
    let hint = if prompt_info.maker_keys.is_empty() {
        format!(
            "The prompt '{}' has no lineage associations. Ensure it has model-specific bodies configured.",
            prompt_name
        )
    } else {
        let lineage_display = prompt_info.maker_keys.join(", ");
        let command_hint = match prompt_info.maker_keys.first().map(|s| s.as_str()) {
            Some("tts") | Some("vits") => "Use `synth` for TTS prompts",
            Some("summarization") | Some("flan-t5") => "Use `summarize` for summarization prompts",
            Some("mlm") | Some("modernbert") => "Use `madlib` for MLM prompts",
            _ => "Use `prompt send` for general prompts",
        };
        format!(
            "Prompt '{}' uses lineage='{}', but this command requires lineage='{}'. {}.",
            prompt_name, lineage_display, expected_lineage_key, command_hint
        )
    };

    Err(ShellError::GenericError {
        error: format!(
            "Prompt '{}' is not a {} prompt",
            prompt_name, expected_lineage_key
        ),
        msg: hint,
        span: Some(span),
        help: Some("Choose a prompt with the correct lineage.".into()),
        inner: vec![],
    })
}

/// Look up a prompt by name from metadata, with a helpful error if not found
#[allow(clippy::result_large_err)]
pub fn lookup_prompt<'a>(
    prompt_name: &str,
    metadata: &'a DappleMetadata,
    span: Span,
) -> Result<&'a PromptInfo, ShellError> {
    metadata.prompt_data.get(prompt_name).ok_or_else(|| {
        let available = metadata.prompt_data.keys().cloned().collect::<Vec<_>>();
        ShellError::GenericError {
            error: format!("Prompt '{}' not found in prompt_data", prompt_name),
            msg: "Check that the prompt is bound to this task via task_prompts".into(),
            span: Some(span),
            help: if available.is_empty() {
                Some("No prompts are bound to this task.".into())
            } else {
                Some(format!("Available prompts: {}", available.join(", ")))
            },
            inner: vec![],
        }
    })
}

/// Execute a prompt via conduit with optional context_data
/// Returns the result as a nu Value (string for text, record for asset)
#[allow(clippy::result_large_err)]
pub fn execute_prompt_with_context(
    prompt_name: &str,
    prompt_info: &PromptInfo,
    metadata: &DappleMetadata,
    context_data: Option<serde_json::Value>,
    span: Span,
) -> Result<Value, ShellError> {
    let conduit_url = get_conduit_url(metadata);

    let client = ConduitClient::new(&conduit_url);
    let request = PromptRequest {
        prompt_id: prompt_info.id,
        attempt_id: metadata.attempt_id,
        playbook_task_id: metadata.playbook_task_id,
        context_data,
        rag_context: metadata.rag_context.clone(),
        dispatch_id: metadata.dispatch_id,
        task_id: metadata.task_id,
    };

    // Execute the prompt using the current tokio runtime, retrying on
    // retryable LLM errors (e.g. Anthropic 529 "Overloaded") with bounded
    // exponential backoff. The retryable flag is sourced from the conduit
    // (`ConduitClientError::PromptError { retryable, .. }`) which maps it
    // out of the upstream provider response.
    let response = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(async { run_prompt_with_retry(&client, request, prompt_name).await })
    });

    match response {
        Ok(result) => prompt_result_to_value(result, span),
        Err(e) => Err(prompt_error_to_shell_error(e, prompt_name, span)),
    }
}

/// Maximum retry attempts for retryable conduit errors.
/// Total worst-case wait is BACKOFF_BASE_MS * (1 + 3 + 9) = ~13s.
const MAX_RETRIES: usize = 3;
const BACKOFF_BASE_MS: u64 = 1000;

async fn run_prompt_with_retry(
    client: &ConduitClient,
    request: PromptRequest,
    prompt_name: &str,
) -> Result<PromptResult, ConduitClientError> {
    let mut attempt: usize = 0;
    loop {
        let request_clone = request.clone();
        match client.execute_prompt(request_clone).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let retryable = matches!(
                    &e,
                    ConduitClientError::PromptError {
                        retryable: true,
                        ..
                    }
                );
                if !retryable || attempt >= MAX_RETRIES {
                    return Err(e);
                }
                let delay_ms = BACKOFF_BASE_MS * 3u64.pow(attempt as u32);
                log::warn!(
                    "retryable conduit error on prompt '{}' (attempt {}/{}); backing off {}ms: {}",
                    prompt_name,
                    attempt + 1,
                    MAX_RETRIES,
                    delay_ms,
                    e
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                attempt += 1;
            }
        }
    }
}

/// Strip markdown code fences from LLM responses.
///
/// LLMs often wrap JSON (or other structured output) in markdown fences like:
///   ```json\n{...}\n```
/// This breaks downstream `from json` parsing in nushell scripts.
/// We find the first opening fence and last closing fence anywhere in the text,
/// extracting just the content between them. This handles prose before/after fences.
fn strip_markdown_fences(text: &str) -> &str {
    let trimmed = text.trim();

    // Find the first opening fence (``` possibly followed by a language tag)
    let Some(open_pos) = trimmed.find("```") else {
        return trimmed;
    };
    let after_open = &trimmed[open_pos + 3..];

    // Skip the optional language tag on the same line as the opening fence
    let body = match after_open.find('\n') {
        Some(pos) => &after_open[pos + 1..],
        None => return trimmed, // ``` at end with nothing after
    };

    // Find the LAST closing fence in the remaining body
    if let Some(close_pos) = body.rfind("```") {
        body[..close_pos].trim()
    } else {
        body.trim()
    }
}

/// Convert a PromptResult to a nu Value.
///
/// For text results, returns a record with `text`, `input_tokens`, `output_tokens`, `latency_ms`,
/// plus `model` and `request_id` (for provenance; `null` if the conduit did not report them)
/// so that callers can access token metadata via signals. The nushell script pattern is:
///   let result = ($in | prompt send "name")
///   { payload: $result.text, signals: { input_tokens: $result.input_tokens, ... } }
#[allow(clippy::result_large_err)]
pub fn prompt_result_to_value(result: PromptResult, span: Span) -> Result<Value, ShellError> {
    match result {
        PromptResult::Text(text_result) => {
            let cleaned = strip_markdown_fences(&text_result.content);
            let record = record! {
                "text" => Value::string(cleaned, span),
                "input_tokens" => Value::int(text_result.usage.input_tokens as i64, span),
                "output_tokens" => Value::int(text_result.usage.output_tokens as i64, span),
                "latency_ms" => Value::int(text_result.usage.latency_ms as i64, span),
                // Provenance: which model wrote this text (see `md provenance`).
                "model" => text_result
                    .model
                    .map(|m| Value::string(m, span))
                    .unwrap_or_else(|| Value::nothing(span)),
                "request_id" => text_result
                    .request_id
                    .map(|id| Value::string(id.to_string(), span))
                    .unwrap_or_else(|| Value::nothing(span)),
            };
            Ok(Value::record(record, span))
        }
        PromptResult::Asset(asset) => {
            let record = record! {
                "asset_id" => Value::string(asset.asset_id.to_string(), span),
                "asset_kind" => Value::string(asset.asset_kind, span),
                "mime_type" => Value::string(asset.mime_type, span),
                "content_length" => Value::int(asset.content_length, span),
                "duration_seconds" => Value::float(asset.duration_seconds as f64, span),
                "sample_rate" => Value::int(asset.sample_rate as i64, span),
                "text" => Value::string(asset.text, span),
            };
            Ok(Value::record(record, span))
        }
    }
}

/// Convert a ConduitClientError to a ShellError
pub fn prompt_error_to_shell_error(
    e: ConduitClientError,
    prompt_name: &str,
    span: Span,
) -> ShellError {
    let (error_msg, retryable) = match &e {
        ConduitClientError::PromptError {
            code,
            message,
            retryable,
        } => (format!("{}: {}", code, message), *retryable),
        _ => (e.to_string(), false),
    };

    ShellError::GenericError {
        error: format!("Prompt '{}' execution failed: {}", prompt_name, error_msg),
        msg: if retryable {
            "This error is retryable".into()
        } else {
            "This error is not retryable".into()
        },
        span: Some(span),
        help: None,
        inner: vec![],
    }
}

/// Convert a nu Value to a JSON value
pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Nothing { .. } => serde_json::Value::Null,
        Value::Bool { val, .. } => serde_json::Value::Bool(*val),
        Value::Int { val, .. } => serde_json::json!(*val),
        Value::Float { val, .. } => serde_json::json!(*val),
        Value::String { val, .. } => serde_json::Value::String(val.clone()),
        Value::List { vals, .. } => {
            serde_json::Value::Array(vals.iter().map(value_to_json).collect())
        }
        Value::Record { val, .. } => {
            let mut map = serde_json::Map::new();
            for (k, v) in val.iter() {
                map.insert(k.clone(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::String(format!("{:?}", value)),
    }
}

// ============================================================================
// Command implementation
// ============================================================================

impl Command for PromptSend {
    fn name(&self) -> &str {
        "prompt send"
    }

    fn signature(&self) -> Signature {
        Signature::build("prompt send")
            .input_output_types(vec![
                (Type::Any, Type::Any), // Can return String (LLM) or record (TTS asset)
                (Type::Nothing, Type::Any),
            ])
            .required(
                "name",
                SyntaxShape::String,
                "The local name of the prompt to execute",
            )
            .category(Category::Custom("dapple".into()))
    }

    fn description(&self) -> &str {
        "Execute a prompt by its local name and stream the response."
    }

    fn extra_description(&self) -> &str {
        r#"Sends the pipeline input to the conduit service for prompt execution.
The prompt is looked up by its local_name within the current task's bound prompts.

The command reads DAPPLE_METADATA from the environment to determine:
- attempt_id: The current task attempt for context tracking
- playbook_task_id: Used for overlay resolution
- conduit_url: The URL of the conduit service (from metadata or DAPPLE_PROMPT_CONDUIT_URL env, required)

Returns:
- For LLM prompts: The response as a string
- For TTS prompts (maker_key=tts): A record with asset_id, duration_seconds, sample_rate, text
"#
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["llm", "ai", "prompt", "execute", "send"]
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Execute a prompt and collect the response",
                example: r#"$input | prompt send "summarize" | str join"#,
                result: None,
            },
            Example {
                description: "Execute a prompt with explicit input",
                example: r#"{ text: "Hello world" } | prompt send "translate""#,
                result: None,
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let span = call.head;
        let name: String = call.req(engine_state, stack, 0)?;

        // Get metadata from environment
        let metadata = get_dapple_metadata(engine_state, stack);

        // Get the input as JSON for context_data
        let input_value = input.into_value(span)?;
        let context_data = Some(value_to_json(&input_value));

        // Look up prompt (try prompt_data first, fall back to prompt_mappings)
        let prompt_info = if let Some(info) = metadata.prompt_data.get(&name) {
            info.clone()
        } else if let Some(object_id) = metadata.prompt_mappings.get(&name) {
            // Legacy fallback: create a minimal PromptInfo from prompt_mappings
            PromptInfo {
                id: *object_id,
                description: None,
                engine_body: None,
                maker_keys: Vec::new(),
            }
        } else {
            return Err(ShellError::GenericError {
                error: format!("Prompt '{}' not found", name),
                msg: "Check that the prompt is bound to this task via task_prompts".into(),
                span: Some(span),
                help: Some(format!(
                    "Available prompts: {:?}",
                    metadata
                        .prompt_data
                        .keys()
                        .chain(metadata.prompt_mappings.keys())
                        .collect::<Vec<_>>()
                )),
                inner: vec![],
            });
        };

        // Execute the prompt via conduit with context_data
        log::debug!(
            "prompt send: calling conduit for prompt '{}', object_id={}, attempt_id={}, conduit_url={:?}",
            name,
            prompt_info.id,
            metadata.attempt_id,
            metadata.conduit_url,
        );
        let response =
            execute_prompt_with_context(&name, &prompt_info, &metadata, context_data, span);

        match &response {
            Ok(val) => log::debug!(
                "prompt send: success, result type={}, first_100={:?}",
                val.get_type(),
                val.to_expanded_string("", &nu_protocol::Config::default())
                    .chars()
                    .take(100)
                    .collect::<String>(),
            ),
            Err(e) => log::error!("prompt send: failed: {:?}", e),
        }

        Ok(PipelineData::Value(response?, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_markdown_fences_json() {
        let input = "```json\n[{\"label\": \"Topic\"}]\n```";
        assert_eq!(strip_markdown_fences(input), "[{\"label\": \"Topic\"}]");
    }

    #[test]
    fn test_strip_markdown_fences_no_language() {
        let input = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_strip_markdown_fences_plain_text() {
        let input = "Just plain text";
        assert_eq!(strip_markdown_fences(input), "Just plain text");
    }

    #[test]
    fn test_strip_markdown_fences_already_json() {
        let input = "[{\"label\": \"Topic\"}]";
        assert_eq!(strip_markdown_fences(input), "[{\"label\": \"Topic\"}]");
    }

    #[test]
    fn test_strip_markdown_fences_with_whitespace() {
        let input = "  ```json\n  {\"a\": 1}\n  ```  ";
        assert_eq!(strip_markdown_fences(input), "{\"a\": 1}");
    }

    #[test]
    fn test_strip_markdown_fences_multiline() {
        let input = "```json\n[\n  {\n    \"label\": \"Should CRISPR Gene Editing Be Used?\",\n    \"value\": \"crispr\"\n  }\n]\n```";
        let result = strip_markdown_fences(input);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
        assert!(!result.contains("```"));
    }

    #[test]
    fn test_strip_markdown_fences_prose_before() {
        let input = "Here's the branding:\n\n```json\n{\"color\": \"#1A1A2E\"}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"color\": \"#1A1A2E\"}");
    }

    #[test]
    fn test_strip_markdown_fences_prose_after() {
        let input = "```json\n{\"color\": \"#1A1A2E\"}\n```\n\nLet me know if you'd like changes!";
        assert_eq!(strip_markdown_fences(input), "{\"color\": \"#1A1A2E\"}");
    }

    #[test]
    fn test_strip_markdown_fences_prose_both() {
        let input = "Here's the config:\n\n```json\n{\"key\": \"val\"}\n```\n\nHope this helps!";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"val\"}");
    }
}
