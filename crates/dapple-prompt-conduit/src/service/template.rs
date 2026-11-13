use dapple_data::model::prompts::RawPromptMessageModel;
use dapple_data::model::value_slots::ValueSlotPath;
use dapple_data::query::prompts::{resolve_prompt_messages, select_prompt_messages_for_engine};
use dapple_data::DbPool;
use dapple_fluent::{engine_chain, locale_chain, substitute_tracking};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

use crate::error::ConduitError;

/// Default locale when no specific locale is provided
const DEFAULT_LOCALE: &str = "en";

/// A single rendered prompt message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: Option<String>,
    pub content: String,
}

/// Result of rendering a prompt template
#[derive(Debug, Clone)]
pub struct RenderedPrompt {
    pub messages: Vec<PromptMessage>,
    pub prompt_name: String,
}

/// Result of resolving and rendering a prompt template
/// Includes prompt_id for response storage
#[derive(Debug, Clone)]
pub struct ResolvedPrompt {
    pub prompt_id: Uuid,
    pub prompt_name: String,
    /// Prompt description (used for TTS voice specification)
    pub prompt_description: Option<String>,
    pub messages: Vec<PromptMessage>,
    /// Context keys the template actually interpolated, across all messages.
    ///
    /// The caller appends the request context as a raw JSON message for prompts
    /// that do not interpolate it; this lets it send only the keys the template
    /// did NOT inline, so each key reaches the model exactly once instead of
    /// twice (which, on a filing-sized context, doubles the prompt).
    pub consumed_context_keys: BTreeSet<String>,
}

/// Service for rendering prompt templates
///
/// Uses a fluent-inspired message resolution strategy with:
/// - Locale fallback chains (e.g., "pt-BR" → "pt" → "en")
/// - Engine fallback chains (e.g., "claude-3.5-sonnet:v1" → "claude-3.5-sonnet" → "claude-3.5")
/// - Simple $variable substitution
pub struct TemplateService {
    /// Engine fallback configurations (engine_key → fallback chain)
    /// This could be loaded from the database in the future
    engine_fallbacks: HashMap<String, Vec<String>>,
}

impl TemplateService {
    pub fn new() -> Self {
        Self {
            engine_fallbacks: HashMap::new(),
        }
    }

    /// Configure engine fallback chains
    ///
    /// Call this after construction to set up engine-specific fallbacks.
    /// For example: "claude-3.5-sonnet" → ["claude-3.5", "claude"]
    pub fn with_engine_fallbacks(mut self, fallbacks: HashMap<String, Vec<String>>) -> Self {
        self.engine_fallbacks = fallbacks;
        self
    }

    /// Add a single engine fallback chain
    pub fn add_engine_fallback(&mut self, engine_key: &str, fallbacks: Vec<String>) {
        self.engine_fallbacks
            .insert(engine_key.to_string(), fallbacks);
    }

