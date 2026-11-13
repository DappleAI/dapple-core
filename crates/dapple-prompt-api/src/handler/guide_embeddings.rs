//! Embedding generation pipeline for guides.
//!
//! Extracts embeddable text from guide blocks, calls Conduit's `/embed`
//! endpoint, and stores the resulting embeddings with tag and locale
//! associations.
//!
//! Two entry points:
//! - `generate_embeddings_for_guide_sync()` — propagates errors to the caller.
//!   Used during ingest so embedding failures fail the ingest when conduit is configured.
//! - `generate_embeddings_for_guide()` — fire-and-forget wrapper that swallows errors.
//!   Kept for any future async-only callers.

use crate::AppState;
use crate::service::embedding_client::EmbeddingClient;
use crate::service::text_window::window_text;
use dapple_data::model::guide_content_embeddings::{
    InsertGuideContentEmbeddingArgs, InsertGuideEmbeddingLocaleArgs, InsertGuideEmbeddingTagArgs,
};
use dapple_data::model::guides::{Guide, GuideBlock};
use dapple_data::query::fluent_messages::{
    select_fluent_bodies_by_message_ids, select_fluent_message_locale_ids,
};
use dapple_data::query::guide_content_embeddings::{
    insert_guide_content_embeddings, insert_guide_embedding_generation,
    insert_guide_embedding_locales, insert_guide_embedding_tags,
};
use dapple_data::query::guides::{load_directive_type_lookup_pool, load_tag_lookup_pool};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

/// Directive types that contain embeddable text content.
/// Blocks with these types are candidates for embedding generation.
/// Excluded: nushell (code, not natural language), brief_pack and
/// inference_profile (structural metadata, not narrative content).
const EMBEDDABLE_DIRECTIVE_TYPES: &[&str] = &["prose", "stage", "step", "block", "ref"];

/// How many texts to send per `/embed` call.
///
/// ModernBERT pads a batch to its LONGEST member, so one long block sets the
/// sequence width for every text batched with it. Sending a whole guide as a
/// single call therefore scales peak memory as
/// `block_count * longest_block_tokens`, which OOM-killed conduit-onnx
/// (exit 137, 12Gi limit) on `mark-the-bdc` — 162 blocks whose longest is
/// 11,455 chars (~2,900 tokens).
///
/// Chunking is a TRANSPORT change only, never a semantic one: the backend
/// masks padding correctly, so a text's vector is bit-identical regardless of
/// what it is batched with (verified 2026-08-01 — cosine 1.0, max elementwise
/// diff exactly 0.0, probe embedded alone vs. batched with that 11,455-char
/// block). Do not "optimize" this back into one call: total compute is
/// unchanged (~68s either way), but peak memory becomes unbounded in the
/// guide's size.
///
/// 8 is the measured-safe width: 21 sequential calls over that same guide,
/// slowest single call 35.6s, zero failures.
///
/// **Not to be confused with windowing** ([`crate::service::text_window`]),
/// which is the *semantic* split — one block into several embedding ROWS. This
/// constant is purely transport: how many already-decided texts share one HTTP
/// call. Windowing does bound the sequence width this constant was working
/// around, since no text sent from here now exceeds
/// [`crate::service::text_window::EMBED_WINDOW_CHARS`].
const EMBED_BATCH_SIZE: usize = 8;

/// One embedding row to be: a **window** of a block, not necessarily a whole
/// block. A block that fits in a single window (the overwhelming majority)
/// produces exactly one of these, with `chunk_ordinal = 0, chunk_count = 1`.
struct EmbeddableBlock {
    guide_id: Uuid,
    chapter_id: Uuid,
    block_id: Uuid,
    directive_type_id: i16,
    directive_type_key: String,
    text: String,
    tags: Vec<String>,
    chapter_title: String,
    block_name: Option<String>,
    position: usize,
    /// true if the block has inline content (locale-neutral);
    /// false if the content came from a Fluent message (locale-specific).
    is_locale_neutral: bool,
    /// 0-based index of this window within its source block.
    chunk_ordinal: usize,
    /// Total windows produced for the source block. 1 for any block that fits.
    /// (Unrelated to `RagResult::chunk_count`, which counts excerpts in a
    /// retrieval frame.)
    chunk_count: usize,
}

