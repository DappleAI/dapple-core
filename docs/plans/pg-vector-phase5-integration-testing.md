# pgvector Phase 5: Comprehensive Integration Testing for Phases 2-4

**Status:** Draft
**Created:** 2026-02-24
**Last Updated:** 2026-02-24
**Parent Plan:** `docs/plans/pg-vector-searching.md`
**Dependencies:** Phase 2 (embedding pipeline), Phase 3 (search endpoint), Phase 4 (agent RAG pipeline)

---

## Executive Summary

Phases 2-4 of the pgvector search initiative deliver three interconnected capabilities: embedding generation after guide ingest (Phase 2), a semantic search endpoint (Phase 3), and RAG context injection for agent prompts (Phase 4). None of these phases have integration tests. This plan specifies the exact test infrastructure additions, trial functions, consumer modules, and assertions needed to provide comprehensive coverage.

The testing strategy splits into two tiers based on environmental availability:

1. **Tier 1 (Always-run)**: Tests that exercise the code paths without requiring Conduit or the embedding service. These verify graceful degradation, schema validation, database-level operations (insert/delete/query on embedding tables), and the `ByClassification` guide query variants. These tests run in every CI and local test invocation.

2. **Tier 2 (Conduit-required)**: Tests that require a running Conduit with the embedding service (`DAPPLE_CONDUIT_URL` set and Conduit responding to `/embed`). These cover the full round-trip: guide ingest triggers embedding generation, search finds the embedded content, and RAG context enriches prompt execution. These tests skip gracefully when Conduit is unavailable.

All tests follow the existing lifecycle test pattern: `libtest_mimic` trials in `user_lifecycle.rs`, `ScenarioContext` for state sharing, and consumer modules in `tests/consumer/`.

---

## 1. Context: Existing Test Infrastructure

### 1a. Test Architecture

Tests run as a single binary (`user-lifecycle`) using `libtest_mimic`. Trials are ordered topologically and execute sequentially (`test_threads = 1`). State flows between trials via `Arc<Mutex<ScenarioContext>>`.

**Key components:**
- `spawn_app()` starts an ephemeral Axum server with a real database connection
- `ScenarioContext` holds batches of created DTOs keyed by name
- `async_trial()` wraps async closures for `libtest_mimic`
- `Endpoint` trait standardizes POST/GET/DELETE operations per entity
- `test_auth_header()` generates valid JWT tokens for the test user

### 1b. Environmental Dependencies

| Variable | Required For | Fallback |
|----------|-------------|----------|
| `DAPPLE_PROMPT_DATABASE_URL` | All tests | None (exit) |
| `DAPPLE_PROMPT_API_SIGNING_KEY` | Auth headers | Warn |
| `DAPPLE_REDIS_HOST` | Cache verification | Noop integrations |
| `DAPPLE_CONDUIT_URL` | Embedding + search | Tier 2 tests skip |

### 1c. Guide Test Fixtures

The existing `consumer/guides.rs` creates guides with:
- 3 chapters (Introduction, Getting Started, Setup Details -- the third nested under the second)
- 2 blocks per chapter, both `prose` directive type
- Tags: first block has `["universal"]`, second has `["agent", "product"]`
- All blocks have inline `content: Some(...)` (no Fluent messages)
- Classification: `"workflow"`

This fixture is sufficient for embedding generation testing because:
- `prose` is in `EMBEDDABLE_DIRECTIVE_TYPES`
- Inline content resolves directly (no Fluent message lookup needed)
- Tags provide audience filtering test coverage
- The `"workflow"` classification supports `ByClassification` testing

---

## 2. Phase 2 Tests: Embedding Pipeline

### 2a. Overview

Phase 2's embedding pipeline runs asynchronously via `tokio::spawn` after guide ingest. The pipeline:
1. Loads the guide with chapters/blocks
2. Resolves block text (inline content or Fluent body)
3. Calls Conduit `/embed` for embedding generation
4. Deletes old embeddings for the guide
5. Inserts new embeddings + tag associations + locale associations

### 2b. Tier 1: Embedding Data Layer Tests

These tests verify the SQL operations work correctly, independent of Conduit.

**Trial: `verify_embedding_insert_delete`**

This trial directly inserts and deletes embeddings using the data layer, bypassing the embedding pipeline. It validates that the SQL files, models, and query functions work correctly.

```rust
// In user_lifecycle.rs
let verify_embedding_insert_delete_trial = async_trial(&rt, "verify_embedding_insert_delete", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            // Get a guide from the scenario context
            let guide = {
                let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                ctx.random_guide_from_named_batch("guides-init")
                    .expect("should have guides")
            };

            // Connect to the database directly
            let pool = dapple_data::DbPool::connect().await
                .expect("db connect");

            // Get a chapter and block from the guide
            let chapter = guide.chapters.first().expect("guide should have chapters");
            let block = chapter.blocks.first().expect("chapter should have blocks");

            // Look up the directive type ID for "prose"
            let directive_types = dapple_data::query::guides::load_directive_type_lookup_pool(&pool)
                .await
                .expect("load directive types");
            let prose_dt_id = *directive_types.get("prose").expect("prose directive type");

            // Insert a test embedding
            let fake_embedding = vec![0.1_f32; 768]; // 768-dim zero vector
            let args = dapple_data::model::guide_content_embeddings::InsertGuideContentEmbeddingArgs {
                guide_ids: vec![guide.id],
                chapter_ids: vec![chapter.id],
                block_ids: vec![block.id],
                directive_type_ids: vec![prose_dt_id],
                chunk_texts: vec!["Test embedding content".to_string()],
                embeddings: vec![fake_embedding],
                metadata: vec![serde_json::json!({"test": true})],
            };

            let inserted = dapple_data::query::guide_content_embeddings::insert_guide_content_embeddings(
                &pool, args,
            )
            .await
            .expect("insert embeddings");

            assert_eq!(inserted.len(), 1, "should insert exactly one embedding");
            let emb_id = inserted[0].id;
            assert_eq!(inserted[0].guide_id, guide.id, "guide_id mismatch");
            assert_eq!(inserted[0].chapter_id, chapter.id, "chapter_id mismatch");
            assert_eq!(inserted[0].block_id, block.id, "block_id mismatch");

            // Insert tag associations
            let tag_lookup = dapple_data::query::guides::load_tag_lookup_pool(&pool)
                .await
                .expect("load tag lookup");
            let universal_tag_id = *tag_lookup.get("universal").expect("universal tag");

            let tag_args = dapple_data::model::guide_content_embeddings::InsertGuideEmbeddingTagArgs {
                embedding_ids: vec![emb_id],
                tag_ids: vec![universal_tag_id],
            };
            let tag_inserted = dapple_data::query::guide_content_embeddings::insert_guide_embedding_tags(
                &pool, tag_args,
            )
            .await
            .expect("insert embedding tags");
            assert_eq!(tag_inserted.len(), 1, "should insert one tag association");

            // Insert locale associations
            let locale_rows = dapple_data::query::fluent_messages::select_fluent_message_locale_ids(
                &pool,
                &["en".to_string()],
            )
            .await
            .expect("select locale ids");

            if !locale_rows.is_empty() {
                let locale_args = dapple_data::model::guide_content_embeddings::InsertGuideEmbeddingLocaleArgs {
                    embedding_ids: vec![emb_id],
                    locale_ids: vec![locale_rows[0].id],
                };
                let locale_inserted = dapple_data::query::guide_content_embeddings::insert_guide_embedding_locales(
                    &pool, locale_args,
                )
                .await
                .expect("insert embedding locales");
                assert_eq!(locale_inserted.len(), 1, "should insert one locale association");
            }

            // Delete embeddings for the guide (cascades to tags + locales)
            let deleted = dapple_data::query::guide_content_embeddings::delete_guide_content_embeddings_by_guide_id(
                &pool, guide.id,
            )
            .await
            .expect("delete embeddings");

            assert!(
                deleted.contains(&emb_id),
                "deleted set should contain our embedding"
            );

            log::info!(
                "verify_embedding_insert_delete: inserted and deleted {} embedding(s) for guide {}",
                inserted.len(),
                guide.id
            );

            Ok(())
        }
    }
});
```