    /// Get engine fallbacks for a given engine key
    fn get_engine_fallbacks(&self, engine_key: &str) -> &[String] {
        self.engine_fallbacks
            .get(engine_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Render a prompt using the fluent-inspired resolver with locale/engine fallback chains
    ///
    /// This is the new preferred method that uses:
    /// - Locale chain built from the provided locale (with default fallback)
    /// - Engine chain built from engine_key and optional version
    /// - $variable substitution instead of Handlebars
    #[allow(clippy::too_many_arguments)]
    pub async fn render_prompt_fluent(
        &self,
        pool: &DbPool,
        prompt_id: Uuid,
        locale: &str,
        engine_key: &str,
        engine_version: Option<&str>,
        context_data: &Value,
        visible_slots: &[ValueSlotPath],
    ) -> Result<RenderedPrompt, ConduitError> {
        // 1. Build fallback chains
        let lc = locale_chain(locale, DEFAULT_LOCALE);
        let fallbacks = self.get_engine_fallbacks(engine_key);
        let ec = engine_chain(engine_key, engine_version, fallbacks);

        // 2. Resolve messages using AND gateway query
        let resolved_messages = resolve_prompt_messages(pool, prompt_id, &lc, &ec)
            .await
            .map_err(ConduitError::Database)?;

        if resolved_messages.is_empty() {
            return Err(ConduitError::NotFound(format!(
                "Prompt {} not found or has no messages",
                prompt_id
            )));
        }

        // 3. Extract prompt name (unwrap Option)
        let prompt_name = resolved_messages
            .first()
            .and_then(|m| m.prompt_name.clone())
            .unwrap_or_default();

        // 4. Build substitution context
        let context = self.build_substitution_context(context_data, None, visible_slots);

        // 5. Substitute variables in each message
        let messages = resolved_messages
            .into_iter()
            .map(|m| {
                let body = m.body.unwrap_or_default();
                let (content, _consumed) = substitute_tracking(&body, &context);
                PromptMessage {
                    role: m.role,
                    content,
                }
            })
            .collect();

        Ok(RenderedPrompt {
            messages,
            prompt_name,
        })
    }

    /// Resolve and render a prompt using the fluent-inspired resolver
    ///
    /// Returns ResolvedPrompt with prompt_id included for database storage.
    /// Uses the new AND gateway resolution with locale/engine fallback chains.
    #[allow(clippy::too_many_arguments)]
    pub async fn resolve_and_render_prompt_fluent(
        &self,
        pool: &DbPool,
        prompt_id: Uuid,
        locale: &str,
        engine_key: &str,
        engine_version: Option<&str>,
        context_data: &Value,
        request_context_data: Option<&Value>,
        visible_slots: &[ValueSlotPath],
    ) -> Result<ResolvedPrompt, ConduitError> {
        // 1. Build fallback chains
        let lc = locale_chain(locale, DEFAULT_LOCALE);
        let fallbacks = self.get_engine_fallbacks(engine_key);
        let ec = engine_chain(engine_key, engine_version, fallbacks);

        // 2. Resolve messages using AND gateway query
        let resolved_messages = resolve_prompt_messages(pool, prompt_id, &lc, &ec)
            .await
            .map_err(ConduitError::Database)?;

        if resolved_messages.is_empty() {
            return Err(ConduitError::NotFound(format!(
                "Prompt {} not found or has no messages",
                prompt_id
            )));
        }

        // 3. Extract prompt metadata from first message (unwrap Options with defaults)
        let first_msg = resolved_messages.first().unwrap();
        let prompt_id = first_msg.prompt_id.unwrap_or_default();
        let prompt_name = first_msg.prompt_name.clone().unwrap_or_default();
        let prompt_description = first_msg.prompt_description.clone();

        // 4. Build substitution context
        let context =
            self.build_substitution_context(context_data, request_context_data, visible_slots);

        // 5. Substitute variables in each message, recording which keys were used
        let mut consumed_context_keys = BTreeSet::new();
        let messages = resolved_messages
            .into_iter()
            .map(|m| {
                let body = m.body.unwrap_or_default();
                let (content, consumed) = substitute_tracking(&body, &context);
                consumed_context_keys.extend(consumed);
                PromptMessage {
                    role: m.role,
                    content,
                }
            })
            .collect();

        Ok(ResolvedPrompt {
            prompt_id,
            prompt_name,
            prompt_description,
            messages,
            consumed_context_keys,
        })
    }

    /// Build the `$variable` substitution map.
    ///
    /// Precedence, lowest first:
    ///
    /// 1. `attempt_context_data` - accumulated task context read from the DB.
    /// 2. visible value slots - `_visible_slots` plus a `{name, value_kind, id}`
    ///    stub per slot.
    /// 3. `request_context_data` - what the caller passed on THIS request (for a
    ///    guide task, the record piped into `prompt send`).
    ///
    /// The request context goes **last and wins**, which is what `PromptRequest`
    /// documents ("takes precedence over context_data fetched from attempt").
    /// Getting this order wrong is not a cosmetic bug: with the slot stubs
    /// applied last, a request key that happens to share a slot's name would be
    /// replaced by the stub, so the template would interpolate
    /// `{"name":...,"value_kind":...}` where the guide's actual value belongs.
    fn build_substitution_context(
        &self,
        attempt_context_data: &Value,
        request_context_data: Option<&Value>,
        visible_slots: &[ValueSlotPath],
    ) -> HashMap<String, Value> {
        let mut context = HashMap::new();

        // Add all attempt context_data fields
        if let Value::Object(data) = attempt_context_data {
            for (k, v) in data {
                context.insert(k.clone(), v.clone());
            }
        }

        // Add visible slot names as metadata
        let slot_names: Vec<Value> = visible_slots
            .iter()
            .map(|s| Value::String(s.name.clone()))
            .collect();
        context.insert("_visible_slots".to_string(), Value::Array(slot_names));

        // Add slot values from visible slots
        for slot in visible_slots {
            let mut slot_obj = Map::new();
            slot_obj.insert("name".to_string(), Value::String(slot.name.clone()));
            slot_obj.insert(
                "value_kind".to_string(),
                Value::String(slot.value_kind.clone()),
            );
            slot_obj.insert("id".to_string(), Value::String(slot.id.to_string()));
            context.insert(slot.name.clone(), Value::Object(slot_obj));
        }

        // This request's context wins over both of the above.
        if let Some(Value::Object(data)) = request_context_data {
            for (k, v) in data {
                context.insert(k.clone(), v.clone());
            }
        }

        context
    }

    // =========================================================================
    // Legacy methods (kept for backward compatibility during migration)
    // These use the old Handlebars-style {{variable}} syntax and maker_key parameter
    // =========================================================================

    /// Render a prompt for a specific engine (legacy method)
    ///
    /// **Deprecated**: Use `render_prompt_fluent` instead, which uses:
    /// - Locale fallback chains instead of hardcoded "en-US" preference
    /// - $variable syntax instead of {{variable}}
    /// - Engine fallback chains for better resolution
    #[allow(clippy::too_many_arguments)]
    pub async fn render_prompt(
        &self,
        pool: &DbPool,
        prompt_id: Uuid,
        maker_key: &str,
        engine_key: &str,
        engine_version: Option<&str>,
        context_data: &Value,
        visible_slots: &[ValueSlotPath],
    ) -> Result<RenderedPrompt, ConduitError> {
        // Fetch prompt messages with appropriate bodies
        let raw_messages = self
            .fetch_prompt_messages(pool, prompt_id, maker_key, engine_key, engine_version)
            .await?;

        if raw_messages.is_empty() {
            return Err(ConduitError::NotFound(format!(
                "Prompt {} not found or has no messages",
                prompt_id
            )));
        }

        let prompt_name = raw_messages
            .first()
            .map(|m| m.prompt_name.clone())
            .unwrap_or_default();

        // Build substitution context
        let context = self.build_substitution_context(context_data, None, visible_slots);

        // Render each message body using $variable substitution
        let messages = raw_messages
            .into_iter()
            .map(|raw| {
                let body = raw.body.unwrap_or_default();
                let (content, _consumed) = substitute_tracking(&body, &context);
                PromptMessage {
                    role: raw.role,
                    content,
                }
            })
            .collect();

        Ok(RenderedPrompt {
            messages,
            prompt_name,
        })
    }

    /// Resolve prompt ID and render messages (legacy method)
    ///
    /// **Deprecated**: Use `resolve_and_render_prompt_fluent` instead.
    #[allow(clippy::too_many_arguments)]
    pub async fn resolve_and_render_prompt(
        &self,
        pool: &DbPool,
        prompt_id: Uuid,
        maker_key: &str,
        engine_key: &str,
        engine_version: Option<&str>,
        context_data: &Value,
        request_context_data: Option<&Value>,
        visible_slots: &[ValueSlotPath],
    ) -> Result<ResolvedPrompt, ConduitError> {
        // Fetch prompt messages
        let raw_messages = self
            .fetch_prompt_messages(pool, prompt_id, maker_key, engine_key, engine_version)
            .await?;

        if raw_messages.is_empty() {
            return Err(ConduitError::NotFound(format!(
                "Prompt {} not found or has no messages",
                prompt_id
            )));
        }

        let first_msg = raw_messages.first().unwrap();
        let prompt_id = first_msg.prompt_id;
        let prompt_name = first_msg.prompt_name.clone();
        let prompt_description = first_msg.prompt_description.clone();

        // Build substitution context
        let context =
            self.build_substitution_context(context_data, request_context_data, visible_slots);

        // Render each message body using $variable substitution, recording usage
        let mut consumed_context_keys = BTreeSet::new();
        let messages = raw_messages
            .into_iter()
            .map(|raw| {
                let body = raw.body.unwrap_or_default();
                let (content, consumed) = substitute_tracking(&body, &context);
                consumed_context_keys.extend(consumed);
                PromptMessage {
                    role: raw.role,
                    content,
                }
            })
            .collect();

        Ok(ResolvedPrompt {
            prompt_id,
            prompt_name,
            prompt_description,
            messages,
            consumed_context_keys,
        })
    }

    /// Fetch prompt messages from the database (legacy helper)
    async fn fetch_prompt_messages(
        &self,
        pool: &DbPool,
        prompt_id: Uuid,
        maker_key: &str,
        engine_key: &str,
        engine_version: Option<&str>,
    ) -> Result<Vec<RawPromptMessageModel>, ConduitError> {
        select_prompt_messages_for_engine(pool, prompt_id, maker_key, engine_key, engine_version)
            .await
            .map_err(ConduitError::Database)
    }
}

impl Default for TemplateService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dapple_fluent::substitute;

    #[test]
    fn test_build_substitution_context_empty_slots() {
        let service = TemplateService::new();

        let context_data = serde_json::json!({
            "user_name": "Alice",
            "task": "summarize"
        });

        let visible_slots: Vec<ValueSlotPath> = vec![];

        let ctx = service.build_substitution_context(&context_data, None, &visible_slots);

        assert_eq!(ctx.get("user_name"), Some(&Value::String("Alice".into())));
        assert_eq!(ctx.get("task"), Some(&Value::String("summarize".into())));
        assert!(ctx.contains_key("_visible_slots"));
        assert_eq!(ctx.get("_visible_slots"), Some(&Value::Array(vec![])));
    }

    /// `PromptRequest.context_data` is documented to take precedence over the
    /// attempt context. The LLM prompt path used to ignore it entirely, so every
    /// `$var` a guide author wrote resolved against a map that never held their
    /// keys and reached the model as literal text.
    #[test]
    fn test_request_context_overrides_attempt_context() {
        let service = TemplateService::new();

        let attempt = serde_json::json!({ "a": "STALE", "only_attempt": "kept" });
        let request = serde_json::json!({ "a": "ZIP", "a_sections": "ITEM 1A" });
        let visible_slots: Vec<ValueSlotPath> = vec![];

        let ctx = service.build_substitution_context(&attempt, Some(&request), &visible_slots);

        assert_eq!(ctx.get("a"), Some(&Value::String("ZIP".into())));
        assert_eq!(
            ctx.get("a_sections"),
            Some(&Value::String("ITEM 1A".into()))
        );
        // Attempt-only keys still come through.
        assert_eq!(ctx.get("only_attempt"), Some(&Value::String("kept".into())));
    }

    #[test]
    fn test_substitute_simple_variable() {
        let mut ctx = HashMap::new();
        ctx.insert("name".to_string(), Value::String("World".to_string()));

        let result = substitute("Hello, $name!", &ctx);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_substitute_missing_variable() {
        let ctx = HashMap::new();

        // Missing variables remain as-is (not empty like Handlebars)
        let result = substitute("Hello, $name!", &ctx);
        assert_eq!(result, "Hello, $name!");
    }

    #[test]
    fn test_engine_fallback_configuration() {
        let mut service = TemplateService::new();
        service.add_engine_fallback(
            "claude-3.5-sonnet",
            vec!["claude-3.5".into(), "claude".into()],
        );

        let fallbacks = service.get_engine_fallbacks("claude-3.5-sonnet");
        assert_eq!(fallbacks, &["claude-3.5", "claude"]);

        // Unknown engine returns empty slice
        let fallbacks = service.get_engine_fallbacks("unknown-engine");
        assert!(fallbacks.is_empty());
    }

    #[test]
    fn test_engine_chain_with_fallbacks() {
        let chain = engine_chain(
            "claude-3.5-sonnet",
            Some("20241022"),
            &["claude-3.5".into(), "claude".into()],
        );
        assert_eq!(
            chain,
            vec![
                "claude-3.5-sonnet:20241022",
                "claude-3.5-sonnet",
                "claude-3.5",
                "claude"
            ]
        );
    }

    #[test]
    fn test_locale_chain_with_region() {
        let chain = locale_chain("pt-BR", "en");
        assert_eq!(chain, vec!["pt-BR", "pt", "en"]);
    }
}