/// Generate embeddings synchronously, propagating errors to the caller.
///
/// Returns `Ok(())` if embeddings were generated successfully.
/// Returns `Err` if conduit is configured but embedding generation fails.
///
/// The caller is responsible for checking `data.conduit_url.is_some()`
/// before calling — this function will return an error if conduit URL
/// is not set, which the caller may treat as a skip or a failure.
pub async fn generate_embeddings_for_guide_sync(
    data: &Arc<AppState>,
    guide_id: Uuid,
) -> Result<(), String> {
    generate_embeddings_inner(data, guide_id).await
}

/// Generate embeddings for a guide (fire-and-forget).
///
/// Any error is logged as a warning but never propagates to the caller.
#[allow(dead_code)]
pub async fn generate_embeddings_for_guide(data: &Arc<AppState>, guide_id: Uuid) {
    if let Err(e) = generate_embeddings_inner(data, guide_id).await {
        log::warn!(
            "Embedding generation failed for guide {}: {}. Guide was stored without embeddings.",
            guide_id,
            e
        );
    }
}

async fn generate_embeddings_inner(data: &Arc<AppState>, guide_id: Uuid) -> Result<(), String> {
    // 0. Check if conduit URL is configured
    let conduit_url = data.conduit_url.as_ref().ok_or_else(|| {
        "DAPPLE_PROMPT_CONDUIT_URL not set; skipping embedding generation".to_string()
    })?;
    let client = EmbeddingClient::new(conduit_url);

    let pool = &data.db;

    // 1. Load the guide with full chapter/block hierarchy
    let guide = dapple_data::query::guides::select_guide_by_object_id(pool, guide_id)
        .await
        .map_err(|e| format!("Failed to load guide: {}", e))?;

    // 2. Collect all fluent_message_ids from blocks
    let mut fluent_ids: Vec<Uuid> = Vec::new();
    for chapter in &guide.chapters {
        for block in &chapter.blocks {
            if let Some(fm_id) = block.fluent_message_id {
                fluent_ids.push(fm_id);
            }
        }
    }
    fluent_ids.sort();
    fluent_ids.dedup();

    // 3. Batch-resolve all Fluent bodies
    let fluent_bodies = select_fluent_bodies_by_message_ids(pool, &fluent_ids)
        .await
        .map_err(|e| format!("Failed to resolve Fluent bodies: {}", e))?;

    // Build lookup: (message_id, locale) -> body
    let mut body_map: HashMap<(Uuid, String), String> = HashMap::new();
    let mut all_locale_tags: HashSet<String> = HashSet::new();
    for row in &fluent_bodies {
        body_map.insert(
            (row.fluent_message_id, row.locale_tag.clone()),
            row.body.clone(),
        );
        all_locale_tags.insert(row.locale_tag.clone());
    }

    // 4. Resolve locale tag -> locale ID mapping
    let locale_tags_vec: Vec<String> = all_locale_tags.iter().cloned().collect();
    let locale_id_rows = select_fluent_message_locale_ids(pool, &locale_tags_vec)
        .await
        .map_err(|e| format!("Failed to resolve locale IDs: {}", e))?;
    let locale_tag_to_id: HashMap<String, i16> =
        locale_id_rows.into_iter().map(|r| (r.tag, r.id)).collect();

    let all_locale_ids: Vec<i16> = locale_tag_to_id.values().cloned().collect();

    // 5. Load directive type and tag lookups
    let directive_types = load_directive_type_lookup_pool(pool)
        .await
        .map_err(|e| format!("Failed to load directive types: {}", e))?;
    let tag_lookup = load_tag_lookup_pool(pool)
        .await
        .map_err(|e| format!("Failed to load tag lookup: {}", e))?;

    // 6. Extract embeddable blocks
    let blocks_to_embed =
        extract_embeddable_blocks(&guide, &body_map, &fluent_bodies, &directive_types)?;

    if blocks_to_embed.is_empty() {
        log::info!(
            "Guide {} has no embeddable blocks; skipping embedding generation",
            guide_id
        );
        return Ok(());
    }

    // 7. Call Conduit for embedding generation, in bounded batches.
    //
    //    Sequential (not concurrent) on purpose: conduit-onnx runs CPU
    //    inference under a 2-core limit, so parallel calls buy no throughput
    //    and simply multiply peak memory — the thing EMBED_BATCH_SIZE exists
    //    to bound. `chunks()` yields in order and we extend in order, so
    //    `embeddings[i]` still corresponds to `texts[i]`.
    let texts: Vec<String> = blocks_to_embed.iter().map(|b| b.text.clone()).collect();
    let total_batches = texts.len().div_ceil(EMBED_BATCH_SIZE);
    let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
    let mut truncated_total = 0usize;

    for (i, chunk) in texts.chunks(EMBED_BATCH_SIZE).enumerate() {
        match client.embed(chunk).await? {
            Some(batch) => {
                truncated_total += batch.truncated_count;
                embeddings.extend(batch.embeddings);
            }
            None => {
                // 503 from conduit — no embedding service configured. Bail
                // before any DB write so we never leave a partial index; the
                // insert below is all-or-nothing by construction.
                log::info!(
                    "Conduit embedding service not available; skipping for guide {}",
                    guide_id
                );
                return Ok(());
            }
        }
        // Ingest blocks on this call, so a slow guide reports progress instead
        // of going silent for minutes (which is exactly what made the 60s
        // ingress timeout so opaque to diagnose).
        log::info!(
            "Embedding guide {}: batch {}/{} done ({}/{} blocks)",
            guide_id,
            i + 1,
            total_batches,
            embeddings.len(),
            texts.len(),
        );
    }

    if embeddings.len() != texts.len() {
        return Err(format!(
            "Embedding count mismatch: expected {}, got {}",
            texts.len(),
            embeddings.len()
        ));
    }

    // 7b. Refuse to index truncated content.
    //
    //     Windowing (`service::text_window`) sizes windows in CHARS against the
    //     conduit's TOKEN cap via an estimate, so a pathological block (dense
    //     tables, CJK) can still overshoot. If it does, the vectors returned
    //     above cover only each text's leading tokens and the rest would be
    //     unreachable forever — silent index loss, which is exactly what
    //     windowing exists to end. Fail here, before the generation row and
    //     before any embedding is written, so the guide keeps its previous
    //     (complete) generation rather than gaining a lossy one.
    //
    //     Availability, never content: the operator sees a loud ingest failure
    //     and can shrink EMBED_WINDOW_CHARS or split the block.
    if truncated_total > 0 {
        return Err(format!(
            "Refusing to index truncated content: conduit truncated {} of {} \
             embedding inputs for guide {}. Those embeddings would cover only \
             the leading part of their text, so the remainder would never be \
             retrievable. The window size in service::text_window \
             (EMBED_WINDOW_CHARS) has drifted above what the conduit's \
             MAX_SEQUENCE_TOKENS accepts for this content — lower it, or split \
             the offending block.",
            truncated_total,
            texts.len(),
            guide_id,
        ));
    }

    // 8. Create a new embedding generation (append-only, no DELETE)
    let generation = insert_guide_embedding_generation(pool, guide_id)
        .await
        .map_err(|e| format!("Failed to create embedding generation: {}", e))?;

    // 9. Build metadata and insert embeddings
    let metadata: Vec<serde_json::Value> = blocks_to_embed
        .iter()
        .map(|b| build_embedding_metadata(b, &guide))
        .collect();

    let count = blocks_to_embed.len();
    let insert_args = InsertGuideContentEmbeddingArgs {
        guide_ids: blocks_to_embed.iter().map(|b| b.guide_id).collect(),
        chapter_ids: blocks_to_embed.iter().map(|b| b.chapter_id).collect(),
        block_ids: blocks_to_embed.iter().map(|b| b.block_id).collect(),
        directive_type_ids: blocks_to_embed
            .iter()
            .map(|b| b.directive_type_id)
            .collect(),
        chunk_texts: texts,
        embeddings,
        metadata,
        generation_ids: vec![generation.id; count],
    };

    let inserted = insert_guide_content_embeddings(pool, insert_args)
        .await
        .map_err(|e| format!("Failed to insert embeddings: {}", e))?;

    // The tag and locale loops below zip `inserted[i]` against
    // `blocks_to_embed[i]`, which is valid only because the insert returns
    // `ORDER BY ne.id` (uuidv7, generated in insert order). Windowing raises the
    // row count of a single insert, so verify the correspondence rather than
    // assume it — a silent reorder would attach tags to the wrong rows, and
    // since every search requires a tag AND locale join, the mis-tagged rows
    // would simply vanish from results with no error anywhere.
    if let Some(i) = (0..count).find(|&i| inserted[i].block_id != blocks_to_embed[i].block_id) {
        return Err(format!(
            "Embedding insert returned rows out of order at index {}: expected \
             block {}, got {}. Tag/locale association would be misattributed.",
            i, blocks_to_embed[i].block_id, inserted[i].block_id,
        ));
    }

    // 10. Insert tag associations
    let mut tag_emb_ids: Vec<Uuid> = Vec::new();
    let mut tag_ids: Vec<i16> = Vec::new();

    for (i, block) in blocks_to_embed.iter().enumerate() {
        let emb_id = inserted[i].id;
        if block.tags.is_empty() {
            // Untagged blocks get the "universal" tag if it exists
            if let Some(&universal_id) = tag_lookup.get("universal") {
                tag_emb_ids.push(emb_id);
                tag_ids.push(universal_id);
            }
        } else {
            for tag_key in &block.tags {
                if let Some(&tag_id) = tag_lookup.get(tag_key) {
                    tag_emb_ids.push(emb_id);
                    tag_ids.push(tag_id);
                } else {
                    log::warn!(
                        "Unknown tag '{}' on block {}; skipping tag association",
                        tag_key,
                        block.block_id
                    );
                }
            }
        }
    }

    if !tag_emb_ids.is_empty() {
        insert_guide_embedding_tags(
            pool,
            InsertGuideEmbeddingTagArgs {
                embedding_ids: tag_emb_ids.clone(),
                tag_ids,
            },
        )
        .await
        .map_err(|e| format!("Failed to insert embedding tags: {}", e))?;
    }

    // 11. Insert locale associations
    let mut locale_emb_ids: Vec<Uuid> = Vec::new();
    let mut locale_ids: Vec<i16> = Vec::new();

    for (i, block) in blocks_to_embed.iter().enumerate() {
        let emb_id = inserted[i].id;
        if block.is_locale_neutral {
            // Locale-neutral blocks (inline content) get ALL locales in the guide
            for &loc_id in &all_locale_ids {
                locale_emb_ids.push(emb_id);
                locale_ids.push(loc_id);
            }
        } else {
            // Locale-specific: associate with "en" (or the locale used for embedding)
            if let Some(&en_id) = locale_tag_to_id.get("en") {
                locale_emb_ids.push(emb_id);
                locale_ids.push(en_id);
            } else if let Some((_, &first_id)) = locale_tag_to_id.iter().next() {
                locale_emb_ids.push(emb_id);
                locale_ids.push(first_id);
            }
        }
    }

    if !locale_emb_ids.is_empty() {
        insert_guide_embedding_locales(
            pool,
            InsertGuideEmbeddingLocaleArgs {
                embedding_ids: locale_emb_ids.clone(),
                locale_ids,
            },
        )
        .await
        .map_err(|e| format!("Failed to insert embedding locales: {}", e))?;
    }

    log::info!(
        "Generated {} embeddings for guide {} ({} tags, {} locale associations)",
        inserted.len(),
        guide_id,
        tag_emb_ids.len(),
        locale_emb_ids.len()
    );

    Ok(())
}

