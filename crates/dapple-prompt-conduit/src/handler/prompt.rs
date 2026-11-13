use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use dapple_data::model::prompt_responses::InsertPromptResponseArgs;
use dapple_data::query::prompt_responses::insert_prompt_response;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::ConduitError;
use crate::event::PromptEvent;
use crate::service::llm::{StreamChunk, UsageMetadata};
use crate::service::rag::RagContext;
use crate::service::task_context_client::TaskContext;
use crate::service::{AppState, ModelConfig};

/// Type alias for the SSE stream used by prompt handlers
type PromptStream = BoxStream<'static, Result<Event, Infallible>>;

/// The platform frame: a conduit-owned, guide-independent system message
/// explaining what a conduit is, how its output is used, and how to read the
/// `# Guide Frame` / `# Dispatch Context` / `# Reference Context` sections.
/// Injected at final index 0 on every dispatch-backed prompt. Editing the
/// asset file changes every prompt on the next conduit deploy — no guide
/// re-ingest involved.
const PLATFORM_FRAME: &str = include_str!("../../assets/platform_frame.md");

/// Request to execute a prompt
#[derive(Debug, Deserialize, Serialize)]
pub struct PromptRequest {
    /// The prompt object ID to execute
    pub prompt_id: Uuid,
    /// The attempt ID for context tracking (context_data fetched via ContextService)
    pub attempt_id: Uuid,
    /// The playbook task ID for overlay and engine resolution
    pub playbook_task_id: Uuid,
    /// Optional context data passed directly from client (for summarize, madlib, synth commands)
    /// This takes precedence over context_data fetched from attempt if present
    #[serde(default)]
    pub context_data: Option<Value>,
    /// RAG context for guide content retrieval.
    /// When present and non-empty guide_ids, Conduit will query pgvector
    /// and inject retrieved chunks as system context.
    #[serde(default)]
    pub rag_context: Option<RagContext>,
    /// Dispatch ID for runtime task-context injection (Phase F). When paired
    /// with `task_id`, Conduit fetches the task context from api and
    /// prepends it as a system message. Backward compatible: absent on
    /// legacy clients and synth/summarize/madlib commands.
    #[serde(default)]
    pub dispatch_id: Option<Uuid>,
    /// Dispatch-task ID (i.e. the `dispatch_task.id`, not the source task_id)
    /// that is currently executing. Pairs with `dispatch_id` to target the
    /// api's `GET /api/dispatches/{id}/task_context/{task_id}` endpoint.
    #[serde(default)]
    pub task_id: Option<Uuid>,
}

/// True when an LLM completion produced no usable text — i.e. the collected
/// token chunks are empty or all-whitespace. The LLM prompt handler treats this
/// as a retryable failure rather than a silent empty success (see the
/// `empty_completion` guard): a transient provider error/overload can end a
/// stream cleanly with zero tokens, and reporting that as `done` lets empty
/// output flow downstream (→ "no candidates" → dead Next).
fn is_empty_completion(tokens: &[String]) -> bool {
    tokens.iter().all(|t| t.trim().is_empty())
}