**Dependency level:** After `guides_trial` (Level 0 guides must exist).

### 2c. Tier 2: Full Embedding Pipeline Test

This trial exercises the complete Phase 2 pipeline by ingesting a guide and verifying that embeddings were generated asynchronously.

**Trial: `verify_guide_embeddings_generated`**

```rust
let verify_guide_embeddings_generated_trial = async_trial(&rt, "verify_guide_embeddings_generated", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            // Skip if DAPPLE_CONDUIT_URL is not set
            if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                log::info!("verify_guide_embeddings_generated: skipping (DAPPLE_CONDUIT_URL not set)");
                return Ok(());
            }

            // Create a fresh guide specifically for embedding testing
            let guides_ep = GuideEndpoint {
                base_url: base_url.clone(),
                context: scenario.clone(),
            };
            let created = guides_ep.post_fake_object_set(1..2).await?;
            let guide = &created[0];

            // Register for downstream trials
            {
                let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                ctx.register_guide_batch("guides-with-embeddings", created.clone());
            }

            // Poll for embeddings to appear (async generation via tokio::spawn)
            // The embedding pipeline takes 1-5 seconds depending on Conduit load.
            let pool = dapple_data::DbPool::connect().await.expect("db connect");
            let mut embedding_count = 0i64;

            for attempt in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                let row = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1"
                )
                .bind(guide.id)
                .fetch_one(pool.get_pool())
                .await
                .expect("count embeddings");

                embedding_count = row;
                if embedding_count > 0 {
                    log::info!(
                        "verify_guide_embeddings_generated: found {} embeddings after {} polls",
                        embedding_count,
                        attempt + 1
                    );
                    break;
                }
            }

            assert!(
                embedding_count > 0,
                "Guide {} should have embeddings generated within 15s (found {}). \
                 Is Conduit's /embed endpoint running and DAPPLE_CONDUIT_URL set?",
                guide.id,
                embedding_count
            );

            // Verify tag associations exist
            let tag_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM rev.guide_embedding_tags et \
                 JOIN rev.guide_content_embeddings e ON e.id = et.embedding_id \
                 WHERE e.guide_id = $1"
            )
            .bind(guide.id)
            .fetch_one(pool.get_pool())
            .await
            .expect("count embedding tags");

            assert!(
                tag_count > 0,
                "Guide {} should have embedding tag associations (found {})",
                guide.id,
                tag_count
            );

            // Verify locale associations exist
            let locale_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM rev.guide_embedding_locales el \
                 JOIN rev.guide_content_embeddings e ON e.id = el.embedding_id \
                 WHERE e.guide_id = $1"
            )
            .bind(guide.id)
            .fetch_one(pool.get_pool())
            .await
            .expect("count embedding locales");

            assert!(
                locale_count > 0,
                "Guide {} should have embedding locale associations (found {})",
                guide.id,
                locale_count
            );

            // Verify embedding count matches embeddable block count
            // The test guide has 3 chapters x 2 prose blocks = 6 embeddable blocks
            let expected_blocks = guide.chapters.iter()
                .flat_map(|ch| ch.blocks.iter())
                .filter(|b| b.directive_type == "prose" && b.content.is_some())
                .filter(|b| !b.content.as_ref().unwrap().trim().is_empty())
                .count() as i64;

            assert_eq!(
                embedding_count, expected_blocks,
                "Embedding count ({}) should match embeddable block count ({})",
                embedding_count, expected_blocks
            );

            log::info!(
                "verify_guide_embeddings_generated: guide {} has {} embeddings, {} tags, {} locales",
                guide.id, embedding_count, tag_count, locale_count
            );

            Ok(())
        }
    }
});
```

### 2d. Tier 2: Re-ingest Replaces Embeddings

This trial verifies that patching (re-ingesting) a guide replaces old embeddings with new ones.

**Trial: `verify_guide_reingest_replaces_embeddings`**

```rust
let verify_guide_reingest_replaces_embeddings_trial = async_trial(&rt, "verify_guide_reingest_replaces_embeddings", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                log::info!("verify_guide_reingest_replaces_embeddings: skipping (DAPPLE_CONDUIT_URL not set)");
                return Ok(());
            }

            // Get the guide created for embedding testing
            let guide = {
                let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                match ctx.guide_named_batches.get("guides-with-embeddings") {
                    Some(handle) => ctx.guide_batches[handle.index][0].clone(),
                    None => {
                        log::info!("verify_guide_reingest_replaces_embeddings: skipping (no embedded guide)");
                        return Ok(());
                    }
                }
            };

            let pool = dapple_data::DbPool::connect().await.expect("db connect");

            // Record the current embedding IDs
            let old_embedding_ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT id FROM rev.guide_content_embeddings WHERE guide_id = $1 ORDER BY id"
            )
            .bind(guide.id)
            .fetch_all(pool.get_pool())
            .await
            .expect("fetch old embedding ids");

            assert!(
                !old_embedding_ids.is_empty(),
                "Guide should have existing embeddings before re-ingest"
            );

            // Re-ingest the guide (creates a new version, which triggers new embedding generation)
            let client = reqwest::Client::new();
            let guides_ep = GuideEndpoint {
                base_url: base_url.clone(),
                context: scenario.clone(),
            };
            let new_guide_schema = guides_ep.fake_create_schema();

            // PATCH the guide via the same path_id
            let patch_body = serde_json::json!({
                "objs": [{
                    "path_id": guide.path_id,
                    "title": new_guide_schema.title,
                    "description": Some("Updated guide for embedding re-test"),
                    "author": new_guide_schema.author,
                    "classification": new_guide_schema.classification,
                    "source_path": new_guide_schema.source_path,
                    "chapters": new_guide_schema.chapters,
                }]
            });

            let resp = client
                .patch(base_url.join("api/guides/").unwrap())
                .header("authorization", test_auth_header())
                .json(&patch_body)
                .send()
                .await?;

            let status = resp.status();
            let body_text = resp.text().await?;
            assert!(
                status.is_success(),
                "PATCH guide failed: {} {}",
                status,
                body_text
            );

            let body: serde_json::Value = serde_json::from_str(&body_text)?;
            let new_guide_id: Uuid = body["objs"][0]["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("new guide id from PATCH");

            // Poll for new embeddings to appear
            let mut new_embedding_count = 0i64;
            for attempt in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                new_embedding_count = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1"
                )
                .bind(new_guide_id)
                .fetch_one(pool.get_pool())
                .await
                .expect("count new embeddings");

                if new_embedding_count > 0 {
                    log::info!(
                        "verify_guide_reingest_replaces_embeddings: new guide {} has {} embeddings after {} polls",
                        new_guide_id, new_embedding_count, attempt + 1
                    );
                    break;
                }
            }

            assert!(
                new_embedding_count > 0,
                "New guide version {} should have embeddings within 15s",
                new_guide_id
            );

            // Verify old guide's embeddings still exist (old version is not deleted,
            // only the new version gets its own embeddings)
            // Note: The delete-then-insert pattern operates on the NEW guide_id,
            // not the old one, because each version is a separate row in rev.guides.
            let old_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1"
            )
            .bind(guide.id)
            .fetch_one(pool.get_pool())
            .await
            .expect("count old embeddings");

            log::info!(
                "verify_guide_reingest_replaces_embeddings: old guide {} has {} embeddings, new guide {} has {}",
                guide.id, old_count, new_guide_id, new_embedding_count
            );

            Ok(())
        }
    }
});
```