/// Extract embeddable blocks from a guide, resolving Fluent bodies where needed.
///
/// Inline content blocks (nushell, ref) use their `content` field directly.
/// Fluent-backed blocks (prose, stage, step, block) use the resolved body
/// from `body_map`, preferring the "en" locale.
///
/// **Emits one entry per WINDOW, not per block.** A block longer than
/// [`EMBED_WINDOW_CHARS`] would otherwise be truncated by the conduit and lose
/// its tail from the index entirely, so it is split into several overlapping
/// windows here (see [`crate::service::text_window`]). Blocks that fit — nearly
/// all of them — still produce exactly one entry whose text is byte-identical to
/// the block.
///
/// Fanning out HERE rather than later is deliberate: everything downstream maps
/// positionally over the returned vec (texts, metadata, the column-wise insert
/// args, and the `inserted[i]` zips that attach tags and locales), so widening
/// the vec before any of that keeps every one of those correspondences valid
/// with no further change.
fn extract_embeddable_blocks(
    guide: &Guide,
    body_map: &HashMap<(Uuid, String), String>,
    fluent_bodies: &[dapple_data::query::fluent_messages::FluentBodyRow],
    directive_types: &HashMap<String, i16>,
) -> Result<Vec<EmbeddableBlock>, String> {
    let mut blocks_to_embed: Vec<EmbeddableBlock> = Vec::new();

    for chapter in &guide.chapters {
        let mut block_position: usize = 0;
        for block in &chapter.blocks {
            if !EMBEDDABLE_DIRECTIVE_TYPES.contains(&block.directive_type.as_str()) {
                block_position += 1;
                continue;
            }

            // Resolve text: inline content takes precedence, then Fluent body
            let text = resolve_block_text(block, body_map, fluent_bodies);

            if text.trim().is_empty() {
                block_position += 1;
                continue;
            }

            let directive_type_id = *directive_types
                .get(&block.directive_type)
                .ok_or_else(|| format!("Unknown directive type: {}", block.directive_type))?;

            // Inline content = locale-neutral
            let is_locale_neutral = block.content.is_some();

            let windows = window_text(&text);
            let chunk_count = windows.len();
            for (chunk_ordinal, window) in windows.into_iter().enumerate() {
                blocks_to_embed.push(EmbeddableBlock {
                    guide_id: guide.id,
                    chapter_id: chapter.id,
                    block_id: block.id,
                    directive_type_id,
                    directive_type_key: block.directive_type.clone(),
                    text: window.text,
                    tags: block.tags.clone(),
                    chapter_title: chapter.title.clone(),
                    block_name: block.name.clone(),
                    position: block_position,
                    is_locale_neutral,
                    chunk_ordinal,
                    chunk_count,
                });
            }

            // Outside the window loop on purpose: `position` identifies the
            // BLOCK within its chapter, so every window of a block shares it.
            // Incrementing per window would shift `metadata.position` for every
            // block after the first over-cap one.
            block_position += 1;
        }
    }

    Ok(blocks_to_embed)
}

