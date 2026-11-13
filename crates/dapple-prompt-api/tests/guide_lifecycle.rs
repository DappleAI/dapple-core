//! # Guide Lifecycle Integration Tests
//!
//! Comprehensive tests for the guide ingestion pipeline covering the full
//! lifecycle: ingest -> query -> versioning -> idempotent sync -> favorites ->
//! deprecation -> error paths.
//!
//! ## Architecture
//!
//! Tests use `libtest_mimic` with `harness = false`. Trials run sequentially
//! and share state via `Arc<Mutex<GuideLifecycleState>>` so each phase can
//! build on the previous (e.g., ingest v1 -> query v1 -> ingest v2 -> 409).
//!
//! ## Running
//!
//! ```bash
//! cargo test --test guide-lifecycle --package dapple-prompt-api
//! RUST_LOG=info cargo test --test guide-lifecycle
//! ```

mod consumer;

use consumer::{spawn_app, test_auth_header};
use libtest_mimic::{Arguments, Trial, run};
use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use uuid::Uuid;

static RT: Lazy<Arc<Runtime>> =
    Lazy::new(|| Arc::new(Runtime::new().expect("failed to create tokio runtime")));

// ============================================================================
// Shared State
// ============================================================================

#[derive(Debug, Default)]
struct GuideLifecycleState {
    base_url: String,
    // v1 ingest results
    v1_guide_id: Option<Uuid>,
    v1_path_id: Option<Uuid>,
    v1_slug: Option<String>,
    v1_content_hash: Option<String>,
    v1_playbook_ids: Vec<Uuid>,
    v1_task_ids: Vec<Uuid>,
    v1_inference_profile_ids: Vec<Uuid>,
    // v2 ingest results
    v2_guide_id: Option<Uuid>,
    v2_playbook_ids: Vec<Uuid>,
    // deprecated guide
    deprecated_path_id: Option<Uuid>,
    // multi-playbook guide results
    multi_pb_guide_id: Option<Uuid>,
    multi_pb_path_id: Option<Uuid>,
    multi_pb_playbook_ids: Vec<Uuid>,
    // unicode-identifier guide results (verifies the fluent-syntax-unicode swap
    // and Migration 42's Unicode-preserving slug derivation end-to-end)
    unicode_guide_id: Option<Uuid>,
    unicode_path_id: Option<Uuid>,
    unicode_slug: Option<String>,
    unicode_content_hash: Option<String>,
    // v1 suffix, reused by the v2 PATCH so it's a true body-only edit
    // (identifiers + title + classification stay fixed; only `variant` flips).
    unicode_suffix: Option<String>,
    // per-run classification to isolate from accumulated test data
    run_classification: Option<String>,
}

// ============================================================================
// Tarball Builders
// ============================================================================

/// Shared helper for appending a file to a tar archive.
fn append_file(
    archive: &mut tar::Builder<flate2::write::GzEncoder<&mut Vec<u8>>>,
    path: &str,
    content: &[u8],
) {
    let mut header = tar::Header::new_gnu();
    header.set_path(path).unwrap();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, content).unwrap();
}

/// Create a full guide tarball with card metadata, stages, steps, and assets.
///
/// The `variant` parameter is included in chapter content to produce different
/// content hashes for each variant. Use distinct variant strings (e.g., "v1", "v2")
/// to create genuinely different guide versions.
fn create_full_guide_tarball(suffix: &str, variant: &str) -> Vec<u8> {
    let mut tarball = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Lifecycle Guide {suffix}\"\n\
         description = \"Full lifecycle test guide\"\n\
         author = \"Test Runner\"\n\
         classification = \"test-{suffix}\"\n\
         \n\
         [card]\n\
         image = \"assets/images/card.jpg\"\n\
         color = \"#2D6A4F\"\n\
         accent = \"#95D5B2\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", guide_toml.as_bytes());

    let summary = "\
# Summary

- [Introduction](ch1.md)
- [Workflow](ch2.md)
";
    append_file(&mut archive, "guide/SUMMARY.md", summary.as_bytes());
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        b"# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "\
# Introduction ({variant})