### 2e. Tier 1: Graceful Degradation Without Conduit

This trial verifies that guide ingest succeeds even when `DAPPLE_CONDUIT_URL` is not set (the embedding pipeline skips silently).

```rust
// Note: This is already implicitly tested by the existing `guides_trial` which runs
// without requiring DAPPLE_CONDUIT_URL. The trial creates guides and verifies they
// are stored correctly regardless of whether embeddings were generated.
// No separate trial is needed -- just document this coverage.
```

The existing `create_guides` trial already validates this because `AppState.conduit_url` is `None` when `DAPPLE_CONDUIT_URL` is unset, and the `tokio::spawn` in `guide_ingest.rs` silently returns from `generate_embeddings_inner` with an info log. The guide is stored successfully.

---

## 3. Phase 3 Tests: Search Endpoint

### 3a. New Consumer Module

**File:** `crates/dapple-prompt-api/tests/consumer/guide_search.rs`

```rust
//! Consumer for the /api/guide-search/ endpoint.
//!
//! Unlike standard CRUD endpoints, the search endpoint does not implement
//! the Endpoint trait -- it has a unique query-only interface with no POST.

use crate::consumer::error::ConsumerError;
use crate::consumer::test_auth_header;
use dapple_data::model::guide_content_embeddings::GuideSearchResult;
use dapple_prompt_api::IndexResponse;
use reqwest::{Client, Url};
use uuid::Uuid;

pub struct GuideSearchEndpoint {
    pub base_url: Url,
}

impl GuideSearchEndpoint {
    /// Perform a semantic search via GET /api/guide-search/.
    ///
    /// Returns the parsed search results, or an error describing what went wrong.
    pub async fn search(
        &self,
        q: &str,
        tags: Option<&[&str]>,
        locale: Option<&str>,
        limit: Option<i32>,
        guide_ids: Option<&[Uuid]>,
        threshold: Option<f64>,
    ) -> Result<Vec<GuideSearchResult>, ConsumerError> {
        let mut url = self
            .base_url
            .join(&format!("api/guide-search/?q={}", urlencoding::encode(q)))
            .map_err(|e| ConsumerError::Custom(format!("URL parse: {}", e)))?;

        {
            let mut pairs = url.query_pairs_mut();
            if let Some(tags) = tags {
                for tag in tags {
                    pairs.append_pair("tags", tag);
                }
            }
            if let Some(locale) = locale {
                pairs.append_pair("locale", locale);
            }
            if let Some(limit) = limit {
                pairs.append_pair("limit", &limit.to_string());
            }
            if let Some(guide_ids) = guide_ids {
                for (i, gid) in guide_ids.iter().enumerate() {
                    pairs.append_pair(&format!("guide_ids[{}]", i), &gid.to_string());
                }
            }
            if let Some(threshold) = threshold {
                pairs.append_pair("threshold", &threshold.to_string());
            }
        }

        let resp = Client::new()
            .get(url.clone())
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            return Err(ConsumerError::Custom(format!(
                "GET {} failed with status {}: {}",
                url, status, text
            )));
        }

        let body: IndexResponse<GuideSearchResult> = serde_json::from_str(&text)?;
        assert_eq!(body.status, "success", "search response status");
        Ok(body.objs)
    }

    /// Verify that the search endpoint returns a 400 for an empty query.
    pub async fn search_empty_query(&self) -> Result<u16, ConsumerError> {
        let url = self
            .base_url
            .join("api/guide-search/?q=")
            .map_err(|e| ConsumerError::Custom(format!("URL parse: {}", e)))?;

        let resp = Client::new()
            .get(url)
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        Ok(resp.status().as_u16())
    }

    /// Verify the search endpoint returns 503 when conduit is unavailable.
    /// This is only useful when DAPPLE_CONDUIT_URL is explicitly set to a
    /// non-responding URL. In practice this test is covered by the Tier 1
    /// test that checks behavior when conduit_url is None.
    pub async fn search_status(&self, q: &str) -> Result<u16, ConsumerError> {
        let url = self
            .base_url
            .join(&format!("api/guide-search/?q={}", urlencoding::encode(q)))
            .map_err(|e| ConsumerError::Custom(format!("URL parse: {}", e)))?;

        let resp = Client::new()
            .get(url)
            .header("Authorization", test_auth_header())
            .send()
            .await?;

        Ok(resp.status().as_u16())
    }
}
```

### 3b. Module Registration

**File:** `crates/dapple-prompt-api/tests/consumer/mod.rs`

Add to the module declarations:

```rust
pub(crate) mod guide_search;
```

And in `user_lifecycle.rs`, add the import:

```rust
use consumer::guide_search::GuideSearchEndpoint;
```

### 3c. Tier 1: Search Endpoint Validation (No Conduit Required)

**Trial: `verify_search_endpoint_validation`**

This trial tests that the search endpoint handles invalid inputs correctly, regardless of whether Conduit is available.

```rust
let verify_search_endpoint_validation_trial = async_trial(&rt, "verify_search_endpoint_validation", {
    let base_url = base_url.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            let search_ep = GuideSearchEndpoint {
                base_url: base_url.clone(),
            };

            // Empty query should return 400
            let status = search_ep.search_empty_query().await?;
            assert_eq!(
                status, 400,
                "Empty search query should return 400 Bad Request"
            );

            // If conduit is not configured, search should return 503
            if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                let status = search_ep.search_status("test query").await?;
                assert_eq!(
                    status, 503,
                    "Search without DAPPLE_CONDUIT_URL should return 503"
                );
            }

            log::info!("verify_search_endpoint_validation: passed");
            Ok(())
        }
    }
});
```