/// Resolve the embeddable text for a single block.
///
/// Priority:
/// 1. Inline `content` (nushell, ref blocks)
/// 2. Fluent message body (prefer "en", fall back to first available)
/// 3. Empty string (will be filtered out by caller)
fn resolve_block_text(
    block: &GuideBlock,
    body_map: &HashMap<(Uuid, String), String>,
    fluent_bodies: &[dapple_data::query::fluent_messages::FluentBodyRow],
) -> String {
    if let Some(ref content) = block.content {
        return content.clone();
    }

    if let Some(fm_id) = block.fluent_message_id {
        // Try "en" first, then fall back to first available locale
        if let Some(body) = body_map.get(&(fm_id, "en".to_string())) {
            return body.clone();
        }
        if let Some(row) = fluent_bodies.iter().find(|r| r.fluent_message_id == fm_id) {
            return row.body.clone();
        }
    }

    String::new()
}

/// Build JSONB metadata for an embedding row.
///
/// Contains contextual information that enriches search results without
/// requiring additional joins at query time.
fn build_embedding_metadata(block: &EmbeddableBlock, guide: &Guide) -> serde_json::Value {
    serde_json::json!({
        "guide_slug": guide.slug,
        "guide_title": guide.title,
        "chapter_title": block.chapter_title,
        "chapter_id": block.chapter_id.to_string(),
        "block_id": block.block_id.to_string(),
        "block_name": block.block_name,
        "directive_type": block.directive_type_key,
        "position": block.position,
        // Which window of its block this row is. `chunk_count` is 1 for any
        // block that fit in a single window. Search DTOs pass `metadata`
        // straight through, so this is also what lets a client tell two windows
        // of one block apart (they share `block_id`) with no DTO change.
        "chunk_ordinal": block.chunk_ordinal,
        "chunk_count": block.chunk_count,
        // Deliberately per-WINDOW, not per-block: these describe the
        // `chunk_text` actually stored and retrieved on this row.
        "has_emphasis": block.text.contains("**") || block.text.contains("__"),
        "has_annotations": !block.tags.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::text_window::EMBED_WINDOW_CHARS;
    use dapple_data::model::guides::GuideChapter;
    use time::OffsetDateTime;

    fn test_guide() -> Guide {
        Guide {
            id: Uuid::nil(),
            path_id: Uuid::nil(),
            title: "Test Guide".to_string(),
            slug: "test-guide".to_string(),
            content_hash: dapple_data::ContentHash::default(),
            deprecation_level: None,
            description: None,
            author: None,
            classification: None,
            source_path: None,
            chapters: Vec::new(),
            card: None,
            assets: Vec::new(),
            playbooks: Vec::new(),
            created_at: OffsetDateTime::UNIX_EPOCH.into(),
        }
    }

    fn test_block(directive_type: &str, content: Option<&str>) -> GuideBlock {
        GuideBlock {
            id: Uuid::now_v7(),
            directive_type: directive_type.to_string(),
            name: None,
            content: content.map(str::to_string),
            attributes: serde_json::json!({}),
            fluent_message_id: None,
            tags: vec!["agent".to_string()],
            created_at: OffsetDateTime::UNIX_EPOCH.into(),
        }
    }

    /// A guide with one chapter holding `blocks`. Inline `content` means
    /// `resolve_block_text` returns it directly, so no Fluent fixtures needed.
    fn test_guide_with_blocks(blocks: Vec<GuideBlock>) -> Guide {
        let mut guide = test_guide();
        guide.chapters = vec![GuideChapter {
            id: Uuid::now_v7(),
            parent_chapter_id: None,
            title: "Chapter 1".to_string(),
            subtitle: None,
            source_file: "ch1.md".to_string(),
            blocks,
            created_at: OffsetDateTime::UNIX_EPOCH.into(),
        }];
        guide
    }

    fn extract(guide: &Guide) -> Vec<EmbeddableBlock> {
        let directive_types =
            HashMap::from([("prose".to_string(), 1i16), ("nushell".to_string(), 2i16)]);
        extract_embeddable_blocks(guide, &HashMap::new(), &[], &directive_types).unwrap()
    }

    // -- windowing fan-out --------------------------------------------------

    /// The zero-blast-radius guarantee: a block that fits produces exactly the
    /// same single row it did before windowing existed.
    #[test]
    fn under_cap_block_yields_one_byte_identical_entry() {
        let content = "Some guide content about testing.";
        let guide = test_guide_with_blocks(vec![test_block("prose", Some(content))]);

        let entries = extract(&guide);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, content);
        assert_eq!(entries[0].chunk_ordinal, 0);
        assert_eq!(entries[0].chunk_count, 1);
        assert_eq!(entries[0].position, 0);
    }

    /// The bug this whole change exists to fix: today the tail of an over-cap
    /// block is truncated by the conduit and is never retrievable.
    #[test]
    fn over_cap_block_fans_out_and_keeps_its_tail() {
        const TAIL: &str = "TAIL-SENTINEL-4c81";
        let content = format!("{}{TAIL}", "The fund reported losses. ".repeat(2_000));
        assert!(content.chars().count() > EMBED_WINDOW_CHARS * 2);

        let block = test_block("prose", Some(&content));
        let block_id = block.id;
        let guide = test_guide_with_blocks(vec![block]);

        let entries = extract(&guide);

        assert!(entries.len() > 1, "expected fan-out, got {}", entries.len());
        assert!(
            entries.iter().any(|e| e.text.contains(TAIL)),
            "the block's tail must survive — that is the content lost today"
        );
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(e.block_id, block_id);
            assert_eq!(e.chunk_ordinal, i, "ordinals must be 0..n in order");
            assert_eq!(e.chunk_count, entries.len());
            // Everything block-scoped is shared across a block's windows.
            assert_eq!(e.position, 0);
            assert_eq!(e.directive_type_id, 1);
            assert_eq!(e.tags, vec!["agent".to_string()]);
            assert!(e.is_locale_neutral);
        }
    }

    /// `position` identifies the BLOCK within its chapter. Incrementing it
    /// inside the window loop is the natural mistake, and it would silently
    /// shift `metadata.position` for every block after an over-cap one.
    #[test]
    fn position_counts_blocks_not_windows() {
        let long = "The fund reported losses. ".repeat(2_000);
        let guide = test_guide_with_blocks(vec![
            test_block("prose", Some(&long)),
            test_block("prose", Some("A short second block.")),
        ]);

        let entries = extract(&guide);
        let short: Vec<_> = entries.iter().filter(|e| e.chunk_count == 1).collect();

        assert_eq!(short.len(), 1);
        assert_eq!(
            short[0].position, 1,
            "the second BLOCK is at position 1 regardless of how many windows the first produced"
        );
    }

    /// The directive filter and the empty-text check must stay ahead of the
    /// windower, and must still advance `position`.
    #[test]
    fn non_embeddable_and_empty_blocks_are_skipped_before_windowing() {
        let guide = test_guide_with_blocks(vec![
            test_block("nushell", Some("ls | where size > 1kb")),
            test_block("prose", Some("   \n\t  ")),
            test_block("prose", Some("Real content.")),
        ]);

        let entries = extract(&guide);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Real content.");
        assert_eq!(entries[0].position, 2);
    }

    // -- metadata -----------------------------------------------------------

    #[test]
    fn test_build_metadata_prose_block() {
        let block = EmbeddableBlock {
            guide_id: Uuid::nil(),
            chapter_id: Uuid::nil(),
            block_id: Uuid::nil(),
            directive_type_id: 1,
            directive_type_key: "prose".to_string(),
            text: "Some guide content about testing.".to_string(),
            tags: vec!["agent".to_string()],
            chapter_title: "Chapter 1".to_string(),
            block_name: Some("intro-block".to_string()),
            position: 0,
            is_locale_neutral: false,
            chunk_ordinal: 0,
            chunk_count: 1,
        };

        let guide = test_guide();
        let metadata = build_embedding_metadata(&block, &guide);

        assert_eq!(metadata["guide_slug"], "test-guide");
        assert_eq!(metadata["guide_title"], "Test Guide");
        assert_eq!(metadata["chapter_title"], "Chapter 1");
        assert_eq!(metadata["block_name"], "intro-block");
        assert_eq!(metadata["directive_type"], "prose");
        assert_eq!(metadata["position"], 0);
        assert_eq!(metadata["chunk_ordinal"], 0);
        assert_eq!(metadata["chunk_count"], 1);
        assert_eq!(metadata["has_emphasis"], false);
        assert_eq!(metadata["has_annotations"], true);
    }

    #[test]
    fn test_build_metadata_emphasis_detection() {
        let block = EmbeddableBlock {
            guide_id: Uuid::nil(),
            chapter_id: Uuid::nil(),
            block_id: Uuid::nil(),
            directive_type_id: 1,
            directive_type_key: "prose".to_string(),
            text: "This has **bold** text and __italic__ markers.".to_string(),
            tags: Vec::new(),
            chapter_title: "Chapter 1".to_string(),
            block_name: None,
            position: 2,
            is_locale_neutral: true,
            chunk_ordinal: 0,
            chunk_count: 1,
        };

        let guide = test_guide();
        let metadata = build_embedding_metadata(&block, &guide);

        assert_eq!(metadata["has_emphasis"], true);
        assert_eq!(metadata["has_annotations"], false);
    }

    /// The window ordinal must reach `metadata`, since that jsonb is passed
    /// straight through by the search DTOs and is therefore the only thing that
    /// distinguishes two windows of one block on the wire.
    #[test]
    fn build_metadata_carries_the_window_ordinal() {
        let block = EmbeddableBlock {
            guide_id: Uuid::nil(),
            chapter_id: Uuid::nil(),
            block_id: Uuid::nil(),
            directive_type_id: 1,
            directive_type_key: "prose".to_string(),
            text: "Third window of four.".to_string(),
            tags: Vec::new(),
            chapter_title: "Chapter 1".to_string(),
            block_name: None,
            position: 5,
            is_locale_neutral: true,
            chunk_ordinal: 2,
            chunk_count: 4,
        };

        let metadata = build_embedding_metadata(&block, &test_guide());

        assert_eq!(metadata["chunk_ordinal"], 2);
        assert_eq!(metadata["chunk_count"], 4);
        assert_eq!(metadata["position"], 5);
    }
}