An introduction to the lifecycle test guide.
"
    );
    append_file(&mut archive, "guide/ch1.md", ch1.as_bytes());

    let ch2 = format!(
        "\
# Workflow ({variant})

:::stage {{ name: \"Stage Alpha\" }}

:::step {{ name: \"Step A1\", task: \"task-a1\" }}
:::

:::step {{ name: \"Step A2\", task: \"task-a2\" }}
:::

:::

:::stage {{ name: \"Stage Beta\" }}

:::step {{ name: \"Step B1\", task: \"task-b1\" }}
:::

:::step {{ name: \"Step B2\", task: \"task-b2\" }}
:::

:::step {{ name: \"Step B3\", task: \"task-b3\" }}
:::

:::

:::nushell {{ name: \"task-a1\" }}
{{ payload: {{}}, signals: {{}} }}
:::

:::nushell {{ name: \"task-a2\" }}
{{ payload: {{}}, signals: {{}} }}
:::

:::nushell {{ name: \"task-b1\" }}
{{ payload: {{}}, signals: {{}} }}
:::

:::nushell {{ name: \"task-b2\" }}
{{ payload: {{}}, signals: {{}} }}
:::

:::nushell {{ name: \"task-b3\" }}
{{ payload: {{}}, signals: {{}} }}
:::
"
    );
    append_file(&mut archive, "guide/ch2.md", ch2.as_bytes());

    let fluent = format!(
        "\
stage-alpha = First stage of the lifecycle test guide.
step-a1 = First step in stage alpha.
step-a2 = Second step in stage alpha.
stage-beta = Second stage of the lifecycle test guide.
step-b1 = First step in stage beta.
step-b2 = Second step in stage beta.
step-b3 = Third step in stage beta.
lifecycle-msg-{suffix} = Lifecycle test unique message.
"
    );
    append_file(&mut archive, "guide/locales/en.ftl", fluent.as_bytes());

    // Minimal valid JPEG (smallest valid JFIF)
    let jpeg_bytes: Vec<u8> = vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xD9,
    ];
    append_file(&mut archive, "guide/assets/images/card.jpg", &jpeg_bytes);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Create a guide tarball with 3 chapters for multi-playbook testing.
///
/// - Chapter 1 "Search": 1 stage (Search Stage), 2 steps (Search-1, Search-2)
/// - Chapter 2 "Concepts": Pure prose, NO stages (tests skip behavior)
/// - Chapter 3 "Browse": 1 stage (Browse Stage), 3 steps (Browse-1, Browse-2, Browse-3)
///
/// This tests: mixed chapters (prose-only skipped), multi-playbook naming,
/// junction table chapter_index gaps (indices 0 and 2, not 0 and 1).
fn create_multi_playbook_mixed_tarball(suffix: &str, variant: &str) -> Vec<u8> {
    let short = &suffix[..8];
    let mut tarball = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Multi PB Guide {suffix}\"\n\
         description = \"Multi-playbook test guide with mixed chapters\"\n\
         author = \"Test Runner\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", guide_toml.as_bytes());

    let summary = "\
# Summary

- [Search](ch1.md)
- [Concepts](ch2.md)
- [Browse](ch3.md)
";
    append_file(&mut archive, "guide/SUMMARY.md", summary.as_bytes());
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        b"# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "\
# Search ({variant})

:::stage {{ name: \"Search Stage {short}\" }}

:::step {{ name: \"Search-1-{short}\", task: \"search-1-task-{short}\" }}
:::

:::step {{ name: \"Search-2-{short}\", task: \"search-2-task-{short}\" }}
:::

:::

:::nushell {{ name: \"search-1-task-{short}\" }}
{{ payload: {{}}, signals: {{}} }}
:::

:::nushell {{ name: \"search-2-task-{short}\" }}
{{ payload: {{}}, signals: {{}} }}
:::
"
    );
    append_file(&mut archive, "guide/ch1.md", ch1.as_bytes());

    let ch2 = format!(
        "\
# Concepts ({variant})

This chapter has no stages — just prose content about concepts.
"
    );
    append_file(&mut archive, "guide/ch2.md", ch2.as_bytes());

    let ch3 = format!(
        "\
# Browse ({variant})

:::stage {{ name: \"Browse Stage {short}\" }}

:::step {{ name: \"Browse-1-{short}\", task: \"browse-1-task-{short}\" }}
:::

:::step {{ name: \"Browse-2-{short}\", task: \"browse-2-task-{short}\" }}
:::

:::step {{ name: \"Browse-3-{short}\", task: \"browse-3-task-{short}\" }}
:::

:::

:::nushell {{ name: \"browse-1-task-{short}\" }}
{{ payload: {{}}, signals: {{}} }}
:::

:::nushell {{ name: \"browse-2-task-{short}\" }}
{{ payload: {{}}, signals: {{}} }}
:::

:::nushell {{ name: \"browse-3-task-{short}\" }}
{{ payload: {{}}, signals: {{}} }}
:::
"
    );
    append_file(&mut archive, "guide/ch3.md", ch3.as_bytes());

    let fluent = format!(
        "\
search-stage-{short} = Search stage description for multi-playbook test.
search-1-{short} = First search step.
search-2-{short} = Second search step.
browse-stage-{short} = Browse stage description for multi-playbook test.
browse-1-{short} = First browse step.
browse-2-{short} = Second browse step.
browse-3-{short} = Third browse step.
multi-pb-msg-{suffix} = Multi-playbook unique message.
"
    );
    append_file(&mut archive, "guide/locales/en.ftl", fluent.as_bytes());

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Create a guide tarball whose chapter titles, directive `name` attributes,
/// and FTL message keys all carry Latin-script diacritics (PT-pt style). The
/// fixture asserts the post-Migration-42 Unicode-preserving slug path and the
/// post-fluent-syntax-unicode FTL-identifier path round-trip cleanly together.
///
/// Authoring convention here matches the acme guide that originally
/// surfaced this work: directive `name=` is already in slug form, so the
/// matching FTL key is byte-identical.
fn create_unicode_guide_tarball(suffix: &str, variant: &str) -> Vec<u8> {
    let mut tarball = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Guía Esboçar {suffix}\"\n\
         description = \"Esboçar identificadores Unicode no pipeline\"\n\
         author = \"Test Runner\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", guide_toml.as_bytes());

    let summary = "\
# Summary

- [Introdução](ch1.md)
- [Estágios](ch2.md)
";
    append_file(&mut archive, "guide/SUMMARY.md", summary.as_bytes());
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        b"# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!("# Introdução ({variant})\n\nConteúdo introdutório.\n");
    append_file(&mut archive, "guide/ch1.md", ch1.as_bytes());

    let ch2 = format!(
        "\
# Estágios ({variant})

:::stage {{ name: \"esboçar-estágio\" }}

:::step {{ name: \"padrão-passo\", task: \"padrão-task\" }}
:::

:::step {{ name: \"o-tráfego-passo\", task: \"o-tráfego-task\" }}
:::

:::

:::nushell {{ name: \"padrão-task\", tags: [coder] }}
{{ payload: {{ ok: true, variant: \"{variant}\" }}, signals: {{ done: true }} }}
:::

:::nushell {{ name: \"o-tráfego-task\", tags: [coder] }}
{{ payload: {{ ok: true, variant: \"{variant}\" }}, signals: {{ done: true }} }}
:::
"
    );
    append_file(&mut archive, "guide/ch2.md", ch2.as_bytes());

    // Every FTL identifier here exercises the relaxed grammar:
    //   - ç in `esboçar-estágio`
    //   - á in `padrão-passo`
    //   - á in `o-tráfego-passo`
    //   - ç + ã in `as-horas-de-operação-intro`
    //   - .descrição attribute key (Unicode attribute identifier)
    let fluent = format!(
        "\
esboçar-estágio = Estágio Esboçar ({variant}).
    .descrição = Descrição estendida do estágio em pt-PT.
padrão-passo = Passo padrão ({variant}).
o-tráfego-passo = Passo sobre o tráfego ({variant}).
as-horas-de-operação-intro = Texto introdutório com ç e ã ({variant}).
unicode-msg-{suffix} = Mensagem única do teste lifecycle.
"
    );
    append_file(&mut archive, "guide/locales/en.ftl", fluent.as_bytes());

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Create a guide tarball with deprecation fields set.
fn create_deprecated_guide_tarball(suffix: &str) -> Vec<u8> {
    let mut tarball = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Deprecated Guide {suffix}"
description = "A guide marked as deprecated"
author = "Test"
classification = "test-{suffix}"

[deprecation]
level = "deprecated"
reason = "Superseded by new guide"
"#
    );
    append_file(&mut archive, "guide/guide.toml", guide_toml.as_bytes());

    let summary = "# Summary\n\n- [Chapter 1](ch1.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary.as_bytes());
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        b"# Conduit Context\n\nTest guide frame.\n",
    );

    let chapter = "# Chapter 1\n\nDeprecated guide content.\n";
    append_file(&mut archive, "guide/ch1.md", chapter.as_bytes());

    let fluent = format!("deprecated-msg-{suffix} = Deprecated message content\n");
    append_file(&mut archive, "guide/locales/en.ftl", fluent.as_bytes());

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Create an invalid tarball (missing guide.toml).
fn create_invalid_tarball() -> Vec<u8> {
    let mut tarball = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let content = "# Just a random file\n\nNo guide.toml here.\n";
    append_file(&mut archive, "guide/random.md", content.as_bytes());

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Create a minimal guide tarball (no stages/steps/card) for lightweight tests.
fn create_minimal_tarball(suffix: &str) -> Vec<u8> {
    let mut tarball = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Minimal Guide {suffix}"
description = "A minimal test guide"
author = "Test"
classification = "test-{suffix}"
"#
    );
    append_file(&mut archive, "guide/guide.toml", guide_toml.as_bytes());

    let summary = "# Summary\n\n- [Chapter 1](ch1.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary.as_bytes());
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        b"# Conduit Context\n\nTest guide frame.\n",
    );

    let chapter = "# Chapter 1\n\nMinimal content.\n";
    append_file(&mut archive, "guide/ch1.md", chapter.as_bytes());

    let fluent = format!("minimal-msg-{suffix} = Minimal message\n");
    append_file(&mut archive, "guide/locales/en.ftl", fluent.as_bytes());

    archive.finish().unwrap();
    drop(archive);
    tarball
}

// ============================================================================
// HTTP Helpers
// ============================================================================

fn http_client() -> reqwest::Client {
    reqwest::Client::new()
}

/// POST a tarball to the ingest endpoint, returning (status_code, json_body).
async fn post_ingest(base_url: &str, tarball_bytes: Vec<u8>, query: &str) -> (u16, Value) {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let url = if query.is_empty() {
        format!("{base_url}/api/guides/ingest")
    } else {
        format!("{base_url}/api/guides/ingest?{query}")
    };

    let resp = http_client()
        .post(&url)
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("POST ingest request failed");

    let status = resp.status().as_u16();
    let json: Value = resp.json().await.expect("invalid json from POST ingest");
    (status, json)
}

/// PATCH a tarball to the version ingest endpoint.
async fn patch_ingest(
    base_url: &str,
    path_id: Uuid,
    tarball_bytes: Vec<u8>,
    query: &str,
) -> (u16, Value) {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let url = if query.is_empty() {
        format!("{base_url}/api/guides/{path_id}/ingest")
    } else {
        format!("{base_url}/api/guides/{path_id}/ingest?{query}")
    };

    let resp = http_client()
        .patch(&url)
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("PATCH ingest request failed");

    let status = resp.status().as_u16();
    let json: Value = resp.json().await.expect("invalid json from PATCH ingest");
    (status, json)
}

/// GET a URL and return (status_code, json_body).
async fn get_json(base_url: &str, path: &str) -> (u16, Value) {
    let url = format!("{base_url}{path}");
    let resp = http_client()
        .get(&url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url} failed: {e}"));

    let status = resp.status().as_u16();
    let text = resp.text().await.expect("failed to read GET response body");
    let json: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid json from GET {url}: {e}\nbody: {text}"));
    (status, json)
}

/// POST to a URL with no multipart body (just auth), returning (status_code, json_body).
async fn post_empty(base_url: &str, path: &str) -> (u16, Value) {
    let url = format!("{base_url}{path}");
    let resp = http_client()
        .post(&url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST {url} failed: {e}"));

    let status = resp.status().as_u16();
    let json: Value = resp.json().await.expect("invalid json from POST");
    (status, json)
}

/// DELETE to a URL, returning (status_code, json_body).
async fn delete_json(base_url: &str, path: &str) -> (u16, Value) {
    let url = format!("{base_url}{path}");
    let resp = http_client()
        .delete(&url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .unwrap_or_else(|e| panic!("DELETE {url} failed: {e}"));

    let status = resp.status().as_u16();
    let json: Value = resp.json().await.expect("invalid json from DELETE");
    (status, json)
}

/// POST multipart to the ingest endpoint with no file field.
async fn post_ingest_no_file(base_url: &str) -> (u16, Value) {
    let form = reqwest::multipart::Form::new().text("not_file", "garbage");

    let resp = http_client()
        .post(format!("{base_url}/api/guides/ingest"))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("POST ingest (no file) request failed");

    let status = resp.status().as_u16();
    let json: Value = resp
        .json()
        .await
        .expect("invalid json from POST ingest (no file)");
    (status, json)
}

// ============================================================================
// Trial builder
// ============================================================================

fn trial<F, Fut>(rt: &Arc<Runtime>, name: &str, f: F) -> Trial
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    let rt = rt.clone();
    let name = name.to_string();
    Trial::test(name, move || rt.block_on(f()).map_err(|e| e.into()))
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    dapple_data::load_env();
    env_logger::init();

    // Initialize Redis (noop fallback for local dev).
    let mut redis_url = std::env::var("DAPPLE_REDIS_HOST")
        .unwrap_or_else(|_| "redis://dapple-redis.kube.dappleai.dev:6379".to_string());
    if std::env::var("DAPPLE_REDIS_PASSWORD").is_ok()
        && !redis_url.contains('@')
        && let Ok(pw) = std::env::var("DAPPLE_REDIS_PASSWORD")
    {
        if let Some(rest) = redis_url.strip_prefix("redis://") {
            redis_url = format!("redis://:{}@{}", pw, rest);
        } else {
            redis_url = format!("redis://:{}@{}", pw, redis_url);
        }
    }
    let init_res = RT.block_on(async {
        dapple_data::integrations::try_init_redis_with_url_async(&redis_url).await
    });
    match init_res {
        Ok(()) => (),
        Err(e) => {
            if std::env::var("DAPPLE_REDIS_STRICT").is_ok() {
                panic!("failed to initialize redis integrations: {:?}", e);
            } else {
                log::warn!("Redis init failed, falling back to noop: {}", e);
                dapple_data::integrations::init_noop();
            }
        }
    }

    let rt = RT.clone();
    let (base_url, _handle) = rt.block_on(spawn_app());

    // Unique suffix for this test run to avoid DB collisions.
    let suffix = Uuid::now_v7().to_string();

    let run_classification = format!("test-{suffix}");
    let state = Arc::new(Mutex::new(GuideLifecycleState {
        base_url: base_url.clone(),
        run_classification: Some(run_classification),
        ..Default::default()
    }));

    let mut args = Arguments::from_args();
    args.test_threads = Some(1); // Tests share state sequentially

    let trials = vec![
        // ================================================================
        // Phase 1: Initial Ingest (POST)
        // ================================================================
        trial(&rt, "01_post_dry_run", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let tarball = create_full_guide_tarball(&suffix, "v1");
                let (status, json) = post_ingest(&base_url, tarball, "dry_run=true").await;

                assert_eq!(status, 200, "dry-run should return 200: {json}");
                assert_eq!(json["valid"], true, "dry-run should be valid: {json}");
                assert!(
                    json["warnings"].is_array(),
                    "warnings should be array: {json}"
                );
                assert!(
                    json["summary"]["chapters"].is_number(),
                    "summary.chapters should be number: {json}"
                );
                let title = json["summary"]["title"].as_str().unwrap_or("");
                assert!(
                    title.contains("Lifecycle Guide"),
                    "summary.title should contain 'Lifecycle Guide': {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "02_post_ingest_v1", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let tarball = create_full_guide_tarball(&suffix, "v1");
                let (status, json) = post_ingest(&base_url, tarball, "").await;

                assert_eq!(status, 201, "ingest should return 201: {json}");
                assert_eq!(
                    json["status"], "success",
                    "status should be success: {json}"
                );

                let guide = &json["guide"];
                let id = guide["id"]
                    .as_str()
                    .ok_or_else(|| format!("guide.id missing: {json}"))?;
                let path_id = guide["path_id"]
                    .as_str()
                    .ok_or_else(|| format!("guide.path_id missing: {json}"))?;
                let title = guide["title"]
                    .as_str()
                    .ok_or_else(|| format!("guide.title missing: {json}"))?;
                assert!(
                    title.contains("Lifecycle Guide"),
                    "title should contain 'Lifecycle Guide': {title}"
                );

                // Check content_hash is present and valid (40 hex chars)
                let hash = guide["content_hash"]
                    .as_str()
                    .ok_or_else(|| format!("content_hash missing: {json}"))?;
                assert_eq!(
                    hash.len(),
                    40,
                    "content_hash should be 40 hex chars: {hash}"
                );

                // Check derived objects
                let derived = &json["derived_objects"];
                let playbook_ids: Vec<Uuid> = derived["playbook_ids"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();
                assert!(
                    !playbook_ids.is_empty(),
                    "should have at least 1 playbook: {json}"
                );

                // Save state
                let mut s = state.lock().unwrap();
                s.v1_guide_id = Some(Uuid::parse_str(id).map_err(|e| e.to_string())?);
                s.v1_path_id = Some(Uuid::parse_str(path_id).map_err(|e| e.to_string())?);
                s.v1_slug = guide["slug"].as_str().map(|s| s.to_string());
                s.v1_content_hash = Some(hash.to_string());
                s.v1_playbook_ids = playbook_ids;
                s.v1_task_ids = derived["task_ids"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();
                s.v1_inference_profile_ids = derived["inference_profile_ids"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();

                Ok(())
            }
        }),
        trial(&rt, "03_post_ingest_source_path_override", {
            let state = state.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let unique = Uuid::now_v7().to_string();
                let tarball = create_minimal_tarball(&unique);
                // The guide slug is now derived from the source_path directory
                // basename (directory = slug), so the override's last segment
                // must be unique per run — the dev DB is shared and slug is
                // UNIQUE per namespace.
                let expected_source_path = format!("custom/test/{unique}");
                let (status, json) = post_ingest(
                    &base_url,
                    tarball,
                    &format!("source_path={expected_source_path}"),
                )
                .await;

                assert_eq!(
                    status, 201,
                    "source_path override should return 201: {json}"
                );
                assert_eq!(json["status"], "success");
                let source_path = json["guide"]["source_path"]
                    .as_str()
                    .ok_or_else(|| format!("guide.source_path missing: {json}"))?;
                assert_eq!(
                    source_path, expected_source_path,
                    "source_path should match override: {json}"
                );
                // slug == the basename of source_path (here, `unique`), not the
                // title — the whole point of the directory = slug rule.
                let slug = json["guide"]["slug"]
                    .as_str()
                    .ok_or_else(|| format!("guide.slug missing: {json}"))?;
                assert_eq!(
                    slug, unique,
                    "slug should be the source_path directory basename: {json}"
                );
                Ok(())
            }
        }),
        // ================================================================
        // Phase 2: Query Verification (GET)
        // ================================================================
        trial(&rt, "04_get_by_object_id", {
            let state = state.clone();
            move || async move {
                let (base_url, v1_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_guide_id.ok_or("v1_guide_id not set")?,
                    )
                };

                let (status, json) = get_json(&base_url, &format!("/api/guides/{v1_id}")).await;

                assert_eq!(status, 200, "GET by id should return 200: {json}");
                let obj = &json["obj"];
                assert_eq!(
                    obj["id"].as_str().unwrap_or(""),
                    v1_id.to_string(),
                    "id should match: {json}"
                );
                assert!(
                    obj["chapters"].is_array(),
                    "chapters should be present: {json}"
                );
                let chapters = obj["chapters"].as_array().unwrap();
                assert!(
                    chapters.len() >= 2,
                    "should have at least 2 chapters: {json}"
                );

                Ok(())
            }
        }),
        trial(&rt, "05_get_by_object_ids_batch", {
            let state = state.clone();
            move || async move {
                let (base_url, v1_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_guide_id.ok_or("v1_guide_id not set")?,
                    )
                };

                let (status, json) = get_json(
                    &base_url,
                    &format!("/api/guides/?query=by_object_ids&ids[0]={v1_id}"),
                )
                .await;

                assert_eq!(status, 200, "batch GET should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert_eq!(objs.len(), 1, "should return 1 guide: {json}");
                assert_eq!(
                    objs[0]["id"].as_str().unwrap_or(""),
                    v1_id.to_string(),
                    "returned id should match: {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "06_get_by_slugs", {
            let state = state.clone();
            move || async move {
                let (base_url, slug) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_slug.clone().ok_or("v1_slug not set")?,
                    )
                };

                let (status, json) = get_json(
                    &base_url,
                    &format!("/api/guides/?query=by_slugs&slugs[0]={slug}"),
                )
                .await;

                assert_eq!(status, 200, "slug query should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert!(
                    !objs.is_empty(),
                    "slug query should return at least 1 guide: {json}"
                );
                let returned_slug = objs[0]["slug"].as_str().unwrap_or("");
                assert_eq!(returned_slug, slug, "slug should match: {json}");
                Ok(())
            }
        }),
        trial(&rt, "07_get_by_path_ids", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let (status, json) = get_json(
                    &base_url,
                    &format!("/api/guides/?query=by_path_ids&path_ids[0]={path_id}"),
                )
                .await;

                assert_eq!(status, 200, "path_ids query should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert!(
                    !objs.is_empty(),
                    "path_ids query should return at least 1: {json}"
                );
                assert_eq!(
                    objs[0]["path_id"].as_str().unwrap_or(""),
                    path_id.to_string(),
                    "path_id should match: {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "08_get_all", {
            let state = state.clone();
            move || async move {
                let (base_url, v1_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_guide_id.ok_or("v1_guide_id not set")?,
                    )
                };

                // Paginate through all guides (limit 50 per page) to find the
                // v1 guide regardless of how many guides exist from prior runs.
                // UUIDv7 IDs are monotonically increasing and the SQL orders by
                // id ASC, so the newest guide is always on the last page.
                let mut found = false;
                let mut page = 1u32;
                loop {
                    let (status, json) = get_json(
                        &base_url,
                        &format!(r#"/api/guides/?query=all&options={{"limit":50,"page":{page}}}"#),
                    )
                    .await;

                    assert_eq!(status, 200, "GET all page {page} should return 200: {json}");
                    let objs = json["objs"]
                        .as_array()
                        .ok_or_else(|| format!("objs should be array: {json}"))?;

                    if objs.is_empty() {
                        break;
                    }

                    if objs
                        .iter()
                        .any(|g| g["id"].as_str() == Some(&v1_id.to_string()))
                    {
                        found = true;
                        break;
                    }

                    page += 1;
                    // Safety valve. The shared dev DB accumulates guides from
                    // every test run (≈10k as of 2026-07-31), and the newest
                    // guide always sorts LAST under id-ASC UUIDv7 ordering —
                    // a valve below count/50 makes this test fail on DB volume
                    // alone.
                    if page > 2000 {
                        break;
                    }
                }

                assert!(
                    found,
                    "v1 guide {v1_id} should appear somewhere in paginated all results"
                );
                Ok(())
            }
        }),
        // ================================================================
        // Phase 3: Guide Card & Assets
        // ================================================================
        trial(&rt, "09_verify_guide_card", {
            let state = state.clone();
            move || async move {
                let (base_url, v1_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_guide_id.ok_or("v1_guide_id not set")?,
                    )
                };

                let (status, json) = get_json(&base_url, &format!("/api/guides/{v1_id}")).await;
                assert_eq!(status, 200);

                let card = &json["obj"]["card"];
                assert!(!card.is_null(), "guide should have a card: {json}");
                assert_eq!(
                    card["color"].as_str().unwrap_or(""),
                    "#2D6A4F",
                    "card.color: {json}"
                );
                assert_eq!(
                    card["accent"].as_str().unwrap_or(""),
                    "#95D5B2",
                    "card.accent: {json}"
                );
                // card_image_asset_id should be populated (image was in tarball)
                assert!(
                    card["card_image_asset_id"].is_string(),
                    "card should have card_image_asset_id: {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "10_verify_guide_assets", {
            let state = state.clone();
            move || async move {
                let (base_url, v1_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_guide_id.ok_or("v1_guide_id not set")?,
                    )
                };

                let (status, json) = get_json(&base_url, &format!("/api/guides/{v1_id}")).await;
                assert_eq!(status, 200);

                let assets = json["obj"]["assets"]
                    .as_array()
                    .ok_or_else(|| format!("assets should be array: {json}"))?;
                assert!(!assets.is_empty(), "should have at least 1 asset: {json}");
                let card_image_asset = assets
                    .iter()
                    .find(|a| a["role"].as_str() == Some("card_image"));
                assert!(
                    card_image_asset.is_some(),
                    "should have an asset with role 'card_image': {json}"
                );
                let asset = card_image_asset.unwrap();
                let rel_path = asset["relative_path"].as_str().unwrap_or("");
                assert!(
                    rel_path.contains("card.jpg"),
                    "relative_path should contain 'card.jpg': {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "11_get_cards_by_classification", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id, classification) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                        s.run_classification
                            .clone()
                            .ok_or("run_classification not set")?,
                    )
                };

                let url = format!(
                    "/api/guides/cards?query=by_classification&classification={classification}"
                );
                let (status, json) = get_json(&base_url, &url).await;

                assert_eq!(status, 200, "cards query should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert!(
                    !objs.is_empty(),
                    "cards should have at least 1 entry: {json}"
                );
                let found = objs
                    .iter()
                    .any(|c| c["path_id"].as_str() == Some(&path_id.to_string()));
                assert!(
                    found,
                    "our guide's card should appear in cards list: {json}"
                );
                Ok(())
            }
        }),
        // ================================================================
        // Phase 4: Derived Object Verification
        // ================================================================
        trial(&rt, "12_verify_playbook_structure", {
            let state = state.clone();
            move || async move {
                let (base_url, playbook_ids) = {
                    let s = state.lock().unwrap();
                    (s.base_url.clone(), s.v1_playbook_ids.clone())
                };
                assert!(
                    !playbook_ids.is_empty(),
                    "v1_playbook_ids should not be empty"
                );

                // Single playbook: only ch2 (Workflow) has stages, ch1 (Introduction) is prose-only
                assert_eq!(
                    playbook_ids.len(),
                    1,
                    "should have 1 playbook (only one chapter has stages)"
                );

                // Query the single playbook
                let (status, json) = get_json(
                    &base_url,
                    &format!(
                        "/api/playbooks/?query=by_object_ids&ids[0]={}",
                        playbook_ids[0]
                    ),
                )
                .await;

                assert_eq!(status, 200, "playbook query should return 200: {json}");
                let playbooks = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert_eq!(playbooks.len(), 1, "should return 1 playbook: {json}");

                let pb = &playbooks[0];
                let name = pb["name"].as_str().unwrap_or("");
                assert!(
                    name.contains("Lifecycle Guide"),
                    "playbook name should contain guide title: {name}"
                );

                let stages = pb["stages"]
                    .as_array()
                    .ok_or_else(|| format!("playbook stages should be array: {json}"))?;
                assert_eq!(stages.len(), 2, "playbook should have 2 stages: {json}");

                // Stage 1 (Stage Alpha): 2 steps
                let stage1_steps = stages[0]["steps"]
                    .as_array()
                    .ok_or_else(|| format!("stage 1 steps missing: {json}"))?;
                assert_eq!(stage1_steps.len(), 2, "stage 1 should have 2 steps: {json}");

                // Stage 2 (Stage Beta): 3 steps
                let stage2_steps = stages[1]["steps"]
                    .as_array()
                    .ok_or_else(|| format!("stage 2 steps missing: {json}"))?;
                assert_eq!(stage2_steps.len(), 3, "stage 2 should have 3 steps: {json}");

                Ok(())
            }
        }),
        trial(&rt, "13_verify_step_names", {
            let state = state.clone();
            move || async move {
                let (base_url, playbook_ids) = {
                    let s = state.lock().unwrap();
                    (s.base_url.clone(), s.v1_playbook_ids.clone())
                };
                assert!(
                    !playbook_ids.is_empty(),
                    "v1_playbook_ids should not be empty"
                );

                // Query the single playbook to collect step IDs from its stages
                assert_eq!(playbook_ids.len(), 1, "expected 1 playbook");

                let (_, json) = get_json(
                    &base_url,
                    &format!(
                        "/api/playbooks/?query=by_object_ids&ids[0]={}",
                        playbook_ids[0]
                    ),
                )
                .await;

                let playbooks = json["objs"].as_array().ok_or("playbooks missing")?;

                let step_ids: Vec<&str> = playbooks
                    .iter()
                    .flat_map(|pb| {
                        pb["stages"].as_array().unwrap().iter().flat_map(|stage| {
                            stage["steps"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|ps| ps["step"]["id"].as_str().unwrap())
                        })
                    })
                    .collect();
                assert_eq!(step_ids.len(), 5, "expected 5 total step refs");

                // Query step entities
                let ids_query: String = step_ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| format!("ids[{}]={}", i, id))
                    .collect::<Vec<_>>()
                    .join("&");

                let (status, steps_json) = get_json(
                    &base_url,
                    &format!("/api/steps/?query=by_object_ids&{ids_query}"),
                )
                .await;
                assert_eq!(status, 200, "steps query: {steps_json}");

                let steps = steps_json["objs"]
                    .as_array()
                    .ok_or("objs should be array")?;
                assert_eq!(steps.len(), 5, "expected 5 step entities: {steps_json}");

                let mut step_names: Vec<&str> = steps
                    .iter()
                    .map(|s| s["name"].as_str().expect("step should have name"))
                    .collect();
                step_names.sort();

                let mut expected = vec!["Step A1", "Step A2", "Step B1", "Step B2", "Step B3"];
                expected.sort();
                assert_eq!(step_names, expected, "step names mismatch");
                Ok(())
            }
        }),
        // ================================================================
        // Phase 4b: Guide Versions Endpoint
        // ================================================================
        trial(&rt, "13b_get_versions_by_slug", {
            let state = state.clone();
            move || async move {
                let (base_url, slug, v1_content_hash) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_slug.clone().ok_or("v1_slug not set")?,
                        s.v1_content_hash.clone().ok_or("v1_content_hash not set")?,
                    )
                };

                let (status, json) =
                    get_json(&base_url, &format!("/api/guides/versions?slug={slug}")).await;

                assert_eq!(status, 200, "versions by slug should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert!(
                    !objs.is_empty(),
                    "versions by slug should return at least 1 version: {json}"
                );

                // The v1 content_hash should appear in the results
                let found = objs
                    .iter()
                    .any(|v| v["content_hash"].as_str() == Some(&v1_content_hash));
                assert!(
                    found,
                    "v1 content_hash {v1_content_hash} should appear in versions: {json}"
                );

                // Verify version structure has expected fields
                let version = &objs[0];
                assert!(
                    version["slug"].is_string(),
                    "version should have slug: {json}"
                );
                assert!(
                    version["title"].is_string(),
                    "version should have title: {json}"
                );
                assert!(
                    version["content_hash"].is_string(),
                    "version should have content_hash: {json}"
                );
                assert!(
                    version["content_hash_short"].is_string(),
                    "version should have content_hash_short: {json}"
                );
                assert!(
                    version["versioned_object_id"].is_string(),
                    "version should have versioned_object_id: {json}"
                );
                assert!(
                    version["path_id"].is_string(),
                    "version should have path_id: {json}"
                );
                assert!(
                    version["created_at"].is_string(),
                    "version should have created_at: {json}"
                );

                // content_hash_short should be an 8-char prefix of content_hash
                let full = version["content_hash"].as_str().unwrap_or("");
                let short = version["content_hash_short"].as_str().unwrap_or("");
                assert_eq!(
                    short.len(),
                    8,
                    "content_hash_short should be 8 chars: {short}"
                );
                assert!(
                    full.starts_with(short),
                    "content_hash should start with content_hash_short: full={full}, short={short}"
                );

                Ok(())
            }
        }),
        trial(&rt, "13c_get_versions_no_filter", {
            let state = state.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();

                let (status, json) = get_json(&base_url, "/api/guides/versions").await;

                assert_eq!(status, 200, "versions unfiltered should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert!(
                    !objs.is_empty(),
                    "unfiltered versions should return at least 1: {json}"
                );
                Ok(())
            }
        }),
        // ================================================================
        // Phase 5: Version Lifecycle (PATCH)
        // ================================================================
        trial(&rt, "14_patch_dry_run", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let tarball = create_full_guide_tarball(&suffix, "v2");
                let (status, json) =
                    patch_ingest(&base_url, path_id, tarball, "dry_run=true").await;

                assert_eq!(status, 200, "PATCH dry-run should return 200: {json}");
                assert_eq!(json["valid"], true, "dry-run should be valid: {json}");
                assert!(
                    json["summary"].is_object(),
                    "summary should be present: {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "15_patch_ingest_v2", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let tarball = create_full_guide_tarball(&suffix, "v2");
                let (status, json) = patch_ingest(&base_url, path_id, tarball, "").await;

                assert_eq!(status, 201, "PATCH ingest v2 should return 201: {json}");
                assert_eq!(json["status"], "success");

                let guide = &json["guide"];
                // Check content_hash is present and different from v1
                let v2_hash = guide["content_hash"]
                    .as_str()
                    .ok_or_else(|| format!("v2 content_hash missing: {json}"))?;
                assert_eq!(
                    v2_hash.len(),
                    40,
                    "v2 content_hash should be 40 hex chars: {v2_hash}"
                );

                let v2_id = guide["id"]
                    .as_str()
                    .ok_or_else(|| format!("v2 guide.id missing: {json}"))?;
                let v2_path = guide["path_id"]
                    .as_str()
                    .ok_or_else(|| format!("v2 guide.path_id missing: {json}"))?;

                // Same path_id, different id
                assert_eq!(
                    v2_path,
                    path_id.to_string(),
                    "path_id should be the same across versions: {json}"
                );

                let v1_id = state.lock().unwrap().v1_guide_id.unwrap();
                let v2_uuid = Uuid::parse_str(v2_id).map_err(|e| e.to_string())?;
                assert_ne!(v2_uuid, v1_id, "v2 id should differ from v1");

                // Save v2 playbook ids for verification
                let v2_playbook_ids: Vec<Uuid> = json["derived_objects"]["playbook_ids"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();

                let mut s = state.lock().unwrap();
                s.v2_guide_id = Some(v2_uuid);
                s.v2_playbook_ids = v2_playbook_ids;
                Ok(())
            }
        }),
        trial(&rt, "15b_verify_v2_playbook_structure", {
            let state = state.clone();
            move || async move {
                let (base_url, v2_playbook_ids) = {
                    let s = state.lock().unwrap();
                    (s.base_url.clone(), s.v2_playbook_ids.clone())
                };

                // With content-addressable versioning, the v2 playbooks may be the
                // same objects as v1 if the playbook structure (stages, steps) hasn't
                // changed. Only the guide narrative text differs between v1 and v2,
                // so the derived playbooks are correctly reused.
                assert!(
                    !v2_playbook_ids.is_empty(),
                    "v2_playbook_ids should not be empty"
                );
                assert_eq!(v2_playbook_ids.len(), 1, "v2 should have 1 playbook");

                // Query the single v2 playbook
                let (status, json) = get_json(
                    &base_url,
                    &format!(
                        "/api/playbooks/?query=by_object_ids&ids[0]={}",
                        v2_playbook_ids[0]
                    ),
                )
                .await;
                assert_eq!(status, 200, "playbook query should return 200: {json}");

                let playbooks = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert_eq!(playbooks.len(), 1, "should return 1 playbook: {json}");

                let stages = playbooks[0]["stages"]
                    .as_array()
                    .ok_or_else(|| format!("playbook stages missing: {json}"))?;
                assert_eq!(stages.len(), 2, "playbook should have 2 stages: {json}");

                // Stage 1: 2 steps, Stage 2: 3 steps
                let stage1_steps = stages[0]["steps"]
                    .as_array()
                    .ok_or_else(|| format!("stage 1 steps missing: {json}"))?;
                let stage2_steps = stages[1]["steps"]
                    .as_array()
                    .ok_or_else(|| format!("stage 2 steps missing: {json}"))?;
                assert_eq!(stage1_steps.len(), 2, "stage 1 should have 2 steps: {json}");
                assert_eq!(stage2_steps.len(), 3, "stage 2 should have 3 steps: {json}");

                Ok(())
            }
        }),
        trial(&rt, "16_get_both_versions", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let (status, json) = get_json(
                    &base_url,
                    &format!("/api/guides/?query=by_path_ids&path_ids[0]={path_id}"),
                )
                .await;

                assert_eq!(status, 200, "both versions query: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert_eq!(objs.len(), 2, "should return both v1 and v2: {json}");

                // Collect content hashes
                let hashes: Vec<&str> = objs
                    .iter()
                    .filter_map(|g| g["content_hash"].as_str())
                    .collect();
                assert_eq!(hashes.len(), 2, "should have 2 content hashes: {hashes:?}");
                assert_ne!(
                    hashes[0], hashes[1],
                    "v1 and v2 should have different content hashes: {hashes:?}"
                );
                Ok(())
            }
        }),
        trial(&rt, "16b_get_versions_both_after_v2", {
            let state = state.clone();
            move || async move {
                let (base_url, slug, v1_content_hash) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_slug.clone().ok_or("v1_slug not set")?,
                        s.v1_content_hash.clone().ok_or("v1_content_hash not set")?,
                    )
                };

                let (status, json) =
                    get_json(&base_url, &format!("/api/guides/versions?slug={slug}")).await;

                assert_eq!(status, 200, "versions after v2 should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert!(
                    objs.len() >= 2,
                    "should have at least 2 versions (v1 + v2): {json}"
                );

                // Collect content hashes from versions
                let hashes: Vec<&str> = objs
                    .iter()
                    .filter_map(|v| v["content_hash"].as_str())
                    .collect();
                assert!(
                    hashes.len() >= 2,
                    "should have at least 2 content hashes: {hashes:?}"
                );

                // v1 hash should be in the list
                assert!(
                    hashes.contains(&v1_content_hash.as_str()),
                    "v1 content_hash {v1_content_hash} should be in versions: {hashes:?}"
                );

                // All versions should share the same path_id and slug
                let first_path_id = objs[0]["path_id"].as_str().unwrap_or("");
                let first_slug = objs[0]["slug"].as_str().unwrap_or("");
                for (i, v) in objs.iter().enumerate() {
                    assert_eq!(
                        v["path_id"].as_str().unwrap_or(""),
                        first_path_id,
                        "version {i} should share path_id: {json}"
                    );
                    assert_eq!(
                        v["slug"].as_str().unwrap_or(""),
                        first_slug,
                        "version {i} should share slug: {json}"
                    );
                }

                // Should be ordered by created_at DESC (newest first)
                let created_ats: Vec<&str> = objs
                    .iter()
                    .filter_map(|v| v["created_at"].as_str())
                    .collect();
                for window in created_ats.windows(2) {
                    assert!(
                        window[0] >= window[1],
                        "versions should be ordered by created_at DESC: {created_ats:?}"
                    );
                }

                Ok(())
            }
        }),
        // ================================================================
        // Phase 6: Idempotent re-sync (200 OK with refresh semantics)
        //
        // The PATCH ingest used to return 409 when the content hash already
        // existed; now it always runs materialization (idempotently) and
        // returns 200 OK with a RefreshResponse. A pure no-op re-sync of
        // the same tarball produces the response with all REUSE counts.
        // ================================================================
        trial(&rt, "17_patch_idempotent_refresh", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let (base_url, path_id, v2_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                        s.v2_guide_id.ok_or("v2_guide_id not set")?,
                    )
                };

                // Re-submit the same v0.2.0 tarball
                let tarball = create_full_guide_tarball(&suffix, "v2");
                let (status, json) = patch_ingest(&base_url, path_id, tarball, "").await;

                assert_eq!(
                    status, 200,
                    "idempotent re-sync should return 200 OK: {json}"
                );
                assert_eq!(
                    json["status"], "refreshed",
                    "status should be refreshed: {json}"
                );
                let returned_guide_id = json["guide_id"]
                    .as_str()
                    .ok_or_else(|| format!("guide_id missing: {json}"))?;
                assert_eq!(
                    returned_guide_id,
                    v2_id.to_string(),
                    "refreshed guide_id should match v2 (existing): {json}"
                );
                Ok(())
            }
        }),
        // ================================================================
        // Phase 7: Favorites Lifecycle
        // ================================================================
        trial(&rt, "18_favorite_add", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let (status, json) =
                    post_empty(&base_url, &format!("/api/guides/{path_id}/favorite")).await;

                assert!(
                    status == 201 || status == 200,
                    "favorite add should return 201 or 200: {status} {json}"
                );
                assert_eq!(json["status"], "success", "status: {json}");
                Ok(())
            }
        }),
        trial(&rt, "19_favorite_check", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let (status, json) =
                    get_json(&base_url, &format!("/api/guides/{path_id}/favorite")).await;

                assert_eq!(status, 200, "favorite check should return 200: {json}");
                assert_eq!(json["is_favorited"], true, "should be favorited: {json}");
                Ok(())
            }
        }),
        trial(&rt, "20_favorite_list", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let (status, json) = get_json(&base_url, "/api/guides/favorites").await;

                assert_eq!(status, 200, "favorites list should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert!(
                    !objs.is_empty(),
                    "favorites list should have at least 1 entry: {json}"
                );
                let found = objs
                    .iter()
                    .any(|f| f["guide"]["id"].as_str() == Some(&path_id.to_string()));
                assert!(found, "our guide should appear in favorites list: {json}");
                Ok(())
            }
        }),
        trial(&rt, "21_favorite_remove", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let (status, json) =
                    delete_json(&base_url, &format!("/api/guides/{path_id}/favorite")).await;

                assert_eq!(status, 200, "favorite remove should return 200: {json}");
                assert_eq!(json["status"], "success", "status: {json}");
                Ok(())
            }
        }),
        trial(&rt, "22_favorite_check_after_remove", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.v1_path_id.ok_or("v1_path_id not set")?,
                    )
                };

                let (status, json) =
                    get_json(&base_url, &format!("/api/guides/{path_id}/favorite")).await;

                assert_eq!(status, 200, "check after remove should return 200: {json}");
                assert_eq!(
                    json["is_favorited"], false,
                    "should not be favorited after remove: {json}"
                );
                Ok(())
            }
        }),
        // ================================================================
        // Phase 8: Deprecation & Error Paths
        // ================================================================
        trial(&rt, "23_post_deprecated_guide", {
            let state = state.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let unique = Uuid::now_v7().to_string();
                let tarball = create_deprecated_guide_tarball(&unique);
                let (status, json) = post_ingest(&base_url, tarball, "").await;

                assert_eq!(
                    status, 201,
                    "deprecated guide ingest should return 201: {json}"
                );
                assert_eq!(json["status"], "success");

                let guide = &json["guide"];
                assert_eq!(
                    guide["deprecation_level"].as_str().unwrap_or(""),
                    "deprecated",
                    "deprecation_level should be 'deprecated': {json}"
                );

                let dep_path_id = guide["path_id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok());
                state.lock().unwrap().deprecated_path_id = dep_path_id;

                Ok(())
            }
        }),
        trial(&rt, "24_post_invalid_tarball", {
            let state = state.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let tarball = create_invalid_tarball();
                let (status, json) = post_ingest(&base_url, tarball, "").await;

                assert_eq!(
                    status, 422,
                    "invalid tarball should return 422: {status} {json}"
                );
                assert_eq!(
                    json["error"], "UNPROCESSABLE_ENTITY",
                    "error code should be 'UNPROCESSABLE_ENTITY': {json}"
                );
                let message = json["message"].as_str().unwrap_or("");
                assert!(
                    message.to_lowercase().contains("guide.toml"),
                    "error should mention guide.toml: {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "25_post_no_file_field", {
            let state = state.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let (status, json) = post_ingest_no_file(&base_url).await;

                assert!(
                    status == 400 || status == 422,
                    "no file field should return 400 or 422: {status} {json}"
                );
                let msg_field = if json["message"].is_string() {
                    json["message"].as_str().unwrap_or("")
                } else {
                    json["error"].as_str().unwrap_or("")
                };
                assert!(
                    msg_field.to_lowercase().contains("file"),
                    "error should mention missing file: {json}"
                );
                Ok(())
            }
        }),
        // ================================================================
        // Phase 9: Multi-Playbook Guide Tests
        // ================================================================
        trial(&rt, "26_post_multi_playbook_guide", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let tarball = create_multi_playbook_mixed_tarball(&suffix, "v1");
                let (status, json) = post_ingest(&base_url, tarball, "").await;

                assert_eq!(
                    status, 201,
                    "multi-playbook ingest should return 201: {json}"
                );
                assert_eq!(json["status"], "success");

                let guide = &json["guide"];
                let id = guide["id"]
                    .as_str()
                    .ok_or_else(|| format!("guide.id missing: {json}"))?;
                let path_id = guide["path_id"]
                    .as_str()
                    .ok_or_else(|| format!("guide.path_id missing: {json}"))?;

                // Check derived_objects.playbook_ids
                let playbook_ids: Vec<Uuid> = json["derived_objects"]["playbook_ids"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();

                // Chapter 2 (Concepts) has no stages — should produce exactly 2 playbooks
                assert_eq!(
                    playbook_ids.len(),
                    2,
                    "should have exactly 2 playbooks (prose-only chapter skipped): {json}"
                );

                let mut s = state.lock().unwrap();
                s.multi_pb_guide_id = Some(Uuid::parse_str(id).map_err(|e| e.to_string())?);
                s.multi_pb_path_id = Some(Uuid::parse_str(path_id).map_err(|e| e.to_string())?);
                s.multi_pb_playbook_ids = playbook_ids;
                Ok(())
            }
        }),
        trial(&rt, "27_verify_multi_playbook_structure", {
            let state = state.clone();
            move || async move {
                let (base_url, playbook_ids) = {
                    let s = state.lock().unwrap();
                    (s.base_url.clone(), s.multi_pb_playbook_ids.clone())
                };
                assert_eq!(playbook_ids.len(), 2, "expected 2 playbook ids");

                let ids_query: String = playbook_ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| format!("ids[{}]={}", i, id))
                    .collect::<Vec<_>>()
                    .join("&");

                let (status, json) = get_json(
                    &base_url,
                    &format!("/api/playbooks/?query=by_object_ids&{ids_query}"),
                )
                .await;
                assert_eq!(status, 200, "playbook query: {json}");

                let playbooks = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs missing: {json}"))?;
                assert_eq!(playbooks.len(), 2, "should return 2 playbooks: {json}");

                // Playbook 1: Search chapter — 1 stage, 2 steps
                let pb1 = &playbooks[0];
                let pb1_name = pb1["name"].as_str().unwrap_or("");
                assert!(
                    pb1_name.contains("Search"),
                    "playbook 1 name should contain 'Search': {pb1_name}"
                );
                let pb1_stages = pb1["stages"]
                    .as_array()
                    .ok_or_else(|| format!("pb1 stages missing: {json}"))?;
                assert_eq!(pb1_stages.len(), 1, "pb1 should have 1 stage: {json}");
                let pb1_steps = pb1_stages[0]["steps"]
                    .as_array()
                    .ok_or_else(|| format!("pb1 stage steps missing: {json}"))?;
                assert_eq!(pb1_steps.len(), 2, "pb1 stage should have 2 steps: {json}");

                // Playbook 2: Browse chapter — 1 stage, 3 steps
                let pb2 = &playbooks[1];
                let pb2_name = pb2["name"].as_str().unwrap_or("");
                assert!(
                    pb2_name.contains("Browse"),
                    "playbook 2 name should contain 'Browse': {pb2_name}"
                );
                let pb2_stages = pb2["stages"]
                    .as_array()
                    .ok_or_else(|| format!("pb2 stages missing: {json}"))?;
                assert_eq!(pb2_stages.len(), 1, "pb2 should have 1 stage: {json}");
                let pb2_steps = pb2_stages[0]["steps"]
                    .as_array()
                    .ok_or_else(|| format!("pb2 stage steps missing: {json}"))?;
                assert_eq!(pb2_steps.len(), 3, "pb2 stage should have 3 steps: {json}");

                // Verify naming includes guide title with em-dash suffix
                assert!(
                    pb1_name.contains("Multi PB Guide"),
                    "pb1 name should contain guide title: {pb1_name}"
                );
                assert!(
                    pb2_name.contains("Multi PB Guide"),
                    "pb2 name should contain guide title: {pb2_name}"
                );

                Ok(())
            }
        }),
        trial(&rt, "28_verify_multi_playbook_step_names", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let short = &suffix[..8];
                let (base_url, playbook_ids) = {
                    let s = state.lock().unwrap();
                    (s.base_url.clone(), s.multi_pb_playbook_ids.clone())
                };

                let pb_ids_query: String = playbook_ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| format!("ids[{}]={}", i, id))
                    .collect::<Vec<_>>()
                    .join("&");

                let (_, json) = get_json(
                    &base_url,
                    &format!("/api/playbooks/?query=by_object_ids&{pb_ids_query}"),
                )
                .await;

                let playbooks = json["objs"].as_array().ok_or("playbooks missing")?;

                let step_ids: Vec<&str> = playbooks
                    .iter()
                    .flat_map(|pb| {
                        pb["stages"].as_array().unwrap().iter().flat_map(|stage| {
                            stage["steps"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|ps| ps["step"]["id"].as_str().unwrap())
                        })
                    })
                    .collect();
                assert_eq!(step_ids.len(), 5, "expected 5 total step refs");

                let ids_query: String = step_ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| format!("ids[{}]={}", i, id))
                    .collect::<Vec<_>>()
                    .join("&");

                let (status, steps_json) = get_json(
                    &base_url,
                    &format!("/api/steps/?query=by_object_ids&{ids_query}"),
                )
                .await;
                assert_eq!(status, 200, "steps query: {steps_json}");

                let steps = steps_json["objs"]
                    .as_array()
                    .ok_or("objs should be array")?;
                assert_eq!(steps.len(), 5, "expected 5 step entities: {steps_json}");

                let mut step_names: Vec<&str> = steps
                    .iter()
                    .map(|s| s["name"].as_str().expect("step should have name"))
                    .collect();
                step_names.sort();

                let mut expected = [
                    format!("Search-1-{short}"),
                    format!("Search-2-{short}"),
                    format!("Browse-1-{short}"),
                    format!("Browse-2-{short}"),
                    format!("Browse-3-{short}"),
                ];
                expected.sort();
                let expected_refs: Vec<&str> = expected.iter().map(|s| s.as_str()).collect();
                assert_eq!(step_names, expected_refs, "step names mismatch");
                Ok(())
            }
        }),
        trial(&rt, "29_verify_guide_playbooks_junction", {
            let state = state.clone();
            move || async move {
                let (guide_id, playbook_ids) = {
                    let s = state.lock().unwrap();
                    (
                        s.multi_pb_guide_id.ok_or("multi_pb_guide_id not set")?,
                        s.multi_pb_playbook_ids.clone(),
                    )
                };

                // Direct DB query against rev.guide_playbooks
                dapple_data::load_env();
                let pool = consumer::test_db_pool();

                let rows = dapple_data::query::guide_playbooks::select_guide_playbooks(
                    pool.get_pool(),
                    guide_id,
                )
                .await
                .map_err(|e| format!("select_guide_playbooks failed: {e}"))?;

                assert_eq!(rows.len(), 2, "should have 2 guide_playbook rows");

                // Verify chapter_index values: 0 (Search) and 2 (Browse), NOT 0 and 1
                let mut chapter_indices: Vec<i16> = rows.iter().map(|r| r.chapter_index).collect();
                chapter_indices.sort();
                assert_eq!(
                    chapter_indices,
                    vec![0, 2],
                    "chapter indices should be 0 and 2 (Concepts chapter skipped)"
                );

                // Verify playbook IDs match those from the ingest response
                let mut junction_pb_ids: Vec<Uuid> = rows.iter().map(|r| r.playbook_id).collect();
                junction_pb_ids.sort();
                let mut expected_pb_ids = playbook_ids.clone();
                expected_pb_ids.sort();
                assert_eq!(
                    junction_pb_ids, expected_pb_ids,
                    "junction playbook_ids should match ingest response"
                );

                Ok(())
            }
        }),
        trial(&rt, "30_patch_multi_playbook_v2", {
            let state = state.clone();
            let suffix = suffix.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.multi_pb_path_id.ok_or("multi_pb_path_id not set")?,
                    )
                };

                let tarball = create_multi_playbook_mixed_tarball(&suffix, "v2");
                let (status, json) = patch_ingest(&base_url, path_id, tarball, "").await;

                assert_eq!(status, 201, "multi-pb PATCH v2 should return 201: {json}");
                assert_eq!(json["status"], "success");

                let v2_playbook_ids: Vec<Uuid> = json["derived_objects"]["playbook_ids"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();

                assert_eq!(
                    v2_playbook_ids.len(),
                    2,
                    "v2 should still have 2 playbooks: {json}"
                );

                Ok(())
            }
        }),
        trial(&rt, "31_verify_multi_playbook_get_by_path_id", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.multi_pb_path_id.ok_or("multi_pb_path_id not set")?,
                    )
                };

                let (status, json) = get_json(
                    &base_url,
                    &format!("/api/guides/?query=by_path_ids&path_ids[0]={path_id}"),
                )
                .await;

                assert_eq!(status, 200, "get by path_id should return 200: {json}");
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert_eq!(
                    objs.len(),
                    2,
                    "should return both v1 and v2 versions: {json}"
                );

                // Verify different content hashes
                let hashes: Vec<&str> = objs
                    .iter()
                    .filter_map(|g| g["content_hash"].as_str())
                    .collect();
                assert_eq!(hashes.len(), 2, "should have 2 content hashes");
                assert_ne!(
                    hashes[0], hashes[1],
                    "v1 and v2 should have different content hashes"
                );

                // Same path_id
                for obj in objs {
                    assert_eq!(
                        obj["path_id"].as_str().unwrap_or(""),
                        path_id.to_string(),
                        "all versions should share path_id: {json}"
                    );
                }

                Ok(())
            }
        }),
        // ================================================================
        // Phase 7: Unicode-identifier round-trip
        //
        // Migration 42 made `rev.slugify()` Unicode-preserving, and the
        // workspace-local `fluent-syntax-unicode` fork loosens the Fluent
        // identifier grammar to accept Latin-script diacritics in message
        // keys. These trials assert both layers ingest, slug-derive, and
        // round-trip an authored guide whose identifiers carry ç / ã / á.
        // ================================================================
        trial(&rt, "32_post_unicode_ingest", {
            let state = state.clone();
            move || async move {
                let base_url = state.lock().unwrap().base_url.clone();
                let unicode_suffix = Uuid::now_v7().to_string();
                let tarball = create_unicode_guide_tarball(&unicode_suffix, "v1");
                let (status, json) = post_ingest(&base_url, tarball, "").await;

                assert_eq!(status, 201, "unicode ingest should return 201: {json}");
                assert_eq!(
                    json["status"], "success",
                    "status should be success: {json}"
                );

                let guide = &json["guide"];
                let id = guide["id"]
                    .as_str()
                    .ok_or_else(|| format!("guide.id missing: {json}"))?;
                let path_id = guide["path_id"]
                    .as_str()
                    .ok_or_else(|| format!("guide.path_id missing: {json}"))?;
                let slug = guide["slug"]
                    .as_str()
                    .ok_or_else(|| format!("guide.slug missing: {json}"))?;
                let hash = guide["content_hash"]
                    .as_str()
                    .ok_or_else(|| format!("content_hash missing: {json}"))?;

                assert!(
                    slug.contains('ç') || slug.contains('í'),
                    "slug should preserve ç/í from 'Guía Esboçar …' title: {slug:?}"
                );

                let title = guide["title"].as_str().unwrap_or("");
                assert!(
                    title.starts_with("Guía Esboçar"),
                    "title should round-trip 'Guía Esboçar' Unicode: {title:?}"
                );

                let mut s = state.lock().unwrap();
                s.unicode_guide_id = Some(Uuid::parse_str(id).map_err(|e| e.to_string())?);
                s.unicode_path_id = Some(Uuid::parse_str(path_id).map_err(|e| e.to_string())?);
                s.unicode_slug = Some(slug.to_string());
                s.unicode_content_hash = Some(hash.to_string());
                s.unicode_suffix = Some(unicode_suffix.clone());
                Ok(())
            }
        }),
        trial(&rt, "33_get_unicode_by_slug", {
            let state = state.clone();
            move || async move {
                let (base_url, slug, guide_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.unicode_slug.clone().ok_or("unicode_slug not set")?,
                        s.unicode_guide_id.ok_or("unicode_guide_id not set")?,
                    )
                };

                // Percent-encode the slug's non-ASCII bytes via reqwest's URL
                // builder (reqwest::Url::parse_with_params doesn't apply to a
                // path-style query param list, so we hand the encoding via
                // url::form_urlencoded).
                use url::form_urlencoded::byte_serialize;
                let encoded: String = byte_serialize(slug.as_bytes()).collect();
                let (status, json) = get_json(
                    &base_url,
                    &format!("/api/guides/?query=by_slugs&slugs[0]={encoded}"),
                )
                .await;
                assert_eq!(
                    status, 200,
                    "by_slugs lookup with Unicode slug should return 200: {json}"
                );
                let objs = json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("objs should be array: {json}"))?;
                assert_eq!(
                    objs.len(),
                    1,
                    "expected single guide for Unicode slug round-trip: {json}"
                );
                assert_eq!(
                    objs[0]["id"].as_str().unwrap_or(""),
                    guide_id.to_string(),
                    "by_slugs hit should match ingested guide: {json}"
                );
                assert_eq!(
                    objs[0]["slug"].as_str().unwrap_or(""),
                    slug,
                    "returned slug should be byte-identical Unicode: {json}"
                );
                Ok(())
            }
        }),
        trial(&rt, "34_verify_unicode_fluent_keys", {
            let state = state.clone();
            move || async move {
                let (base_url, guide_id) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.unicode_guide_id.ok_or("unicode_guide_id not set")?,
                    )
                };

                // 1. GET the guide and harvest the fluent_message_id from each block
                //    whose directive carried a Unicode identifier name.
                let (status, json) = get_json(&base_url, &format!("/api/guides/{guide_id}")).await;
                assert_eq!(status, 200, "GET guide should return 200: {json}");
                let obj = &json["obj"];
                let chapters = obj["chapters"]
                    .as_array()
                    .ok_or_else(|| format!("chapters missing: {json}"))?;

                let mut block_fm_ids: Vec<Uuid> = Vec::new();
                for chapter in chapters {
                    let blocks = match chapter["blocks"].as_array() {
                        Some(b) => b,
                        None => continue,
                    };
                    for block in blocks {
                        if let Some(fm_id) = block["fluent_message_id"].as_str()
                            && let Ok(id) = Uuid::parse_str(fm_id)
                        {
                            block_fm_ids.push(id);
                        }
                    }
                }
                assert!(
                    !block_fm_ids.is_empty(),
                    "expected at least one block with fluent_message_id: {json}"
                );

                // 2. Fetch the fluent-messages those blocks point at and assert
                //    the FTL key (`name`) survived byte-identical with diacritics.
                let ids_query: String = block_fm_ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| format!("ids[{}]={}", i, id))
                    .collect::<Vec<_>>()
                    .join("&");
                let (status, fm_json) = get_json(
                    &base_url,
                    &format!("/api/fluent-messages/?query=by_object_ids&{ids_query}"),
                )
                .await;
                assert_eq!(status, 200, "fluent-messages query: {fm_json}");
                let messages = fm_json["objs"]
                    .as_array()
                    .ok_or_else(|| format!("fluent-messages objs missing: {fm_json}"))?;

                let names: std::collections::HashSet<&str> =
                    messages.iter().filter_map(|m| m["name"].as_str()).collect();

                // Block-attached Unicode FTL keys (from `:::stage` / `:::step`).
                for expected in ["esboçar-estágio", "padrão-passo", "o-tráfego-passo"] {
                    assert!(
                        names.contains(expected),
                        "expected FTL key {expected:?} (Unicode) to round-trip; \
                         got names: {:?}",
                        names
                    );
                }
                Ok(())
            }
        }),
        trial(&rt, "35_patch_unicode_v2", {
            let state = state.clone();
            move || async move {
                let (base_url, path_id, v1_hash, unicode_suffix) = {
                    let s = state.lock().unwrap();
                    (
                        s.base_url.clone(),
                        s.unicode_path_id.ok_or("unicode_path_id not set")?,
                        s.unicode_content_hash
                            .clone()
                            .ok_or("unicode_content_hash not set")?,
                        s.unicode_suffix.clone().ok_or("unicode_suffix not set")?,
                    )
                };

                // Re-ingest as a TRUE body-only edit: reuse the v1 suffix so the
                // FTL keys (incl. unicode-msg-{suffix}), title, and classification
                // stay byte-identical; only `variant` ("v1"→"v2") flips, changing
                // chapter prose + every FTL message *value*. The new content hash
                // must then come from the Fluent-body→entity Merkle fold, not an
                // identifier change — which is the behavior this trial guards.
                let tarball = create_unicode_guide_tarball(&unicode_suffix, "v2");
                let (status, json) = patch_ingest(&base_url, path_id, tarball, "").await;
                assert_eq!(status, 201, "Unicode v2 PATCH should return 201: {json}");
                assert_eq!(
                    json["status"], "success",
                    "status should be success: {json}"
                );

                let new_hash = json["guide"]["content_hash"]
                    .as_str()
                    .ok_or_else(|| format!("content_hash missing: {json}"))?;
                assert_ne!(
                    new_hash, v1_hash,
                    "Unicode v2 should produce a new content hash: {json}"
                );

                let new_slug = json["guide"]["slug"].as_str().unwrap_or("");
                assert!(
                    new_slug.contains('ç') || new_slug.contains('í'),
                    "v2 slug should still carry Unicode: {new_slug:?}"
                );
                Ok(())
            }
        }),
    ];

    run(&args, trials).exit_if_failed();
}