**Dependency level:** Level 0 (no dependencies, runs immediately after server starts).

### 3d. Tier 1: ByClassification Query Variant

**Trial: `verify_guides_by_classification`**

```rust
let verify_guides_by_classification_trial = async_trial(&rt, "verify_guides_by_classification", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            // Get guides created earlier (they all have classification = "workflow")
            let expected_guide_ids: Vec<Uuid> = {
                let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                let handle = ctx.guide_named_batches.get("guides-init").unwrap();
                ctx.guide_batches[handle.index]
                    .iter()
                    .map(|g| g.id)
                    .collect()
            };

            // Query by classification = "workflow"
            let guides_ep = GuideEndpoint {
                base_url: base_url.clone(),
                context: scenario.clone(),
            };
            let found = guides_ep
                .get_object_set(GetGuideSetSchema::ByClassification {
                    classification: "workflow".to_string(),
                    scope: None,
                    environment: None,
                    options: Some(dapple_data::FilterOptions {
                        limit: Some(100),
                        page: Some(1),
                    }),
                })
                .await?;

            // All our test guides should appear in the results
            let found_ids: Vec<Uuid> = found.iter().map(|g| g.id).collect();
            for expected_id in &expected_guide_ids {
                assert!(
                    found_ids.contains(expected_id),
                    "Guide {} should be in ByClassification('workflow') results",
                    expected_id,
                );
            }

            // Query by a classification that none of our guides have
            let empty_results = guides_ep
                .get_object_set(GetGuideSetSchema::ByClassification {
                    classification: "nonexistent-classification".to_string(),
                    scope: None,
                    environment: None,
                    options: Some(dapple_data::FilterOptions {
                        limit: Some(100),
                        page: Some(1),
                    }),
                })
                .await?;

            assert!(
                empty_results.is_empty(),
                "ByClassification with nonexistent classification should return empty, got {}",
                empty_results.len()
            );

            log::info!(
                "verify_guides_by_classification: found {}/{} workflow guides, 0 nonexistent",
                expected_guide_ids.len(),
                found.len()
            );

            Ok(())
        }
    }
});
```

**Dependency level:** After `guides_trial` (Level 1, alongside `verify_guides_by_slugs`).

### 3e. Tier 2: Full Search Round-Trip

**Trial: `verify_guide_search_round_trip`**

This is the most important search test: ingest a guide, wait for embeddings, then search for content from that guide.

```rust
let verify_guide_search_round_trip_trial = async_trial(&rt, "verify_guide_search_round_trip", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                log::info!("verify_guide_search_round_trip: skipping (DAPPLE_CONDUIT_URL not set)");
                return Ok(());
            }

            // Get the guide that was created with embeddings
            let guide = {
                let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                match ctx.guide_named_batches.get("guides-with-embeddings") {
                    Some(handle) => ctx.guide_batches[handle.index][0].clone(),
                    None => {
                        log::info!("verify_guide_search_round_trip: skipping (no embedded guide)");
                        return Ok(());
                    }
                }
            };

            // Verify embeddings exist before searching
            let pool = dapple_data::DbPool::connect().await.expect("db connect");
            let emb_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1"
            )
            .bind(guide.id)
            .fetch_one(pool.get_pool())
            .await
            .expect("count embeddings");

            if emb_count == 0 {
                log::warn!(
                    "verify_guide_search_round_trip: guide {} has no embeddings, skipping search test",
                    guide.id
                );
                return Ok(());
            }

            let search_ep = GuideSearchEndpoint {
                base_url: base_url.clone(),
            };

            // Extract a text snippet from the guide's first block to use as search query
            let search_text = guide
                .chapters
                .first()
                .and_then(|ch| ch.blocks.first())
                .and_then(|b| b.content.clone())
                .unwrap_or_else(|| "guide content".to_string());

            // Use the first few words as the search query
            let query: String = search_text
                .split_whitespace()
                .take(5)
                .collect::<Vec<_>>()
                .join(" ");

            // 1. Basic search with default parameters
            let results = search_ep
                .search(&query, None, None, Some(10), None, None)
                .await?;

            log::info!(
                "verify_guide_search_round_trip: search for '{}' returned {} results",
                query,
                results.len()
            );

            // Results should exist (we just searched for content we know is embedded)
            assert!(
                !results.is_empty(),
                "Search for '{}' should return results (guide {} has {} embeddings)",
                query,
                guide.id,
                emb_count
            );

            // All results should have positive similarity scores
            for r in &results {
                assert!(
                    r.similarity > 0.0,
                    "Search result similarity should be > 0, got {}",
                    r.similarity
                );
            }

            // Results should be ordered by similarity (descending)
            for window in results.windows(2) {
                assert!(
                    window[0].similarity >= window[1].similarity,
                    "Results should be ordered by similarity desc: {} < {}",
                    window[0].similarity,
                    window[1].similarity
                );
            }

            // 2. Search scoped to the specific guide
            let scoped_results = search_ep
                .search(
                    &query,
                    Some(&["universal"]),
                    Some("en"),
                    Some(10),
                    Some(&[guide.id]),
                    None,
                )
                .await?;

            // All scoped results should belong to our guide
            for r in &scoped_results {
                assert_eq!(
                    r.guide_id, guide.id,
                    "Scoped search result guide_id should match filter"
                );
            }

            // 3. Search with high threshold should return fewer (or no) results
            let threshold_results = search_ep
                .search(&query, None, None, Some(10), None, Some(0.99))
                .await?;

            assert!(
                threshold_results.len() <= results.len(),
                "High threshold should return fewer results: {} > {}",
                threshold_results.len(),
                results.len()
            );

            // All threshold results should meet the threshold
            for r in &threshold_results {
                assert!(
                    r.similarity >= 0.99,
                    "Result with threshold 0.99 has similarity {}, expected >= 0.99",
                    r.similarity
                );
            }

            // 4. Search with limit = 1 should return at most 1 result
            let limited_results = search_ep
                .search(&query, None, None, Some(1), None, None)
                .await?;

            assert!(
                limited_results.len() <= 1,
                "Search with limit=1 should return at most 1 result, got {}",
                limited_results.len()
            );

            // 5. Verify result structure
            if let Some(first) = results.first() {
                // metadata should contain expected fields
                assert!(
                    first.metadata.get("guide_slug").is_some(),
                    "Result metadata should contain guide_slug"
                );
                assert!(
                    first.metadata.get("guide_title").is_some(),
                    "Result metadata should contain guide_title"
                );
                assert!(
                    first.metadata.get("directive_type").is_some(),
                    "Result metadata should contain directive_type"
                );
                assert!(
                    !first.chunk_text.is_empty(),
                    "Result chunk_text should not be empty"
                );
                assert!(
                    !first.directive_type.is_empty(),
                    "Result directive_type should not be empty"
                );
            }

            log::info!(
                "verify_guide_search_round_trip: all search assertions passed ({} default, {} scoped, {} threshold, {} limited)",
                results.len(),
                scoped_results.len(),
                threshold_results.len(),
                limited_results.len()
            );

            Ok(())
        }
    }
});
```