/// Handle a prompt execution request via SSE
pub async fn handle_prompt(
    State(state): State<AppState>,
    Json(req): Json<PromptRequest>,
) -> Result<Sse<PromptStream>, ConduitError> {
    let request_id = Uuid::now_v7();
    let start = Instant::now();
    let started_at = OffsetDateTime::now_utc();

    log::info!(
        "Handling prompt request: request_id={}, prompt_id={}, playbook_task_id={}",
        request_id,
        req.prompt_id,
        req.playbook_task_id
    );

    // 0. Route image prompts by the prompt's OWN maker/lineage keys, before the
    //    generic model resolution below. Image generation is selected by the
    //    prompt carrying the "image" maker, not by an inference profile, so it
    //    routes here directly (additive; non-image prompts are untouched). The
    //    lookup runs UNCONDITIONALLY — NOT gated on the image service — so an image
    //    prompt always reaches handle_image_prompt, which fails cleanly with
    //    "image_unavailable" when the service is off. Gating on
    //    image_service.is_some() previously let an image prompt fall through to the
    //    LLM backend (Anthropic), which then 400s on an empty text content block.
    let maker_keys =
        dapple_data::query::prompts::select_prompt_maker_keys(&state.pool, req.prompt_id)
            .await
            .unwrap_or_default();
    if maker_keys
        .iter()
        .any(|k| k == crate::handler::image::IMAGE_MAKER_KEY)
    {
        return crate::handler::image::handle_image_prompt(state, req, request_id, start).await;
    }
    // Video generation is selected the same way (by the "video" maker/lineage
    // key). Like the image path this runs unconditionally so a video prompt
    // always reaches handle_video_prompt, which fails cleanly with
    // "video_unavailable" when the service is off (FAL_AUTH_KEY unset).
    if maker_keys
        .iter()
        .any(|k| k == crate::handler::video::VIDEO_MAKER_KEY)
    {
        return crate::handler::video::handle_video_prompt(state, req, request_id, start).await;
    }
    // TTS (text-to-speech) is selected the same way (by the "tts" maker/lineage
    // key), replacing the removed VITS/ONNX path. Runs unconditionally so a TTS
    // prompt always reaches handle_tts_prompt, which fails cleanly with
    // "tts_unavailable" when the service is off (OPENAI_API_KEY unset).
    if maker_keys
        .iter()
        .any(|k| k == crate::handler::tts::TTS_MAKER_KEY)
    {
        return crate::handler::tts::handle_tts_prompt(state, req, request_id, start).await;
    }

    // 1. Resolve model config (before stream to enable routing). Prefers the
    //    journey's authored inference profile (keyed by dispatch + prompt),
    //    falling back to the env default when no profile config applies.
    let model = state
        .model_service
        .resolve_model_for_prompt(req.dispatch_id, req.prompt_id)
        .await?;

    log::debug!(
        "Resolved model: lineage={}, provider={}, tier={}, release={:?}",
        model.lineage_key,
        model.provider_key,
        model.tier,
        model.release
    );

    // 2. Route requests to dedicated handlers based on lineage_key
    // These lineages use specialized backends instead of LLM. (TTS moved to the
    // phase-0 maker-key route above, alongside image/video.)
    if model.lineage_key == "flan-t5" {
        return handle_summarization_prompt(state, req, model, request_id, start).await;
    }
    if model.lineage_key == "modernbert" {
        return handle_mlm_prompt(state, req, model, request_id, start).await;
    }

    // Clone state values for use in the async stream
    let pool = Arc::clone(&state.pool);

    // Create the SSE stream for LLM prompts
    let stream = async_stream::stream! {
        // 2. Send started event
        let started = PromptEvent::started(request_id, model.engine_key(), req.prompt_id);
        yield Ok(Event::default().data(started.to_sse_data()));

        // 2.5. Fetch context data from attempt
        let attempt_context = match state.context_service
            .fetch_context_for_attempt(req.attempt_id)
            .await
        {
            Ok(ctx) => {
                log::debug!(
                    "Fetched context for attempt {}: task={}",
                    req.attempt_id,
                    ctx.task_id
                );
                ctx
            }
            Err(e) => {
                let error = PromptEvent::error("context_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 3. Resolve visible value slots for the task
        let visible_slots = match state.overlay_service.resolve_visible_slots(req.playbook_task_id).await {
            Ok(slots) => {
                log::debug!(
                    "Resolved {} visible value slots for task {}",
                    slots.len(),
                    req.playbook_task_id
                );
                slots
            }
            Err(e) => {
                let error = PromptEvent::error("overlay_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 4. Render prompt template via TemplateService (with prompt_id resolution)
        // Use lineage_key as maker_key for template resolution compatibility
        let resolved = match state.template_service.resolve_and_render_prompt(
            state.pool.as_ref(),
            req.prompt_id,
            &model.lineage_key,
            &model.engine_key(),
            model.release.as_deref(),
            &attempt_context.context_data,
            req.context_data.as_ref(),
            &visible_slots,
        ).await {
            Ok(resolved) => {
                log::debug!(
                    "Resolved prompt '{}' (id={}) with {} messages",
                    resolved.prompt_name,
                    resolved.prompt_id,
                    resolved.messages.len()
                );
                resolved
            }
            Err(e) => {
                let error = PromptEvent::error("template_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // Create a RenderedPrompt for LLM execution (convert from ResolvedPrompt)
        let mut rendered = crate::service::template::RenderedPrompt {
            messages: resolved.messages.clone(),
            prompt_name: resolved.prompt_name.clone(),
        };

        // 4.4. Task-context fetch (Phase F).
        //
        // When the request carries both `dispatch_id` and `task_id`, Conduit
        // calls api's `task_context` endpoint to collect runtime dispatch
        // state (bindings, asset extraction availability, prior task outcomes,
        // brief context). The rendered section is injected as a system
        // message later, AFTER the RAG block, at index 0 — so the final
        // system-message order is `[task_context, rag, ...rest]`.
        //
        // Best-effort: transport / decode failures are logged and the stream
        // continues without injection. Backward compatible: if either field
        // is missing, task-context injection is a silent no-op, which lets
        // legacy clients (summarize/madlib/synth, tests) keep working.
        //
        // MVP NOTE: shared conceptual knowledge (_shared/dapple/agent-model),
        // overlay inference, and unified retrieval over asset_content_embeddings
        // are deliberately out of scope for this phase. See the content
        // pipeline plan for the follow-up work.
        let (task_context_section, guide_frame_section): (Option<String>, Option<String>) =
            match (req.dispatch_id, req.task_id) {
                (Some(dispatch_id), Some(task_id)) => {
                    match state.task_context_client.fetch(dispatch_id, task_id).await {
                        Ok(Some(ctx)) => {
                            // Guide frame: the pinned guide version's CONDUITS.md,
                            // rendered as its own stable system message ahead of the
                            // volatile per-task dispatch context.
                            let guide_frame = ctx
                                .guide_frame
                                .as_deref()
                                .filter(|s| !s.trim().is_empty())
                                .map(render_guide_frame);
                            let rendered_section = render_task_context(&ctx);
                            let section = if rendered_section.is_empty() {
                                None
                            } else {
                                log::info!(
                                    "task_context: injected section for dispatch={}, task={} ({} chars)",
                                    dispatch_id,
                                    task_id,
                                    rendered_section.len()
                                );
                                Some(rendered_section)
                            };
                            (section, guide_frame)
                        }
                        Ok(None) => {
                            log::debug!(
                                "task_context: api returned no obj or client not configured (dispatch={}, task={})",
                                dispatch_id,
                                task_id
                            );
                            (None, None)
                        }
                        Err(e) => {
                            log::warn!(
                                "task_context fetch failed for dispatch={}, task={}, continuing without: {}",
                                dispatch_id,
                                task_id,
                                e
                            );
                            (None, None)
                        }
                    }
                }
                _ => (None, None),
            };

        // 4.5. RAG context retrieval (if requested)
        // Inject retrieved guide content as a system message before the user message.
        // RAG failure is non-fatal: log and continue without RAG context.
        if let Some(ref rag_ctx) = req.rag_context {
            if let Some(ref rag_service) = state.rag_service {
                // Prefer context_data (task input) over system prompt for RAG query —
                // the task content is semantically closer to the agent prose blocks.
                let context_query = req.context_data
                    .as_ref()
                    .and_then(|v| v.as_str().map(String::from).or_else(|| serde_json::to_string(v).ok()))
                    .unwrap_or_default();
                let prompt_text = if context_query.is_empty() {
                    rendered.messages.first().map(|m| m.content.as_str()).unwrap_or("")
                } else {
                    &context_query
                };

                match rag_service.retrieve(rag_ctx, prompt_text).await {
                    Ok(rag_result) if !rag_result.context_text.is_empty() => {
                        log::info!(
                            "RAG: injected {} chunks for prompt_id={}",
                            rag_result.chunk_count,
                            req.prompt_id
                        );

                        // Prepend RAG context as first system message
                        rendered.messages.insert(0, crate::service::template::PromptMessage {
                            role: Some("system".to_string()),
                            content: rag_result.context_text,
                        });
                    }
                    Ok(_) => {
                        log::debug!("RAG: no relevant chunks found for prompt_id={}", req.prompt_id);
                    }
                    Err(e) => {
                        // RAG failure is non-fatal -- log and continue without RAG context
                        log::warn!("RAG retrieval failed, continuing without: {}", e);
                    }
                }
            } else {
                log::debug!("RAG: rag_context provided but RAG service not available");
            }
        }

        // 4.6. Frame + task-context injection.
        //
        // Successive inserts at index 0 build the final system-message order
        // `[platform_frame, guide_frame, task_context, rag, ...rest]`:
        // the stable platform frame first, then the guide's own frame
        // (CONDUITS.md), then the volatile per-task dispatch context, then
        // retrieved reference excerpts. Each section is independent and
        // optional — absent sections simply drop out of the order.
        if let Some(task_context_text) = task_context_section {
            rendered.messages.insert(
                0,
                crate::service::template::PromptMessage {
                    role: Some("system".to_string()),
                    content: task_context_text,
                },
            );
        }
        if let Some(guide_frame_text) = guide_frame_section {
            rendered.messages.insert(
                0,
                crate::service::template::PromptMessage {
                    role: Some("system".to_string()),
                    content: guide_frame_text,
                },
            );
        }
        // Platform frame: injected for every dispatch-backed prompt (same
        // condition as the task-context fetch), independent of whether the
        // fetch itself succeeded — the frame is a static in-crate constant.
        if req.dispatch_id.is_some() && req.task_id.is_some() {
            rendered.messages.insert(
                0,
                crate::service::template::PromptMessage {
                    role: Some("system".to_string()),
                    content: PLATFORM_FRAME.to_string(),
                },
            );
        }

        // Inject req.context_data as user message (matches TTS/summarize handler pattern)
        // The nushell `prompt send` command packs pipeline input into context_data,
        // which contains the actual user request (e.g., "Generate 8 essay topics...Return ONLY JSON...")
        //
        // Only the keys the TEMPLATE DID NOT already interpolate are shipped here.
        // A prompt that writes `$a_sections` now has that text inlined where the
        // author put it, and re-appending the whole record would send the same
        // filing twice — on a 260 KB context that doubles the prompt for nothing.
        // Prompts that interpolate nothing (the common case) keep the full dump and
        // are byte-identical to before.
        if let Some(ref ctx) = req.context_data {
            let consumed = &resolved.consumed_context_keys;
            let remaining = match ctx {
                Value::Object(map) if !consumed.is_empty() => {
                    let kept: serde_json::Map<String, Value> = map
                        .iter()
                        .filter(|(k, _)| !consumed.contains(*k))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    if kept.is_empty() {
                        None
                    } else {
                        Some(Value::Object(kept))
                    }
                }
                other => Some(other.clone()),
            };

            if let Some(remaining) = remaining {
                let user_text = match remaining.as_str() {
                    Some(s) => s.to_string(),
                    None => serde_json::to_string(&remaining).unwrap_or_default(),
                };
                if !user_text.is_empty() && user_text != "null" {
                    log::debug!(
                        "Injecting context_data as user message: {} chars ({} key(s) already inlined by the template)",
                        user_text.len(),
                        consumed.len()
                    );
                    rendered.messages.push(crate::service::template::PromptMessage {
                        role: Some("user".to_string()),
                        content: user_text,
                    });
                }
            } else {
                log::debug!(
                    "Skipping context_data user message: all {} key(s) were interpolated into the template",
                    consumed.len()
                );
            }
        }

        // Estimate input tokens as fallback (used when backend doesn't report real counts)
        let estimated_input_tokens: u32 = rendered.messages.iter()
            .map(|m| m.content.len() as u32 / 4)
            .sum();

        // Serialize request messages for storage (includes injected user message)
        let request_messages_json = json!(rendered.messages.iter().map(|m| {
            json!({
                "role": m.role,
                "content": m.content
            })
        }).collect::<Vec<_>>());

        // 5. Execute via LLM service (streaming) — returns a stream of StreamChunks.
        //    Usage is delivered at stream-END via the trailing StreamChunk::Final.
        let token_stream = match state.llm_service.execute_streaming(&rendered, &model).await {
            Ok(result) => result,
            Err(e) => {
                log::error!("LLM execution failed: {}", e);
                let error = PromptEvent::error("llm_error", e.to_string(), true);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 6. Stream tokens as SSE events and collect for storage
        let mut streamed_output_tokens = 0u32;
        let mut collected_tokens: Vec<String> = Vec::new();
        let mut final_usage = UsageMetadata::default();
        let mut token_stream = std::pin::pin!(token_stream);

        while let Some(result) = token_stream.next().await {
            match result {
                Ok(StreamChunk::Token(token)) => {
                    // Count only token chunks (NOT the trailing Final).
                    streamed_output_tokens += 1;
                    collected_tokens.push(token.clone());
                    let event = PromptEvent::token(token);
                    yield Ok(Event::default().data(event.to_sse_data()));
                }
                Ok(StreamChunk::Final(usage)) => {
                    final_usage = usage;
                }
                Err(e) => {
                    let error = PromptEvent::error("stream_error", e.to_string(), true);
                    yield Ok(Event::default().data(error.to_sse_data()));
                    return;
                }
            }
        }

        // Guard: a completion that produced NO text is a retryable FAILURE, not
        // a silent empty success. A backend (rig's Anthropic stream, observed)
        // can end cleanly with zero tokens and no Err on a transient provider
        // error/overload; without this guard the handler would send `done` with
        // empty content, which downstream parses as "no candidates" (→ unbound
        // pick slot → dead Next). Failing here makes the task `failed` (retryable
        // via re-dispatch) and visible in /task_attempts. Provider-agnostic. For
        // text generation an empty completion is never valid; revisit if
        // tool-only responses are ever expected on this path.
        if is_empty_completion(&collected_tokens) {
            log::warn!(
                "Empty LLM completion for prompt_id={} (model {}/{}): no output text, treating as retryable failure",
                req.prompt_id,
                model.lineage_key,
                model.tier,
            );
            let error = PromptEvent::error(
                "empty_completion",
                "LLM returned an empty completion (no text) — likely a transient provider error or overload; retry",
                true,
            );
            yield Ok(Event::default().data(error.to_sse_data()));
            return;
        }

        // Use real token counts from backend when available, fall back to estimates
        let input_tokens = final_usage.input_tokens.unwrap_or(estimated_input_tokens);
        let output_tokens = final_usage.output_tokens.unwrap_or(streamed_output_tokens);

        // 7. Send done event
        let latency_ms = start.elapsed().as_millis() as u64;
        let completed_at = OffsetDateTime::now_utc();
        let done = PromptEvent::done(
            output_tokens,
            input_tokens,
            latency_ms,
            "stop",
        );
        yield Ok(Event::default().data(done.to_sse_data()));

        log::info!(
            "Completed prompt request: request_id={}, output_tokens={}, input_tokens={}, latency_ms={}, real_usage={}",
            request_id,
            output_tokens,
            input_tokens,
            latency_ms,
            final_usage.input_tokens.is_some(),
        );

        // 8. Spawn fire-and-forget task to store response
        let response_content = collected_tokens.join("");
        let storage_pool = pool.clone();
        let prompt_id = resolved.prompt_id;
        let attempt_id = req.attempt_id;
        let playbook_task_id = req.playbook_task_id;
        // Store lineage_key as maker_key for backwards compatibility in prompt_responses
        let lineage_key = model.lineage_key.clone();
        let engine_key = model.engine_key();
        let release = model.release.clone();

        if playbook_task_id.is_nil() {
            log::warn!(
                "Skipping prompt response storage: playbook_task_id=0 (would cause FK violation), prompt_id={}",
                prompt_id
            );
        } else {
            tokio::spawn(async move {
                let args = InsertPromptResponseArgs {
                    attempt_ids: vec![attempt_id],
                    prompt_ids: vec![prompt_id],
                    step_task_ids: vec![playbook_task_id],
                    maker_keys: vec![lineage_key],
                    engine_keys: vec![engine_key],
                    engine_versions: vec![release],
                    request_messages: vec![request_messages_json],
                    response_contents: vec![response_content],
                    input_tokens: vec![input_tokens as i32],
                    output_tokens: vec![output_tokens as i32],
                    latency_ms: vec![latency_ms as i32],
                    finish_reasons: vec!["stop".to_string()],
                    started_ats: vec![started_at],
                    completed_ats: vec![completed_at],
                };

                if let Err(e) = insert_prompt_response(&storage_pool, args).await {
                    log::error!("Failed to store prompt response: {}", e);
                } else {
                    log::debug!("Stored prompt response for prompt_id={}", prompt_id);
                }
            });
        }
    };

    Ok(Sse::new(Box::pin(stream)))
}

/// Render a `TaskContext` into the Markdown-flavored system message that
/// Conduit injects as runtime dispatch state.
///
/// Keeps only non-empty sections so the model isn't distracted by empty
/// "## Bindings" / "## Active Briefs" stubs.
///
/// # Format
///
/// ```text
/// # Dispatch Context
///
/// You are executing task `name` in stage N, step M.
///
/// ## Bindings
/// - `slot` (kind): preview
///
/// ## Available Asset Extractions
/// - Asset `uuid` (kind, status=..., N pages, M chars)
///   Retrieve via: `asset text <uuid>`
///
/// ## Prior Task Outcomes
/// - `name`: status
///   Summary: <summary if present>
///
/// ## Active Briefs
/// - `brief-name`
///   Units:
///     - kind `name`: <submitted_value>
/// ```
fn render_task_context(ctx: &TaskContext) -> String {
    let mut out = String::new();
    out.push_str("# Dispatch Context\n\n");

    // Header line: task name + stage/step indices. Indices are 0-based in the
    // API response; we surface them 1-based here so they line up with how
    // authors think about stage/step ordering in guides.
    let task_label = ctx
        .dispatch_position
        .task_name
        .as_deref()
        .unwrap_or("<unnamed>");
    out.push_str(&format!(
        "You are executing task `{}` in stage {}, step {}.\n",
        task_label,
        ctx.dispatch_position.stage_index + 1,
        ctx.dispatch_position.step_index + 1,
    ));

    // --- Position (authored pane/interaction context) -----------------------
    // Journey-pinned stage/step names + descriptions, when the api resolved
    // them: which pane of the journey this work sits on and what it is for.
    let pos = &ctx.dispatch_position;
    if pos.stage_name.is_some() || pos.step_name.is_some() {
        out.push_str("\n## Position\n");
        if let Some(stage_name) = pos.stage_name.as_deref() {
            let ord_part = match (pos.stage_ord, pos.total_stages) {
                (Some(ord), Some(total)) => format!("Stage {}/{}", ord, total),
                _ => "Stage".to_string(),
            };
            out.push_str(&format!("- {}: `{}`", ord_part, stage_name));
            if let Some(desc) = pos.stage_description.as_deref().filter(|d| !d.is_empty()) {
                out.push_str(&format!(" — {}", desc));
            }
            out.push('\n');
        }
        if let Some(step_name) = pos.step_name.as_deref() {
            out.push_str(&format!("- Step: `{}`", step_name));
            if let Some(desc) = pos.step_description.as_deref().filter(|d| !d.is_empty()) {
                out.push_str(&format!(" — {}", desc));
            }
            out.push('\n');
        }
    }

    // --- Bindings -----------------------------------------------------------
    if !ctx.bindings.is_empty() {
        out.push_str("\n## Bindings\n");
        for b in &ctx.bindings {
            let preview = binding_preview_to_string(&b.value_preview);
            out.push_str(&format!(
                "- `{}` ({}): {}\n",
                b.slot_name, b.value_kind, preview
            ));
        }
    }

    // --- Asset extractions --------------------------------------------------
    if !ctx.assets.is_empty() {
        out.push_str("\n## Available Asset Extractions\n");
        for a in &ctx.assets {
            let page_part = a
                .page_count
                .map(|p| format!("{} pages", p))
                .unwrap_or_else(|| "unknown pages".to_string());
            let text_part = a
                .text_length
                .map(|t| format!("{} chars", t))
                .unwrap_or_else(|| "unknown chars".to_string());
            out.push_str(&format!(
                "- Asset `{}` ({}, status={}, {}, {})\n",
                a.asset_id, a.extraction_kind, a.status, page_part, text_part,
            ));
            out.push_str(&format!("  Retrieve via: `{}`\n", a.retrieval_command));
        }
    }

    // --- Prior task outcomes ------------------------------------------------
    if !ctx.prior_tasks.is_empty() {
        out.push_str("\n## Prior Task Outcomes\n");
        for p in &ctx.prior_tasks {
            let name = p.task_name.as_deref().unwrap_or("<unnamed>");
            out.push_str(&format!("- `{}`: {}\n", name, p.status));
            if let Some(summary) = p.summary.as_deref().filter(|s| !s.is_empty()) {
                out.push_str(&format!("  Summary: {}\n", summary));
            }
        }
    }

    // --- Active briefs ------------------------------------------------------
    if !ctx.brief_context.is_empty() {
        out.push_str("\n## Active Briefs\n");
        for b in &ctx.brief_context {
            let name = b.brief_name.as_deref().unwrap_or("<unnamed>");
            out.push_str(&format!("- `{}`\n", name));
            if !b.units.is_empty() {
                out.push_str("  Units:\n");
                for u in &b.units {
                    let unit_name = u.name.as_deref().unwrap_or("<unnamed>");
                    let value_str = u
                        .submitted_value
                        .as_ref()
                        .map(binding_preview_to_string)
                        .unwrap_or_else(|| "<not submitted>".to_string());
                    out.push_str(&format!(
                        "    - {} `{}`: {}\n",
                        u.unit_kind, unit_name, value_str
                    ));
                }
            }
        }
    }

    out
}

/// Render the guide frame system message: a fixed header + preamble, then the
/// guide's raw CONDUITS.md markdown (author-controlled, journey-pinned).
fn render_guide_frame(conduits_md: &str) -> String {
    format!(
        "# Guide Frame\n\n_Your operating context for this guide: its bindings, \
         their types, and how to read retrieved guide prose._\n\n{}",
        conduits_md.trim()
    )
}

/// Render a `serde_json::Value` preview into a compact single-line string
/// suitable for a markdown bullet body. Strings pass through unquoted (the
/// api already truncates them); everything else goes through `to_string` to
/// keep JSON semantics intact.
fn binding_preview_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// Handle a summarization prompt execution request via SSE
/// Routes lineage_key="flan-t5" prompts to the summarization service
async fn handle_summarization_prompt(
    state: AppState,
    req: PromptRequest,
    model: ModelConfig,
    request_id: Uuid,
    start: Instant,
) -> Result<Sse<PromptStream>, ConduitError> {
    log::info!(
        "Handling summarization prompt: request_id={}, prompt_id={}",
        request_id,
        req.prompt_id
    );

    // Create the SSE stream for summarization
    let stream = async_stream::stream! {
        // 1. Send started event
        let started = PromptEvent::started(request_id, model.engine_key(), req.prompt_id);
        yield Ok(Event::default().data(started.to_sse_data()));

        // 2. Get context data - prefer request context_data if provided, fall back to attempt context
        let context_data = if let Some(ctx) = &req.context_data {
            ctx.clone()
        } else {
            match state.context_service.fetch_context_for_attempt(req.attempt_id).await {
                Ok(ctx) => ctx.context_data,
                Err(e) => {
                    let error = PromptEvent::error("context_error", e.to_string(), false);
                    yield Ok(Event::default().data(error.to_sse_data()));
                    return;
                }
            }
        };

        // 3. Extract text from context_data
        let input_text = match context_data.get("text").and_then(|v| v.as_str()) {
            Some(text) => text.to_string(),
            None => {
                let error = PromptEvent::error(
                    "input_error",
                    "No 'text' field in context_data for summarization",
                    false,
                );
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 4. Check summarization service availability
        let summarization_service = match &state.summarization_service {
            Some(svc) => svc,
            None => {
                let error = PromptEvent::error(
                    "summarization_unavailable",
                    "Summarization service is not configured",
                    false,
                );
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 5. Get prompt prefix from model_body if available
        // First resolve the prompt to get model_body
        let visible_slots = match state.overlay_service.resolve_visible_slots(req.playbook_task_id).await {
            Ok(slots) => slots,
            Err(e) => {
                let error = PromptEvent::error("overlay_error", e.to_string(), false);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 6. Fetch prompt config to get model_body with prefix
        let prompt_prefix = match state.template_service.resolve_and_render_prompt(
            state.pool.as_ref(),
            req.prompt_id,
            &model.lineage_key,
            &model.engine_key(),
            model.release.as_deref(),
            // `context_data` here is already request-context-preferred (see the
            // resolution above), so there is nothing further to layer on top.
            &context_data,
            None,
            &visible_slots,
        ).await {
            Ok(resolved) => {
                // Try to parse the first message content as JSON to extract prefix
                resolved.messages.first()
                    .and_then(|m| serde_json::from_str::<serde_json::Value>(&m.content).ok())
                    .and_then(|v| v.get("prefix").and_then(|p| p.as_str()).map(String::from))
            }
            Err(e) => {
                log::warn!("Failed to resolve prompt template for prefix: {}", e);
                None
            }
        };

        log::debug!(
            "Summarization prompt: text_len={}, prefix={:?}",
            input_text.len(),
            prompt_prefix
        );

        // 7. Perform summarization
        let summary = match summarization_service.summarize_text_with_prefix(&input_text, prompt_prefix.as_deref()).await {
            Ok(s) => s,
            Err(e) => {
                let error = PromptEvent::error("summarization_error", e.to_string(), true);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 8. Send result as tokens (single token for simplicity)
        let token_event = PromptEvent::token(summary.clone());
        yield Ok(Event::default().data(token_event.to_sse_data()));

        // 9. Send done event
        let latency_ms = start.elapsed().as_millis() as u64;
        let done = PromptEvent::done(1, (input_text.len() / 4) as u32, latency_ms, "stop");
        yield Ok(Event::default().data(done.to_sse_data()));

        log::info!(
            "Completed summarization prompt: request_id={}, latency_ms={}",
            request_id,
            latency_ms
        );
    };

    Ok(Sse::new(Box::pin(stream)))
}

/// Handle an MLM prompt execution request via SSE
/// Routes lineage_key="modernbert" prompts to the MLM service
async fn handle_mlm_prompt(
    state: AppState,
    req: PromptRequest,
    model: ModelConfig,
    request_id: Uuid,
    start: Instant,
) -> Result<Sse<PromptStream>, ConduitError> {
    log::info!(
        "Handling MLM prompt: request_id={}, prompt_id={}",
        request_id,
        req.prompt_id
    );

    // Create the SSE stream for MLM
    let stream = async_stream::stream! {
        // 1. Send started event
        let started = PromptEvent::started(request_id, model.engine_key(), req.prompt_id);
        yield Ok(Event::default().data(started.to_sse_data()));

        // 2. Get context data - prefer request context_data if provided, fall back to attempt context
        let context_data = if let Some(ctx) = &req.context_data {
            ctx.clone()
        } else {
            match state.context_service.fetch_context_for_attempt(req.attempt_id).await {
                Ok(ctx) => ctx.context_data,
                Err(e) => {
                    let error = PromptEvent::error("context_error", e.to_string(), false);
                    yield Ok(Event::default().data(error.to_sse_data()));
                    return;
                }
            }
        };

        // 3. Extract text and parameters from context_data
        let input_text = match context_data.get("text").and_then(|v| v.as_str()) {
            Some(text) => text.to_string(),
            None => {
                let error = PromptEvent::error(
                    "input_error",
                    "No 'text' field in context_data for MLM",
                    false,
                );
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        let top_k = context_data.get("top_k")
            .and_then(|v| v.as_i64())
            .map(|v| v as usize)
            .unwrap_or(1);

        let verbose = context_data.get("verbose")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // 4. Check MLM service availability
        let mlm_service = match &state.mlm_service {
            Some(svc) => svc,
            None => {
                let error = PromptEvent::error(
                    "mlm_unavailable",
                    "MLM service is not configured",
                    false,
                );
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        log::debug!(
            "MLM prompt: text_len={}, top_k={}, verbose={}",
            input_text.len(),
            top_k,
            verbose
        );

        // 5. Perform mask filling
        let result = match mlm_service.fill_masks(&input_text, top_k).await {
            Ok(r) => r,
            Err(e) => {
                let error = PromptEvent::error("mlm_error", e.to_string(), true);
                yield Ok(Event::default().data(error.to_sse_data()));
                return;
            }
        };

        // 6. Send result as token(s)
        if verbose {
            // Return full result as JSON
            let result_json = json!({
                "filled_text": result.filled_text,
                "predictions": result.predictions.iter().map(|p| {
                    json!({
                        "position": p.position,
                        "candidates": p.candidates.iter().map(|c| {
                            json!({
                                "token": c.token,
                                "score": c.score
                            })
                        }).collect::<Vec<_>>()
                    })
                }).collect::<Vec<_>>()
            });
            let token_event = PromptEvent::token(result_json.to_string());
            yield Ok(Event::default().data(token_event.to_sse_data()));
        } else {
            // Return just the filled text
            let token_event = PromptEvent::token(result.filled_text.clone());
            yield Ok(Event::default().data(token_event.to_sse_data()));
        }

        // 7. Send done event
        let latency_ms = start.elapsed().as_millis() as u64;
        let done = PromptEvent::done(1, (input_text.len() / 4) as u32, latency_ms, "stop");
        yield Ok(Event::default().data(done.to_sse_data()));

        log::info!(
            "Completed MLM prompt: request_id={}, latency_ms={}",
            request_id,
            latency_ms
        );
    };

    Ok(Sse::new(Box::pin(stream)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_completion_detects_no_output() {
        // No chunks at all (the transient-provider-failure case: stream ended
        // with zero tokens) — must be flagged so the handler fails retryably.
        assert!(is_empty_completion(&[]));
        // All-whitespace chunks are also "no usable text".
        assert!(is_empty_completion(&["".to_string(), "  \n".to_string()]));
    }

    #[test]
    fn empty_completion_allows_real_output() {
        // Any non-blank chunk means a real completion — not flagged.
        assert!(!is_empty_completion(&["hello".to_string()]));
        assert!(!is_empty_completion(&[
            "  ".to_string(),
            "world".to_string(),
        ]));
    }

    #[test]
    fn platform_frame_is_present_and_headed() {
        assert!(PLATFORM_FRAME.starts_with("# Platform Frame"));
        assert!(PLATFORM_FRAME.contains("# Guide Frame"));
        assert!(PLATFORM_FRAME.contains("# Dispatch Context"));
        assert!(PLATFORM_FRAME.contains("# Reference Context"));
    }

    #[test]
    fn guide_frame_renders_header_and_content() {
        let rendered = render_guide_frame("# Conduit Context\n\nBindings here.\n");
        assert!(rendered.starts_with("# Guide Frame\n"));
        assert!(rendered.contains("_Your operating context for this guide"));
        assert!(rendered.contains("Bindings here."));
    }

    #[test]
    fn task_context_renders_position_section() {
        use crate::service::task_context_client::{TaskContext, TaskContextDispatchPosition};
        let ctx = TaskContext {
            dispatch_id: Uuid::now_v7(),
            task_id: Uuid::now_v7(),
            bindings: Vec::new(),
            assets: Vec::new(),
            dispatch_position: TaskContextDispatchPosition {
                stage_index: 0,
                stage_name: Some("read-the-filing".to_string()),
                step_index: 0,
                step_name: Some("mine-the-notes".to_string()),
                task_index: 0,
                task_name: Some("miner".to_string()),
                stage_description: Some("Read the filing whole.".to_string()),
                step_description: Some("Mine typed disclosure beats.".to_string()),
                stage_ord: Some(3),
                total_stages: Some(8),
            },
            prior_tasks: Vec::new(),
            brief_context: Vec::new(),
            guide_frame: None,
        };
        let rendered = render_task_context(&ctx);
        assert!(rendered.contains("## Position"));
        assert!(rendered.contains("Stage 3/8: `read-the-filing` — Read the filing whole."));
        assert!(rendered.contains("Step: `mine-the-notes` — Mine typed disclosure beats."));
    }

    #[test]
    fn task_context_omits_position_when_unresolved() {
        use crate::service::task_context_client::{TaskContext, TaskContextDispatchPosition};
        let ctx = TaskContext {
            dispatch_id: Uuid::now_v7(),
            task_id: Uuid::now_v7(),
            bindings: Vec::new(),
            assets: Vec::new(),
            dispatch_position: TaskContextDispatchPosition {
                stage_index: 1,
                stage_name: None,
                step_index: 0,
                step_name: None,
                task_index: 0,
                task_name: None,
                stage_description: None,
                step_description: None,
                stage_ord: None,
                total_stages: None,
            },
            prior_tasks: Vec::new(),
            brief_context: Vec::new(),
            guide_frame: None,
        };
        let rendered = render_task_context(&ctx);
        assert!(!rendered.contains("## Position"));
    }
}