**Dependency level:** After `verify_guide_embeddings_generated` (embeddings must exist).

---

## 4. Phase 4 Tests: Agent RAG Pipeline

Phase 4 adds RAG context injection to the prompt execution pipeline. Since Phase 4 code (RagService, RagContext, PromptRequest extensions) is not yet implemented at time of writing, the test plan specifies tests that the implementation agent should create alongside the Phase 4 code.

### 4a. Tier 1: RagContext Serialization

These are unit tests placed directly in the Conduit crate, not lifecycle tests.

**File:** `crates/dapple-prompt-conduit/src/service/rag.rs` (add `#[cfg(test)]` block)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rag_context_defaults() {
        let json = r#"{"guide_ids": []}"#;
        let ctx: RagContext = serde_json::from_str(json).unwrap();
        assert!(ctx.audience_tags.is_empty());
        assert_eq!(ctx.locale, "en");
        assert_eq!(ctx.limit, 10);
        assert!(ctx.exclude_block_ids.is_empty());
        assert!(ctx.query_hint.is_none());
    }

    #[test]
    fn test_rag_context_full_deserialization() {
        let json = r#"{
            "guide_ids": ["019504b0-7b6a-7000-8000-000000000001"],
            "audience_tags": ["agent", "universal"],
            "locale": "es",
            "limit": 5,
            "exclude_block_ids": ["019504b0-7b6a-7000-8000-000000000002"],
            "query_hint": "What is authentication?"
        }"#;
        let ctx: RagContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.guide_ids.len(), 1);
        assert_eq!(ctx.audience_tags, vec!["agent", "universal"]);
        assert_eq!(ctx.locale, "es");
        assert_eq!(ctx.limit, 5);
        assert_eq!(ctx.exclude_block_ids.len(), 1);
        assert_eq!(ctx.query_hint.as_deref(), Some("What is authentication?"));
    }

    #[test]
    fn test_format_rag_context_empty() {
        let text = format_rag_context(&[]);
        assert!(text.is_empty());
    }

    #[test]
    fn test_format_rag_context_single_chunk() {
        let result = GuideEmbeddingSearchResult {
            id: Uuid::nil(),
            guide_id: Uuid::nil(),
            chapter_id: Uuid::nil(),
            block_id: Uuid::nil(),
            directive_type_key: "prose".to_string(),
            chunk_text: "This is relevant content.".to_string(),
            metadata: serde_json::json!({
                "guide_title": "My Guide",
                "chapter_title": "Chapter 1"
            }),
            similarity: 0.85,
        };

        let text = format_rag_context(&[result]);
        assert!(text.contains("Reference Context"));
        assert!(text.contains("My Guide"));
        assert!(text.contains("Chapter 1"));
        assert!(text.contains("This is relevant content."));
        assert!(text.contains("85%")); // similarity formatted as percentage
    }
}
```

### 4b. Tier 1: Flow RagContext Builder

Unit tests in the Flow crate for `build_rag_context` and helper functions.

**File:** `crates/dapple-prompt-flow/src/dispatch_runner.rs` (add `#[cfg(test)]` block)

```rust
#[cfg(test)]
mod rag_tests {
    use super::*;

    #[test]
    fn test_extract_guide_ids_from_function_input() {
        let function_input = serde_json::json!({
            "rag_guide_ids": ["019504b0-7b6a-7000-8000-000000000001"]
        });
        let ids = extract_guide_ids(&function_input, &json!({}), &json!({}));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn test_extract_guide_ids_from_stage_params_fallback() {
        let stage_params = serde_json::json!({
            "guide_id": "019504b0-7b6a-7000-8000-000000000001"
        });
        let ids = extract_guide_ids(&json!({}), &json!({}), &stage_params);
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn test_extract_guide_ids_empty() {
        let ids = extract_guide_ids(&json!({}), &json!({}), &json!({}));
        assert!(ids.is_empty());
    }

    #[test]
    fn test_extract_audience_tags_default() {
        let tags = extract_audience_tags(&json!({}), &json!({}));
        assert_eq!(tags, vec!["agent", "universal"]);
    }

    #[test]
    fn test_extract_audience_tags_override() {
        let step_params = serde_json::json!({
            "rag_audience_tags": ["coder", "product"]
        });
        let tags = extract_audience_tags(&json!({}), &step_params);
        assert_eq!(tags, vec!["coder", "product"]);
    }

    #[test]
    fn test_extract_locale_default() {
        let locale = extract_locale(&json!({}), &json!({}));
        assert_eq!(locale, "en");
    }

    #[test]
    fn test_extract_locale_override() {
        let step_params = serde_json::json!({ "rag_locale": "pt-br" });
        let locale = extract_locale(&json!({}), &step_params);
        assert_eq!(locale, "pt-br");
    }

    #[test]
    fn test_build_rag_context_returns_none_without_guide_ids() {
        let dispatch = /* minimal dispatch fixture */;
        let result = build_rag_context(
            &dispatch,
            &dispatch.stages[0],
            &dispatch.stages[0].steps[0],
            &task_entity,
            &serde_json::Map::new(),
        );
        assert!(result.is_none());
    }
}
```

### 4c. Tier 2: Backward Compatibility (No RagContext)

**Trial: `verify_prompt_without_rag_context`**

This trial verifies that prompt execution works normally when `rag_context` is absent from the request (backward compatibility with pre-Phase-4 callers).

```rust
let verify_prompt_without_rag_context_trial = async_trial(&rt, "verify_prompt_without_rag_context", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            // This is implicitly covered by the existing dispatch_execution_trial,
            // which creates a dispatch with a Nushell task that calls `prompt send`
            // without any rag_context. If Phase 4 breaks backward compatibility,
            // the dispatch_execution_trial will fail.
            //
            // No additional trial needed -- document this as existing coverage.
            log::info!("verify_prompt_without_rag_context: covered by dispatch_execution_trial");
            Ok(())
        }
    }
});
```

### 4d. Tier 2: RAG Graceful Degradation

**Trial: `verify_rag_graceful_degradation`**

This trial verifies that prompt execution succeeds when `rag_context` specifies guides but the embedding service is unavailable or no embeddings exist.

```rust
let verify_rag_graceful_degradation_trial = async_trial(&rt, "verify_rag_graceful_degradation", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                log::info!("verify_rag_graceful_degradation: skipping (DAPPLE_CONDUIT_URL not set)");
                return Ok(());
            }

            let client = reqwest::Client::new();
            let auth = test_auth_header();

            // Create a self-contained task/step/playbook/journey/dispatch
            // with a rag_context pointing to a non-existent guide ID
            let task_body = serde_json::json!({
                "objs": [{
                    "name": format!("rag-degrade-task-{}", Uuid::now_v7()),
                    "description": "Task for RAG degradation test",
                    "function": "nushell",
                    "function_input": {
                        "script": "{ payload: { result: 'rag_degradation_ok' }, signals: { done: true } }",
                        "rag_guide_ids": [Uuid::now_v7().to_string()]  // non-existent guide
                    },
                    "prompt_ids": []
                }]
            });

            let resp = client
                .post(base_url.join("api/tasks/").unwrap())
                .header("authorization", &auth)
                .json(&task_body)
                .send()
                .await?;
            assert!(resp.status().is_success(), "create task failed");
            let body: serde_json::Value = resp.json().await?;
            let task_id: Uuid = body["objs"][0]["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("task id");

            // Create step -> playbook -> journey -> dispatch (same pattern as dispatch_execution_trial)
            // ... (abbreviated -- follows the exact same pattern as dispatch_execution_trial)

            // The key assertion: dispatch execution should succeed even though
            // rag_guide_ids points to a non-existent guide. RAG retrieval fails
            // gracefully and the task executes normally.

            // Poll for task attempt completion
            // ... (same polling pattern as dispatch_execution_trial)

            log::info!("verify_rag_graceful_degradation: dispatch completed despite non-existent RAG guide");
            Ok(())
        }
    }
});
```

### 4e. Tier 2: Full RAG Integration (End-to-End)

**Trial: `verify_rag_end_to_end`**

This is the most comprehensive test: it creates a guide with embeddings, creates a task that references that guide for RAG, executes a dispatch, and verifies that the execution succeeds. Verifying the actual RAG content injection into the prompt is done by checking the prompt response metadata or the task output signals.

```rust
let verify_rag_end_to_end_trial = async_trial(&rt, "verify_rag_end_to_end", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                log::info!("verify_rag_end_to_end: skipping (DAPPLE_CONDUIT_URL not set)");
                return Ok(());
            }

            // Get a guide with verified embeddings
            let guide = {
                let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                match ctx.guide_named_batches.get("guides-with-embeddings") {
                    Some(handle) => ctx.guide_batches[handle.index][0].clone(),
                    None => {
                        log::info!("verify_rag_end_to_end: skipping (no embedded guide)");
                        return Ok(());
                    }
                }
            };

            let client = reqwest::Client::new();
            let auth = test_auth_header();

            // 1. Create a task with rag_guide_ids pointing to our embedded guide
            let task_body = serde_json::json!({
                "objs": [{
                    "name": format!("rag-e2e-task-{}", Uuid::now_v7()),
                    "description": "Task for RAG E2E test",
                    "function": "nushell",
                    "function_input": {
                        "script": "{ payload: { result: 'rag_e2e_ok' }, signals: { done: true } }",
                        "rag_guide_ids": [guide.id.to_string()],
                        "rag_query_hint": "guide content"
                    },
                    "prompt_ids": []
                }]
            });

            let resp = client
                .post(base_url.join("api/tasks/").unwrap())
                .header("authorization", &auth)
                .json(&task_body)
                .send()
                .await?;
            assert!(resp.status().is_success(), "create task failed");
            let body: serde_json::Value = resp.json().await?;
            let task_id: Uuid = body["objs"][0]["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("task id");

            // 2-6: Create step -> playbook -> journey -> dispatch
            // (same entity creation pattern as dispatch_execution_trial)

            // Step
            let step_body = serde_json::json!({
                "objs": [{
                    "name": format!("rag-e2e-step-{}", Uuid::now_v7()),
                    "description": "Step for RAG E2E test",
                    "tasks": [{ "task_id": task_id }]
                }]
            });
            let resp = client.post(base_url.join("api/steps/").unwrap())
                .header("authorization", &auth).json(&step_body).send().await?;
            let body: serde_json::Value = resp.json().await?;
            let step_id: Uuid = body["objs"][0]["id"]
                .as_str().and_then(|s| Uuid::parse_str(s).ok()).expect("step id");

            // Category (reuse from context)
            let category_id = {
                let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                ctx.random_category_from_named_batch("categories-init")
                    .expect("should have categories").id
            };

            // Playbook
            let pb_body = serde_json::json!({
                "objs": [{
                    "name": format!("rag-e2e-pb-{}", Uuid::now_v7()),
                    "description": "Playbook for RAG E2E test",
                    "stages": [{"steps": [{"step_id": step_id, "params": {}, "tasks": [{"task_id": task_id}]}]}],
                    "categories": [category_id]
                }]
            });
            let resp = client.post(base_url.join("api/playbooks/").unwrap())
                .header("authorization", &auth).json(&pb_body).send().await?;
            let body: serde_json::Value = resp.json().await?;
            let pb_id: Uuid = body["objs"][0]["id"]
                .as_str().and_then(|s| Uuid::parse_str(s).ok()).expect("playbook id");
            let pb_step_id: Uuid = body["objs"][0]["stages"][0]["steps"][0]["id"]
                .as_str().and_then(|s| Uuid::parse_str(s).ok()).expect("playbook step id");
            let pb_task_id: Uuid = body["objs"][0]["stages"][0]["steps"][0]["tasks"][0]["id"]
                .as_str().and_then(|s| Uuid::parse_str(s).ok()).expect("playbook task id");

            // Journey
            let j_body = serde_json::json!({
                "objs": [{"name": format!("rag-e2e-j-{}", Uuid::now_v7()), "playbook_id": pb_id, "brief_pack_ids": [], "inference_profile_ids": []}]
            });
            let resp = client.post(base_url.join("api/journeys/").unwrap())
                .header("authorization", &auth).json(&j_body).send().await?;
            let body: serde_json::Value = resp.json().await?;
            let journey_id: Uuid = body["objs"][0]["id"]
                .as_str().and_then(|s| Uuid::parse_str(s).ok()).expect("journey id");

            // Dispatch
            let d_body = serde_json::json!({
                "objs": [{"journey_id": journey_id, "intent": {}, "steps": [{"playbook_step_id": pb_step_id, "params": {}, "tasks": [{"playbook_task_id": pb_task_id}]}]}]
            });
            let resp = client.post(base_url.join("api/dispatches/").unwrap())
                .header("authorization", &auth).json(&d_body).send().await?;
            assert!(resp.status().is_success(), "create dispatch failed");
            let body: serde_json::Value = resp.json().await?;
            let dispatch_id: Uuid = body["objs"][0]["id"]
                .as_str().and_then(|s| Uuid::parse_str(s).ok()).expect("dispatch id");

            // 7. Poll for completion
            let attempts_url = base_url
                .join(&format!("api/dispatches/{}/task_attempt/", dispatch_id))
                .unwrap();

            let mut found_ok = false;
            for attempt_num in 0..50 {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                let resp = client.get(attempts_url.clone())
                    .header("authorization", &auth).send().await?;
                if !resp.status().is_success() { continue; }
                let body: serde_json::Value = resp.json().await?;
                if let Some(objs) = body.get("objs").and_then(|v| v.as_array()) {
                    for attempt in objs {
                        let status = attempt.get("status").and_then(|s| s.as_str()).unwrap_or("");
                        if status == "ok" {
                            found_ok = true;
                            log::info!(
                                "verify_rag_end_to_end: attempt completed with 'ok' after {} polls",
                                attempt_num + 1
                            );
                            break;
                        }
                    }
                }
                if found_ok { break; }
            }

            assert!(
                found_ok,
                "Dispatch {} should complete successfully with RAG context for guide {}",
                dispatch_id, guide.id
            );

            log::info!("verify_rag_end_to_end: dispatch with RAG completed successfully");
            Ok(())
        }
    }
});
```

**Dependency level:** After `verify_guide_embeddings_generated` and `categories_trial`.

---

## 5. Cross-Phase Test: Full Pipeline Round-Trip

### 5a. Trial: `verify_full_search_pipeline`

This trial ties all three phases together: ingest -> embed -> search -> verify consistency.

```rust
let verify_full_search_pipeline_trial = async_trial(&rt, "verify_full_search_pipeline", {
    let base_url = base_url.clone();
    let scenario = scenario.clone();
    move || {
        let base_url = base_url.clone();
        async move {
            if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                log::info!("verify_full_search_pipeline: skipping (DAPPLE_CONDUIT_URL not set)");
                return Ok(());
            }

            // 1. Create a guide with distinctive content for search targeting
            let client = reqwest::Client::new();
            let auth = test_auth_header();

            let unique_term = format!("xyzzy{}", Uuid::now_v7().to_string().replace('-', "").chars().take(8).collect::<String>());
            let guide_body = serde_json::json!({
                "objs": [{
                    "title": format!("Search Pipeline Test Guide {}", unique_term),
                    "description": format!("A guide about {} for testing the full search pipeline", unique_term),
                    "author": "Test Author",
                    "classification": "workflow",
                    "source_path": format!("docs/guides/search-test-{}", unique_term),
                    "chapters": [{
                        "title": "Introduction",
                        "source_file": "intro.md",
                        "parent_index": null,
                        "blocks": [{
                            "directive_type": "prose",
                            "name": null,
                            "content": format!("This guide explains {} in detail. It covers the fundamentals of {} and provides practical examples.", unique_term, unique_term),
                            "attributes": {},
                            "fluent_message_id": null,
                            "tags": ["universal"]
                        }]
                    }]
                }]
            });

            let resp = client
                .post(base_url.join("api/guides/").unwrap())
                .header("authorization", &auth)
                .json(&guide_body)
                .send()
                .await?;
            assert!(resp.status().is_success(), "create guide failed");
            let body: serde_json::Value = resp.json().await?;
            let guide_id: Uuid = body["objs"][0]["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .expect("guide id");

            // 2. Wait for embeddings
            let pool = dapple_data::DbPool::connect().await.expect("db connect");
            let mut has_embeddings = false;
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let count = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1"
                )
                .bind(guide_id)
                .fetch_one(pool.get_pool())
                .await
                .expect("count embeddings");

                if count > 0 {
                    has_embeddings = true;
                    break;
                }
            }

            assert!(has_embeddings, "Guide {} should have embeddings within 15s", guide_id);

            // 3. Search using the unique term
            let search_ep = GuideSearchEndpoint {
                base_url: base_url.clone(),
            };

            let results = search_ep
                .search(&unique_term, Some(&["universal"]), Some("en"), Some(5), None, None)
                .await?;

            // The guide we just created should appear in results
            let found = results.iter().any(|r| r.guide_id == guide_id);
            assert!(
                found,
                "Search for '{}' should return guide {} in results (got {} results: {:?})",
                unique_term,
                guide_id,
                results.len(),
                results.iter().map(|r| r.guide_id).collect::<Vec<_>>()
            );

            // 4. Search scoped to this guide specifically
            let scoped = search_ep
                .search(&unique_term, None, None, Some(5), Some(&[guide_id]), None)
                .await?;

            assert!(
                !scoped.is_empty(),
                "Scoped search for '{}' in guide {} should return results",
                unique_term,
                guide_id
            );

            // Verify the chunk_text contains our unique term
            let has_term = scoped.iter().any(|r| r.chunk_text.contains(&unique_term));
            assert!(
                has_term,
                "At least one result should contain the unique term '{}'",
                unique_term
            );

            log::info!(
                "verify_full_search_pipeline: full round-trip succeeded for unique term '{}' (guide {})",
                unique_term,
                guide_id
            );

            Ok(())
        }
    }
});
```

---

## 6. Trial Ordering and Dependencies

### 6a. Updated Dependency Graph

```text
Level 0: categories, hats, fluent_messages, value_slots, assets, directives, inference_profiles, guides
         verify_search_endpoint_validation (NEW, no deps)

Level 1: briefs, verify_guides_by_slugs, deprecate_guides
         verify_guides_by_classification (NEW, depends on guides)
         verify_embedding_insert_delete (NEW, depends on guides)

Level 2: brief_groups, brief_units, prompts
         verify_guide_embeddings_generated (NEW, Tier 2, depends on guides)

Level 3: stages, tasks, brief_packs

Level 4: steps, verify_tasks_by_slugs

Level 5: playbooks, journeys, deprecate_briefs
         verify_guide_search_round_trip (NEW, Tier 2, depends on verify_guide_embeddings_generated)
         verify_guide_reingest_replaces_embeddings (NEW, Tier 2, depends on verify_guide_embeddings_generated)

Level 6: dispatches, verify_journeys_recent

Level 7: attempts, dispatch_intent_bindings, dispatch_execution
         verify_rag_graceful_degradation (NEW, Tier 2, depends on categories)
         verify_rag_end_to_end (NEW, Tier 2, depends on verify_guide_embeddings_generated + categories)
         verify_full_search_pipeline (NEW, Tier 2, depends on verify_guide_embeddings_generated)
```

### 6b. Placement in `run()` Vec

```rust
let conclusion = run(
    &args,
    vec![
        // Level 0: No dependencies
        categories_trial,
        hats_trial,
        fluent_messages_trial,
        value_slots_trial,
        assets_trial,
        directives_trial,
        inference_profiles_trial,
        guides_trial,
        verify_search_endpoint_validation_trial,           // NEW
        // Level 1: Depend on Level 0
        briefs_trial,
        verify_guides_by_slugs_trial,
        deprecate_guides_trial,
        verify_guides_by_classification_trial,              // NEW
        verify_embedding_insert_delete_trial,               // NEW
        // Level 2: Depend on Level 1
        brief_groups_trial,
        brief_units_trial,
        prompts_trial,
        verify_guide_embeddings_generated_trial,            // NEW (Tier 2)
        // Level 3: Depend on Level 2
        stages_trial,
        tasks_trial,
        brief_packs_trial,
        // Level 4: Depend on Level 3
        steps_trial,
        verify_tasks_by_slugs_trial,
        // Level 5: Depend on Level 4
        playbooks_trial,
        journeys_trial,
        deprecate_briefs_trial,
        verify_guide_search_round_trip_trial,               // NEW (Tier 2)
        verify_guide_reingest_replaces_embeddings_trial,    // NEW (Tier 2)
        // Level 6: Depend on Level 5
        dispatches_trial,
        verify_journeys_recent_trial,
        // Level 7: Depend on Level 6
        attempts_trial,
        dispatch_intent_bindings_trial,
        dispatch_execution_trial,
        verify_rag_graceful_degradation_trial,              // NEW (Tier 2)
        verify_rag_end_to_end_trial,                        // NEW (Tier 2)
        verify_full_search_pipeline_trial,                  // NEW (Tier 2)
    ],
);
```

---

## 7. ScenarioContext Additions

No new batch types are needed in `ScenarioContext`. The embedding tests use:
- `guide_batches` / `guide_named_batches` with key `"guides-with-embeddings"` (existing `Guide` type)
- Direct database queries for embedding verification (embeddings are not DTOs)

The `"guides-with-embeddings"` key is registered by `verify_guide_embeddings_generated_trial` and consumed by downstream trials (`verify_guide_search_round_trip`, `verify_rag_end_to_end`, etc.).

---

## 8. New File Manifest

### New Files

| File | Purpose |
|------|---------|
| `crates/dapple-prompt-api/tests/consumer/guide_search.rs` | `GuideSearchEndpoint` consumer for search endpoint testing |

### Modified Files

| File | Change |
|------|--------|
| `crates/dapple-prompt-api/tests/consumer/mod.rs` | Add `pub(crate) mod guide_search;` |
| `crates/dapple-prompt-api/tests/user_lifecycle.rs` | Add `use consumer::guide_search::GuideSearchEndpoint;`, add 8 new trial definitions, add trials to `run()` vec |

### No New Files Needed (Phase 4 Unit Tests)

Phase 4 unit tests live inside existing source files:
- `crates/dapple-prompt-conduit/src/service/rag.rs` -- `#[cfg(test)]` module
- `crates/dapple-prompt-flow/src/dispatch_runner.rs` -- `#[cfg(test)]` module

---

## 9. Implementation Guidance

### 9a. Tier 2 Skip Pattern

All Tier 2 tests MUST check for `DAPPLE_CONDUIT_URL` at the start and return `Ok(())` if absent. This ensures the test suite passes in CI environments without Conduit:

```rust
if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
    log::info!("{}: skipping (DAPPLE_CONDUIT_URL not set)", trial_name);
    return Ok(());
}
```

### 9b. Polling Pattern for Async Embeddings

The embedding pipeline runs in a `tokio::spawn` after guide ingest. Tests must poll the database to verify embeddings were created. Use a maximum of 30 attempts at 500ms intervals (15 seconds total). This is generous enough for ONNX inference on CPU.

```rust
for attempt in 0..30 {
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let count = sqlx::query_scalar::<_, i64>(...)
        .fetch_one(pool.get_pool())
        .await?;
    if count > 0 {
        break;
    }
}
```

### 9c. Database Direct Access Pattern

Several tests need to query `rev.guide_content_embeddings` directly (not through the API). Use `dapple_data::DbPool::connect()` to get a separate pool:

```rust
let pool = dapple_data::DbPool::connect().await.expect("db connect");
let count = sqlx::query_scalar::<_, i64>(
    "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1"
)
.bind(guide_id)
.fetch_one(pool.get_pool())
.await?;
```

This is acceptable because the test binary already has `DAPPLE_PROMPT_DATABASE_URL` in its environment.

### 9d. Unique Search Terms

The `verify_full_search_pipeline` trial uses a unique nonsense term (e.g., `xyzzy1a2b3c4d`) to ensure search results are definitively from the test guide. This avoids false positives from pre-existing guide content in the database.

### 9e. Phase 4 Test Conditionality

Phase 4 tests (`verify_rag_graceful_degradation` and `verify_rag_end_to_end`) depend on Phase 4 code being implemented. The implementation agent should:
1. Implement the Phase 4 code (RagService, PromptRequest changes, Flow changes)
2. Add the Phase 4 tests from this plan
3. Verify tests pass end-to-end

If Phase 4 is not yet implemented when this plan is executed, the Phase 4 trials should be commented out with a `// TODO: Enable after Phase 4 implementation` marker.

---

## 10. Test Coverage Summary

| Test | Phase | Tier | What It Verifies |
|------|-------|------|------------------|
| `verify_embedding_insert_delete` | 2 | 1 | SQL insert/delete/cascade for embeddings + tags + locales |
| `verify_guide_embeddings_generated` | 2 | 2 | Full async embedding pipeline after guide ingest |
| `verify_guide_reingest_replaces_embeddings` | 2 | 2 | PATCH ingest generates embeddings for new version |
| (existing `create_guides`) | 2 | 1 | Graceful degradation when Conduit unavailable |
| `verify_search_endpoint_validation` | 3 | 1 | Empty query 400, missing conduit 503 |
| `verify_guides_by_classification` | 3 | 1 | `ByClassification` query variant for guides |
| `verify_guide_search_round_trip` | 3 | 2 | Full search: query params, scoping, threshold, limits, result structure |
| RagContext unit tests | 4 | 1 | Serialization, defaults, format_rag_context |
| build_rag_context unit tests | 4 | 1 | Guide ID extraction, tag/locale defaults, overrides |
| `verify_rag_graceful_degradation` | 4 | 2 | Dispatch succeeds when RAG guide doesn't exist |
| `verify_rag_end_to_end` | 4 | 2 | Dispatch with RAG context referencing embedded guide |
| `verify_full_search_pipeline` | All | 2 | Ingest -> embed -> search -> find (unique term round-trip) |

---

## 11. Open Questions

### Q1: Should Tier 2 tests be in a separate test binary?

Currently all lifecycle tests run in one binary. Tier 2 tests add ~45 seconds of polling time when Conduit is available. If this slows down the base test suite unacceptably, Tier 2 tests could be split into a separate binary (`search_lifecycle.rs`) with its own `spawn_app()`. **Recommendation:** Keep them in one binary for now. The skip-when-no-conduit pattern means they add zero time in most CI/local runs.

### Q2: Should we test with the StubEmbeddingBackend?

Conduit has a `StubEmbeddingBackend` that produces deterministic hash-based embeddings. Tests could set `EMBEDDING_BACKEND=stub` to avoid needing the ONNX model. This would make Tier 2 tests runnable without the ModernBERT model file. **Recommendation:** Worth exploring. The stub produces valid 768-dim vectors, so pgvector operations would work. However, similarity search results with stub embeddings would be unpredictable (hash-based vectors don't have semantic similarity). The `verify_full_search_pipeline` test would need to relax its assertions.

### Q3: How to test `ByClassification` on guide cards?

Phase 3 adds `ByClassification` to both `GetGuideSetSchema` and `GetGuideCardSetSchema`. The current test plan only tests the guides variant. Guide cards testing requires a guide with a card, which is only created via the tarball ingest pipeline (not the JSON API). **Recommendation:** Defer guide card classification testing until a test tarball fixture exists.
