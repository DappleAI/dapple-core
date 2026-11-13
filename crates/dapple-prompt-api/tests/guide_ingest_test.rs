//! Integration tests for the guide tarball ingest endpoint.

mod consumer;
use consumer::{spawn_app, test_auth_header, test_auth_user_id, test_db_pool};

/// Create a minimal test tarball in memory with a unique suffix to avoid
/// unique-constraint collisions across test runs and concurrent tests.
fn create_minimal_test_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    // Add guide.toml
    let guide_toml = format!(
        r#"title = "Test Guide {suffix}"
description = "A minimal test guide"
author = "Test"
classification = "test-{suffix}""#
    );

    let mut header = tar::Header::new_gnu();
    header.set_path("guide/guide.toml").unwrap();
    header.set_size(guide_toml.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, guide_toml.as_bytes()).unwrap();

    // Add SUMMARY.md
    let summary = "# Summary\n\n- [Chapter 1](ch1.md)\n";
    let mut header = tar::Header::new_gnu();
    header.set_path("guide/SUMMARY.md").unwrap();
    header.set_size(summary.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, summary.as_bytes()).unwrap();

    // Add CONDUITS.md
    let conduits = "# Conduit Context\n\nTest guide frame.\n";
    let mut header = tar::Header::new_gnu();
    header.set_path("guide/CONDUITS.md").unwrap();
    header.set_size(conduits.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, conduits.as_bytes()).unwrap();

    // Add ch1.md
    let chapter = "# Chapter 1\n\nTest content.\n";
    let mut header = tar::Header::new_gnu();
    header.set_path("guide/ch1.md").unwrap();
    header.set_size(chapter.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, chapter.as_bytes()).unwrap();

    // Add locales/en.ftl with unique message key
    let fluent = format!("test-message-{suffix} = Test message content\n");
    let mut header = tar::Header::new_gnu();
    header.set_path("guide/locales/en.ftl").unwrap();
    header.set_size(fluent.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, fluent.as_bytes()).unwrap();

    archive.finish().unwrap();
    drop(archive); // Ensure encoder is flushed

    tarball
}

/// Create a test tarball with stage/step directives for playbook materialization tests.
/// Structure: 2 chapters — Introduction (prose only) + Workflow (both stages).
/// This produces a single playbook with 2 stages (Stage Alpha: 2 steps, Stage Beta: 3 steps).
fn create_playbook_test_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Playbook Test Guide {suffix}"
description = "Guide with stages and steps for playbook tests"
author = "Test"
classification = "test-{suffix}""#
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "\
# Summary

- [Introduction](ch1.md)
- [Workflow](ch2.md)
";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = "\
# Introduction

Just prose content.
";
    append_file(&mut archive, "guide/ch1.md", ch1);

    // Suffix all directive + fluent names so concurrent test runs (this fixture
    // is shared by two tests) never collide on the global *_path_details and
    // fluent_message name UNIQUE constraints. Every step references a nushell
    // task — ingest validation requires it.
    let short = &suffix[28..];
    let ch2 = format!(
        "# Workflow\n\n\
         :::stage {{ name: \"Stage Alpha {short}\" }}\n\n\
         :::step {{ name: \"Step A1 {short}\", task: \"task-a1-{short}\" }}\n\
         :::\n\n\
         :::step {{ name: \"Step A2 {short}\", task: \"task-a2-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::stage {{ name: \"Stage Beta {short}\" }}\n\n\
         :::step {{ name: \"Step B1 {short}\", task: \"task-b1-{short}\" }}\n\
         :::\n\n\
         :::step {{ name: \"Step B2 {short}\", task: \"task-b2-{short}\" }}\n\
         :::\n\n\
         :::step {{ name: \"Step B3 {short}\", task: \"task-b3-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-a1-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-a2-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-b1-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-b2-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-b3-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch2.md", &ch2);

    let fluent = format!(
        "stage-alpha-{short} = First stage of the test guide.\n\
         step-a1-{short} = First step in stage alpha.\n\
         step-a2-{short} = Second step in stage alpha.\n\
         stage-beta-{short} = Second stage of the test guide.\n\
         step-b1-{short} = First step in stage beta.\n\
         step-b2-{short} = Second step in stage beta.\n\
         step-b3-{short} = Third step in stage beta.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);

    tarball
}

/// Ingest a tarball and return the parsed JSON response.
async fn ingest_tarball(base_url: &str, tarball_bytes: Vec<u8>) -> serde_json::Value {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/guides/ingest", base_url))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("request failed");

    let status = response.status();
    let json: serde_json::Value = response.json().await.expect("invalid json");
    assert_eq!(status, 201, "ingest response: {json}");
    assert_eq!(json["status"], "success", "response status: {json}");
    json
}

/// Test dry-run validation of a guide tarball.
#[tokio::test]
async fn test_guide_ingest_dry_run() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball_bytes = create_minimal_test_tarball(&suffix);

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/guides/ingest?dry_run=true", base_url))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("request failed");

    let status = response.status();
    let json: serde_json::Value = response.json().await.expect("invalid json");

    assert_eq!(status, 200, "dry-run response: {json}");
    assert_eq!(json["valid"], true, "dry-run should be valid: {json}");
    assert!(
        json["warnings"].is_array(),
        "warnings should be array: {json}"
    );
    assert!(
        json["summary"]["chapters"].is_number(),
        "summary.chapters should be number: {json}"
    );
}

/// Test actual guide ingestion (not dry-run).
#[tokio::test]
async fn test_guide_ingest_full() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball_bytes = create_minimal_test_tarball(&suffix);

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/guides/ingest", base_url))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("request failed");

    let status = response.status();
    let json: serde_json::Value = response.json().await.expect("invalid json");

    assert_eq!(status, 201, "full ingest response: {json}");
    assert_eq!(json["status"], "success", "response status: {json}");
    assert!(
        json["guide"]["id"].is_string(),
        "guide should have id: {json}"
    );
    assert!(
        json["guide"]["title"]
            .as_str()
            .unwrap()
            .starts_with("Test Guide"),
        "guide title: {json}"
    );
}

/// Test that ingesting a guide with stages and steps materializes a single playbook
/// with the correct structure: 2 stages, 2+3 steps, correct step names.
#[tokio::test]
async fn test_guide_ingest_playbook_stages_and_steps() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball_bytes = create_playbook_test_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    // Extract playbook_ids from derived_objects
    let playbook_ids: Vec<&str> = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("derived_objects.playbook_ids should be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // Only one chapter has stages, so we get exactly 1 playbook
    assert_eq!(
        playbook_ids.len(),
        1,
        "should have 1 playbook (only ch2 has stages)"
    );

    // Query the playbook via GET /api/playbooks
    let client = reqwest::Client::new();
    let playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&ids[0]={}",
        base_url, playbook_ids[0]
    );
    let resp = client
        .get(&playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("playbook query failed");

    let resp_status = resp.status();
    let resp_text = resp.text().await.expect("failed to read playbook response");
    assert!(
        resp_status.is_success(),
        "playbook query returned {resp_status}: {resp_text}"
    );
    let playbook_json: serde_json::Value =
        serde_json::from_str(&resp_text).expect("invalid playbook json");
    let playbooks = playbook_json["objs"]
        .as_array()
        .expect("objs should be array");
    assert_eq!(playbooks.len(), 1, "expected 1 playbook: {playbook_json}");

    let playbook = &playbooks[0];

    // The single playbook should have 2 stages (Stage Alpha and Stage Beta)
    let stages = playbook["stages"]
        .as_array()
        .expect("playbook stages should be array");
    assert_eq!(
        stages.len(),
        2,
        "playbook should have 2 stages: {playbook_json}"
    );

    // Stage Alpha (stage 1) has 2 steps
    let stage1_steps = stages[0]["steps"]
        .as_array()
        .expect("stage 1 steps should be array");
    assert_eq!(
        stage1_steps.len(),
        2,
        "Stage Alpha should have 2 steps: {playbook_json}"
    );

    // Stage Beta (stage 2) has 3 steps
    let stage2_steps = stages[1]["steps"]
        .as_array()
        .expect("stage 2 steps should be array");
    assert_eq!(
        stage2_steps.len(),
        3,
        "Stage Beta should have 3 steps: {playbook_json}"
    );

    // Collect all 5 step ids from the playbook
    let step_ids: Vec<&str> = stages
        .iter()
        .flat_map(|stage| {
            stage["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|ps| ps["step"]["id"].as_str().unwrap())
        })
        .collect();
    assert_eq!(step_ids.len(), 5, "expected 5 total step refs");

    // Query step entities to verify names
    let step_ids_query: String = step_ids
        .iter()
        .enumerate()
        .map(|(i, id)| format!("ids[{}]={}", i, id))
        .collect::<Vec<_>>()
        .join("&");
    let steps_url = format!(
        "{}/api/steps/?query=by_object_ids&{}",
        base_url, step_ids_query
    );
    let resp = client
        .get(&steps_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("steps query failed");

    let resp_status = resp.status();
    let resp_text = resp.text().await.expect("failed to read steps response");
    assert!(
        resp_status.is_success(),
        "steps query returned {resp_status}: {resp_text}"
    );
    let steps_json: serde_json::Value =
        serde_json::from_str(&resp_text).expect("invalid steps json");
    let steps = steps_json["objs"].as_array().expect("objs should be array");
    assert_eq!(steps.len(), 5, "expected 5 step entities: {steps_json}");

    let mut step_names: Vec<String> = steps
        .iter()
        .map(|s| {
            s["name"]
                .as_str()
                .expect("step should have name")
                .to_string()
        })
        .collect();
    step_names.sort();

    let short = &suffix[28..];
    let mut expected_names = vec![
        format!("Step A1 {short}"),
        format!("Step A2 {short}"),
        format!("Step B1 {short}"),
        format!("Step B2 {short}"),
        format!("Step B3 {short}"),
    ];
    expected_names.sort();
    assert_eq!(step_names, expected_names, "step names mismatch");
}

/// Test that the playbook name matches the guide title exactly (no em-dash suffix)
/// and that step entities carry the correct names (not synthetic auto-generated names).
#[tokio::test]
async fn test_guide_ingest_playbook_naming() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball_bytes = create_playbook_test_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    let playbook_ids: Vec<&str> = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("derived_objects.playbook_ids should be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // Only one chapter has stages, so we get exactly 1 playbook
    assert_eq!(
        playbook_ids.len(),
        1,
        "should have 1 playbook (only ch2 has stages)"
    );

    // Query the playbook and verify its name
    let client = reqwest::Client::new();
    let playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&ids[0]={}",
        base_url, playbook_ids[0]
    );
    let resp = client
        .get(&playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("playbook query failed");

    let resp_status = resp.status();
    let resp_text = resp.text().await.expect("failed to read playbook response");
    assert!(
        resp_status.is_success(),
        "playbook query returned {resp_status}: {resp_text}"
    );
    let playbook_json: serde_json::Value =
        serde_json::from_str(&resp_text).expect("invalid playbook json");
    let playbooks = playbook_json["objs"]
        .as_array()
        .expect("objs should be array");
    assert_eq!(playbooks.len(), 1, "expected 1 playbook: {playbook_json}");

    let playbook = &playbooks[0];

    // Single-playbook: name = guide title exactly (no em-dash suffix)
    let expected_name = format!("Playbook Test Guide {suffix}");
    assert_eq!(
        playbook["name"].as_str().unwrap(),
        expected_name,
        "playbook name should match guide title exactly (no suffix)"
    );

    // Verify slug is kebab-case
    let slug = playbook["slug"].as_str().unwrap();
    assert!(
        slug.starts_with("playbook-test-guide-"),
        "playbook slug should be kebab-case: {slug}"
    );

    // Verify content_hash is present
    let hash = playbook["content_hash"].as_str().unwrap();
    assert_eq!(
        hash.len(),
        40,
        "playbook content_hash should be 40 hex chars: {hash}"
    );
}

/// Create a test tarball with unique suffixed fluent message keys for orphan testing.
/// The suffix is baked into directive names and .ftl keys so they don't collide
/// with other test runs or existing DB data.
fn create_orphan_test_tarball(suffix: &str) -> (Vec<u8>, Vec<String>) {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    // Use a short hash from the suffix for readable but unique names
    let short = &suffix[28..];

    // These are the fluent message keys that will be in the tarball
    let stage_key = format!("stage-{short}");
    let step_key = format!("step-{short}");
    let orphan_keys = vec![stage_key.clone(), step_key.clone()];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Orphan Test Guide {suffix}"
description = "Guide for testing orphaned fluent path details"
author = "Test"
classification = "test-{suffix}""#
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Chapter](ch1.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Chapter\n\n\
         :::stage {{ name: \"Stage {short}\" }}\n\n\
         :::step {{ name: \"Step {short}\", task: \"task-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n\
         {{ payload: {{}}, signals: {{}} }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "{stage_key} = Stage description for orphan test.\n\
         {step_key} = Step description for orphan test.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);

    (tarball, orphan_keys)
}

/// Test that guide ingest succeeds when orphaned fluent_message_path_details exist
/// (path_details rows without corresponding versioned_objects or fluent_messages).
/// This reproduces the "duplicate key value violates unique constraint
/// fluent_message_path_details_name_key" bug that occurred when the dedup query
/// used INNER JOINs and missed orphaned path_details.
#[tokio::test]
async fn test_guide_ingest_orphaned_fluent_path_details() {
    let (base_url, _handle) = spawn_app().await;

    // Connect directly to the database to inject orphaned path_details
    dapple_data::load_env();
    let pool = test_db_pool();
    let sql_pool = pool.get_pool();

    // Use a unique suffix so this test is isolated from others
    let suffix = uuid::Uuid::now_v7().to_string();
    let (tarball_bytes, orphan_names) = create_orphan_test_tarball(&suffix);

    // Create orphaned path_details: object_paths + fluent_message_path_details
    // WITHOUT versioned_objects or fluent_messages.
    //
    // Migrations 33 + 41 made `namespace_company_id` NOT NULL on both
    // object_paths and fluent_message_path_details. Seed them to the dapple
    // UUID so this orphan-injection test continues to model orphan rows in
    // the same namespace the ingest will run against.
    let _orphan_rows = sqlx::query(
        "WITH et AS (
            SELECT id FROM rev.entity_types WHERE key = 'fluent_message'
        ),
        new_paths AS (
            INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope)
            SELECT et.id,
                   '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID,
                   'public'
            FROM et, UNNEST($1::TEXT[]) WITH ORDINALITY AS n(name, ord)
            ORDER BY ord
            RETURNING id, namespace_company_id
        ),
        numbered AS (
            SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord FROM new_paths np
        )
        INSERT INTO rev.fluent_message_path_details (path_id, name, namespace_company_id)
        SELECT numbered.id, n.name, numbered.namespace_company_id
        FROM UNNEST($1::TEXT[]) WITH ORDINALITY AS n(name, ord)
        JOIN numbered ON numbered.ord = n.ord
        RETURNING path_id, name",
    )
    .bind(&orphan_names)
    .fetch_all(sql_pool)
    .await
    .expect("failed to inject orphaned path_details");

    // Now ingest the tarball which uses these same fluent message names.
    // Before the fix, this would fail with a unique constraint violation because the
    // dedup INNER JOIN query missed the orphaned path_details, so the INSERT tried to
    // create new path_details with the same name.
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    // Verify the guide was created successfully
    assert!(
        json["guide"]["id"].is_string(),
        "guide should have id: {json}"
    );

    // Verify the derived playbook exists
    let playbook_ids = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("derived_objects.playbook_ids should be array");
    assert!(!playbook_ids.is_empty(), "playbook_ids should not be empty");
}

/// Create a single-chapter tarball with 2 stages (backward compat: 1 playbook, no name suffix).
fn create_single_playbook_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Single PB Guide {suffix}"
description = "Single chapter with multiple stages"
author = "Test"
classification = "test-{suffix}""#
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Workflow](ch1.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = "\
# Workflow

:::stage { name: \"Stage X\" }

:::step { name: \"Step X1\", task: \"task-x1\" }
:::

:::

:::stage { name: \"Stage Y\" }

:::step { name: \"Step Y1\", task: \"task-y1\" }
:::

:::step { name: \"Step Y2\", task: \"task-y2\" }
:::

:::

:::nushell { name: \"task-x1\" }
{ payload: {}, signals: {} }
:::

:::nushell { name: \"task-y1\" }
{ payload: {}, signals: {} }
:::

:::nushell { name: \"task-y2\" }
{ payload: {}, signals: {} }
:::
";
    append_file(&mut archive, "guide/ch1.md", ch1);

    let fluent = "\
stage-x = Stage X description.
step-x1 = Step X1 description.
stage-y = Stage Y description.
step-y1 = Step Y1 description.
step-y2 = Step Y2 description.
";
    append_file(&mut archive, "guide/locales/en.ftl", fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Create a multi-playbook tarball with 3 chapters (2 with stages, 1 prose-only).
fn create_multi_playbook_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Multi PB Ingest Guide {suffix}"
description = "Multi-playbook test with mixed chapters"
author = "Test"
classification = "test-{suffix}""#
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "\
# Summary

- [Search](ch1.md)
- [Concepts](ch2.md)
- [Browse](ch3.md)
";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // Suffix stage/step names to avoid content-hash collisions with prior test runs.
    // Since stage_steps are global (belong to the stage, not the playbook), reusing
    // a stage from a previous run would inherit its old stage_steps.
    let short = &suffix[..8];

    let ch1 = format!(
        "# Search\n\n:::stage {{ name: \"Search Stage {short}\" }}\n\n:::step {{ name: \"Find-1-{short}\", task: \"find-1-task-{short}\" }}\n:::\n\n:::step {{ name: \"Find-2-{short}\", task: \"find-2-task-{short}\" }}\n:::\n\n:::\n\n:::nushell {{ name: \"find-1-task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n:::nushell {{ name: \"find-2-task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let ch2 = "\
# Concepts

Just prose — no stages here.
";
    append_file(&mut archive, "guide/ch2.md", ch2);

    let ch3 = format!(
        "# Browse\n\n:::stage {{ name: \"Browse Stage {short}\" }}\n\n:::step {{ name: \"List-1-{short}\", task: \"list-1-task-{short}\" }}\n:::\n\n:::\n\n:::nushell {{ name: \"list-1-task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch3.md", &ch3);

    let fluent = format!(
        "search-stage-{short} = Search stage description.\nfind-1-{short} = Find step 1.\nfind-2-{short} = Find step 2.\nbrowse-stage-{short} = Browse stage description.\nlist-1-{short} = List step 1.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Test that a single-chapter guide produces exactly 1 playbook with no name suffix.
#[tokio::test]
async fn test_guide_ingest_single_chapter_single_playbook() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball_bytes = create_single_playbook_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    let playbook_ids: Vec<&str> = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("playbook_ids should be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // Single chapter with stages → 1 playbook
    assert_eq!(
        playbook_ids.len(),
        1,
        "should have exactly 1 playbook: {json}"
    );

    // Query the playbook
    let client = reqwest::Client::new();
    let playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&ids[0]={}",
        base_url, playbook_ids[0]
    );
    let resp = client
        .get(&playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("playbook query failed");

    let resp_text = resp.text().await.expect("failed to read response");
    let playbook_json: serde_json::Value = serde_json::from_str(&resp_text).expect("invalid json");
    let playbooks = playbook_json["objs"]
        .as_array()
        .expect("objs should be array");
    assert_eq!(playbooks.len(), 1, "expected 1 playbook");

    let playbook = &playbooks[0];

    // Single-chapter playbook name = guide title (no chapter suffix)
    let expected_name = format!("Single PB Guide {suffix}");
    assert_eq!(
        playbook["name"].as_str().unwrap(),
        expected_name,
        "single-playbook name should match guide title (no suffix)"
    );

    // 2 stages, 3 total steps
    let stages = playbook["stages"]
        .as_array()
        .expect("stages should be array");
    assert_eq!(stages.len(), 2, "should have 2 stages");

    let stage1_steps = stages[0]["steps"].as_array().unwrap();
    let stage2_steps = stages[1]["steps"].as_array().unwrap();
    assert_eq!(stage1_steps.len(), 1, "stage X should have 1 step");
    assert_eq!(stage2_steps.len(), 2, "stage Y should have 2 steps");
}

/// Create a guide with ONE stage / ONE step that references THREE ordered tasks
/// via `tasks: [...]`. Exercises the step-as-ordered-task-set materialization.
fn create_multitask_step_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Multitask Step Guide {suffix}"
description = "One step, three ordered tasks"
author = "Test"
classification = "test-{suffix}""#
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Workflow](ch1.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::stage {{ name: \"Pipeline {short}\" }}\n\n\
         :::step {{ name: \"Coherent {short}\", tasks: [\"get-{short}\", \"retrieve-{short}\", \"mangle-{short}\"] }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"get-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"retrieve-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"mangle-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!("pipeline-{short} = Pipeline stage.\ncoherent-{short} = Coherent step.\n");
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// A step authored with `tasks: [a, b, c]` materializes THREE `step_tasks` in
/// authored (`ord`) order — the step-as-ordered-task-set contract end to end.
#[tokio::test]
async fn test_guide_ingest_multitask_step_ordered() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let tarball_bytes = create_multitask_step_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    let playbook_ids = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("playbook_ids should be array");
    assert_eq!(playbook_ids.len(), 1, "expected 1 playbook: {json}");

    // Verify in SQL: the step's step_tasks are the three authored tasks, in ord order.
    let pool = get_db_pool().await;
    let step_slug = format!("coherent-{short}");
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT tpd.slug
         FROM rev.steps s
         JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
         JOIN rev.object_paths sop ON sop.id = svo.path_id
         JOIN rev.step_path_details spd ON spd.path_id = sop.id
         JOIN rev.step_tasks st ON st.step_id = s.id
         JOIN rev.tasks t ON t.id = st.task_id
         JOIN rev.versioned_objects tvo ON tvo.id = t.versioned_object_id
         JOIN rev.object_paths top2 ON top2.id = tvo.path_id
         JOIN rev.task_path_details tpd ON tpd.path_id = top2.id
         WHERE spd.slug = $1
         ORDER BY st.ord",
    )
    .bind(&step_slug)
    .fetch_all(&pool)
    .await
    .expect("query step_tasks");

    let slugs: Vec<String> = rows.into_iter().map(|(s,)| s).collect();
    assert_eq!(
        slugs,
        vec![
            format!("get-{short}"),
            format!("retrieve-{short}"),
            format!("mangle-{short}"),
        ],
        "step must materialize its three tasks in authored ord order"
    );
}

/// Create a guide whose step NESTS its three :::nushell tasks as children (no
/// `tasks:` attribute). Exercises step-scoped nested-task authoring.
fn create_nested_step_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Nested Step Guide {suffix}"
description = "One step nesting three tasks"
author = "Test"
classification = "test-{suffix}""#
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Workflow](ch1.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::stage {{ name: \"Pipeline {short}\" }}\n\n\
         :::step {{ name: \"Coherent {short}\" }}\n\n\
         :::nushell {{ name: \"get-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"retrieve-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"mangle-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::\n\n\
         :::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!("pipeline-{short} = Pipeline stage.\ncoherent-{short} = Coherent step.\n");
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// A step that NESTS its `:::nushell` tasks as children materializes those tasks
/// as step_tasks in document order, with names step-scoped (`<step>/<task>`) so
/// they can't collide across steps. Nested authoring is the linkage path that
/// best teaches "group a step's tasks together".
#[tokio::test]
async fn test_guide_ingest_nested_step_tasks_ordered() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let tarball_bytes = create_nested_step_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    let playbook_ids = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("playbook_ids should be array");
    assert_eq!(playbook_ids.len(), 1, "expected 1 playbook: {json}");

    // Verify in SQL: the step's step_tasks are the three nested tasks, in ord
    // order, with names scoped to the step.
    let pool = get_db_pool().await;
    let step_slug = format!("coherent-{short}");
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT tpd.name
         FROM rev.steps s
         JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
         JOIN rev.object_paths sop ON sop.id = svo.path_id
         JOIN rev.step_path_details spd ON spd.path_id = sop.id
         JOIN rev.step_tasks st ON st.step_id = s.id
         JOIN rev.tasks t ON t.id = st.task_id
         JOIN rev.versioned_objects tvo ON tvo.id = t.versioned_object_id
         JOIN rev.object_paths top2 ON top2.id = tvo.path_id
         JOIN rev.task_path_details tpd ON tpd.path_id = top2.id
         WHERE spd.slug = $1
         ORDER BY st.ord",
    )
    .bind(&step_slug)
    .fetch_all(&pool)
    .await
    .expect("query step_tasks");

    let names: Vec<String> = rows.into_iter().map(|(n,)| n).collect();
    assert_eq!(
        names,
        vec![
            format!("Coherent {short}/get-{short}"),
            format!("Coherent {short}/retrieve-{short}"),
            format!("Coherent {short}/mangle-{short}"),
        ],
        "nested tasks must materialize step-scoped, in document order"
    );
}

// ============================================================================
// Backfill test helpers
// ============================================================================

/// Create a backfill test tarball with:
/// - guide.toml with [card] (color/accent)
/// - 2 chapters: ch1.md (prose block + step block with tag "universal"), ch2.md (stage block)
/// - locales/en.ftl with keyed messages for all named blocks
///
/// All names embed `&suffix[28..]` for isolation.
fn create_backfill_test_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Backfill Guide {suffix}\"\n\
         description = \"Guide for backfill repair tests\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n\
         \n\
         [card]\n\
         color = \"#2D6A4F\"\n\
         accent = \"#95D5B2\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Chapter 1](ch1.md)\n- [Chapter 2](ch2.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // ch1: prose block (unnamed, inline) + step block (named, fluent-backed, tagged "universal")
    let ch1 = format!(
        "# Chapter 1\n\n\
         Some prose content for chapter 1.\n\n\
         :::step {{ name: \"step-{short}\", tags: [\"universal\"] }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    // ch2: stage block (named, fluent-backed) with a step that references a
    // nushell task (every playbook step must have a task — see ingest validation).
    let ch2 = format!(
        "# Chapter 2\n\n\
         :::stage {{ name: \"stage-{short}\" }}\n\n\
         :::step {{ name: \"inner-step-{short}\", task: \"inner-task-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"inner-task-{short}\" }}\n\
         {{ payload: {{}}, signals: {{}} }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/ch2.md", &ch2);

    let fluent = format!(
        "step-{short} = Step description for backfill test.\n\
         stage-{short} = Stage description for backfill test.\n\
         inner-step-{short} = Inner step description for backfill test.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);

    tarball
}

/// Helper: PATCH ingest a tarball with optional backfill flag.
async fn ingest_tarball_patch(
    base_url: &str,
    path_id: &str,
    tarball_bytes: Vec<u8>,
    backfill: bool,
) -> (reqwest::StatusCode, serde_json::Value) {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let mut url = format!("{}/api/guides/{}/ingest", base_url, path_id);
    if backfill {
        url.push_str("?backfill=true");
    }

    let client = reqwest::Client::new();
    let response = client
        .patch(&url)
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("request failed");

    let status = response.status();
    let json: serde_json::Value = response.json().await.expect("invalid json");
    (status, json)
}

/// Helper: initial POST ingest, returns (path_id, guide_id).
async fn initial_ingest(base_url: &str, tarball_bytes: Vec<u8>) -> (String, String) {
    let json = ingest_tarball(base_url, tarball_bytes).await;
    let path_id = json["guide"]["path_id"]
        .as_str()
        .expect("guide.path_id")
        .to_string();
    let guide_id = json["guide"]["id"].as_str().expect("guide.id").to_string();
    (path_id, guide_id)
}

/// Get a direct DB pool for corruption operations.
async fn get_db_pool() -> sqlx::PgPool {
    dapple_data::load_env();
    test_db_pool().get_pool().clone()
}

// ============================================================================
// Backfill tests
// ============================================================================

/// Test 1: Backfill with no corruption — all counts should be 0.
#[tokio::test]
async fn test_backfill_noop() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, _guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;

    assert_eq!(status, 200, "refresh response: {json}");
    assert_eq!(json["status"], "refreshed", "response: {json}");
    assert_eq!(json["blocks_added"], 0, "no blocks should be added: {json}");
    assert_eq!(
        json["chapters_repaired"], 0,
        "no chapters should be repaired: {json}"
    );
    assert_eq!(
        json["tags_repaired"], 0,
        "no tags should be repaired: {json}"
    );
    assert_eq!(
        json["fluent_links_repaired"], 0,
        "no fluent links should be repaired: {json}"
    );
    assert_eq!(
        json["card_repaired"], false,
        "card should not be repaired: {json}"
    );
}

/// Test 2: Delete 1 block from ch1, backfill should re-add it.
#[tokio::test]
async fn test_backfill_missing_blocks() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Find a block in ch1 (the step block)
    let short = &suffix[28..];
    let step_name = format!("step-{short}");
    let block_row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT gb.id FROM rev.guide_blocks gb
         JOIN rev.guide_chapters gc ON gc.id = gb.chapter_id
         WHERE gc.guide_id = $1 AND gb.name = $2",
    )
    .bind(guide_uuid)
    .bind(&step_name)
    .fetch_one(&db)
    .await
    .expect("block not found");

    // Delete embeddings, tags, then block (FK order)
    sqlx::query("DELETE FROM rev.guide_content_embeddings WHERE block_id = $1")
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM rev.guide_block_tags WHERE block_id = $1")
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM rev.guide_blocks WHERE id = $1")
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");
    assert!(
        json["blocks_added"].as_u64().unwrap() >= 1,
        "should add missing block: {json}"
    );
}

/// Test 3: Delete ALL blocks from ch1, backfill should re-add them all.
#[tokio::test]
async fn test_backfill_all_blocks_deleted() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Get ch1 chapter id
    let ch1_row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT id FROM rev.guide_chapters WHERE guide_id = $1 AND source_file = 'ch1.md'",
    )
    .bind(guide_uuid)
    .fetch_one(&db)
    .await
    .expect("ch1 not found");

    // Count blocks in ch1 before deletion
    let count_row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM rev.guide_blocks WHERE chapter_id = $1")
            .bind(ch1_row.0)
            .fetch_one(&db)
            .await
            .unwrap();
    let original_block_count = count_row.0 as u64;
    assert!(original_block_count > 0, "ch1 should have blocks");

    // Delete embeddings first (if any), then tags, then blocks (FK order)
    sqlx::query(
        "DELETE FROM rev.guide_content_embeddings WHERE block_id IN
         (SELECT id FROM rev.guide_blocks WHERE chapter_id = $1)",
    )
    .bind(ch1_row.0)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM rev.guide_block_tags WHERE block_id IN
         (SELECT id FROM rev.guide_blocks WHERE chapter_id = $1)",
    )
    .bind(ch1_row.0)
    .execute(&db)
    .await
    .unwrap();
    let del_result = sqlx::query("DELETE FROM rev.guide_blocks WHERE chapter_id = $1")
        .bind(ch1_row.0)
        .execute(&db)
        .await
        .unwrap();
    assert!(del_result.rows_affected() > 0, "should have deleted blocks");

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");
    let blocks_added = json["blocks_added"].as_u64().unwrap();
    assert_eq!(
        blocks_added, original_block_count,
        "should re-add all blocks: {json}"
    );
}

/// Test 4: Delete chapter 2 entirely (tags→blocks→chapter), backfill should re-create it.
#[tokio::test]
async fn test_backfill_missing_chapter() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Get ch2 chapter id
    let ch2_row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT id FROM rev.guide_chapters WHERE guide_id = $1 AND source_file = 'ch2.md'",
    )
    .bind(guide_uuid)
    .fetch_one(&db)
    .await
    .expect("ch2 not found");

    // Delete in FK order: embeddings → tags → blocks → chapter
    sqlx::query(
        "DELETE FROM rev.guide_content_embeddings WHERE block_id IN
         (SELECT id FROM rev.guide_blocks WHERE chapter_id = $1)",
    )
    .bind(ch2_row.0)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM rev.guide_block_tags WHERE block_id IN
         (SELECT id FROM rev.guide_blocks WHERE chapter_id = $1)",
    )
    .bind(ch2_row.0)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("DELETE FROM rev.guide_blocks WHERE chapter_id = $1")
        .bind(ch2_row.0)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM rev.guide_chapters WHERE id = $1")
        .bind(ch2_row.0)
        .execute(&db)
        .await
        .unwrap();

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");
    assert_eq!(
        json["chapters_repaired"], 1,
        "should repair 1 chapter: {json}"
    );
}

/// Test 5: Delete tags from ch1 blocks, backfill should re-add tag associations.
#[tokio::test]
async fn test_backfill_missing_block_tags() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Find blocks in ch1 that have tags
    let tagged_blocks: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT DISTINCT gbt.block_id FROM rev.guide_block_tags gbt
         JOIN rev.guide_blocks gb ON gb.id = gbt.block_id
         JOIN rev.guide_chapters gc ON gc.id = gb.chapter_id
         WHERE gc.guide_id = $1 AND gc.source_file = 'ch1.md'",
    )
    .bind(guide_uuid)
    .fetch_all(&db)
    .await
    .unwrap();

    assert!(
        !tagged_blocks.is_empty(),
        "should have tagged blocks in ch1"
    );

    // Delete all tags from ch1 blocks
    let block_ids: Vec<uuid::Uuid> = tagged_blocks.iter().map(|r| r.0).collect();
    for bid in &block_ids {
        sqlx::query("DELETE FROM rev.guide_block_tags WHERE block_id = $1")
            .bind(bid)
            .execute(&db)
            .await
            .unwrap();
    }

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");
    assert!(
        json["tags_repaired"].as_u64().unwrap() >= 1,
        "should repair tags: {json}"
    );
    assert_eq!(json["blocks_added"], 0, "no blocks should be added: {json}");
}

/// Test 6: Delete guide card, backfill should re-create it.
#[tokio::test]
async fn test_backfill_missing_guide_card() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Delete guide card
    let deleted = sqlx::query("DELETE FROM rev.guide_cards WHERE guide_id = $1")
        .bind(guide_uuid)
        .execute(&db)
        .await
        .unwrap();
    assert!(
        deleted.rows_affected() > 0,
        "should have deleted a guide card"
    );

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");
    assert_eq!(
        json["card_repaired"], true,
        "card should be repaired: {json}"
    );
}

/// Test 7: NULL out fluent_message_id on a block, backfill should repair it.
#[tokio::test]
async fn test_backfill_missing_fluent_link() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Find a block with a fluent_message_id
    let block_row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT gb.id FROM rev.guide_blocks gb
         JOIN rev.guide_chapters gc ON gc.id = gb.chapter_id
         WHERE gc.guide_id = $1 AND gb.fluent_message_id IS NOT NULL
         LIMIT 1",
    )
    .bind(guide_uuid)
    .fetch_one(&db)
    .await
    .expect("no block with fluent_message_id found");

    // NULL out the fluent_message_id
    sqlx::query("UPDATE rev.guide_blocks SET fluent_message_id = NULL WHERE id = $1")
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");
    assert!(
        json["fluent_links_repaired"].as_u64().unwrap() >= 1,
        "should repair fluent link: {json}"
    );
}

/// Test 8: Insert a rogue block into ch1, backfill should reject with 409 CONFLICT.
#[tokio::test]
async fn test_backfill_rejects_extra_db_block() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Get ch1 chapter id and a directive_type_id
    let ch1_row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT id FROM rev.guide_chapters WHERE guide_id = $1 AND source_file = 'ch1.md'",
    )
    .bind(guide_uuid)
    .fetch_one(&db)
    .await
    .expect("ch1 not found");

    let dt_row: (i16,) = sqlx::query_as(
        "SELECT id FROM rev.guide_block_directive_types WHERE key = 'prose' LIMIT 1",
    )
    .fetch_one(&db)
    .await
    .expect("prose directive type not found");

    // Insert rogue block
    sqlx::query(
        "INSERT INTO rev.guide_blocks (id, chapter_id, directive_type_id, name, content, attributes)
         VALUES (uuidv7(), $1, $2, 'rogue-block', 'rogue content', '{}'::jsonb)",
    )
    .bind(ch1_row.0)
    .bind(dt_row.0)
    .execute(&db)
    .await
    .unwrap();

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 409, "should reject with conflict: {json}");
    let msg = json["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("rogue-block"),
        "error should mention the rogue block: {json}"
    );
}

/// Test 9: Corrupt a block's directive_type_id, backfill should reject with 409 CONFLICT
/// (because the block identity won't match).
#[tokio::test]
async fn test_backfill_rejects_corrupted_directive_type() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Find the step block in ch1
    let short = &suffix[28..];
    let step_name = format!("step-{short}");
    let block_row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT gb.id FROM rev.guide_blocks gb
         JOIN rev.guide_chapters gc ON gc.id = gb.chapter_id
         WHERE gc.guide_id = $1 AND gb.name = $2",
    )
    .bind(guide_uuid)
    .bind(&step_name)
    .fetch_one(&db)
    .await
    .expect("step block not found");

    // Get a different directive_type_id (use 'prose' if the block was 'step')
    let other_dt: (i16,) = sqlx::query_as(
        "SELECT id FROM rev.guide_block_directive_types WHERE key = 'prose' LIMIT 1",
    )
    .fetch_one(&db)
    .await
    .unwrap();

    // Corrupt the block's directive_type_id
    sqlx::query("UPDATE rev.guide_blocks SET directive_type_id = $1 WHERE id = $2")
        .bind(other_dt.0)
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();

    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(
        status, 409,
        "should reject corrupted directive type: {json}"
    );
}

/// Test 10: Delete a block, backfill once (repairs), backfill again (no-op). Idempotent.
#[tokio::test]
async fn test_backfill_idempotent() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_backfill_test_tarball(&suffix);

    let (path_id, guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    // Delete a block from ch1
    let short = &suffix[28..];
    let step_name = format!("step-{short}");
    let block_row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT gb.id FROM rev.guide_blocks gb
         JOIN rev.guide_chapters gc ON gc.id = gb.chapter_id
         WHERE gc.guide_id = $1 AND gb.name = $2",
    )
    .bind(guide_uuid)
    .bind(&step_name)
    .fetch_one(&db)
    .await
    .expect("step block not found");

    sqlx::query("DELETE FROM rev.guide_content_embeddings WHERE block_id = $1")
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM rev.guide_block_tags WHERE block_id = $1")
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM rev.guide_blocks WHERE id = $1")
        .bind(block_row.0)
        .execute(&db)
        .await
        .unwrap();

    // First backfill: should repair
    let (status1, json1) = ingest_tarball_patch(&base_url, &path_id, tarball.clone(), true).await;
    assert_eq!(status1, 200, "first backfill: {json1}");
    assert!(
        json1["blocks_added"].as_u64().unwrap() >= 1,
        "first backfill should add blocks: {json1}"
    );

    // Second backfill: should be a no-op
    let (status2, json2) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status2, 200, "second backfill: {json2}");
    assert_eq!(
        json2["blocks_added"], 0,
        "second backfill should be no-op: {json2}"
    );
    assert_eq!(
        json2["chapters_repaired"], 0,
        "second run no chapters repaired: {json2}"
    );
    assert_eq!(
        json2["tags_repaired"], 0,
        "second run no tags repaired: {json2}"
    );
    assert_eq!(
        json2["fluent_links_repaired"], 0,
        "second run no fluent links repaired: {json2}"
    );
    assert_eq!(
        json2["card_repaired"], false,
        "second run card not repaired: {json2}"
    );
}

/// Test that a multi-chapter guide with mixed chapters (prose-only + stages)
/// produces the correct number of playbooks and skips prose-only chapters.
#[tokio::test]
async fn test_guide_ingest_multi_playbook_mixed_chapters() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball_bytes = create_multi_playbook_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    let playbook_ids: Vec<&str> = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("playbook_ids should be array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // 2 chapters with stages (prose-only chapter skipped) → 2 playbooks
    assert_eq!(
        playbook_ids.len(),
        2,
        "should have exactly 2 playbooks (prose-only chapter skipped): {json}"
    );

    // Query both playbooks
    let client = reqwest::Client::new();
    let ids_query: String = playbook_ids
        .iter()
        .enumerate()
        .map(|(i, id)| format!("ids[{}]={}", i, id))
        .collect::<Vec<_>>()
        .join("&");
    let playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&{}",
        base_url, ids_query
    );
    let resp = client
        .get(&playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("playbook query failed");

    let resp_text = resp.text().await.expect("failed to read response");
    let playbook_json: serde_json::Value = serde_json::from_str(&resp_text).expect("invalid json");
    let playbooks = playbook_json["objs"]
        .as_array()
        .expect("objs should be array");
    assert_eq!(playbooks.len(), 2, "expected 2 playbooks: {playbook_json}");

    // Both should have suffixed names (em-dash separator)
    for pb in playbooks {
        let name = pb["name"].as_str().unwrap();
        assert!(
            name.contains("Multi PB Ingest Guide"),
            "playbook name should contain guide title: {name}"
        );
        assert!(
            name.contains("\u{2014}"),
            "multi-playbook name should have em-dash separator: {name}"
        );
    }

    // Playbook 1: Search chapter with 2 steps
    let pb1_name = playbooks[0]["name"].as_str().unwrap();
    assert!(
        pb1_name.contains("Search"),
        "playbook 1 should reference Search chapter: {pb1_name}"
    );
    let pb1_stages = playbooks[0]["stages"].as_array().unwrap();
    assert_eq!(pb1_stages.len(), 1, "pb1 should have 1 stage");
    let pb1_steps = pb1_stages[0]["steps"].as_array().unwrap();
    assert_eq!(pb1_steps.len(), 2, "pb1 stage should have 2 steps");

    // Playbook 2: Browse chapter with 1 step
    let pb2_name = playbooks[1]["name"].as_str().unwrap();
    assert!(
        pb2_name.contains("Browse"),
        "playbook 2 should reference Browse chapter: {pb2_name}"
    );
    let pb2_stages = playbooks[1]["stages"].as_array().unwrap();
    assert_eq!(pb2_stages.len(), 1, "pb2 should have 1 stage");
    let pb2_steps = pb2_stages[0]["steps"].as_array().unwrap();
    assert_eq!(pb2_steps.len(), 1, "pb2 stage should have 1 step");
}

// ============================================================================
// Workflow-style guide tests (Phases 3 & 4)
// ============================================================================

/// Create a workflow guide tarball with full directive coverage: brief-pack,
/// inference-profile, 2 stages (Planning: 3 steps, Execution: 2 steps),
/// nushell tasks, prose, and fluent messages for everything.
///
/// An optional `variant` parameter changes the brainstorm nushell script body
/// to produce a different content hash. Pass `None` for the default.
fn create_workflow_guide_tarball_with_variant(suffix: &str, variant: Option<&str>) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];
    let variant_tag = variant.unwrap_or("v1");

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Workflow Guide {suffix}\"\n\
         description = \"Full workflow test guide\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n\
         \n\
         [card]\n\
         color = \"#2D6A4F\"\n\
         accent = \"#95D5B2\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Introduction](introduction.md)\n- [Workflow](workflow.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let introduction = format!(
        "# Introduction\n\n\
         :::brief-pack {{ name: \"test-pack-{short}\" }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/introduction.md", &introduction);

    let workflow = format!(
        "# Workflow\n\n\
         :::stage {{ name: \"Planning-{short}\" }}\n\n\
         :::step {{ name: \"Brainstorm-{short}\", task: \"brainstorm-task-{short}\" }}\n\
         :::\n\n\
         :::step {{ name: \"Select-{short}\", task: \"select-task-{short}\" }}\n\
         :::\n\n\
         :::step {{ name: \"Validate-{short}\", task: \"validate-task-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"brainstorm-task-{short}\", tags: [coder] }}\n\
         {{ payload: {{ topics: [\"A\",\"B\"], variant: \"{variant_tag}\" }}, signals: {{}} }}\n\
         :::\n\n\
         :::nushell {{ name: \"validate-task-{short}\", tags: [coder] }}\n\
         {{ payload: {{ valid: true }}, signals: {{}} }}\n\
         :::\n\n\
         :::nushell {{ name: \"select-task-{short}\", tags: [coder] }}\n\
         {{ payload: {{ choices: [\"A\",\"B\"] }}, signals: {{}} }}\n\
         :::\n\n\
         :::prose {{ tags: [agent] }}\n\
         Agent guidance text.\n\
         :::\n\n\
         :::stage {{ name: \"Execution-{short}\" }}\n\n\
         :::step {{ name: \"Execute-{short}\", task: \"execute-task-{short}\" }}\n\
         :::\n\n\
         :::step {{ name: \"Review-{short}\", task: \"review-task-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"execute-task-{short}\", tags: [coder] }}\n\
         {{ payload: {{ done: true }}, signals: {{}} }}\n\
         :::\n\n\
         :::nushell {{ name: \"review-task-{short}\", tags: [coder] }}\n\
         {{ payload: {{ reviewed: true }}, signals: {{}} }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/workflow.md", &workflow);

    let fluent = format!(
        "test-pack-{short} = Test brief pack content.\n\
         planning-{short} = Planning stage description.\n\
         brainstorm-{short} = Brainstorm step description.\n\
         select-{short} = Select step description.\n\
         validate-{short} = Validate step description.\n\
         execution-{short} = Execution stage description.\n\
         execute-{short} = Execute step description.\n\
         review-{short} = Review step description.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);

    tarball
}

/// Convenience wrapper: create workflow tarball with default variant.
fn create_workflow_guide_tarball(suffix: &str) -> Vec<u8> {
    create_workflow_guide_tarball_with_variant(suffix, None)
}

/// Phase 3: Verify that the workflow guide produces the correct derived objects
/// (1 playbook, 5 tasks, inference profiles) and the playbook has the expected
/// stage/step structure (2 stages: Planning with 3 steps, Execution with 2 steps).
#[tokio::test]
async fn test_workflow_guide_full_structure() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball_bytes = create_workflow_guide_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    // 1. Verify derived objects
    let playbook_ids: Vec<&str> = json["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("playbook_ids")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(playbook_ids.len(), 1, "should have 1 playbook: {json}");

    let task_ids: Vec<&str> = json["derived_objects"]["task_ids"]
        .as_array()
        .expect("task_ids")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(task_ids.len(), 5, "should have 5 tasks: {json}");

    // brief_pack_ids field should exist (brief packs not yet materialized during ingest)
    let _brief_pack_ids = json["derived_objects"]["brief_pack_ids"]
        .as_array()
        .expect("brief_pack_ids should be present");

    // 2. Query playbook and verify structure
    let client = reqwest::Client::new();
    let playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&ids[0]={}",
        base_url, playbook_ids[0]
    );
    let resp = client
        .get(&playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("playbook query failed");
    let resp_text = resp.text().await.expect("failed to read response");
    let playbook_json: serde_json::Value = serde_json::from_str(&resp_text).expect("invalid json");
    let playbooks = playbook_json["objs"].as_array().expect("objs");
    assert_eq!(playbooks.len(), 1, "expected 1 playbook: {playbook_json}");

    let playbook = &playbooks[0];
    let expected_name = format!("Workflow Guide {suffix}");
    assert_eq!(
        playbook["name"].as_str().unwrap(),
        expected_name,
        "playbook name should be guide title (no suffix)"
    );

    let stages = playbook["stages"].as_array().expect("stages");
    assert_eq!(stages.len(), 2, "should have 2 stages: {playbook_json}");

    // Stage 1 (Planning): 3 steps
    let stage1_steps = stages[0]["steps"].as_array().expect("stage1 steps");
    assert_eq!(
        stage1_steps.len(),
        3,
        "Planning stage should have 3 steps: {playbook_json}"
    );

    // Stage 2 (Execution): 2 steps
    let stage2_steps = stages[1]["steps"].as_array().expect("stage2 steps");
    assert_eq!(
        stage2_steps.len(),
        2,
        "Execution stage should have 2 steps: {playbook_json}"
    );
}

/// Phase 4: Verify step-to-task associations in the workflow guide. Steps that
/// reference a task via the `task:` attribute should have a non-empty `tasks`
/// array; steps without a task reference should have an empty `tasks` array.
#[tokio::test]
async fn test_workflow_guide_step_task_linkage() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let tarball_bytes = create_workflow_guide_tarball(&suffix);
    let json = ingest_tarball(&base_url, tarball_bytes).await;

    let playbook_ids: Vec<&str> = json["derived_objects"]["playbook_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let task_ids: Vec<String> = json["derived_objects"]["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    assert_eq!(playbook_ids.len(), 1);
    assert_eq!(task_ids.len(), 5);

    // Query the playbook
    let client = reqwest::Client::new();
    let playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&ids[0]={}",
        base_url, playbook_ids[0]
    );
    let resp = client
        .get(&playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("playbook query failed");
    let resp_text = resp.text().await.expect("failed to read response");
    let playbook_json: serde_json::Value = serde_json::from_str(&resp_text).expect("invalid json");
    let playbook = &playbook_json["objs"][0];

    let stages = playbook["stages"].as_array().unwrap();

    // Stage 1 (Planning): 3 steps
    let stage1_steps = stages[0]["steps"].as_array().unwrap();
    assert_eq!(stage1_steps.len(), 3);

    // Step 0: Brainstorm (has task)
    let brainstorm_tasks = stage1_steps[0]["tasks"].as_array().unwrap();
    assert!(
        !brainstorm_tasks.is_empty(),
        "Brainstorm step should have a task: {playbook_json}"
    );
    let brainstorm_task_id = brainstorm_tasks[0]["task"]["id"].as_str().unwrap();
    assert!(
        task_ids.contains(&brainstorm_task_id.to_string()),
        "Brainstorm task id should be in derived_objects.task_ids"
    );

    // Step 1: Select (has task — every step must reference a task)
    let select_tasks = stage1_steps[1]["tasks"].as_array().unwrap();
    assert!(
        !select_tasks.is_empty(),
        "Select step should have a task: {playbook_json}"
    );
    let select_task_id = select_tasks[0]["task"]["id"].as_str().unwrap();
    assert!(
        task_ids.contains(&select_task_id.to_string()),
        "Select task id should be in derived_objects.task_ids"
    );

    // Step 2: Validate (has task)
    let validate_tasks = stage1_steps[2]["tasks"].as_array().unwrap();
    assert!(
        !validate_tasks.is_empty(),
        "Validate step should have a task: {playbook_json}"
    );
    let validate_task_id = validate_tasks[0]["task"]["id"].as_str().unwrap();
    assert!(
        task_ids.contains(&validate_task_id.to_string()),
        "Validate task id should be in derived_objects.task_ids"
    );

    // Stage 2 (Execution): 2 steps
    let stage2_steps = stages[1]["steps"].as_array().unwrap();
    assert_eq!(stage2_steps.len(), 2);

    // Step 0: Execute (has task)
    let execute_tasks = stage2_steps[0]["tasks"].as_array().unwrap();
    assert!(
        !execute_tasks.is_empty(),
        "Execute step should have a task: {playbook_json}"
    );
    let execute_task_id = execute_tasks[0]["task"]["id"].as_str().unwrap();
    assert!(
        task_ids.contains(&execute_task_id.to_string()),
        "Execute task id should be in derived_objects.task_ids"
    );

    // Step 1: Review (has task — every step must reference a task)
    let review_tasks = stage2_steps[1]["tasks"].as_array().unwrap();
    assert!(
        !review_tasks.is_empty(),
        "Review step should have a task: {playbook_json}"
    );
    let review_task_id = review_tasks[0]["task"]["id"].as_str().unwrap();
    assert!(
        task_ids.contains(&review_task_id.to_string()),
        "Review task id should be in derived_objects.task_ids"
    );

    // Verify all 5 task ids are distinct
    let mut unique_task_ids = vec![
        brainstorm_task_id,
        select_task_id,
        validate_task_id,
        execute_task_id,
        review_task_id,
    ];
    unique_task_ids.sort();
    unique_task_ids.dedup();
    assert_eq!(
        unique_task_ids.len(),
        5,
        "all 5 task ids should be distinct"
    );

    // Query task entities and verify names
    let ids_query: String = task_ids
        .iter()
        .enumerate()
        .map(|(i, id)| format!("ids[{}]={}", i, id))
        .collect::<Vec<_>>()
        .join("&");
    let tasks_url = format!("{}/api/tasks/?query=by_object_ids&{}", base_url, ids_query);
    let resp = client
        .get(&tasks_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("tasks query failed");
    let resp_text = resp.text().await.expect("failed to read response");
    let tasks_json: serde_json::Value = serde_json::from_str(&resp_text).expect("invalid json");
    let tasks = tasks_json["objs"].as_array().expect("objs");
    assert_eq!(tasks.len(), 5, "expected 5 task entities");

    // Verify task names contain the short suffix
    for task in tasks {
        let name = task["name"].as_str().expect("task name");
        assert!(
            name.contains(short),
            "task name should contain short suffix {short}: {name}"
        );
    }
}

// ============================================================================
// Re-ingest dedup tests (Phase 5)
// ============================================================================

/// Phase 5: Verify that re-ingesting the same workflow tarball refreshes in place
/// (200 "refreshed", same content_hash → materializers re-run idempotently, no new
/// version), and that ingesting a modified tarball creates a new version with correct
/// structure.
#[tokio::test]
async fn test_workflow_guide_reingest_preserves_linkage() {
    let (base_url, _handle) = spawn_app().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];

    // 1. POST initial ingest
    let tarball_v1 = create_workflow_guide_tarball(&suffix);
    let json_v1 = ingest_tarball(&base_url, tarball_v1).await;

    let path_id = json_v1["guide"]["path_id"]
        .as_str()
        .expect("guide.path_id")
        .to_string();

    let v1_playbook_ids: Vec<String> = json_v1["derived_objects"]["playbook_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    assert_eq!(v1_playbook_ids.len(), 1, "v1 should have 1 playbook");

    let v1_task_ids: Vec<String> = json_v1["derived_objects"]["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    assert_eq!(v1_task_ids.len(), 5, "v1 should have 5 tasks");

    // 2. PATCH same tarball → 200 "refreshed". An identical content_hash is a
    //    refresh-in-place (the materializers re-run idempotently); it is NOT a
    //    409 conflict. See `refresh_existing_guide` in guide_ingest.rs and the
    //    matching `test_backfill_normal_path_still_dedups`.
    let tarball_same = create_workflow_guide_tarball(&suffix);
    let (status_same, json_same) =
        ingest_tarball_patch(&base_url, &path_id, tarball_same, false).await;
    assert_eq!(
        status_same.as_u16(),
        200,
        "re-ingest of identical tarball should refresh in place: {json_same}"
    );
    assert_eq!(
        json_same["status"], "refreshed",
        "status should be refreshed: {json_same}"
    );

    // 3. PATCH modified tarball (changed brainstorm script) → expect 201
    let tarball_v2 = create_workflow_guide_tarball_with_variant(&suffix, Some("v2-modified"));
    let (status_v2, json_v2) = ingest_tarball_patch(&base_url, &path_id, tarball_v2, false).await;
    assert_eq!(
        status_v2.as_u16(),
        201,
        "ingest of modified tarball should return 201: {json_v2}"
    );
    assert_eq!(
        json_v2["status"], "success",
        "status should be success: {json_v2}"
    );

    let v2_playbook_ids: Vec<String> = json_v2["derived_objects"]["playbook_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    assert_eq!(
        v2_playbook_ids.len(),
        1,
        "v2 should have 1 playbook: {json_v2}"
    );

    let v2_task_ids: Vec<String> = json_v2["derived_objects"]["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    assert_eq!(v2_task_ids.len(), 5, "v2 should have 5 tasks: {json_v2}");

    // 4. GET v2 playbook → verify structure (2 stages, 5 steps)
    let client = reqwest::Client::new();
    let playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&ids[0]={}",
        base_url, v2_playbook_ids[0]
    );
    let resp = client
        .get(&playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("playbook query failed");
    let resp_text = resp.text().await.expect("failed to read response");
    let playbook_json: serde_json::Value = serde_json::from_str(&resp_text).expect("invalid json");
    let playbook = &playbook_json["objs"][0];

    let stages = playbook["stages"].as_array().expect("stages");
    assert_eq!(
        stages.len(),
        2,
        "v2 playbook should have 2 stages: {playbook_json}"
    );

    let stage1_steps = stages[0]["steps"].as_array().expect("stage1 steps");
    let stage2_steps = stages[1]["steps"].as_array().expect("stage2 steps");
    assert_eq!(
        stage1_steps.len(),
        3,
        "v2 Planning stage should have 3 steps: {playbook_json}"
    );
    assert_eq!(
        stage2_steps.len(),
        2,
        "v2 Execution stage should have 2 steps: {playbook_json}"
    );

    // 5. Verify step→task associations still populated in v2
    // Brainstorm step has task
    let brainstorm_tasks = stage1_steps[0]["tasks"].as_array().unwrap();
    assert!(
        !brainstorm_tasks.is_empty(),
        "v2 Brainstorm step should still have a task: {playbook_json}"
    );

    // Select step has task
    let select_tasks = stage1_steps[1]["tasks"].as_array().unwrap();
    assert!(
        !select_tasks.is_empty(),
        "v2 Select step should still have a task: {playbook_json}"
    );

    // Validate step has task
    let validate_tasks = stage1_steps[2]["tasks"].as_array().unwrap();
    assert!(
        !validate_tasks.is_empty(),
        "v2 Validate step should still have a task: {playbook_json}"
    );

    // Execute step has task
    let execute_tasks = stage2_steps[0]["tasks"].as_array().unwrap();
    assert!(
        !execute_tasks.is_empty(),
        "v2 Execute step should still have a task: {playbook_json}"
    );

    // Review step has task
    let review_tasks = stage2_steps[1]["tasks"].as_array().unwrap();
    assert!(
        !review_tasks.is_empty(),
        "v2 Review step should still have a task: {playbook_json}"
    );

    // 6. Verify task entities from v2
    let ids_query: String = v2_task_ids
        .iter()
        .enumerate()
        .map(|(i, id)| format!("ids[{}]={}", i, id))
        .collect::<Vec<_>>()
        .join("&");
    let tasks_url = format!("{}/api/tasks/?query=by_object_ids&{}", base_url, ids_query);
    let resp = client
        .get(&tasks_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("tasks query failed");
    let resp_text = resp.text().await.expect("failed to read response");
    let tasks_json: serde_json::Value = serde_json::from_str(&resp_text).expect("invalid json");
    let tasks = tasks_json["objs"].as_array().expect("objs");
    assert_eq!(
        tasks.len(),
        5,
        "v2 should have 5 task entities: {tasks_json}"
    );

    for task in tasks {
        let name = task["name"].as_str().expect("task name");
        assert!(
            name.contains(short),
            "v2 task name should contain short suffix {short}: {name}"
        );
    }
}

// ============================================================================
// Merkle hash cascade tests (Phase 6)
// ============================================================================

/// Test that re-ingesting a guide with a modified task creates a new playbook
/// (Merkle hash cascade: task change -> step -> stage -> playbook).
#[tokio::test]
async fn test_reingest_creates_new_playbook_on_task_change() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();

    // 1. POST initial guide with workflow (5 tasks, 1 playbook)
    let v1_tarball = create_workflow_guide_tarball_with_variant(&suffix, Some("v1"));
    let v1_json = ingest_tarball(&base_url, v1_tarball).await;

    let v1_playbook_id = v1_json["derived_objects"]["playbook_ids"][0]
        .as_str()
        .expect("v1 playbook_id");
    let v1_task_ids: Vec<&str> = v1_json["derived_objects"]["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(v1_task_ids.len(), 5, "v1 should have 5 tasks");

    let v1_path_id = v1_json["guide"]["path_id"].as_str().expect("v1 path_id");

    // 2. PATCH with modified brainstorm task script (variant changes from "v1" to "v2")
    let v2_tarball = create_workflow_guide_tarball_with_variant(&suffix, Some("v2"));
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(v2_tarball)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let client = reqwest::Client::new();
    let response = client
        .patch(format!("{}/api/guides/{}/ingest", base_url, v1_path_id))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("patch request failed");

    let status = response.status();
    let v2_json: serde_json::Value = response.json().await.expect("invalid json");
    assert_eq!(status, 201, "patch response: {v2_json}");

    let v2_playbook_id = v2_json["derived_objects"]["playbook_ids"][0]
        .as_str()
        .expect("v2 playbook_id");
    let v2_task_ids: Vec<&str> = v2_json["derived_objects"]["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // 3. Assert: v2 playbook_id != v1 playbook_id (Merkle hash cascade)
    assert_ne!(
        v1_playbook_id, v2_playbook_id,
        "Playbook should have new ID after task change (Merkle cascade)"
    );

    // 4. Assert: at least one task ID changed (brainstorm task was modified)
    let changed_tasks: Vec<_> = v2_task_ids
        .iter()
        .filter(|id| !v1_task_ids.contains(id))
        .collect();
    assert!(
        !changed_tasks.is_empty(),
        "At least one task should have a new ID after script change"
    );

    // 5. Old playbook still accessible (immutability)
    let old_playbook_url = format!(
        "{}/api/playbooks/?query=by_object_ids&ids[0]={}",
        base_url, v1_playbook_id
    );
    let resp = client
        .get(&old_playbook_url)
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("old playbook query failed");
    assert!(
        resp.status().is_success(),
        "old playbook should still be accessible"
    );
    let old_pb_json: serde_json::Value = resp.json().await.unwrap();
    let old_pbs = old_pb_json["objs"].as_array().expect("objs");
    assert_eq!(old_pbs.len(), 1, "old playbook should still exist");
}

/// Test Merkle cascade: changing one task propagates through step -> stage -> playbook,
/// while unchanged tasks/steps are reused with the same IDs.
#[tokio::test]
async fn test_merkle_cascade_task_to_playbook() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();

    // 1. POST v1
    let v1_tarball = create_workflow_guide_tarball_with_variant(&suffix, Some("v1"));
    let v1_json = ingest_tarball(&base_url, v1_tarball).await;

    let v1_playbook_id = v1_json["derived_objects"]["playbook_ids"][0]
        .as_str()
        .unwrap();
    let v1_task_ids: Vec<String> = v1_json["derived_objects"]["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    let v1_path_id = v1_json["guide"]["path_id"].as_str().unwrap();

    // 2. PATCH v2 — only brainstorm task script changes (variant "v1" -> "v2")
    let v2_tarball = create_workflow_guide_tarball_with_variant(&suffix, Some("v2"));
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(v2_tarball)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let client = reqwest::Client::new();
    let response = client
        .patch(format!("{}/api/guides/{}/ingest", base_url, v1_path_id))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("patch failed");
    let v2_json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(v2_json["status"], "success", "v2: {v2_json}");

    let v2_task_ids: Vec<String> = v2_json["derived_objects"]["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .map(String::from)
        .collect();
    let v2_playbook_id = v2_json["derived_objects"]["playbook_ids"][0]
        .as_str()
        .unwrap();

    // 3. Unchanged tasks should be reused (same IDs)
    // validate-task and execute-task should keep same IDs
    let reused_count = v2_task_ids
        .iter()
        .filter(|id| v1_task_ids.contains(id))
        .count();
    assert!(
        reused_count >= 2,
        "At least 2 unchanged tasks should be reused. v1={:?}, v2={:?}",
        v1_task_ids,
        v2_task_ids
    );

    // 4. Playbook should have new ID (Merkle cascade)
    assert_ne!(
        v1_playbook_id, v2_playbook_id,
        "Playbook should get new ID from Merkle cascade"
    );
}

// ============================================================================
// Real backfill repair tests for derived objects
// ============================================================================
//
// These tests replace two earlier `test_backfill_*` derived-object tests that
// asserted only on `response.derived_objects_repaired == true`. The old tests
// passed against a code path where `?backfill=true` was a documented no-op —
// see commit history for `let _ = params.backfill;` — and shipped the false
// confidence that "backfill repairs derived objects" while it never had.
//
// Per the rule in `crates/dapple-prompt-api/AGENTS.md` → "Testing Repair
// Operations": a test for a repair endpoint MUST corrupt DB state first and
// then assert via direct SQL that the corruption is gone after the operation.
// Asserting on response shape (`status: "refreshed"`,
// `derived_objects_repaired: true`) is not sufficient — that's just the
// endpoint reporting it ran the materializers, not that they actually wrote
// anything.

/// Helper: build a guide tarball that defines:
///  - A value-slot `topic-slot-{short}` (text)
///  - A brief-input `topic-input-{short}` referencing the slot
///  - A brief `topic-brief-{short}` with a single unit (the input)
///  - A brief-group + brief-pack to wrap them
///
/// This is the minimal fixture needed to exercise the
/// `brief_unit_input.control_spec` repair path. Mirrors the broken-slot
/// fixture used by the external-API loud-error test but with a slot that
/// actually exists, so ingest succeeds and we can corrupt the row.
fn create_brief_input_test_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Brief Input Guide {suffix}\"\n\
         description = \"Fixture with brief-input wired to a real value-slot.\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Intro](intro.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let intro = format!(
        "# Intro\n\n\
         :::value-slot {{ name: \"topic-slot-{short}\", kind: \"text\" }}\n\
         :::\n\n\
         :::brief-input {{ name: \"topic-input-{short}\", control: \"text\", slot: \"topic-slot-{short}\" }}\n\
         :::\n\n\
         :::brief {{ name: \"topic-brief-{short}\", units: [\"topic-input-{short}\"] }}\n\
         :::\n\n\
         :::brief-group {{ name: \"topic-group-{short}\", briefs: [\"topic-brief-{short}\"] }}\n\
         :::\n\n\
         :::brief-pack {{ name: \"topic-pack-{short}\", groups: [\"topic-group-{short}\"] }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/intro.md", &intro);

    let fluent = format!(
        "topic-input-{short} = Topic input label\n\
         topic-brief-{short} = Topic brief title\n\
         topic-group-{short} = Topic group title\n\
         topic-pack-{short} = Topic pack title\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);

    tarball
}

/// Look up the latest brief id by `brief_path_details.name`. Tests that need
/// the *latest* version use this; tests that want to count versions across
/// all history use a COUNT query directly.
async fn latest_brief_id_by_name(db: &sqlx::PgPool, brief_name: &str) -> uuid::Uuid {
    let row: (uuid::Uuid,) = sqlx::query_as(
        "SELECT b.id
         FROM rev.briefs b
         JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id
         JOIN rev.brief_path_details bpd ON bpd.path_id = vo.path_id
         WHERE bpd.name = $1
         ORDER BY vo.created_at DESC
         LIMIT 1",
    )
    .bind(brief_name)
    .fetch_one(db)
    .await
    .expect("brief not found by name");
    row.0
}

/// Count brief versions for a given path-details name (across all versions).
async fn brief_version_count_by_name(db: &sqlx::PgPool, brief_name: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)
         FROM rev.briefs b
         JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id
         JOIN rev.brief_path_details bpd ON bpd.path_id = vo.path_id
         WHERE bpd.name = $1",
    )
    .bind(brief_name)
    .fetch_one(db)
    .await
    .expect("brief version count failed");
    row.0
}

/// Get the `control_spec` of the brief_unit_input row attached to the given
/// brief's first input unit. Returns None if no row exists.
async fn brief_input_control_spec(
    db: &sqlx::PgPool,
    brief_id: uuid::Uuid,
) -> Option<serde_json::Value> {
    sqlx::query_as::<_, (serde_json::Value,)>(
        "SELECT bui.control_spec
         FROM rev.brief_unit_input bui
         JOIN rev.brief_units bu ON bu.id = bui.brief_unit_id
         WHERE bu.brief_id = $1
         LIMIT 1",
    )
    .bind(brief_id)
    .fetch_optional(db)
    .await
    .expect("brief_unit_input query failed")
    .map(|t| t.0)
}

/// Test that `?backfill=true` repairs a broken `brief_unit_input.control_spec`
/// (the canonical hype-vs-reality failure). The old `derived_objects_repaired`
/// boolean lied: it claimed repair while the materializer's content-hash dedup
/// caused the brief to be REUSED, leaving the corrupted input row untouched.
/// Under `IngestMode::ForceRepair` the brief gets re-PATCHed (new version)
/// and a fresh `brief_unit_input` row with the correct `value_slot_id` is
/// inserted.
#[tokio::test]
async fn test_backfill_repairs_brief_unit_input_control_spec() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let brief_name = format!("topic-brief-{short}");

    // 1. Ingest the fixture. Capture the latest brief id and verify the
    //    initial control_spec carries a value_slot_id.
    let tarball = create_brief_input_test_tarball(&suffix);
    let (path_id, _guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;

    let v1_brief_id = latest_brief_id_by_name(&db, &brief_name).await;
    let v1_spec = brief_input_control_spec(&db, v1_brief_id)
        .await
        .expect("v1 brief_unit_input row must exist");
    assert!(
        v1_spec.get("value_slot_id").is_some(),
        "initial control_spec should carry value_slot_id, got: {v1_spec}"
    );

    // 2. Corrupt the row: blank out the control_spec on the v1 brief's input.
    //    Direct UPDATE mirrors what historical broken-ingest data looks like
    //    in production (empty control_spec that escaped before the loud-error
    //    slot check landed).
    let rows_affected = sqlx::query(
        "UPDATE rev.brief_unit_input
         SET control_spec = '{}'::jsonb
         WHERE brief_unit_id IN (
             SELECT bu.id FROM rev.brief_units bu WHERE bu.brief_id = $1
         )",
    )
    .bind(v1_brief_id)
    .execute(&db)
    .await
    .expect("corruption update failed")
    .rows_affected();
    assert!(rows_affected >= 1, "should corrupt at least 1 row");

    let corrupted_spec = brief_input_control_spec(&db, v1_brief_id)
        .await
        .expect("corrupted brief_unit_input row");
    assert_eq!(
        corrupted_spec,
        serde_json::json!({}),
        "control_spec should be empty after corruption"
    );

    // 3. PATCH ingest with backfill=true to repair.
    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");
    assert_eq!(json["status"], "refreshed");

    // 4. Model B repairs IN PLACE: the brief keeps its SAME version id (no new
    //    version), and its brief_unit_input row is delete-reinserted with the
    //    correct value_slot_id. (Pre-Model-B this minted a new version; the
    //    nonce churn was the wrong angle — same content ⇒ same version.)
    let v2_brief_id = latest_brief_id_by_name(&db, &brief_name).await;
    assert_eq!(
        v1_brief_id, v2_brief_id,
        "backfill of unchanged content must repair in place — SAME brief version"
    );
    let v2_spec = brief_input_control_spec(&db, v2_brief_id)
        .await
        .expect("brief_unit_input row must exist after repair");
    assert!(
        v2_spec.get("value_slot_id").is_some(),
        "post-backfill control_spec should carry value_slot_id, got: {v2_spec}"
    );
}

/// Resolve a brief's pinned title Fluent reference + its live-resolved title,
/// exactly as the read path does: `COALESCE(rev.fluent_body(ref), text, name)`.
async fn resolve_brief_title(
    db: &sqlx::PgPool,
    brief_id: uuid::Uuid,
) -> (Option<uuid::Uuid>, String) {
    sqlx::query_as::<_, (Option<uuid::Uuid>, String)>(
        "SELECT b.title_fluent_message_id,
                COALESCE(rev.fluent_body(b.title_fluent_message_id, 'en'), b.title, bpd.name)
         FROM rev.briefs b
         JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id
         JOIN rev.brief_path_details bpd ON bpd.path_id = vo.path_id
         WHERE b.id = $1",
    )
    .bind(brief_id)
    .fetch_one(db)
    .await
    .expect("brief title resolution failed")
}

/// Regression for the brief-title slug rot: a brief's display title is the
/// Fluent message BODY (resolved live from `title_fluent_message_id`), and the
/// PATCH/re-version path must re-pin that reference rather than baking the slug.
///
/// Corrupt-then-verify in SQL (per "Testing Repair Operations"): null the
/// reference + write the slug into the text column (the exact pre-fix shape),
/// then re-ingest and confirm the new version resolves to the Fluent body again
/// with the reference repopulated — never the slug.
#[tokio::test]
async fn test_brief_title_pins_fluent_and_survives_reingest() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let brief_name = format!("topic-brief-{short}");
    let expected_title = "Topic brief title"; // the Fluent body, not the slug

    let tarball = create_brief_input_test_tarball(&suffix);
    let (path_id, _guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;

    // v1: title resolves from the Fluent body and the reference is pinned.
    let v1_id = latest_brief_id_by_name(&db, &brief_name).await;
    let (v1_ref, v1_title) = resolve_brief_title(&db, v1_id).await;
    assert!(v1_ref.is_some(), "v1 brief must pin a title Fluent message");
    assert_eq!(
        v1_title, expected_title,
        "v1 title must resolve from the Fluent body, not the slug"
    );
    assert_ne!(v1_title, brief_name, "title must never be the slug");

    // Corrupt v1 into the pre-fix shape: drop the reference, bake the slug.
    sqlx::query("UPDATE rev.briefs SET title_fluent_message_id = NULL, title = $2 WHERE id = $1")
        .bind(v1_id)
        .bind(&brief_name)
        .execute(&db)
        .await
        .expect("corruption update failed");
    let (corrupt_ref, corrupt_title) = resolve_brief_title(&db, v1_id).await;
    assert!(
        corrupt_ref.is_none(),
        "corruption should drop the reference"
    );
    assert_eq!(
        corrupt_title, brief_name,
        "corrupted v1 resolves to the slug (the bug we are fixing)"
    );

    // Backfill (ForceRepair) → Model B re-pins the Fluent reference IN PLACE on
    // the SAME version (the title ref / baked-slug live on the brief ROW, which
    // the repair UPDATEs directly — see materialize_brief_system).
    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");

    let v2_id = latest_brief_id_by_name(&db, &brief_name).await;
    assert_eq!(
        v1_id, v2_id,
        "Model B backfill must repair the title in place — SAME brief version"
    );
    let (v2_ref, v2_title) = resolve_brief_title(&db, v2_id).await;
    assert!(
        v2_ref.is_some(),
        "re-ingest must re-pin the title Fluent message (not bake the slug)"
    );
    assert_eq!(
        v2_title, expected_title,
        "re-ingest must resolve the title from the Fluent body, never the slug"
    );
    assert_ne!(
        v2_title, brief_name,
        "re-ingested title must never be the slug"
    );
}

/// Model B: `?backfill=true` on unchanged content repairs IN PLACE and mints
/// NO new brief version (the old "always re-version" rule was the wrong angle —
/// content-identical ⇒ same version). The repair re-materializes the brief's
/// owned children against the same version; the version count stays at 1.
#[tokio::test]
async fn test_backfill_repairs_in_place_no_new_brief_version() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let brief_name = format!("topic-brief-{short}");

    let tarball = create_brief_input_test_tarball(&suffix);
    let (path_id, _guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;

    let initial_count = brief_version_count_by_name(&db, &brief_name).await;
    assert_eq!(
        initial_count, 1,
        "should have exactly 1 brief version after POST"
    );

    // PATCH with backfill=true on identical content. Model B repairs the
    // existing version in place (delete-reinsert its owned children) rather
    // than minting a new version, so the version count stays at 1.
    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status, 200, "backfill response: {json}");

    let post_count = brief_version_count_by_name(&db, &brief_name).await;
    assert_eq!(
        post_count, 1,
        "Model B backfill must repair in place — NO new brief version (still 1)"
    );
}

/// Regression: two backfills in a row on the same source must not collide and
/// must NOT grow the version DAG.
///
/// Under Model B each backfill repairs the SAME brief version in place — it
/// delete-reinserts the owned unit subtree (cascade via migration 52), so the
/// re-insert can never collide on UNIQUE(brief_id, unit_key) and no new version
/// is minted. This is what made the old nonce-rewrite hack — which existed only
/// to dodge that collision by forcing a fresh version every backfill — entirely
/// unnecessary. The version count stays at 1 across repeated backfills.
#[tokio::test]
async fn test_backfill_twice_no_collision() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let brief_name = format!("topic-brief-{short}");

    let tarball = create_brief_input_test_tarball(&suffix);
    let (path_id, _guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;

    assert_eq!(
        brief_version_count_by_name(&db, &brief_name).await,
        1,
        "should have exactly 1 brief version after POST"
    );

    // First backfill — repairs in place, still 1 version.
    let (status1, json1) = ingest_tarball_patch(&base_url, &path_id, tarball.clone(), true).await;
    assert_eq!(status1, 200, "first backfill response: {json1}");
    assert_eq!(
        brief_version_count_by_name(&db, &brief_name).await,
        1,
        "first backfill must repair in place — still 1 brief version"
    );

    // Second backfill — must NOT collide. This is the exact failure mode the
    // user hit in production ("duplicate key value violates unique constraint
    // brief_units_brief_id_unit_key_key"); Model B's delete-then-reinsert makes
    // it impossible. Still 1 version (no DAG growth).
    let (status2, json2) = ingest_tarball_patch(&base_url, &path_id, tarball, true).await;
    assert_eq!(status2, 200, "second backfill response: {json2}");
    assert_eq!(
        brief_version_count_by_name(&db, &brief_name).await,
        1,
        "second backfill must repair in place without colliding — still 1 brief version"
    );

    // And the latest brief still has correct control_spec — the repair
    // chain populated value_slot_id on the new brief_unit_input rows.
    let latest_id = latest_brief_id_by_name(&db, &brief_name).await;
    let spec = brief_input_control_spec(&db, latest_id)
        .await
        .expect("latest brief should have a brief_unit_input row");
    assert!(
        spec.get("value_slot_id").is_some(),
        "latest brief's input control_spec must carry value_slot_id after two backfills; got: {spec}"
    );
}

/// Test that PATCH ingest WITHOUT `?backfill=true` does NOT create a new
/// brief version when content_hash matches. Locks in the Normal-path dedup
/// behavior — ForceRepair must not silently leak into normal traffic and
/// double-write everything.
#[tokio::test]
async fn test_backfill_normal_path_still_dedups() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let brief_name = format!("topic-brief-{short}");

    let tarball = create_brief_input_test_tarball(&suffix);
    let (path_id, _guide_id) = initial_ingest(&base_url, tarball.clone()).await;
    let db = get_db_pool().await;

    let initial_count = brief_version_count_by_name(&db, &brief_name).await;
    assert_eq!(
        initial_count, 1,
        "should have exactly 1 brief version after POST"
    );

    // PATCH with backfill=false (the default). Same content → content_hash
    // matches → REUSE → no new brief version.
    let (status, json) = ingest_tarball_patch(&base_url, &path_id, tarball, false).await;
    assert_eq!(status, 200, "patch response: {json}");

    let post_count = brief_version_count_by_name(&db, &brief_name).await;
    assert_eq!(
        post_count, 1,
        "Normal-path PATCH with matching content_hash should NOT create a new brief version"
    );
}

// ============================================================================
// Brief content-revert regression (brief_units_brief_id_unit_key_key)
// ============================================================================

/// Fixture for the brief-content-revert regression. A single brief whose
/// `units` list is the only hash-sensitive knob:
/// - `include_extra = false` → units = [intro-md, input]            (hash H1)
/// - `include_extra = true`  → units = [intro-md, input, extra-md]  (hash H2 ≠ H1)
///
/// `hash_brief` is deterministic over {name, units}, so two `false` calls
/// produce byte-identical decompressed content → identical `hash_guide_directory`
/// AND identical brief `content_hash`. That lets a test ingest H1, PATCH to H2,
/// then PATCH back to H1 — driving a brief whose natural hash REVERTS to a
/// historical version that already exists in `rev.versioned_objects`.
fn create_brief_revert_test_tarball(suffix: &str, include_extra: bool) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Brief Revert Guide {suffix}\"\n\
         description = \"Fixture for the brief content-revert regression.\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Intro](intro.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // The brief's `units` list is the hash-sensitive knob. When `include_extra`
    // is set, a second brief-markdown unit is declared AND referenced, changing
    // the brief's {name, units} hash. Removing it reverts the hash exactly.
    let extra_block = if include_extra {
        format!(
            ":::brief-markdown {{ name: \"extra-md-{short}\" }}\n\
             Extra prose present only in the v2 variant.\n\
             :::\n\n"
        )
    } else {
        String::new()
    };
    let units = if include_extra {
        format!("[\"intro-md-{short}\", \"topic-input-{short}\", \"extra-md-{short}\"]")
    } else {
        format!("[\"intro-md-{short}\", \"topic-input-{short}\"]")
    };

    let intro = format!(
        "# Intro\n\n\
         :::value-slot {{ name: \"topic-slot-{short}\", kind: \"text\" }}\n\
         :::\n\n\
         :::brief-markdown {{ name: \"intro-md-{short}\" }}\n\
         Intro prose for the brief.\n\
         :::\n\n\
         :::brief-input {{ name: \"topic-input-{short}\", control: \"text\", slot: \"topic-slot-{short}\" }}\n\
         :::\n\n\
         {extra_block}\
         :::brief {{ name: \"topic-brief-{short}\", units: {units} }}\n\
         :::\n\n\
         :::brief-group {{ name: \"topic-group-{short}\", briefs: [\"topic-brief-{short}\"] }}\n\
         :::\n\n\
         :::brief-pack {{ name: \"topic-pack-{short}\", groups: [\"topic-group-{short}\"] }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/intro.md", &intro);

    let fluent = format!(
        "topic-input-{short} = Topic input label\n\
         topic-brief-{short} = Topic brief title\n\
         topic-group-{short} = Topic group title\n\
         topic-pack-{short} = Topic pack title\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);

    tarball
}

/// All `unit_key`s attached to a specific brief id, sorted. A duplicate-row
/// bug would surface as a 500 (the UNIQUE constraint blocks the second INSERT),
/// so this asserts the reverted-to version's child subtree is exactly intact.
async fn brief_unit_keys(db: &sqlx::PgPool, brief_id: uuid::Uuid) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT unit_key FROM rev.brief_units WHERE brief_id = $1 ORDER BY unit_key",
    )
    .bind(brief_id)
    .fetch_all(db)
    .await
    .expect("brief_units query failed");
    rows.into_iter().map(|r| r.0).collect()
}

/// Count `guide_brief_packs` rows for a guide id (the guide↔pack association).
async fn guide_brief_pack_count(db: &sqlx::PgPool, guide_id: uuid::Uuid) -> i64 {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM rev.guide_brief_packs WHERE guide_id = $1")
            .bind(guide_id)
            .fetch_one(db)
            .await
            .expect("guide_brief_packs count failed");
    row.0
}

/// Regression: re-ingesting a guide whose brief content REVERTS to a historical
/// hash must not crash on `brief_units_brief_id_unit_key_key`.
///
/// The bug (see `materialize_brief_system`): a brief whose natural hash equals a
/// PAST version (but differs from the LATEST) is classified PATCH; `patch_briefs`
/// then `ON CONFLICT (path_id, content_hash) DO NOTHING` resolves back to the
/// PRE-EXISTING brief id, which already carries its full unit subtree. The
/// child-creation loop re-inserted `[intro-md, input]` against a brief that
/// already had them → duplicate-key 500. `brief_groups` / `brief_packs`
/// Merkle-propagate from the brief hash and have the identical else-branch +
/// link-insert pattern, so the fix prunes all three gate sets.
///
/// Reproduces the production DB shape (corrupt-then-verify per
/// dapple-prompt-api AGENTS.md): v1 → v2 (different units) → v1 (revert).
#[tokio::test]
async fn test_reingest_brief_revert_to_historical_hash() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let brief_name = format!("topic-brief-{short}");

    let db = get_db_pool().await;

    // v1: brief units = [intro-md, input] → hash H1. Capture the brief id and
    //     its original child subtree.
    let v1_tarball = create_brief_revert_test_tarball(&suffix, false);
    let (path_id, guide_id) = initial_ingest(&base_url, v1_tarball.clone()).await;
    let guide_uuid = uuid::Uuid::parse_str(&guide_id).expect("guide id");

    let v1_brief_id = latest_brief_id_by_name(&db, &brief_name).await;
    let expected_units = vec![format!("intro-md-{short}"), format!("topic-input-{short}")];
    assert_eq!(
        brief_unit_keys(&db, v1_brief_id).await,
        expected_units,
        "v1 brief should have exactly [intro-md, input]"
    );
    assert_eq!(
        brief_version_count_by_name(&db, &brief_name).await,
        1,
        "exactly 1 brief version after POST"
    );

    // v2: add a second markdown unit → hash H2 ≠ H1. New guide content hash →
    //     201 Created, and a fresh brief version.
    let v2_tarball = create_brief_revert_test_tarball(&suffix, true);
    let (status_v2, json_v2) = ingest_tarball_patch(&base_url, &path_id, v2_tarball, false).await;
    assert_eq!(
        status_v2, 201,
        "v2 ingest (new content) response: {json_v2}"
    );
    assert_eq!(
        brief_version_count_by_name(&db, &brief_name).await,
        2,
        "v2 should create a second brief version"
    );

    // v3: revert to the original units → hash reverts to H1, which already
    //     exists in versioned_objects. This is the crash trigger. The guide
    //     content is byte-identical to v1, so this routes through the refresh
    //     branch (200) in Normal mode — no `?backfill=true`.
    let v3_tarball = create_brief_revert_test_tarball(&suffix, false);
    let (status_v3, json_v3) = ingest_tarball_patch(&base_url, &path_id, v3_tarball, false).await;
    assert_eq!(
        status_v3, 200,
        "v3 revert must succeed (pre-fix: 500 brief_units_brief_id_unit_key_key); response: {json_v3}"
    );
    assert_eq!(json_v3["status"], "refreshed");

    // The revert resolved (via ON CONFLICT) to the v1 brief id without creating
    // a new version and WITHOUT re-creating its children: v1 still has exactly
    // its original two units, and no third version appeared.
    assert_eq!(
        brief_unit_keys(&db, v1_brief_id).await,
        expected_units,
        "reverted-to brief must still have exactly [intro-md, input] (no duplicate/extra units)"
    );
    assert_eq!(
        brief_version_count_by_name(&db, &brief_name).await,
        2,
        "revert must resolve to the existing v1 version, not create a third"
    );

    // The guide↔brief_pack association still resolves after the refresh.
    assert!(
        guide_brief_pack_count(&db, guide_uuid).await >= 1,
        "guide must still be associated with its brief_pack after the revert"
    );
}

// ============================================================================
// Journey ↔ Guide version pinning regression
// ============================================================================
//
// Guarantee: a journey captures its `guide_id` at create time and continues
// resolving against that specific guide version even after a later re-ingest
// produces a new version at the same path. Brief-pack auto-derive must walk
// `journey.guide_id → guide_brief_packs`, not through the playbook's full
// guide-version history. A regression here would re-introduce the duplicate-
// attachment bug we just fixed (a 5-min-old journey ending up with brief_packs
// from every historical guide version).

/// Minimal "playbook + brief-pack" fixture for the pinning regression. Has
/// exactly one stage with one step backed by a single nushell task, plus a
/// brief-pack with one brief/group. The `variant` knob changes a content-
/// hash-sensitive piece (the nushell payload) so a second ingest at the same
/// path produces a distinct guide version without invalidating any FK links.
fn create_pin_fixture_tarball(suffix: &str, variant: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Pin Fixture {suffix}\"\n\
         description = \"Pin regression fixture, variant={variant}\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    let summary = "# Summary\n\n- [Workflow](workflow.md)\n";
    append_file(&mut archive, "guide/SUMMARY.md", summary);
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let workflow = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"pin-slot-{short}\", kind: \"text\" }}\n\
         :::\n\n\
         :::brief-input {{ name: \"pin-input-{short}\", control: \"text\", slot: \"pin-slot-{short}\" }}\n\
         :::\n\n\
         :::brief {{ name: \"pin-brief-{short}\", units: [\"pin-input-{short}\"] }}\n\
         :::\n\n\
         :::brief-group {{ name: \"pin-group-{short}\", briefs: [\"pin-brief-{short}\"] }}\n\
         :::\n\n\
         :::brief-pack {{ name: \"pin-pack-{short}\", groups: [\"pin-group-{short}\"] }}\n\
         :::\n\n\
         :::stage {{ name: \"Plan-{short}\" }}\n\n\
         :::step {{ name: \"Run-{short}\", task: \"pin-task-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"pin-task-{short}\", tags: [coder] }}\n\
         {{ payload: {{ done: true, variant: \"{variant}\" }}, signals: {{}} }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/workflow.md", &workflow);

    let fluent = format!(
        "pin-input-{short} = Pin input label\n\
         pin-brief-{short} = Pin brief title\n\
         pin-group-{short} = Pin group title\n\
         pin-pack-{short} = Pin pack title\n\
         plan-{short} = Plan stage description.\n\
         run-{short} = Run step description.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);

    tarball
}

/// Fetch a journey's pinned `guide_id` and its current `journey_brief_packs`
/// attachments directly from the DB so the test can compare snapshots.
async fn journey_pin_snapshot(
    db: &sqlx::PgPool,
    journey_id: uuid::Uuid,
) -> (Option<uuid::Uuid>, Vec<uuid::Uuid>) {
    let guide_id: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT guide_id FROM rev.journeys WHERE id = $1")
            .bind(journey_id)
            .fetch_one(db)
            .await
            .expect("journey lookup failed");

    let mut packs: Vec<uuid::Uuid> = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.journey_brief_packs WHERE journey_id = $1",
    )
    .bind(journey_id)
    .fetch_all(db)
    .await
    .expect("journey_brief_packs lookup failed");
    packs.sort();
    (guide_id, packs)
}

/// Resolve the playbook id derived from a guide ingest response. The
/// brief-pack fixture produces exactly one playbook.
fn playbook_id_from_ingest(json: &serde_json::Value) -> uuid::Uuid {
    let raw = json["derived_objects"]["playbook_ids"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .expect("derived_objects.playbook_ids[0]");
    uuid::Uuid::parse_str(raw).expect("playbook_id parse")
}

/// POST a journey via the HTTP surface and return its UUID + the body. The
/// journey-POST handler now REQUIRES an explicit `guide_id` and validates that
/// the guide exposes the playbook (there is no auto-pin), so callers pass the
/// guide_id resolved from the ingest that created the `(guide, playbook)` link.
async fn post_journey_for_playbook(
    base_url: &str,
    playbook_id: uuid::Uuid,
    guide_id: uuid::Uuid,
    name: &str,
) -> (reqwest::StatusCode, serde_json::Value) {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "objs": [{
            "name": name,
            "playbook_id": playbook_id,
            "guide_id": guide_id,
            "brief_pack_ids": [],
            "inference_profile_ids": []
        }]
    });
    let resp = client
        .post(format!("{}/api/journeys/", base_url))
        .header("Authorization", test_auth_header())
        .json(&body)
        .send()
        .await
        .expect("POST journey request failed");
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.expect("invalid journey response json");
    (status, json)
}

#[tokio::test]
async fn test_journey_guide_pin_stable_across_reingest() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let db = get_db_pool().await;

    // 1. Ingest v1 of the fixture; capture the derived playbook + guide_id.
    //    The pin fixture defines BOTH a brief-pack and a 1-stage / 1-step
    //    playbook so we can create a journey (needs playbook_id) AND
    //    exercise brief_pack auto-derive (needs at least one pack).
    let tarball_v1 = create_pin_fixture_tarball(&suffix, "v1");
    let v1_json = ingest_tarball(&base_url, tarball_v1).await;
    let guide_id_v1 = uuid::Uuid::parse_str(
        v1_json["guide"]["id"]
            .as_str()
            .expect("guide.id from v1 ingest"),
    )
    .expect("guide_id_v1 parse");
    let path_id = v1_json["guide"]["path_id"]
        .as_str()
        .expect("guide.path_id from v1 ingest")
        .to_string();
    let playbook_id = playbook_id_from_ingest(&v1_json);

    // 2. POST a journey against the playbook with an EXPLICIT guide_id of
    //    guide_id_v1 (the guide that just exposed this playbook). The handler
    //    validates the pair is linked and stamps the journey with v1's guide_id.
    let (status, j1_body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id_v1,
        &format!("pin-journey-1-{suffix}"),
    )
    .await;
    assert!(
        status.is_success(),
        "POST journey #1 status {status}: {j1_body}"
    );
    let journey_1_id =
        uuid::Uuid::parse_str(j1_body["objs"][0]["id"].as_str().expect("journey #1 id"))
            .expect("journey_1_id parse");
    let (j1_guide_after_create, j1_packs_initial) = journey_pin_snapshot(&db, journey_1_id).await;
    assert_eq!(
        j1_guide_after_create,
        Some(guide_id_v1),
        "journey #1 must be pinned to guide_id_v1 at create time"
    );
    assert!(
        !j1_packs_initial.is_empty(),
        "journey #1 must auto-derive at least one brief_pack from guide_id_v1"
    );

    // 3. Re-ingest with a different `variant` so the nushell script body
    //    changes — that flips the content_hash and forces a new guide
    //    version at the same path. Confirm a second guide row exists.
    let tarball_v2 = create_pin_fixture_tarball(&suffix, "v2-revised");
    let (patch_status, patch_json) =
        ingest_tarball_patch(&base_url, &path_id, tarball_v2, false).await;
    assert!(
        patch_status.is_success() || patch_status == reqwest::StatusCode::CREATED,
        "PATCH ingest v2 status {patch_status}: {patch_json}"
    );
    let path_uuid = uuid::Uuid::parse_str(&path_id).expect("path_id parse");
    let guide_versions_at_path: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM rev.guides g
         JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
         WHERE vo.path_id = $1",
    )
    .bind(path_uuid)
    .fetch_one(&db)
    .await
    .expect("guide version count");
    assert!(
        guide_versions_at_path >= 2,
        "v2 re-ingest must produce a second guide row at the path; got {guide_versions_at_path}",
    );

    // 4. PINNING ASSERTION — journey #1's guide_id must STILL be v1, and
    //    its brief_pack attachments must be unchanged. This is the
    //    regression guard for the "shifts underfoot on re-ingest" bug.
    let (j1_guide_after_reingest, j1_packs_after_reingest) =
        journey_pin_snapshot(&db, journey_1_id).await;
    assert_eq!(
        j1_guide_after_reingest,
        Some(guide_id_v1),
        "journey #1 guide_id must remain pinned to v1 after a v2 re-ingest"
    );
    assert_eq!(
        j1_packs_after_reingest, j1_packs_initial,
        "journey #1 brief_pack attachments must be unchanged after a v2 re-ingest"
    );

    // Note: "new journey created after re-ingest picks v2" is NOT a
    // separate guarantee at this layer — playbooks are themselves
    // versioned objects, so a re-ingest creates a new playbook version
    // with a new id. A POST against the OLD playbook_id will (correctly)
    // pin to v1; picking up v2 requires the caller to POST against the
    // new playbook_id surfaced by the patch response. The contract we're
    // guarding here is the pinning STABILITY of an existing journey,
    // which is what Step 4 just asserted.
}

/// A guide whose chapter declares the SAME value-slot name twice. Exercises the
/// materializer's name-dedup: without it, each occurrence PATCH/INSERTs a
/// separate version at one path in a single transaction.
fn create_dup_value_slot_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];
    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Dup Value Slot Guide {suffix}\"\n\
         description = \"Same value-slot name declared twice\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // slot-dup-{short} declared TWICE (identical definitions).
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-dup-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"slot-dup-{short}\", kind: \"text\" }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Count value_slot VERSIONS at the path of a given value-slot name.
async fn value_slot_version_count(sql: &sqlx::PgPool, name: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM rev.value_slot_path_details vspd \
         JOIN rev.object_paths op ON op.id = vspd.path_id \
         JOIN rev.versioned_objects vo ON vo.path_id = op.id \
         JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id \
         WHERE vspd.name = $1",
    )
    .bind(name)
    .fetch_one(sql)
    .await
    .expect("value_slot version count")
}

/// Guard regression: duplicate value-slot names are rejected at ingest
/// (`validate_ingest_plan` → `check_unique_names`), BEFORE materialization. This
/// is why the materializer can assume `derived_value_slots` is name-unique and a
/// single ingest never mints two versions at one value_slot path.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_duplicate_value_slot_name_rejected_at_ingest() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(create_dup_value_slot_guide_tarball(&suffix))
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let resp = reqwest::Client::new()
        .post(format!("{}/api/guides/ingest", base_url))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("ingest request failed");
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.expect("invalid json");

    assert_ne!(
        status, 201,
        "duplicate value-slot names must NOT ingest: {json}"
    );
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Duplicate :::value-slot"),
        "rejection should name the duplicate value-slot guard: {json}"
    );
}

/// A stage/step `expose:` that names something other than a declared
/// `:::value-slot` (a typo, or the brief-unit name instead of the slot slug) used
/// to be SILENTLY skipped by the materializer (`warn!(...skipped)`) — the guide
/// ingested "successfully" with a quietly wrong overlay set. `validate_ingest_plan`
/// now rejects it. This is the loud-failure guard behind the o-sítio
/// empty-brief-form class of bug.
#[tokio::test]
async fn test_expose_unknown_value_slot_rejected_at_ingest() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(create_unresolved_expose_guide_tarball(&suffix))
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let resp = reqwest::Client::new()
        .post(format!("{}/api/guides/ingest", base_url))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("ingest request failed");
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.expect("invalid json");

    assert_ne!(
        status, 201,
        "a stage exposing a non-value-slot name must NOT ingest: {json}"
    );
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("exposes/masks value slots that were never declared"),
        "rejection should name the unresolved-expose guard: {json}"
    );
}

/// Guide that declares `slot-a` and a brief-input pinning it, but whose stage
/// exposes a name (`bogus-…`) that is NOT a declared value-slot. Otherwise valid
/// (passes every earlier `validate_ingest_plan` check) so it reaches Check A.
fn create_unresolved_expose_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Unresolved Expose Guide {suffix}\"\n\
         description = \"Stage exposes a name that is not a declared value-slot\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // `slot-a` is declared and pinned by `input-a`, but the stage exposes
    // `bogus-{short}` (never declared) — Check A must reject this.
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-a-{short}\", kind: \"text\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-a-{short}\", control: \"text\", slot: \"slot-a-{short}\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-a-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"bogus-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "input-a-{short} = Topic A input\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// A1/A2 regression: re-ingesting the IDENTICAL guide bytes (a no-op save) must
/// not mint any new value_slot versions — content-unchanged slots REUSE.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_noop_reingest_mints_no_new_value_slot_versions() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;

    let v1 = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix)).await;
    let path_id = v1["guide"]["path_id"]
        .as_str()
        .expect("guide.path_id")
        .to_string();

    let name = format!("slot-a-{short}");
    let before = value_slot_version_count(&sql, &name).await;
    assert_eq!(before, 1, "first ingest mints one version");

    // Re-ingest the SAME bytes (Normal mode). Guide content_hash matches →
    // refresh re-runs the materializers, which REUSE every unchanged slot.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_overlay_brief_guide_tarball(&suffix),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "no-op re-ingest status {status}: {body}"
    );

    let after = value_slot_version_count(&sql, &name).await;
    assert_eq!(
        before, after,
        "a no-op re-ingest must not mint new value_slot versions ({before} -> {after})"
    );
}

/// A minimal guide whose sole value-slot's `kind` is parameterized. Re-ingesting
/// with a different `kind` is the exact crosstalk repro (a slot that changed
/// text->json between guide versions).
fn create_kind_slot_tarball(suffix: &str, kind: &str) -> Vec<u8> {
    use flate2::{Compression, write::GzEncoder};
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];
    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Kind Fixture {suffix}\"\n\
         description = \"value_kind versioning fixture\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](workflow.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let workflow = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"kind-slot-{short}\", kind: \"{kind}\" }}\n\
         :::\n\n\
         :::brief-input {{ name: \"kind-input-{short}\", control: \"text\", slot: \"kind-slot-{short}\" }}\n\
         :::\n\n\
         :::brief {{ name: \"kind-brief-{short}\", units: [\"kind-input-{short}\"] }}\n\
         :::\n\n\
         :::brief-group {{ name: \"kind-group-{short}\", briefs: [\"kind-brief-{short}\"] }}\n\
         :::\n\n\
         :::brief-pack {{ name: \"kind-pack-{short}\", groups: [\"kind-group-{short}\"] }}\n\
         :::\n\n\
         :::stage {{ name: \"Plan-{short}\" }}\n\n\
         :::step {{ name: \"Run-{short}\", task: \"kind-task-{short}\" }}\n\
         :::\n\n\
         :::\n\n\
         :::nushell {{ name: \"kind-task-{short}\", tags: [coder] }}\n\
         {{ payload: {{ done: true }}, signals: {{}} }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/workflow.md", &workflow);

    let fluent = format!(
        "kind-input-{short} = Kind input label\n\
         kind-brief-{short} = Kind brief title\n\
         kind-group-{short} = Kind group title\n\
         kind-pack-{short} = Kind pack title\n\
         plan-{short} = Plan stage description.\n\
         run-{short} = Run step description.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// The kind of a value-slot version at the extreme of creation order (latest or earliest).
async fn value_slot_kind_at(sql: &sqlx::PgPool, name: &str, latest: bool) -> String {
    const LATEST: &str = "SELECT vk.key FROM rev.value_slot_path_details vspd \
         JOIN rev.object_paths op ON op.id = vspd.path_id \
         JOIN rev.versioned_objects vo ON vo.path_id = op.id \
         JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id \
         JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id \
         WHERE vspd.name = $1 ORDER BY vo.created_at DESC, vo.id DESC LIMIT 1";
    const EARLIEST: &str = "SELECT vk.key FROM rev.value_slot_path_details vspd \
         JOIN rev.object_paths op ON op.id = vspd.path_id \
         JOIN rev.versioned_objects vo ON vo.path_id = op.id \
         JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id \
         JOIN rev.value_kinds vk ON vk.id = vs.value_kind_id \
         WHERE vspd.name = $1 ORDER BY vo.created_at ASC, vo.id ASC LIMIT 1";
    sqlx::query_scalar::<_, String>(if latest { LATEST } else { EARLIEST })
        .bind(name)
        .fetch_one(sql)
        .await
        .expect("value_slot kind lookup")
}

/// Migration 70 repro: a value-slot whose `kind` changes text->json across guide
/// versions must RE-VERSION and the LATEST version must carry the new kind (json),
/// while the earliest version keeps text (append-only). Pre-fix, kind was frozen on
/// the path and the json declaration silently never took effect (the crosstalk
/// "[object Object]" bug). Corrupt-then-verify entirely in SQL against the version.
#[tokio::test]
async fn test_value_slot_kind_change_reversions_to_new_kind() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let name = format!("kind-slot-{short}");

    // v1: kind = text.
    let v1 = ingest_tarball(&base_url, create_kind_slot_tarball(&suffix, "text")).await;
    let path_id = v1["guide"]["path_id"]
        .as_str()
        .expect("guide.path_id")
        .to_string();
    assert_eq!(
        value_slot_version_count(&sql, &name).await,
        1,
        "v1 mints one version"
    );
    assert_eq!(
        value_slot_kind_at(&sql, &name, true).await,
        "text",
        "v1 latest is text"
    );

    // v2: SAME slot, kind = json. Must re-version (kind is now in the content hash).
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_kind_slot_tarball(&suffix, "json"),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "kind-change re-ingest status {status}: {body}"
    );

    assert_eq!(
        value_slot_version_count(&sql, &name).await,
        2,
        "a kind change must mint a new value_slot version"
    );
    assert_eq!(
        value_slot_kind_at(&sql, &name, true).await,
        "json",
        "the LATEST version must carry the new kind (json) -- the crosstalk fix"
    );
    assert_eq!(
        value_slot_kind_at(&sql, &name, false).await,
        "text",
        "the earliest version keeps text (append-only, no in-place mutation)"
    );
}

/// B regression: `select_step_binding_status_for_journey` must report the
/// JOURNEY-PINNED value_slot version (what `/brief-form` exposes), NOT the
/// global-latest version at the path. Corrupt-then-verify: seed a newer
/// value_slot version at the exposed slot's path (simulating re-ingest churn),
/// then assert `/steps` still returns the version the journey's brief pins.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_steps_reports_journey_pinned_value_slot_version() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    // 1. Ingest a guide whose stage exposes slot-a and whose brief input pins
    //    slot-a, then capture the v1 value_slot the brief references.
    let json = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix)).await;
    let guide_id =
        uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&json);
    let slot_name = format!("slot-a-{short}");
    let slot_a_v1 = value_slot_id_by_name(sql, &slot_name).await;

    // 2. Create the journey via the POST handler so it auto-derives the guide's
    //    brief_packs (which pin slot_a_v1 through the brief input's control_spec).
    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("steps-pin-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");

    // 3. CORRUPT: seed a NEWER value_slot version at slot-a's path (a higher
    //    UUIDv7 + later created_at), exactly the churn a re-ingest would create.
    //    This becomes the global-latest the buggy query would wrongly return.
    let slot_a_v2: uuid::Uuid = sqlx::query_scalar(
        "WITH src AS ( \
             SELECT vo.path_id, vs.title, vs.description, vs.constraints, vs.value_kind_id \
             FROM rev.value_slots vs \
             JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id \
             WHERE vs.id = $1 \
         ), nvo AS ( \
             INSERT INTO rev.versioned_objects (path_id, content_hash) \
             SELECT path_id, repeat('f', 40) FROM src \
             RETURNING id \
         ) \
         INSERT INTO rev.value_slots (versioned_object_id, title, description, constraints, value_kind_id) \
         SELECT nvo.id, src.title, src.description, src.constraints, src.value_kind_id FROM nvo, src \
         RETURNING id",
    )
    .bind(slot_a_v1)
    .fetch_one(sql)
    .await
    .expect("seed newer value_slot version");
    assert_ne!(slot_a_v2, slot_a_v1, "seeded a distinct newer version");
    assert_eq!(
        value_slot_id_by_name(sql, &slot_name).await,
        slot_a_v2,
        "the seeded version is now the global-latest at the path"
    );

    // 4. VERIFY: /steps must report the journey-PINNED v1 (matching /brief-form),
    //    not the global-latest v2.
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    let slot = steps
        .iter()
        .flat_map(|s| s.exposed_slots.iter())
        .find(|s| s.name == slot_name)
        .unwrap_or_else(|| panic!("slot-a not exposed by any step: {steps:?}"));
    assert_eq!(
        slot.value_slot_id,
        Some(slot_a_v1),
        "/steps must report the journey-pinned value_slot version (v1), not the global-latest (v2)"
    );
}

/// C regression: journey-create validates that the caller-supplied
/// `(guide_id, playbook_id)` pair is consistent — the guide must expose the
/// playbook. A guide from one ingest + a playbook from another is rejected;
/// there is no auto-pin to paper over the mismatch.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_journey_create_rejects_guide_playbook_mismatch() {
    let (base_url, _handle) = spawn_app().await;
    let suffix_a = uuid::Uuid::now_v7().to_string();
    let suffix_b = uuid::Uuid::now_v7().to_string();

    let a = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix_a)).await;
    let b = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix_b)).await;
    let playbook_a = playbook_id_from_ingest(&a);
    let guide_b =
        uuid::Uuid::parse_str(b["guide"]["id"].as_str().expect("guide.id")).expect("guide_b");

    // playbook from ingest A + guide from ingest B → not linked in guide_playbooks.
    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_a,
        guide_b,
        &format!("mismatch-{}", &suffix_a[28..]),
    )
    .await;
    assert!(
        !status.is_success(),
        "mismatched (guide_id, playbook_id) must be rejected, got {status}: {body}"
    );
    let text = body.to_string();
    assert!(
        text.contains("does not expose playbook") || text.contains("GUIDE_PLAYBOOK_MISMATCH"),
        "rejection should explain the guide/playbook mismatch: {body}"
    );
}

// ============================================================================
// Value-slot overlay materialization (append-only + point-in-time integrity)
// ============================================================================

/// Build a single-chapter guide tarball whose one stage exposes the given
/// value slots. A `:::value_slot` is declared for each exposed slot so overlay
/// name resolution has something to bind to. The same `(suffix, slots)` always
/// yields identical file content, so an unchanged re-ingest is a hash match.
fn create_overlay_guide_tarball(suffix: &str, expose_slots: &[String]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Overlay Guide {suffix}\"\n\
         description = \"Guide with stage value-slot overlays\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\""
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let mut value_slots = String::new();
    for slot in expose_slots {
        value_slots.push_str(&format!(
            ":::value_slot {{ name: \"{slot}\", kind: \"text\" }}\n:::\n\n"
        ));
    }
    // Stage exposes the slots (omit the attribute entirely when none, so the
    // empty-slots fixture represents a guide with no overlay authoring at all).
    let stage_attrs = if expose_slots.is_empty() {
        "name: \"Stage One\"".to_string()
    } else {
        let expose_list = expose_slots
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("name: \"Stage One\", expose: [{expose_list}]")
    };
    // Every :::step must reference a :::nushell task (ingest validation), and
    // the overlay resolver joins through step_tasks — so the stage only gets a
    // playbook_stage row reachable for overlays when its step has a task.
    let task = format!("task-one-{suffix}");
    let ch1 = format!(
        "# Workflow\n\n\
         {value_slots}\
         :::stage {{ {stage_attrs} }}\n\n\
         :::step {{ name: \"Step One\", task: \"{task}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"{task}\" }}\n\
         {{ payload: {{}}, signals: {{}} }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);
    append_file(
        &mut archive,
        "guide/locales/en.ftl",
        "stage-one = The only stage.\nstep-one = The only step.\n",
    );

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Stage-overlay row ids for a playbook (joined through its playbook_stages).
/// Stable ids across re-ingest prove append-only behaviour: a delete+reinsert
/// would mint fresh UUIDv7 ids.
async fn stage_overlay_ids(sql: &sqlx::PgPool, playbook_id: &str) -> Vec<uuid::Uuid> {
    let pid = uuid::Uuid::parse_str(playbook_id).expect("playbook uuid");
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT o.id FROM rev.playbook_stage_overlays o \
         JOIN rev.playbook_stages ps ON ps.id = o.playbook_stage_id \
         WHERE ps.playbook_id = $1 ORDER BY o.id",
    )
    .bind(pid)
    .fetch_all(sql)
    .await
    .expect("overlay id query")
}

/// B1+B2+B3: ingest populates overlays; an identical re-ingest doesn't churn
/// them; changing the exposed slot set forks a NEW playbook version while the
/// OLD version's overlays stay byte-identical (point-in-time integrity).
///
/// Serialized with the other overlay ingest test: two guide ingests running
/// concurrently can deadlock in the (pre-existing) fluent-message
/// materialization phase on shared locale rows — unrelated to overlays.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_overlay_authoring_append_only_and_point_in_time() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let topic = format!("topic-{suffix}");
    let audience = format!("audience-{suffix}");

    let pool = test_db_pool();
    let sql = pool.get_pool();

    // v1: stage exposes [topic]
    let v1_json = ingest_tarball(
        &base_url,
        create_overlay_guide_tarball(&suffix, std::slice::from_ref(&topic)),
    )
    .await;
    let v1_pb = v1_json["derived_objects"]["playbook_ids"][0]
        .as_str()
        .expect("v1 playbook_id");
    let path_id = v1_json["guide"]["path_id"]
        .as_str()
        .expect("path_id")
        .to_string();

    // B1: authoring populated exactly one (expose) overlay on the stage.
    let v1_ids = stage_overlay_ids(sql, v1_pb).await;
    assert_eq!(
        v1_ids.len(),
        1,
        "v1 stage should carry exactly one expose overlay (got {v1_ids:?})"
    );

    // B2: identical re-ingest is a content-hash match -> 200 "refreshed". The
    // refresh path re-runs the materializers (incl. the overlay pass), so this
    // exercises ON CONFLICT DO NOTHING: overlay ids must be unchanged (a
    // delete+reinsert would mint new ids).
    let (status_same, json_same) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_overlay_guide_tarball(&suffix, std::slice::from_ref(&topic)),
        false,
    )
    .await;
    assert_eq!(
        status_same, 200,
        "identical re-ingest should be 200 refreshed: {json_same}"
    );
    assert_eq!(
        stage_overlay_ids(sql, v1_pb).await,
        v1_ids,
        "identical re-ingest must not mutate overlays (append-only / ON CONFLICT)"
    );

    // B3: changed expose set ([topic] -> [topic, audience]) forks a new version.
    let (status_v2, v2_json) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_overlay_guide_tarball(&suffix, &[topic.clone(), audience.clone()]),
        false,
    )
    .await;
    assert_eq!(
        status_v2, 201,
        "changed-overlay re-ingest should create a new version: {v2_json}"
    );
    let v2_pb = v2_json["derived_objects"]["playbook_ids"][0]
        .as_str()
        .expect("v2 playbook_id");
    assert_ne!(
        v1_pb, v2_pb,
        "an overlay edit must fork a new playbook version"
    );

    // Point-in-time integrity: v1's overlays are byte-identical (same ids =>
    // never deleted/reinserted) even though a new version now exists.
    assert_eq!(
        stage_overlay_ids(sql, v1_pb).await,
        v1_ids,
        "v1 overlays must be untouched after a v2 re-ingest"
    );
    // v2 exposes both slots.
    assert_eq!(
        stage_overlay_ids(sql, v2_pb).await.len(),
        2,
        "v2 stage should expose both slots"
    );
}

/// B4 (strict default): a guide whose stages declare no expose/mask produces
/// zero overlays, so the resolver yields an empty required set.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_overlay_strict_default_no_overlays() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    // Same fixture as the overlay tests but with NO exposed slots: a valid
    // guide (step has a task) whose stage declares no expose/mask.
    let json = ingest_tarball(&base_url, create_overlay_guide_tarball(&suffix, &[])).await;
    let pb = json["derived_objects"]["playbook_ids"][0]
        .as_str()
        .expect("playbook_id");

    assert!(
        stage_overlay_ids(sql, pb).await.is_empty(),
        "a guide with no expose/mask must produce zero overlays (strict default)"
    );
}

/// Combined fixture for the deeper completion + brief-form tests. One playbook
/// whose single stage exposes `slot-a` (only). Two value slots (`slot-a`,
/// `slot-b`) each wired to a brief input, all wrapped in one brief-pack.
fn create_overlay_brief_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Overlay Brief Guide {suffix}\"\n\
         description = \"Stage overlay + briefs on the same value slots\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-a-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"slot-b-{short}\", kind: \"text\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-a-{short}\", control: \"text\", slot: \"slot-a-{short}\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-b-{short}\", control: \"text\", slot: \"slot-b-{short}\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-a-{short}\", \"input-b-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"slot-a-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "input-a-{short} = Topic A input\n\
         input-b-{short} = Topic B input\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Latest value_slots.id for a value-slot path by name.
async fn value_slot_id_by_name(sql: &sqlx::PgPool, name: &str) -> uuid::Uuid {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT vs.id FROM rev.value_slots vs \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id \
         WHERE vspd.name = $1 ORDER BY vo.created_at DESC LIMIT 1",
    )
    .bind(name)
    .fetch_one(sql)
    .await
    .expect("value_slot by name")
}

const DAPPLE_COMPANY: &str = "019e132e-2610-7a4a-b64a-77a49e286ac0";

/// Resolve the test user's human principal (seeded by spawn_app).
async fn test_principal_id(sql: &sqlx::PgPool) -> uuid::Uuid {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT hp.principal_id FROM rev.human_principals hp WHERE hp.user_id = $1",
    )
    .bind(test_auth_user_id())
    .fetch_one(sql)
    .await
    .expect("resolve test principal")
}

/// Deeper completion test (B5): a stage exposing `slot-a` is NOT done until a
/// dispatch binds `slot-a`, then flips to done. Exercises is_stage_fully_bound
/// and the per-journey status view end-to-end (corrupt-then-verify in SQL).
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_stage_completion_flips_after_binding() {
    use dapple_data::query::journeys::{
        is_stage_fully_bound, select_stage_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix)).await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();

    let stage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(playbook_id)
    .fetch_one(sql)
    .await
    .expect("playbook_stage");
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{short}")).await;

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("OverlayCompletion-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    // Before binding: slot-a is unbound, so the stage is not done.
    assert!(
        !is_stage_fully_bound(&pool, journey_id, stage_id)
            .await
            .expect("is_stage_fully_bound before"),
        "stage must NOT be done before slot-a is bound"
    );
    let before = select_stage_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("status before");
    assert!(
        before
            .iter()
            .any(|s| s.playbook_stage_id == stage_id && !s.done),
        "per-journey view must report the stage not-done before binding: {before:?}"
    );

    // Bind slot-a via a dispatch on the journey.
    let dispatch_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatches (journey_id, user_id, intent, owner_company_id, principal_id, created_at) \
         VALUES ($1, $2, '{}'::jsonb, $3, $4, now()) RETURNING id",
    )
    .bind(journey_id)
    .bind(test_auth_user_id())
    .bind(dapple_company)
    .bind(principal_id)
    .fetch_one(sql)
    .await
    .expect("seed dispatch");
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now())",
    )
    .bind(dispatch_id)
    .bind(slot_a)
    .execute(sql)
    .await
    .expect("seed binding");

    // After binding slot-a (the stage's only exposed slot) the stage is done.
    assert!(
        is_stage_fully_bound(&pool, journey_id, stage_id)
            .await
            .expect("is_stage_fully_bound after"),
        "stage must be done after slot-a is bound"
    );
    let after = select_stage_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("status after");
    assert!(
        after
            .iter()
            .any(|s| s.playbook_stage_id == stage_id && s.done),
        "per-journey view must report the stage done after binding: {after:?}"
    );

    // The internal HTTP endpoint surfaces the same result.
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/journeys/{}/stages", base_url, journey_id))
        .header("authorization", test_auth_header())
        .send()
        .await
        .expect("GET /stages failed");
    assert!(
        resp.status().is_success(),
        "GET /stages status {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.expect("invalid /stages json");
    let objs = body["objs"].as_array().expect("objs array");
    let stage_done = objs.iter().any(|o| {
        o["playbook_stage_id"].as_str() == Some(stage_id.to_string().as_str())
            && o["done"].as_bool() == Some(true)
    });
    assert!(stage_done, "GET /stages must report the stage done: {body}");
}

/// Step-scoped twin of `test_stage_completion_flips_after_binding`: a step whose
/// stage exposes `slot-a` is NOT done until a dispatch binds `slot-a`, then flips
/// to done. Exercises is_step_fully_bound, select_step_binding_status_for_journey,
/// and GET /api/journeys/{id}/steps end-to-end (corrupt-then-verify in SQL). This
/// is the gate that lets a refine step idle until its pick is bound.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_step_completion_flips_after_binding() {
    use dapple_data::query::journeys::{
        is_step_fully_bound, select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix)).await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();

    let stage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(playbook_id)
    .fetch_one(sql)
    .await
    .expect("playbook_stage");
    let step_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss \
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
         WHERE ps.id = $1 ORDER BY ss.id LIMIT 1",
    )
    .bind(stage_id)
    .fetch_one(sql)
    .await
    .expect("stage_step");
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{short}")).await;

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("StepCompletion-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    // Before binding: the step exposes slot-a (via its stage), so it is not done.
    assert!(
        !is_step_fully_bound(&pool, journey_id, stage_id, step_id)
            .await
            .expect("is_step_fully_bound before"),
        "step must NOT be done before slot-a is bound"
    );
    let before = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("status before");
    let before_step = before
        .iter()
        .find(|s| s.stage_step_id == step_id)
        .expect("step present in status");
    assert!(!before_step.done, "step must be not-done before binding");
    // This journey is seeded directly with NO pinned brief_packs, so neither the
    // operator nor the computed slot resolution finds a journey-pinned version —
    // the exposed slot resolves with `value_slot_id == None` (the genuine no-pin
    // edge case). It is still identified by `name`, tracked by the path-based
    // `bound` flag, and reports `producer = "operator"`. (When a pinned pack
    // exists, both operator inputs and computed outputs carry an id; see
    // ExposedSlot docs.)
    let slot_a_name = format!("slot-a-{short}");
    assert!(
        before_step
            .exposed_slots
            .iter()
            .any(|s| s.name == slot_a_name && !s.bound),
        "exposed_slots must list slot-a as unbound: {before_step:?}"
    );

    // Bind slot-a via a dispatch on the journey.
    let dispatch_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatches (journey_id, user_id, intent, owner_company_id, principal_id, created_at) \
         VALUES ($1, $2, '{}'::jsonb, $3, $4, now()) RETURNING id",
    )
    .bind(journey_id)
    .bind(test_auth_user_id())
    .bind(dapple_company)
    .bind(principal_id)
    .fetch_one(sql)
    .await
    .expect("seed dispatch");
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now())",
    )
    .bind(dispatch_id)
    .bind(slot_a)
    .execute(sql)
    .await
    .expect("seed binding");

    // After binding slot-a (the step's only exposed slot) the step is done.
    assert!(
        is_step_fully_bound(&pool, journey_id, stage_id, step_id)
            .await
            .expect("is_step_fully_bound after"),
        "step must be done after slot-a is bound"
    );
    let after = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("status after");
    let after_step = after
        .iter()
        .find(|s| s.stage_step_id == step_id)
        .expect("step present after");
    assert!(after_step.done, "step must be done after binding");
    assert!(
        after_step
            .exposed_slots
            .iter()
            .any(|s| s.name == slot_a_name && s.bound),
        "exposed_slots must report slot-a bound after binding: {after_step:?}"
    );

    // The internal HTTP endpoint surfaces the same result.
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/journeys/{}/steps", base_url, journey_id))
        .header("authorization", test_auth_header())
        .send()
        .await
        .expect("GET /steps failed");
    assert!(
        resp.status().is_success(),
        "GET /steps status {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.expect("invalid /steps json");
    let objs = body["objs"].as_array().expect("objs array");
    let step_done = objs.iter().any(|o| {
        o["stage_step_id"].as_str() == Some(step_id.to_string().as_str())
            && o["done"].as_bool() == Some(true)
    });
    assert!(step_done, "GET /steps must report the step done: {body}");
}

/// Deeper brief-form lockstep test: the journey's stage exposes only `slot-a`,
/// so the operator form shows the `slot-a` input and hides the `slot-b` input.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_brief_form_filters_to_exposed_slots() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix)).await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide id")).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{short}")).await;
    let slot_b = value_slot_id_by_name(sql, &format!("slot-b-{short}")).await;

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("OverlayForm-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach brief_pack");

    let (form, _etag) = dapple_data::view::journey_brief_form::build_brief_form(&pool, journey_id)
        .await
        .expect("build_brief_form");

    let input_slots: Vec<uuid::Uuid> = form
        .briefs
        .iter()
        .flat_map(|b| b.units.iter())
        .filter_map(|u| u.input.as_ref().map(|i| i.value_slot_id))
        .collect();

    assert!(
        input_slots.contains(&slot_a),
        "brief-form must show the exposed input (slot-a); got {input_slots:?}"
    );
    assert!(
        !input_slots.contains(&slot_b),
        "brief-form must hide the non-exposed input (slot-b); got {input_slots:?}"
    );
}

/// Fixture for the author-controlled `optional` flag: two value slots each wired
/// to a brief input — one marked `optional: true`, one left default (required).
/// The single stage exposes BOTH slots so both inputs appear in the brief form.
fn create_optional_brief_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Optional Brief Guide {suffix}\"\n\
         description = \"One optional and one required brief-input\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-opt-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"slot-req-{short}\", kind: \"text\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-opt-{short}\", control: \"textarea\", slot: \"slot-opt-{short}\", optional: true }}\n:::\n\n\
         :::brief-input {{ name: \"input-req-{short}\", control: \"textarea\", slot: \"slot-req-{short}\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-opt-{short}\", \"input-req-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"slot-opt-{short}\", \"slot-req-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "input-opt-{short} = Optional input\n\
         input-req-{short} = Required input\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// End-to-end for the author-controlled `optional` flag: an `optional: true`
/// brief-input surfaces as `InputView.optional == true` in the brief form, while
/// a plain input surfaces as `false`. Exercises the whole signal path
/// (parse → `control_spec` write → SQL projection → view → DTO).
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_brief_form_reports_optional_flag() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(&base_url, create_optional_brief_guide_tarball(&suffix)).await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide id")).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let slot_opt = value_slot_id_by_name(sql, &format!("slot-opt-{short}")).await;
    let slot_req = value_slot_id_by_name(sql, &format!("slot-req-{short}")).await;

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("OptionalForm-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach brief_pack");

    let (form, _etag) = dapple_data::view::journey_brief_form::build_brief_form(&pool, journey_id)
        .await
        .expect("build_brief_form");

    let optional_for = |slot: uuid::Uuid| -> bool {
        form.briefs
            .iter()
            .flat_map(|b| b.units.iter())
            .filter_map(|u| u.input.as_ref())
            .find(|i| i.value_slot_id == slot)
            .unwrap_or_else(|| panic!("input for slot {slot} not in brief form"))
            .optional
    };

    assert!(
        optional_for(slot_opt),
        "input marked `optional: true` must report InputView.optional == true"
    );
    assert!(
        !optional_for(slot_req),
        "plain input must report InputView.optional == false (required default)"
    );
}

/// Regression for step-level dispatch firing (Issue #501 follow-up): a dispatch
/// must become ready once ITS OWN step's exposed slots are bound, even when the
/// journey's whole brief form is NOT fully bound.
///
/// The bug: `dispatch_ready` ANDed in `is_journey_fully_bound`, which demands
/// every brief input across the journey be bound. With the lockstep fixture the
/// stage exposes only `slot-a` while the brief-pack also carries `slot-b` (which
/// the form even hides), so `is_journey_fully_bound` is permanently false and
/// the step-1 dispatch never fired. The fix drops the journey-floor term, so the
/// gate is purely the step's exposed slots. Old code: final assert fails (gate
/// returns false); new code: passes.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_dispatch_ready_fires_on_step_binding_without_full_journey() {
    use dapple_data::model::dispatches::InsertDispatchArgs;
    use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
    use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
    use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
    use dapple_data::query::dispatches::insert_dispatches;
    use dapple_data::query::journeys::{dispatch_ready, is_step_fully_bound};

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(&base_url, create_overlay_brief_guide_tarball(&suffix)).await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide id")).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{short}")).await;
    // slot-b is a brief input the stage does NOT expose: it keeps the journey's
    // whole brief form unfilled even after the step's slot-a is bound.
    let slot_b = value_slot_id_by_name(sql, &format!("slot-b-{short}")).await;

    let stage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(playbook_id)
    .fetch_one(sql)
    .await
    .expect("playbook_stage");
    let step_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss \
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
         WHERE ps.id = $1 ORDER BY ss.id LIMIT 1",
    )
    .bind(stage_id)
    .fetch_one(sql)
    .await
    .expect("stage_step");
    let step_task_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT stk.id FROM rev.step_tasks stk \
         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
    )
    .bind(step_id)
    .fetch_one(sql)
    .await
    .expect("step_task");

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("StepDispatch-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    // Attach the brief-pack so the journey carries both slot-a and slot-b inputs;
    // this is what makes `is_journey_fully_bound` meaningfully false below.
    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach brief_pack");

    // Persist a dispatch covering the single step, mirroring the POST handler's
    // stage-grouping but WITHOUT going through HTTP (which would auto-spawn
    // execution). insert_dispatches returns the structured DispatchModel.
    let args = InsertDispatchArgs {
        intents: vec![serde_json::json!({})],
        stages: vec![InsertDispatchStagesArgs {
            playbook_stage_ids: vec![stage_id],
            params: vec![serde_json::json!({})],
            steps: vec![InsertDispatchStepsArgs {
                stage_step_ids: vec![step_id],
                params: vec![serde_json::json!({})],
                tasks: vec![InsertDispatchTaskArgs {
                    step_task_ids: vec![step_task_id],
                }],
            }],
        }],
    };
    let dispatches = insert_dispatches(&pool, vec![journey_id], vec![args])
        .await
        .expect("insert dispatch");
    let dispatch = &dispatches[0];

    // Precondition (corrupt state): nothing bound -> the dispatch is not ready.
    assert!(
        !dispatch_ready(&pool, dispatch)
            .await
            .expect("dispatch_ready before"),
        "dispatch must NOT be ready before any slot is bound"
    );

    // Bind ONLY slot-a (the step's exposed slot) on this dispatch. slot-b — a
    // brief input the stage does NOT expose — stays unbound.
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now())",
    )
    .bind(dispatch.id)
    .bind(slot_a)
    .execute(sql)
    .await
    .expect("bind slot-a");

    // The journey is NOT fully bound: slot-b is a brief input with no binding on
    // any of the journey's dispatches. This is exactly the precondition the OLD
    // `is_journey_fully_bound AND ...` gate failed on, suppressing execution
    // forever.
    let slot_b_bindings: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.dispatch_intent_bindings dib \
         JOIN rev.dispatches d ON d.id = dib.dispatch_id \
         WHERE d.journey_id = $1 AND dib.value_slot_id = $2",
    )
    .bind(journey_id)
    .bind(slot_b)
    .fetch_one(sql)
    .await
    .expect("count slot-b bindings");
    assert_eq!(
        slot_b_bindings, 0,
        "slot-b must stay unbound, so the journey's brief form is not fully filled"
    );
    // The step the dispatch covers IS fully bound (slot-a is its only exposed slot).
    assert!(
        is_step_fully_bound(&pool, journey_id, stage_id, step_id)
            .await
            .expect("is_step_fully_bound"),
        "the dispatch's step must be fully bound after slot-a"
    );

    // The fix: the dispatch is ready on the step binding alone, regardless of the
    // unbound journey-wide brief input. This assertion fails on the pre-fix gate.
    assert!(
        dispatch_ready(&pool, dispatch)
            .await
            .expect("dispatch_ready after"),
        "dispatch must be ready once its own step's exposed slots are bound, \
         even with the journey not fully bound"
    );
}

/// Sibling of [`create_overlay_brief_guide_tarball`] whose single stage exposes
/// BOTH a bindable input (`slot-a`) and a COMPUTED OUTPUT (`synth`, authored as
/// `:::brief-output { source: "computed" }`). The computed slot is produced by
/// the task and has no brief-input pinning it, so a consumer can never bind it.
/// Used to prove the dispatch START gate ignores computed outputs while the DONE
/// rollup still counts them.
fn create_overlay_computed_output_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Computed Output Guide {suffix}\"\n\
         description = \"Stage exposes a bindable input and a computed output\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // `synth` is exposed by the stage but pinned by NO brief-input — only a
    // computed `:::brief-output`. It must be excluded from the start gate.
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-a-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"synth-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-a-{short}\", control: \"text\", slot: \"slot-a-{short}\" }}\n:::\n\n\
         :::brief-output {{ name: \"synth-out-{short}\", slot: \"synth-{short}\", source: \"computed\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-a-{short}\", \"synth-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"slot-a-{short}\", \"synth-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "input-a-{short} = Topic A input\n\
         synth-out-{short} = Synthesis output\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Fixture for the duplicate-computed-output regression: TWO briefs in the
/// same pack declare computed outputs on the SAME slot — legitimate authoring
/// (a later pane's brief re-declares an output because its step REFRESHES the
/// value, latest-wins across dispatches; learn-from-gaming-vod does exactly
/// this with dl_beats/dl_scenes). Two flavors in one fixture:
///   * `synth`  — both outputs share source_path "/same" (identical rows; the
///     SQL DISTINCT must collapse them),
///   * `synth2` — outputs diverge ("/x" vs "/y"; DISTINCT can't collapse, the
///     producer's in-batch seen-set must keep the first).
fn create_duplicate_computed_output_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Duplicate Computed Output Guide {suffix}\"\n\
         description = \"Two briefs declare computed outputs on the same slot\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-a-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"synth-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"synth2-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-a-{short}\", control: \"text\", slot: \"slot-a-{short}\" }}\n:::\n\n\
         :::brief-output {{ name: \"synth-out-{short}\", slot: \"synth-{short}\", source: \"computed\", source_path: \"/same\" }}\n:::\n\n\
         :::brief-output {{ name: \"synth-dup-out-{short}\", slot: \"synth-{short}\", source: \"computed\", source_path: \"/same\" }}\n:::\n\n\
         :::brief-output {{ name: \"synth2-x-out-{short}\", slot: \"synth2-{short}\", source: \"computed\", source_path: \"/x\" }}\n:::\n\n\
         :::brief-output {{ name: \"synth2-y-out-{short}\", slot: \"synth2-{short}\", source: \"computed\", source_path: \"/y\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-a-{short}\", \"synth-out-{short}\", \"synth2-x-out-{short}\"] }}\n:::\n\n\
         :::brief {{ name: \"brief2-{short}\", units: [\"synth-dup-out-{short}\", \"synth2-y-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\", \"brief2-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"slot-a-{short}\", \"synth-{short}\", \"synth2-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{ same: {{ v: 1 }}, x: {{ v: 2 }}, y: {{ v: 3 }} }}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "input-a-{short} = Topic A input\n\
         synth-out-{short} = Synthesis output\n\
         synth-dup-out-{short} = Synthesis output redeclared\n\
         synth2-x-out-{short} = Second output x\n\
         synth2-y-out-{short} = Second output y\n\
         brief-{short} = The brief\n\
         brief2-{short} = The second brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Regression: a step that exposes a COMPUTED OUTPUT (`:::brief-output {
/// source: "computed" }`) alongside its bindable input must still become
/// dispatch-ready once the INPUT is bound. The computed output is produced by
/// the task and can never be operator-bound, so requiring it before the task
/// runs is unsatisfiable — the deadlock acme hit (dispatch created and
/// input-bound, yet `status: null` with zero attempts forever).
///
/// Old gate: `required_paths` = every exposed slot, so the computed output is
/// required → `is_step_fully_bound`/`dispatch_ready` never true → never spawns.
/// New gate: `required_paths` is bindable-inputs only. The DONE rollup is the
/// counterpoint — it KEEPS counting the computed output, so the step is not
/// `done` until `synth` is produced. Same fixture, same bindings, opposite
/// answers: that start-vs-done asymmetry is the whole point.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_dispatch_ready_ignores_computed_output_in_expose() {
    use dapple_data::model::dispatches::InsertDispatchArgs;
    use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
    use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
    use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
    use dapple_data::query::dispatches::insert_dispatches;
    use dapple_data::query::journeys::{
        dispatch_ready, is_step_fully_bound, select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(
        &base_url,
        create_overlay_computed_output_guide_tarball(&suffix),
    )
    .await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide id")).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{short}")).await;

    let stage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(playbook_id)
    .fetch_one(sql)
    .await
    .expect("playbook_stage");
    let step_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss \
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
         WHERE ps.id = $1 ORDER BY ss.id LIMIT 1",
    )
    .bind(stage_id)
    .fetch_one(sql)
    .await
    .expect("stage_step");
    let step_task_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT stk.id FROM rev.step_tasks stk \
         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
    )
    .bind(step_id)
    .fetch_one(sql)
    .await
    .expect("step_task");

    // --- Precondition: the fixture really does create the deadlock shape. ---
    // `synth` is exposed as a stage overlay...
    let synth_exposed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.playbook_stage_overlays o \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = o.value_slot_path_id \
         WHERE o.playbook_stage_id = $1 AND o.visibility = 'expose' AND vspd.name = $2",
    )
    .bind(stage_id)
    .bind(format!("synth-{short}"))
    .fetch_one(sql)
    .await
    .expect("synth overlay count");
    assert_eq!(
        synth_exposed, 1,
        "fixture must expose the computed output `synth` as a stage overlay"
    );
    // ...and it is a genuine computed brief-output (not just an orphan slot).
    let synth_computed_outputs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.brief_outputs bo \
         JOIN rev.brief_output_source_kinds k ON k.id = bo.source_kind_id \
         JOIN rev.value_slots vs ON vs.id = bo.value_slot_id \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id \
         WHERE vspd.name = $1 AND k.key = 'computed'",
    )
    .bind(format!("synth-{short}"))
    .fetch_one(sql)
    .await
    .expect("synth computed-output count");
    assert!(
        synth_computed_outputs >= 1,
        "`synth` must materialize as a computed brief-output"
    );

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("ComputedOutput-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach brief_pack");

    // Persist a dispatch covering the single step (mirrors the POST handler's
    // grouping but WITHOUT HTTP, which would auto-spawn execution).
    let args = InsertDispatchArgs {
        intents: vec![serde_json::json!({})],
        stages: vec![InsertDispatchStagesArgs {
            playbook_stage_ids: vec![stage_id],
            params: vec![serde_json::json!({})],
            steps: vec![InsertDispatchStepsArgs {
                stage_step_ids: vec![step_id],
                params: vec![serde_json::json!({})],
                tasks: vec![InsertDispatchTaskArgs {
                    step_task_ids: vec![step_task_id],
                }],
            }],
        }],
    };
    let dispatches = insert_dispatches(&pool, vec![journey_id], vec![args])
        .await
        .expect("insert dispatch");
    let dispatch = &dispatches[0];

    // Nothing bound yet -> not ready (slot-a is a required bindable input).
    assert!(
        !dispatch_ready(&pool, dispatch)
            .await
            .expect("dispatch_ready before"),
        "dispatch must NOT be ready before the input slot-a is bound"
    );

    // Bind ONLY the input slot-a. The computed output `synth` stays unbound —
    // a consumer cannot bind it (it has no value_slot_id in any brief form).
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now())",
    )
    .bind(dispatch.id)
    .bind(slot_a)
    .execute(sql)
    .await
    .expect("bind slot-a");

    // START GATE: ready on the input alone — the computed output is excluded.
    // (Pre-fix this is false: `synth` is exposed-but-unbound, so the step is
    // never "fully bound" and the dispatch never auto-spawns.)
    assert!(
        is_step_fully_bound(&pool, journey_id, stage_id, step_id)
            .await
            .expect("is_step_fully_bound"),
        "step must be fully bound for START once its bindable input is bound, \
         ignoring the computed output it exposes"
    );
    assert!(
        dispatch_ready(&pool, dispatch)
            .await
            .expect("dispatch_ready after"),
        "dispatch must be ready (and auto-spawn) once the input is bound"
    );

    // DONE ROLLUP: unchanged — the computed output still counts, so the step is
    // NOT done until `synth` is produced. This is the start-vs-done asymmetry.
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step binding status");
    let step = steps
        .iter()
        .find(|s| s.stage_step_id == step_id)
        .expect("step status row");
    assert!(
        !step.done,
        "done rollup must still count the computed output: step not done until `synth` is produced"
    );
    let synth_slot = step
        .exposed_slots
        .iter()
        .find(|s| s.name == format!("synth-{short}"))
        .expect("synth must appear as an exposed slot in the done rollup");
    assert!(
        !synth_slot.bound,
        "the computed output must show as unbound in the done rollup"
    );
    // Self-describing contract: the computed output is identified by an explicit
    // `producer = "computed"` (NOT by a NULL id, the old implicit signal). It now
    // carries the journey-pinned value_slot_id and exposes no operator render
    // spec. Unbindability is enforced by the start gate (asserted above via
    // is_step_fully_bound), not by withholding the id.
    assert_eq!(
        synth_slot.producer, "computed",
        "the computed output must report producer = computed"
    );
    assert!(
        synth_slot.value_slot_id.is_some(),
        "the computed output now carries the journey-pinned value_slot_id"
    );
    assert!(
        synth_slot.input.is_none(),
        "a computed output has no operator render spec"
    );
}

/// A guide whose stage exposes a REQUIRED input (slot-a) and an OPTIONAL input
/// (slot-b, `optional: true`). Used to prove an unbound optional input doesn't
/// gate auto-spawn or DONE.
fn create_overlay_optional_brief_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Overlay Optional Brief Guide {suffix}\"\n\
         description = \"Stage exposes a required + an optional input\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // The stage exposes BOTH slots, so the ONLY reason slot-b won't block the
    // gate is its `optional: true` flag.
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-a-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"slot-b-{short}\", kind: \"text\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-a-{short}\", control: \"text\", slot: \"slot-a-{short}\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-b-{short}\", control: \"text\", slot: \"slot-b-{short}\", optional: true }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-a-{short}\", \"input-b-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"slot-a-{short}\", \"slot-b-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "input-a-{short} = Required input\n\
         input-b-{short} = Optional input\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// A step exposes a REQUIRED input (slot-a) and an OPTIONAL input (slot-b).
/// Binding only slot-a must satisfy the START gate (`is_step_fully_bound` /
/// `dispatch_ready`) and the DONE rollups — an unbound OPTIONAL input must NOT
/// make the dispatch wait forever. This is the "submit, then hang" bug: the gate
/// treated every exposed operator input as required, so leaving an optional
/// field blank blocked auto-spawn (and `done`) permanently.
///
/// (The journey-pinned-vs-stale-version facet — an older brief version missing
/// the `optional` key must not re-introduce the block — is verified by the live
/// reproduction; this single-version test pins the core contract.)
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_dispatch_ready_ignores_optional_unbound_input() {
    use dapple_data::model::dispatches::InsertDispatchArgs;
    use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
    use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
    use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
    use dapple_data::query::dispatches::insert_dispatches;
    use dapple_data::query::journeys::{
        dispatch_ready, is_step_fully_bound, select_stage_binding_status_for_journey,
        select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(
        &base_url,
        create_overlay_optional_brief_guide_tarball(&suffix),
    )
    .await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide id")).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{short}")).await;

    let stage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(playbook_id)
    .fetch_one(sql)
    .await
    .expect("playbook_stage");
    let step_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss \
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
         WHERE ps.id = $1 ORDER BY ss.id LIMIT 1",
    )
    .bind(stage_id)
    .fetch_one(sql)
    .await
    .expect("stage_step");
    let step_task_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT stk.id FROM rev.step_tasks stk \
         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
    )
    .bind(step_id)
    .fetch_one(sql)
    .await
    .expect("step_task");

    // Precondition: the optional input slot-b is genuinely exposed on the stage,
    // so the ONLY reason it won't block is its `optional` flag.
    let slot_b_exposed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.playbook_stage_overlays o \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = o.value_slot_path_id \
         WHERE o.playbook_stage_id = $1 AND o.visibility = 'expose' AND vspd.name = $2",
    )
    .bind(stage_id)
    .bind(format!("slot-b-{short}"))
    .fetch_one(sql)
    .await
    .expect("slot-b overlay count");
    assert_eq!(
        slot_b_exposed, 1,
        "fixture must expose the optional slot-b on the stage"
    );

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("OptionalInput-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach brief_pack");

    let args = InsertDispatchArgs {
        intents: vec![serde_json::json!({})],
        stages: vec![InsertDispatchStagesArgs {
            playbook_stage_ids: vec![stage_id],
            params: vec![serde_json::json!({})],
            steps: vec![InsertDispatchStepsArgs {
                stage_step_ids: vec![step_id],
                params: vec![serde_json::json!({})],
                tasks: vec![InsertDispatchTaskArgs {
                    step_task_ids: vec![step_task_id],
                }],
            }],
        }],
    };
    let dispatches = insert_dispatches(&pool, vec![journey_id], vec![args])
        .await
        .expect("insert dispatch");
    let dispatch = &dispatches[0];

    // Nothing bound -> not ready: the REQUIRED slot-a is unbound.
    assert!(
        !dispatch_ready(&pool, dispatch)
            .await
            .expect("dispatch_ready before"),
        "dispatch must NOT be ready before the required slot-a is bound"
    );

    // Bind ONLY the required slot-a. The OPTIONAL slot-b stays unbound.
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now())",
    )
    .bind(dispatch.id)
    .bind(slot_a)
    .execute(sql)
    .await
    .expect("bind slot-a");

    // START GATE: ready on the required input alone — the optional input is
    // excluded. (Pre-fix this is false: slot-b is exposed-but-unbound, so the
    // step is never "fully bound" and the dispatch never auto-spawns -> hang.)
    assert!(
        is_step_fully_bound(&pool, journey_id, stage_id, step_id)
            .await
            .expect("is_step_fully_bound"),
        "step must be fully bound once its required input is bound, ignoring the optional one"
    );
    assert!(
        dispatch_ready(&pool, dispatch)
            .await
            .expect("dispatch_ready after"),
        "dispatch must be ready once the required input is bound, even with the optional one blank"
    );

    // DONE ROLLUPS: step and stage are done despite the optional slot-b being
    // unbound — but slot-b still appears in exposed_slots (so a client can still
    // offer it), flagged optional + unbound.
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step binding status");
    let step = steps
        .iter()
        .find(|s| s.stage_step_id == step_id)
        .expect("step status row");
    assert!(
        step.done,
        "step must be done: an unbound OPTIONAL input does not block done"
    );
    let opt_slot = step
        .exposed_slots
        .iter()
        .find(|s| s.name == format!("slot-b-{short}"))
        .expect("optional slot-b must still appear in exposed_slots");
    assert!(!opt_slot.bound, "optional slot-b is unbound");
    assert!(
        opt_slot.input.as_ref().map(|i| i.optional).unwrap_or(false),
        "slot-b must be flagged optional in its render spec"
    );

    let stages = select_stage_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("stage binding status");
    let stage = stages
        .iter()
        .find(|s| s.playbook_stage_id == stage_id)
        .expect("stage status row");
    assert!(
        stage.done,
        "stage must be done: an unbound OPTIONAL input does not block done"
    );
}

/// A guide with TWO stages, each holding a pick whose tiles come from the SAME
/// computed candidate slot. Stage 1's pick is REQUIRED; stage 2's is OPTIONAL.
///
/// The pair is the point: an empty candidate source is fatal to exactly one of
/// them. A `min >= 1` pick with no tiles can never be satisfied by any operator
/// action, while an optional pick is entirely happy with zero candidates — so a
/// gate that keys on "has a candidate source" rather than on requiredness would
/// wrongly kill half of this fixture.
fn create_blocked_pick_guide_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Blocked Pick Guide {suffix}\"\n\
         description = \"Required + optional picks over one candidate source\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // Both stages EXPOSE their pick, which is what puts the slot in `required`
    // (the rollups read overlays, not brief units). The candidate slot is exposed
    // on stage 1 alongside its consumer, exactly as a generate → pick stage does.
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"cand-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"pick-{short}\", kind: \"json\", cardinality: \"multi\", min: 1 }}\n:::\n\n\
         :::value-slot {{ name: \"optpick-{short}\", kind: \"json\", cardinality: \"multi\", min: 1 }}\n:::\n\n\
         :::brief-input {{ name: \"pick-input-{short}\", control: \"timeline\", slot: \"pick-{short}\", candidate_source: \"cand-{short}\", item_fields: [{{ key: \"beat\", control: \"textarea\" }}] }}\n:::\n\n\
         :::brief-input {{ name: \"optpick-input-{short}\", control: \"timeline\", slot: \"optpick-{short}\", candidate_source: \"cand-{short}\", optional: true, item_fields: [{{ key: \"beat\", control: \"textarea\" }}] }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"pick-input-{short}\", \"optpick-input-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-req-{short}\", expose: [\"cand-{short}\", \"pick-{short}\"] }}\n\n\
         :::step {{ name: \"step-req-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::stage {{ name: \"stage-opt-{short}\", expose: [\"optpick-{short}\"] }}\n\n\
         :::step {{ name: \"step-opt-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "pick-input-{short} = Required pick\n    .beat-label = Beat\n\
         optpick-input-{short} = Optional pick\n    .beat-label = Beat\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-req-{short} = The required stage\n\
         stage-opt-{short} = The optional stage\n\
         step-req-{short} = The required step\n\
         step-opt-{short} = The optional step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// The READ half of migration 92, which the producer-side tests do not touch:
/// `blocked` on `select_stage_binding_status_for_journey` /
/// `select_step_binding_status_for_journey`.
///
/// `done: false` used to be two different situations wearing one value — a stage
/// WAITING on the operator, and a stage with nothing to wait for. Everything
/// downstream assumed the first, which is why a dead journey rendered as a live
/// one. `blocked` is the third state, and the whole point of it is that it is NOT
/// recoverable by waiting.
///
/// Walks the full lifecycle in one journey, because the transitions are the
/// contract and each one has a way to be wrong:
///
///  A. required pick, candidate source bound `produced_empty` → BLOCKED
///  B. optional pick over the SAME empty source → NOT blocked (it is entitled to
///     zero candidates)
///  C. a later NON-empty candidate binding → block CLEARS (latest-wins; an
///     `EXISTS (… empty)` would pin it forever on the strength of the first run)
///  D. an empty regenerate landing AFTER the pick was made → still not blocked
///     (an already-bound pick is never blocked, whatever arrives later)
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_blocked_reports_a_pick_whose_candidate_source_produced_empty() {
    use dapple_data::model::dispatches::InsertDispatchArgs;
    use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
    use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
    use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
    use dapple_data::query::dispatches::insert_dispatches;
    use dapple_data::query::journeys::{
        select_stage_binding_status_for_journey, select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let pool = test_db_pool();
    let sql = pool.get_pool();

    let json = ingest_tarball(&base_url, create_blocked_pick_guide_tarball(&suffix)).await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide id")).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let cand_slot = value_slot_id_by_name(sql, &format!("cand-{short}")).await;
    let pick_slot = value_slot_id_by_name(sql, &format!("pick-{short}")).await;

    // Stage order is authored order (UUIDv7): required stage first.
    let stage_ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY ord, id",
    )
    .bind(playbook_id)
    .fetch_all(sql)
    .await
    .expect("playbook_stages");
    assert_eq!(stage_ids.len(), 2, "fixture must author two stages");
    let (req_stage_id, opt_stage_id) = (stage_ids[0], stage_ids[1]);

    let req_step_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss \
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
         WHERE ps.id = $1 ORDER BY ss.id LIMIT 1",
    )
    .bind(req_stage_id)
    .fetch_one(sql)
    .await
    .expect("required stage_step");
    let step_task_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT stk.id FROM rev.step_tasks stk \
         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
    )
    .bind(req_step_id)
    .fetch_one(sql)
    .await
    .expect("step_task");

    // Precondition: the pick's control_spec really did resolve `candidate_source`
    // to a slot id. Without it `journey_operator_inputs.candidate_source_path_id`
    // is NULL and every `blocked` assertion below would pass vacuously as false.
    let cand_ref: Option<String> = sqlx::query_scalar(
        "SELECT bui.control_spec ->> 'candidate_source_value_slot_id' \
         FROM rev.brief_unit_input bui \
         JOIN rev.brief_units bu ON bu.id = bui.brief_unit_id \
         WHERE bui.control_spec ->> 'value_slot_id' = $1::TEXT LIMIT 1",
    )
    .bind(pick_slot.to_string())
    .fetch_optional(sql)
    .await
    .expect("candidate_source lookup")
    .flatten();
    assert!(
        cand_ref.is_some(),
        "fixture must resolve candidate_source into control_spec"
    );

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("BlockedPick-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");

    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach brief_pack");

    // A binding is UNIQUE per (dispatch, slot), so each successive write below —
    // the empty run, the good re-run, the late empty regenerate — needs its own
    // dispatch. That is also what a real re-dispatch is.
    let new_dispatch = async || -> uuid::Uuid {
        let args = InsertDispatchArgs {
            intents: vec![serde_json::json!({})],
            stages: vec![InsertDispatchStagesArgs {
                playbook_stage_ids: vec![req_stage_id],
                params: vec![serde_json::json!({})],
                steps: vec![InsertDispatchStepsArgs {
                    stage_step_ids: vec![req_step_id],
                    params: vec![serde_json::json!({})],
                    tasks: vec![InsertDispatchTaskArgs {
                        step_task_ids: vec![step_task_id],
                    }],
                }],
            }],
        };
        insert_dispatches(&pool, vec![journey_id], vec![args])
            .await
            .expect("insert dispatch")[0]
            .id
    };

    let bind = async |dispatch_id: uuid::Uuid, slot: uuid::Uuid, produced_empty: bool| {
        sqlx::query(
            "INSERT INTO rev.dispatch_intent_bindings \
             (dispatch_id, value_slot_id, produced_empty, created_at) \
             VALUES ($1, $2, $3, now())",
        )
        .bind(dispatch_id)
        .bind(slot)
        .bind(produced_empty)
        .execute(sql)
        .await
        .expect("insert binding");
    };

    let stage_status = async |stage: uuid::Uuid| -> (bool, bool) {
        let rows = select_stage_binding_status_for_journey(&pool, journey_id)
            .await
            .expect("stage binding status");
        let row = rows
            .iter()
            .find(|s| s.playbook_stage_id == stage)
            .expect("stage status row");
        (row.done, row.blocked)
    };
    let step_status = async |step: uuid::Uuid, slot_name: String| -> (bool, bool, bool) {
        let rows = select_step_binding_status_for_journey(&pool, journey_id)
            .await
            .expect("step binding status");
        let row = rows
            .iter()
            .find(|s| s.stage_step_id == step)
            .expect("step status row");
        let slot = row
            .exposed_slots
            .iter()
            .find(|s| s.name == slot_name)
            .expect("exposed slot");
        (row.done, row.blocked, slot.blocked)
    };

    // ---- A. the empty run ------------------------------------------------
    // The producer succeeded and produced nothing. Before migration 92 the row
    // below was indistinguishable from a good one, so `blocked` was unsayable.
    let d1 = new_dispatch().await;
    bind(d1, cand_slot, true).await;

    let (done, blocked) = stage_status(req_stage_id).await;
    assert!(
        !done,
        "the required pick is unbound, so the stage isn't done"
    );
    assert!(
        blocked,
        "a REQUIRED pick whose candidate source produced an empty value can never \
         be satisfied — the stage must report blocked, not merely not-done"
    );
    let (step_done, step_blocked, slot_blocked) =
        step_status(req_step_id, format!("pick-{short}")).await;
    assert!(!step_done);
    assert!(step_blocked, "the step rolls its blocked slot up");
    assert!(slot_blocked, "the pick slot itself is blocked");

    // ---- B. the optional pick over the SAME empty source ------------------
    let (opt_done, opt_blocked) = stage_status(opt_stage_id).await;
    assert!(
        !opt_blocked,
        "an OPTIONAL pick is entitled to zero candidates — flagging it blocked \
         would kill a stage that is working exactly as authored"
    );
    assert!(
        opt_done,
        "an optional-only stage is trivially done, empty candidates or not"
    );

    // ---- C. a successful re-run clears it --------------------------------
    // The LATEST binding decides. An `EXISTS (… produced_empty)` would leave the
    // block set forever on the strength of the first attempt, which would make
    // the retry affordance a lie.
    let d2 = new_dispatch().await;
    bind(d2, cand_slot, false).await;

    let (done, blocked) = stage_status(req_stage_id).await;
    assert!(
        !blocked,
        "a later NON-empty candidate binding must clear the block — latest-wins, not EXISTS"
    );
    assert!(
        !done,
        "still waiting on the operator to pick: cleared-block is not done"
    );

    // ---- D. an empty regenerate arriving after the pick was made ---------
    let d3 = new_dispatch().await;
    bind(d3, pick_slot, false).await;
    let (done, blocked) = stage_status(req_stage_id).await;
    assert!(done, "the required pick is bound, so the stage is done");
    assert!(!blocked, "a done stage is never blocked");

    let d4 = new_dispatch().await;
    bind(d4, cand_slot, true).await;
    let (done, blocked) = stage_status(req_stage_id).await;
    assert!(
        done && !blocked,
        "an empty regenerate landing after a good pick must not un-do the stage: \
         `blocked` is scoped to stages that are still unbound"
    );
    let (_, step_blocked, slot_blocked) = step_status(req_step_id, format!("pick-{short}")).await;
    assert!(
        !step_blocked && !slot_blocked,
        "a BOUND slot is never blocked — the two flags must not contradict each other on the wire"
    );
}

/// Shared setup for the computed-output producer tests: ingest the fixture,
/// seed a journey + pinned brief_pack + a one-step dispatch, and bind the
/// operator input `slot-a` (so only the computed `synth` is left for `done`).
struct SeededComputedDispatch {
    journey_id: uuid::Uuid,
    stage_id: uuid::Uuid,
    step_id: uuid::Uuid,
    dispatch: dapple_data::model::dispatches::DispatchModel,
    short: String,
}

async fn seed_computed_output_dispatch(
    base_url: &str,
    pool: &dapple_data::DbPool,
    suffix: &str,
) -> SeededComputedDispatch {
    seed_dispatch_for_guide_tarball(
        base_url,
        pool,
        suffix,
        create_overlay_computed_output_guide_tarball(suffix),
    )
    .await
}

/// Ingest `tarball` and wire journey + pack pin + one dispatch over its first
/// stage/step, binding the `slot-a-{short}` operator input. Shared by the
/// computed-output fixtures (they all follow the same slot naming).
async fn seed_dispatch_for_guide_tarball(
    base_url: &str,
    pool: &dapple_data::DbPool,
    suffix: &str,
    tarball: Vec<u8>,
) -> SeededComputedDispatch {
    use dapple_data::model::dispatches::InsertDispatchArgs;
    use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
    use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
    use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
    use dapple_data::query::dispatches::insert_dispatches;

    let short = suffix[28..].to_string();
    let sql = pool.get_pool();

    let json = ingest_tarball(base_url, tarball).await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().unwrap()).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{short}")).await;

    let stage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(playbook_id)
    .fetch_one(sql)
    .await
    .expect("playbook_stage");
    let step_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss \
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
         WHERE ps.id = $1 ORDER BY ss.id LIMIT 1",
    )
    .bind(stage_id)
    .fetch_one(sql)
    .await
    .expect("stage_step");
    let step_task_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT stk.id FROM rev.step_tasks stk \
         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
    )
    .bind(step_id)
    .fetch_one(sql)
    .await
    .expect("step_task");

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("ComputedExec-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");
    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach brief_pack");

    let args = InsertDispatchArgs {
        intents: vec![serde_json::json!({})],
        stages: vec![InsertDispatchStagesArgs {
            playbook_stage_ids: vec![stage_id],
            params: vec![serde_json::json!({})],
            steps: vec![InsertDispatchStepsArgs {
                stage_step_ids: vec![step_id],
                params: vec![serde_json::json!({})],
                tasks: vec![InsertDispatchTaskArgs {
                    step_task_ids: vec![step_task_id],
                }],
            }],
        }],
    };
    let dispatch = insert_dispatches(pool, vec![journey_id], vec![args])
        .await
        .expect("insert dispatch")
        .into_iter()
        .next()
        .expect("one dispatch");

    // Bind the operator INPUT; the computed `synth` is the only slot left for
    // `done`, and only the task can produce it.
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now())",
    )
    .bind(dispatch.id)
    .bind(slot_a)
    .execute(sql)
    .await
    .expect("bind slot-a");

    SeededComputedDispatch {
        journey_id,
        stage_id,
        step_id,
        dispatch,
        short,
    }
}

/// Count `dispatch_intent_bindings` rows for the named value-slot on a dispatch.
async fn binding_count_for_slot_name(
    sql: &sqlx::PgPool,
    dispatch_id: uuid::Uuid,
    slot_name: &str,
) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM rev.dispatch_intent_bindings dib \
         JOIN rev.value_slots vs ON vs.id = dib.value_slot_id \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id \
         WHERE dib.dispatch_id = $1 AND vspd.name = $2",
    )
    .bind(dispatch_id)
    .bind(slot_name)
    .fetch_one(sql)
    .await
    .expect("binding count")
}

/// End-to-end: running a dispatch through the REAL runner must PRODUCE the
/// computed output's binding and flip the per-step AND per-stage `done` rollups
/// to true. This is the coverage that was missing — the producer gap surfaced
/// from an external-API consumer precisely because no internal test ran a
/// dispatch and *then* asserted `done`. It guards the call-site wiring in
/// `execute_dispatch`: deleting the `produce_computed_output_bindings` call makes
/// this test fail (synth stays unbound, the step never reaches `done`).
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_execute_dispatch_produces_computed_output_and_flips_done() {
    use dapple_data::query::journeys::{
        select_stage_binding_status_for_journey, select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let SeededComputedDispatch {
        journey_id,
        stage_id,
        step_id,
        dispatch,
        short,
    } = seeded;
    let dispatch_id = dispatch.id;
    let synth_name = format!("synth-{short}");

    // PRECONDITION (corrupt state): synth not yet produced -> unbound -> the step
    // (which also exposes the now-bound input) is NOT done.
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        0,
        "synth must be unbound before the dispatch runs"
    );
    let steps_before = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("steps before");
    assert!(
        !steps_before
            .iter()
            .find(|s| s.stage_step_id == step_id)
            .expect("step row")
            .done,
        "step must not be done before the task produces synth"
    );

    // ACTION: run the dispatch through the real runner (executes the nushell
    // task, which returns `{ payload: {}, signals: {} }`; the producer then
    // binds synth from that payload via source_path "").
    dapple_prompt_flow::dispatch_runner::execute_dispatch(&pool, dispatch, 4)
        .await
        .expect("execute_dispatch");

    // VERIFY (restored state): the runner produced synth's binding, so the step
    // and its stage now report done.
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        1,
        "the runner must produce exactly one synth binding"
    );
    let steps_after = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("steps after");
    assert!(
        steps_after
            .iter()
            .find(|s| s.stage_step_id == step_id)
            .expect("step row")
            .done,
        "step must be done once the task produced synth"
    );
    let stages_after = select_stage_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("stages after");
    assert!(
        stages_after
            .iter()
            .find(|s| s.playbook_stage_id == stage_id)
            .expect("stage row")
            .done,
        "stage must be done once its only step is done (step -> stage rollup)"
    );

    // IDEMPOTENCY: producing again is a no-op — the slot is already bound, so no
    // duplicate row and no UNIQUE error.
    let reproduced = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        stage_id,
        step_id,
        &serde_json::json!({}),
    )
    .await
    .expect("producer idempotent")
    .iter()
    .filter(|o| o.outcome == "produced")
    .count();
    assert_eq!(reproduced, 0, "re-producing must write no new bindings");
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        1,
        "synth binding count stays 1 after a re-produce"
    );
}

/// Regression (2026-07-16 learn-from-gaming-vod stall): two briefs in one pack
/// may legitimately declare computed outputs on the SAME slot (a later pane's
/// brief re-declares the output because its step refreshes the value). The
/// producer query used to return one row per brief-output, so the duplicate
/// slot rode into a single multi-row INSERT, tripped
/// UNIQUE(dispatch_id, value_slot_id), and Postgres rejected the ENTIRE batch
/// — the step cohered `ok` yet published ZERO outputs and the journey stalled
/// invisibly. Two defenses under test: SQL DISTINCT collapses identical
/// duplicate rows (`synth`, same source_path), and the producer's in-batch
/// seen-set keeps the first when source_paths diverge (`synth2`, "/x" vs "/y",
/// ordered so "/x" wins).
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_duplicate_computed_outputs_bind_once_per_slot() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_dispatch_for_guide_tarball(
        &base_url,
        &pool,
        &suffix,
        create_duplicate_computed_output_guide_tarball(&suffix),
    )
    .await;
    let SeededComputedDispatch {
        journey_id,
        stage_id: _,
        step_id,
        dispatch,
        short,
    } = seeded;
    let dispatch_id = dispatch.id;
    let synth_name = format!("synth-{short}");
    let synth2_name = format!("synth2-{short}");

    dapple_prompt_flow::dispatch_runner::execute_dispatch(&pool, dispatch, 4)
        .await
        .expect("execute_dispatch");

    // One binding per slot — the duplicate declarations must not have poisoned
    // the batch (pre-fix this was 0 and 0: the whole INSERT was rejected).
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        1,
        "synth must bind exactly once despite two identical output declarations"
    );
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth2_name).await,
        1,
        "synth2 must bind exactly once despite diverging source_paths"
    );

    // First-wins determinism on the diverging pair: rows arrive ordered by
    // (value_slot_id, source_path), so "/x" beats "/y".
    let synth2_value: serde_json::Value = sqlx::query_scalar(
        "SELECT ji.value FROM rev.dispatch_intent_binding_json_items ji \
         JOIN rev.dispatch_intent_bindings dib ON dib.id = ji.dispatch_intent_binding_id \
         JOIN rev.value_slots vs ON vs.id = dib.value_slot_id \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id \
         WHERE dib.dispatch_id = $1 AND vspd.name = $2",
    )
    .bind(dispatch_id)
    .bind(&synth2_name)
    .fetch_one(sql)
    .await
    .expect("synth2 json item");
    assert_eq!(
        synth2_value,
        serde_json::json!({ "v": 2 }),
        "synth2 must carry the FIRST declaration's extraction (/x), not /y"
    );

    // The ok event records exactly one produced outcome per slot.
    let computed_outputs: serde_json::Value = sqlx::query_scalar(
        "SELECT e.context_metadata->'computed_outputs' \
         FROM rev.dispatch_task_attempt_events e \
         JOIN rev.dispatch_task_attempts dta ON dta.id = e.attempt_id \
         JOIN rev.dispatch_steps dst ON dst.id = dta.dispatch_step_id \
         JOIN rev.dispatch_stages ds ON ds.id = dst.dispatch_stage_id \
         WHERE ds.dispatch_id = $1 AND e.phase_code = 'ok'",
    )
    .bind(dispatch_id)
    .fetch_one(sql)
    .await
    .expect("ok event computed_outputs");
    let produced = computed_outputs
        .as_array()
        .expect("computed_outputs array")
        .iter()
        .filter(|o| o["outcome"] == "produced")
        .count();
    assert_eq!(
        produced, 2,
        "ok event must record exactly one produced outcome per slot"
    );

    // And the rollup agrees: with both computed slots bound the step is done.
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("steps after");
    assert!(
        steps
            .iter()
            .find(|s| s.stage_step_id == step_id)
            .expect("step row")
            .done,
        "step must be done once both computed slots are produced"
    );
}

/// Doctrine regression (2026-07-16): a journey's PINNED pack version must keep
/// resolving after a later sync DEPRECATES it — multiple versions run actively
/// side by side; that is what pinning is for. The 2026-06-01 deprecation filter
/// in the pinned-pack walk silently nulled out every pre-re-ingest journey:
/// the producer published nothing, the DONE rollups lost their required slots,
/// and the journey brief read went empty. Corrupt-then-verify: deprecate the
/// pinned pack FIRST, then assert the producer, the rollup, and the brief read
/// all still resolve the pinned version.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_deprecated_pinned_pack_still_produces_and_rolls_up() {
    use dapple_data::query::journeys::{
        select_briefs_by_journey_object_id, select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let SeededComputedDispatch {
        journey_id,
        stage_id: _,
        step_id,
        dispatch,
        short,
    } = seeded;
    let dispatch_id = dispatch.id;
    let synth_name = format!("synth-{short}");

    // CORRUPT: deprecate the journey's pinned pack version, exactly what a
    // later guide re-sync does to every already-running journey.
    sqlx::query(
        "INSERT INTO rev.deprecation_events (versioned_object_id, to_level_id, reason) \
         SELECT bp.versioned_object_id, (SELECT id FROM rev.deprecation_levels ORDER BY id LIMIT 1), 'test re-sync' \
         FROM rev.journey_brief_packs jbp \
         JOIN rev.brief_packs bp ON bp.id = jbp.brief_pack_id \
         WHERE jbp.journey_id = $1",
    )
    .bind(journey_id)
    .execute(sql)
    .await
    .expect("deprecate pinned pack");

    // The rollup must still see the journey's slots: the step exposes a bound
    // input and an unproduced computed output, so it is NOT done — a
    // deprecation-nulled pack would have erased the requirement instead.
    let steps_before = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("steps before");
    assert!(
        !steps_before
            .iter()
            .find(|s| s.stage_step_id == step_id)
            .expect("step row")
            .done,
        "deprecating the pinned pack must not erase the step's required slots"
    );

    // The brief read must still render the pinned version's briefs.
    let briefs = select_briefs_by_journey_object_id(&pool, journey_id)
        .await
        .expect("briefs for journey");
    assert!(
        !briefs.is_empty(),
        "journey briefs must keep resolving the pinned (now deprecated) pack version"
    );

    // And the producer must still publish the computed output.
    dapple_prompt_flow::dispatch_runner::execute_dispatch(&pool, dispatch, 4)
        .await
        .expect("execute_dispatch");
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        1,
        "the producer must bind the computed output despite the deprecated pinned pack"
    );
    let steps_after = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("steps after");
    assert!(
        steps_after
            .iter()
            .find(|s| s.stage_step_id == step_id)
            .expect("step row")
            .done,
        "step must reach done on the pinned pack version"
    );
}

/// Option-A containment: when a producing task is followed by a LATER task that
/// FAILS in the same step, the step does not cohere, so its computed output is
/// NEVER published — the (possibly poisoned) value stays out of the binding layer
/// entirely (`$in.bindings.*`, chained-journey inheritance, pollers, DONE
/// rollup). The producer still ran and is recorded `ok` (history is never rolled
/// back); only the step's *answer* is withheld. This is the property that
/// justified producing per-step at completion instead of per-task.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_failed_later_task_withholds_computed_output() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    // Seed the producer step (one task that produces synth), then APPEND a failing
    // task to the same step and run a fresh dispatch over [producer, fail].
    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let SeededComputedDispatch {
        journey_id,
        stage_id,
        step_id,
        dispatch: _seed_dispatch,
        short,
    } = seeded;
    let synth_name = format!("synth-{short}");

    let versioned_step_id: uuid::Uuid =
        sqlx::query_scalar("SELECT step_id FROM rev.stage_steps WHERE id = $1")
            .bind(step_id)
            .fetch_one(sql)
            .await
            .expect("versioned step id");
    let producer_step_task_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM rev.step_tasks WHERE step_id = $1 ORDER BY ord LIMIT 1")
            .bind(versioned_step_id)
            .fetch_one(sql)
            .await
            .expect("producer step_task");

    // A failing task entity, linked as a SECOND step_task at a later ord.
    let fail_task = dapple_data::query::tasks::insert_tasks(
        &pool,
        vec![dapple_data::model::tasks::InsertTaskArgs {
            names: vec![format!("fail-task-{short}")],
            descriptions: vec!["intentional failure".to_string()],
            functions: vec!["nushell".to_string()],
            function_inputs: vec![serde_json::json!({ "script": "error make { msg: \"boom\" }" })],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        }],
    )
    .await
    .expect("insert fail task")
    .into_iter()
    .next()
    .expect("one fail task");
    let fail_step_task_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.step_tasks (step_id, task_id, ord) VALUES ($1, $2, 999) RETURNING id",
    )
    .bind(versioned_step_id)
    .bind(fail_task.id)
    .fetch_one(sql)
    .await
    .expect("append fail step_task");

    // A fresh dispatch over the same journey/stage/step with [producer, fail].
    let args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![serde_json::json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage_id],
                params: vec![serde_json::json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step_id],
                        params: vec![serde_json::json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: vec![producer_step_task_id, fail_step_task_id],
                            },
                        ],
                    },
                ],
            },
        ],
    };
    let dispatch =
        dapple_data::query::dispatches::insert_dispatches(&pool, vec![journey_id], vec![args])
            .await
            .expect("insert 2-task dispatch")
            .into_iter()
            .next()
            .expect("one dispatch");
    let dispatch_id = dispatch.id;

    // ACTION: run the 2-task step. Producer succeeds; the trailing task errors.
    dapple_prompt_flow::dispatch_runner::execute_dispatch(&pool, dispatch, 4)
        .await
        .expect("execute_dispatch");

    // VERIFY (containment): synth was NEVER published on this dispatch.
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        0,
        "a failed step must NOT publish its computed output"
    );
    // ...so the step is not done (synth is the missing slot).
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("steps");
    assert!(
        !steps
            .iter()
            .find(|s| s.stage_step_id == step_id)
            .expect("step row")
            .done,
        "step must not be done when its computed output was withheld"
    );

    // History is kept: the producer ran and is recorded `ok`; the trailing task
    // is recorded `failed` (that is WHY the step did not cohere).
    let attempts = dapple_data::query::dispatch_task_attempts::select_task_attempts_by_dispatch_id(
        &pool,
        dispatch_id,
    )
    .await
    .expect("attempts");
    let mut ok = 0;
    let mut failed = 0;
    for a in &attempts {
        for e in
            dapple_data::query::dispatch_task_attempts::select_attempt_events_by_attempt_object_id(
                &pool, a.id,
            )
            .await
            .expect("events")
        {
            match e.phase_code.as_str() {
                "ok" => ok += 1,
                "failed" => failed += 1,
                _ => {}
            }
        }
    }
    assert_eq!(ok, 1, "the producer ran and is recorded ok (history kept)");
    assert_eq!(
        failed, 1,
        "the trailing task failed, so the step did not cohere"
    );
}

/// The producer must persist the REAL produced value (not a bare existence
/// marker) in the typed item its value_kind names, so binding readers
/// (`$in.bindings.*` seed, brief prefill, effective-binding inheritance) can
/// decode it. Drives the producer directly with a rich payload and asserts the
/// value round-trips through the public binding DTO + carries the `computed`
/// provenance marker.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_producer_persists_computed_value_for_readers() {
    use dapple_data::query::dispatch_intent_bindings::select_dispatch_intent_bindings_by_dispatch_ids;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);

    // A rich payload that the task could have produced; source_path "" selects
    // the whole payload as the (json-kind) synth value.
    let payload = serde_json::json!({ "answer": "42", "detail": { "k": "v" } });

    // PRECONDITION
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        0,
        "synth unbound before producing"
    );

    // ACTION
    let produced = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        seeded.stage_id,
        seeded.step_id,
        &payload,
    )
    .await
    .expect("produce")
    .iter()
    .filter(|o| o.outcome == "produced")
    .count();
    assert_eq!(produced, 1, "exactly one computed output produced");

    // VERIFY value is readable through the SAME folded DTO binding readers use.
    let bindings = select_dispatch_intent_bindings_by_dispatch_ids(&pool, vec![dispatch_id])
        .await
        .expect("bindings");
    let synth = bindings
        .iter()
        .find(|b| b.value_slot_name == synth_name)
        .expect("synth binding must be present for readers");
    assert_eq!(
        synth.json_items.len(),
        1,
        "a computed json output must carry its value as a json item"
    );
    assert_eq!(
        synth.json_items[0].value, payload,
        "the persisted value must equal the produced payload"
    );

    // VERIFY provenance marker.
    let producer: Option<String> = sqlx::query_scalar(
        "SELECT vm.producer FROM rev.dispatch_intent_binding_value_meta vm \
         JOIN rev.dispatch_intent_bindings dib ON dib.id = vm.dispatch_intent_binding_id \
         WHERE dib.dispatch_id = $1 AND dib.value_slot_id = $2",
    )
    .bind(dispatch_id)
    .bind(synth.value_slot_id)
    .fetch_one(sql)
    .await
    .expect("producer meta");
    assert_eq!(
        producer.as_deref(),
        Some("computed"),
        "computed-output binding must record producer = 'computed'"
    );
}

/// Observability: when a computed output's `source_path` does not resolve in the
/// task payload, the producer must (a) write NO binding and (b) report a
/// `source_path_missing` outcome — the queryable signal the external
/// `/task_attempts` surface projects (via `context_metadata.computed_outputs`) so
/// a customer can see WHY a computed slot is stuck instead of reading dapple pod
/// logs. Corrupt-then-verify against the live DB (per the repair-test rule).
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_producer_reports_source_path_missing_outcome() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);
    let synth_slot = value_slot_id_by_name(sql, &synth_name).await;

    // CORRUPT: point the computed output's source_path at a key the payload
    // below will not contain.
    sqlx::query("UPDATE rev.brief_outputs SET source_path = '/missing' WHERE value_slot_id = $1")
        .bind(synth_slot)
        .execute(sql)
        .await
        .expect("set source_path");

    // ACTION: run the producer with a payload lacking `/missing`.
    let outcomes = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        seeded.stage_id,
        seeded.step_id,
        &serde_json::json!({ "present": "value" }),
    )
    .await
    .expect("producer runs (warn-and-continue; a per-output skip never errors)");

    // VERIFY: the outcome is surfaced AND no binding was written.
    assert!(
        outcomes
            .iter()
            .any(|o| o.name == synth_name && o.outcome == "source_path_missing"),
        "producer must report source_path_missing for the unresolved synth output: {outcomes:?}"
    );
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        0,
        "no binding may be written when source_path does not resolve"
    );
}

/// The distinction the whole system was missing: a task that SUCCEEDS and
/// produces NOTHING.
///
/// `{"choices": []}` with `source_path = "/choices"` resolves perfectly well, so
/// it is not `source_path_missing`, and `[]` coerces cleanly to a json slot, so
/// it is not `type_mismatch`. Before migration 92 the producer recorded a plain
/// `produced` and wrote an ordinary binding row, and since every gate asks only
/// `EXISTS (binding row)`, the slot read as bound and the step as done. A pick
/// fed by that slot then had no tiles and could never be satisfied, and nothing
/// anywhere could say so — the journey stalled behind a pane reporting success.
///
/// So: the binding IS still written (the value is real, downstream tasks may
/// legitimately read an empty list), but the row carries `produced_empty` and
/// the outcome says `produced_empty`.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_producer_reports_produced_empty_for_a_present_but_empty_value() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);
    let synth_slot = value_slot_id_by_name(sql, &synth_name).await;

    sqlx::query("UPDATE rev.brief_outputs SET source_path = '/choices' WHERE value_slot_id = $1")
        .bind(synth_slot)
        .execute(sql)
        .await
        .expect("set source_path");

    // ACTION: the exact payload the stalled guide produced.
    let outcomes = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        seeded.stage_id,
        seeded.step_id,
        &serde_json::json!({ "choices": [] }),
    )
    .await
    .expect("producer runs");

    assert!(
        outcomes
            .iter()
            .any(|o| o.name == synth_name && o.outcome == "produced_empty"),
        "an empty extracted value must report produced_empty, not produced: {outcomes:?}"
    );
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        1,
        "the binding is still written — an empty value is a real value"
    );
    let produced_empty: Option<bool> = sqlx::query_scalar(
        "SELECT dib.produced_empty FROM rev.dispatch_intent_bindings dib \
         JOIN rev.value_slots vs ON vs.id = dib.value_slot_id \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id \
         WHERE dib.dispatch_id = $1 AND vspd.name = $2",
    )
    .bind(dispatch_id)
    .bind(&synth_name)
    .fetch_one(sql)
    .await
    .expect("produced_empty flag");
    assert_eq!(
        produced_empty,
        Some(true),
        "the binding row must record the emptiness so the journey gates can read it"
    );
}

/// The other half of the contract: a NON-empty value must be entirely unaffected.
/// Guards against a widened emptiness test quietly blocking healthy journeys —
/// `false` and `0` are values a task meant to produce, not absences.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_producer_still_reports_produced_for_a_non_empty_value() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);
    let synth_slot = value_slot_id_by_name(sql, &synth_name).await;

    sqlx::query("UPDATE rev.brief_outputs SET source_path = '/choices' WHERE value_slot_id = $1")
        .bind(synth_slot)
        .execute(sql)
        .await
        .expect("set source_path");

    let outcomes = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        seeded.stage_id,
        seeded.step_id,
        &serde_json::json!({ "choices": [{ "label": "a", "value": "a" }] }),
    )
    .await
    .expect("producer runs");

    assert!(
        outcomes
            .iter()
            .any(|o| o.name == synth_name && o.outcome == "produced"),
        "a non-empty value must still report plain produced: {outcomes:?}"
    );
    let produced_empty: Option<bool> = sqlx::query_scalar(
        "SELECT dib.produced_empty FROM rev.dispatch_intent_bindings dib \
         JOIN rev.value_slots vs ON vs.id = dib.value_slot_id \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id \
         WHERE dib.dispatch_id = $1 AND vspd.name = $2",
    )
    .bind(dispatch_id)
    .bind(&synth_name)
    .fetch_one(sql)
    .await
    .expect("produced_empty flag");
    assert_eq!(produced_empty, Some(false), "a real value is not empty");
}

/// The shared `/steps` data layer (`select_step_binding_status_for_journey`, used
/// by BOTH the internal and external surfaces) must make each exposed slot
/// self-describing: an operator slot reports `producer = "operator"` with its
/// render spec (control_type + choices, so a client renders a real control, not
/// the JSON textarea acme fell back to); a computed slot reports
/// `producer = "computed"`, carries the journey-pinned `value_slot_id`, and has
/// no render spec.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_step_status_exposes_producer_and_input_render_spec() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let slot_a_name = format!("slot-a-{}", seeded.short);
    let synth_name = format!("synth-{}", seeded.short);
    let slot_a = value_slot_id_by_name(sql, &slot_a_name).await;

    // Make the operator input a `select` with two choices so the render spec has
    // something to carry. The markdown DSL has no `choices` attribute, so we
    // inject the rows directly (the same approach the brief-form fixtures use).
    let brief_unit_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_unit_id FROM rev.brief_unit_input \
         WHERE (control_spec ->> 'value_slot_id')::uuid = $1",
    )
    .bind(slot_a)
    .fetch_one(sql)
    .await
    .expect("input brief_unit");
    sqlx::query(
        "UPDATE rev.brief_unit_input \
         SET control_type_id = (SELECT id FROM rev.brief_input_control_types WHERE key = 'select') \
         WHERE brief_unit_id = $1",
    )
    .bind(brief_unit_id)
    .execute(sql)
    .await
    .expect("set control select");
    sqlx::query(
        "INSERT INTO rev.brief_unit_input_choices (brief_unit_id, label_text, value_text) \
         VALUES ($1, 'Alpha', 'a'), ($1, 'Beta', 'b')",
    )
    .bind(brief_unit_id)
    .execute(sql)
    .await
    .expect("insert choices");

    let steps = select_step_binding_status_for_journey(&pool, seeded.journey_id)
        .await
        .expect("step status");

    let slot_a_view = steps
        .iter()
        .flat_map(|s| &s.exposed_slots)
        .find(|sl| sl.name == slot_a_name)
        .expect("operator slot-a must be exposed");
    assert_eq!(slot_a_view.producer, "operator", "slot-a is a brief input");
    assert!(
        slot_a_view.value_slot_id.is_some(),
        "operator slot carries its journey-pinned id"
    );
    let input = slot_a_view
        .input
        .as_ref()
        .expect("operator slot must carry a render spec");
    assert_eq!(input.control_type, "select", "control type flows to /steps");
    assert_eq!(input.choices.len(), 2, "choices flow to /steps");

    let synth_view = steps
        .iter()
        .flat_map(|s| &s.exposed_slots)
        .find(|sl| sl.name == synth_name)
        .expect("computed synth must be exposed");
    assert_eq!(
        synth_view.producer, "computed",
        "synth is a computed output"
    );
    assert!(
        synth_view.value_slot_id.is_some(),
        "computed slot now carries the journey-pinned id (was NULL before)"
    );
    assert!(
        synth_view.input.is_none(),
        "computed slots have no operator render spec"
    );
}

/// Where the computed `synth` output is exposed in the multi-step ownership
/// fixture — selects which producer-of-record case is exercised.
enum ComputedOwnershipExpose {
    /// `synth` exposed at the STEP level on step 2 only (→ `stage_step_overlays`,
    /// priority 2). Expect: step 1 owns nothing; step 2 owns `synth` (rule a).
    StepLevelOnStep2,
    /// `synth` exposed at the STAGE level with two steps (→
    /// `playbook_stage_overlays`, priority 1, multi-step). Expect: NEITHER step
    /// owns `synth` — ambiguous, so it stays unbound (rule b's single-step guard).
    StageLevelTwoSteps,
}

/// A `:::stage` with TWO `:::step` children that share one computed
/// `:::brief-output` (`synth`). `variant` decides WHERE `synth` is exposed,
/// exercising the producer-of-record ownership rule in
/// `select_computed_outputs_for_step`. Mirrors the directive/fluent shape of
/// `create_overlay_computed_output_guide_tarball` (value-slot, brief-input,
/// computed brief-output, brief/group/pack, stage, steps, tasks).
fn create_multistep_computed_ownership_tarball(
    suffix: &str,
    variant: ComputedOwnershipExpose,
) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Multistep Computed Ownership {suffix}\"\n\
         description = \"Two-step stage sharing one computed output\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // Per-variant overlay placement. `slot-a`/`slot-b` are plain inputs so each
    // step also exposes a bindable input (and the steps differ, so the stage is
    // genuinely 2-step). `synth` is the computed output whose owner we test.
    let (stage_expose, step1_expose, step2_expose) = match variant {
        ComputedOwnershipExpose::StepLevelOnStep2 => (
            // stage exposes only the plain inputs; synth lives on step 2.
            format!("expose: [\"slot-a-{short}\", \"slot-b-{short}\"]"),
            format!(", expose: [\"slot-a-{short}\"]"),
            format!(", expose: [\"slot-b-{short}\", \"synth-{short}\"]"),
        ),
        ComputedOwnershipExpose::StageLevelTwoSteps => (
            // stage exposes synth (ambiguous across the two steps).
            format!("expose: [\"slot-a-{short}\", \"slot-b-{short}\", \"synth-{short}\"]"),
            format!(", expose: [\"slot-a-{short}\"]"),
            format!(", expose: [\"slot-b-{short}\"]"),
        ),
    };

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-a-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"slot-b-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"synth-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-a-{short}\", control: \"text\", slot: \"slot-a-{short}\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-b-{short}\", control: \"text\", slot: \"slot-b-{short}\" }}\n:::\n\n\
         :::brief-output {{ name: \"synth-out-{short}\", slot: \"synth-{short}\", source: \"computed\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-a-{short}\", \"input-b-{short}\", \"synth-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", {stage_expose} }}\n\n\
         :::step {{ name: \"step1-{short}\", task: \"task1-{short}\"{step1_expose} }}\n:::\n\n\
         :::step {{ name: \"step2-{short}\", task: \"task2-{short}\"{step2_expose} }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task1-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task2-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "input-a-{short} = Topic A input\n\
         input-b-{short} = Topic B input\n\
         synth-out-{short} = Synthesis output\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step1-{short} = The first step\n\
         step2-{short} = The second step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Seeded multi-step ownership scenario: ingested fixture + journey (pinned
/// pack) + a 2-step dispatch. The producer is called per-step in the tests; the
/// dispatch tree only needs a valid journey, but we build it faithfully.
struct SeededMultistep {
    stage_id: uuid::Uuid,
    step1: uuid::Uuid,
    step2: uuid::Uuid,
    dispatch_id: uuid::Uuid,
    short: String,
}

async fn seed_multistep_ownership(
    base_url: &str,
    pool: &dapple_data::DbPool,
    suffix: &str,
    variant: ComputedOwnershipExpose,
) -> SeededMultistep {
    use dapple_data::model::dispatches::InsertDispatchArgs;
    use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
    use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
    use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
    use dapple_data::query::dispatches::insert_dispatches;

    let short = suffix[28..].to_string();
    let sql = pool.get_pool();

    let json = ingest_tarball(
        base_url,
        create_multistep_computed_ownership_tarball(suffix, variant),
    )
    .await;
    let playbook_id =
        uuid::Uuid::parse_str(json["derived_objects"]["playbook_ids"][0].as_str().unwrap())
            .unwrap();
    let guide_id = uuid::Uuid::parse_str(json["guide"]["id"].as_str().unwrap()).unwrap();

    let principal_id = test_principal_id(sql).await;
    let dapple_company = uuid::Uuid::parse_str(DAPPLE_COMPANY).unwrap();

    let stage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(playbook_id)
    .fetch_one(sql)
    .await
    .expect("playbook_stage");
    let step_ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss \
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
         WHERE ps.id = $1 ORDER BY ss.id",
    )
    .bind(stage_id)
    .fetch_all(sql)
    .await
    .expect("stage_steps");
    assert_eq!(step_ids.len(), 2, "fixture must build a 2-step stage");
    let (step1, step2) = (step_ids[0], step_ids[1]);

    let task1: uuid::Uuid = sqlx::query_scalar(
        "SELECT stk.id FROM rev.step_tasks stk \
         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
    )
    .bind(step1)
    .fetch_one(sql)
    .await
    .expect("task1");
    let task2: uuid::Uuid = sqlx::query_scalar(
        "SELECT stk.id FROM rev.step_tasks stk \
         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
    )
    .bind(step2)
    .fetch_one(sql)
    .await
    .expect("task2");

    let brief_pack_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT brief_pack_id FROM rev.guide_brief_packs WHERE guide_id = $1 LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(sql)
    .await
    .expect("guide brief_pack");
    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at) \
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook_id)
    .bind(dapple_company)
    .bind(principal_id)
    .bind(format!("Multistep-{short}"))
    .fetch_one(sql)
    .await
    .expect("seed journey");
    sqlx::query("INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)")
        .bind(journey_id)
        .bind(brief_pack_id)
        .execute(sql)
        .await
        .expect("attach pack");

    let args = InsertDispatchArgs {
        intents: vec![serde_json::json!({})],
        stages: vec![InsertDispatchStagesArgs {
            playbook_stage_ids: vec![stage_id],
            params: vec![serde_json::json!({})],
            steps: vec![
                InsertDispatchStepsArgs {
                    stage_step_ids: vec![step1],
                    params: vec![serde_json::json!({})],
                    tasks: vec![InsertDispatchTaskArgs {
                        step_task_ids: vec![task1],
                    }],
                },
                InsertDispatchStepsArgs {
                    stage_step_ids: vec![step2],
                    params: vec![serde_json::json!({})],
                    tasks: vec![InsertDispatchTaskArgs {
                        step_task_ids: vec![task2],
                    }],
                },
            ],
        }],
    };
    let dispatch = insert_dispatches(pool, vec![journey_id], vec![args])
        .await
        .expect("insert dispatch")
        .into_iter()
        .next()
        .expect("one dispatch");

    SeededMultistep {
        stage_id,
        step1,
        step2,
        dispatch_id: dispatch.id,
        short,
    }
}

/// Producer-of-record, rule (a): a computed output exposed at the STEP level on
/// step 2 is owned by step 2 ALONE. Step 1 (which runs first) must bind NOTHING
/// for synth; step 2 binds it. Guards against the first-writer-wins wrong-data
/// bug where a stage-wide visibility let an earlier step claim a later step's
/// output.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_computed_output_owned_by_step_level_exposing_step() {
    use dapple_data::query::journeys::select_computed_outputs_for_step;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_multistep_ownership(
        &base_url,
        &pool,
        &suffix,
        ComputedOwnershipExpose::StepLevelOnStep2,
    )
    .await;
    let SeededMultistep {
        stage_id,
        step1,
        step2,
        dispatch_id,
        short,
    } = seeded;
    let synth_slot = value_slot_id_by_name(sql, &format!("synth-{short}")).await;
    let synth_name = format!("synth-{short}");

    // OWNERSHIP via the scoping query directly.
    let step1_owns = select_computed_outputs_for_step(&pool, dispatch_id, stage_id, step1)
        .await
        .expect("step1 outputs");
    let step2_owns = select_computed_outputs_for_step(&pool, dispatch_id, stage_id, step2)
        .await
        .expect("step2 outputs");
    assert!(
        step1_owns.is_empty(),
        "step 1 must NOT own synth (exposed on step 2)"
    );
    assert_eq!(step2_owns.len(), 1, "step 2 must own exactly synth");
    assert_eq!(
        step2_owns[0].value_slot_id, synth_slot,
        "owned slot is synth"
    );

    // PRODUCER scoping: step 1 writes no synth binding; step 2 does.
    let n1 = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        stage_id,
        step1,
        &serde_json::json!({}),
    )
    .await
    .expect("produce step1")
    .iter()
    .filter(|o| o.outcome == "produced")
    .count();
    assert_eq!(n1, 0, "step 1 must bind nothing");
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        0,
        "synth must be unbound after step 1's producer runs"
    );

    let n2 = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        stage_id,
        step2,
        &serde_json::json!({"k": "v"}),
    )
    .await
    .expect("produce step2")
    .iter()
    .filter(|o| o.outcome == "produced")
    .count();
    assert_eq!(n2, 1, "step 2 must produce synth");
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        1,
        "synth bound exactly once, by step 2"
    );
}

/// Write-once-per-dispatch: a computed output exposed at the STAGE level is
/// visible to every step of the stage (the cascade), so BOTH steps "expose" it.
/// Bindings are immutable within a dispatch (like operator answers), so the
/// FIRST producing step binds it and a later step in the same dispatch is a
/// no-op. (To get a later value you re-dispatch — latest-wins across dispatches.)
/// This is the behavior after dropping the old "ownership gate": a whole stage
/// can be fired as one dispatch and its stage-level computed output still binds.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_computed_output_stage_level_multistep_binds_write_once() {
    use dapple_data::query::dispatch_intent_bindings::select_dispatch_intent_bindings_by_dispatch_ids;
    use dapple_data::query::journeys::select_computed_outputs_for_step;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_multistep_ownership(
        &base_url,
        &pool,
        &suffix,
        ComputedOwnershipExpose::StageLevelTwoSteps,
    )
    .await;
    let SeededMultistep {
        stage_id,
        step1,
        step2,
        dispatch_id,
        short,
    } = seeded;
    let synth_name = format!("synth-{short}");

    // Precondition: synth is a stage-level expose, so the cascade exposes it to
    // BOTH steps (not scoped to one step).
    let synth_stage_overlay: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.playbook_stage_overlays o \
         JOIN rev.value_slot_path_details vspd ON vspd.path_id = o.value_slot_path_id \
         WHERE o.playbook_stage_id = $1 AND o.visibility = 'expose' AND vspd.name = $2",
    )
    .bind(stage_id)
    .bind(&synth_name)
    .fetch_one(sql)
    .await
    .expect("overlay count");
    assert_eq!(
        synth_stage_overlay, 1,
        "synth must be a stage-level expose in this variant"
    );

    // BOTH steps expose synth (no ownership gate).
    let step1_exposes = select_computed_outputs_for_step(&pool, dispatch_id, stage_id, step1)
        .await
        .expect("step1 outputs");
    let step2_exposes = select_computed_outputs_for_step(&pool, dispatch_id, stage_id, step2)
        .await
        .expect("step2 outputs");
    assert_eq!(
        step1_exposes.len(),
        1,
        "step 1 must see the stage-exposed synth"
    );
    assert_eq!(
        step2_exposes.len(),
        1,
        "step 2 must see the stage-exposed synth"
    );

    // Write-once: step 1's producer binds synth from step 1's payload; step 2's
    // producer is a no-op (already bound within this dispatch).
    let payload1 = serde_json::json!({"from": "step1"});
    let n1 = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        stage_id,
        step1,
        &payload1,
    )
    .await
    .expect("produce step1")
    .iter()
    .filter(|o| o.outcome == "produced")
    .count();
    assert_eq!(n1, 1, "step 1 binds synth (first writer)");

    let n2 = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        stage_id,
        step2,
        &serde_json::json!({"from": "step2"}),
    )
    .await
    .expect("produce step2")
    .iter()
    .filter(|o| o.outcome == "produced")
    .count();
    assert_eq!(
        n2, 0,
        "step 2 is a no-op (binding is immutable within a dispatch)"
    );

    // Exactly one binding, carrying the FIRST writer's value.
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        1,
        "synth bound exactly once per dispatch"
    );
    let bindings = select_dispatch_intent_bindings_by_dispatch_ids(&pool, vec![dispatch_id])
        .await
        .expect("bindings");
    let synth = bindings
        .iter()
        .find(|b| b.value_slot_name == synth_name)
        .expect("synth binding present");
    assert_eq!(
        synth.json_items[0].value, payload1,
        "the bound value is the first producing step's payload"
    );
}

/// Build a guide whose single brief carries a `computed` `:::brief-output` with
/// an authored `source_path` (caller-supplied), correctly wired into the brief's
/// `units`. Used to prove the author's `source_path` survives ingest and that
/// editing it re-versions the brief.
fn create_source_path_output_tarball(suffix: &str, source_path: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Source Path Output Guide {suffix}\"\n\
         description = \"Computed brief-output with an authored source_path\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"candidates-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-output {{ name: \"cand-out-{short}\", slot: \"candidates-{short}\", source: \"computed\", source_path: \"{source_path}\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"cand-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"candidates-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "cand-out-{short} = Candidates output\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Regression: an authored `source_path` on a computed `:::brief-output` must be
/// persisted to `rev.brief_outputs.source_path`. The parser previously discarded
/// `source_path` (no field on `BriefOutputDef`) and the materializer hardcoded it
/// to `""`, so every computed output silently bound the whole payload — breaking
/// any output that extracts a sub-path (e.g. `/choices`). fetch_one also proves
/// the output materialized at all (it would be absent if not wired into `units`).
#[tokio::test]
async fn test_brief_output_source_path_persisted() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;

    ingest_tarball(
        &base_url,
        create_source_path_output_tarball(&suffix, "/choices"),
    )
    .await;

    let unit_key = format!("cand-out-{short}");
    let source_path: String =
        sqlx::query_scalar("SELECT source_path FROM rev.brief_outputs WHERE unit_key = $1")
            .bind(&unit_key)
            .fetch_one(&sql)
            .await
            .expect("the computed brief-output must materialize as a rev.brief_outputs row");

    assert_eq!(
        source_path, "/choices",
        "the authored source_path must survive ingest (was hardcoded to empty before the fix)"
    );
}

/// Latest `rev.brief_outputs.source_path` for an output by its unit_key,
/// resolving to the newest brief version at that path.
async fn latest_output_source_path(sql: &sqlx::PgPool, unit_key: &str) -> String {
    sqlx::query_scalar(
        "SELECT bo.source_path \
         FROM rev.brief_outputs bo \
         JOIN rev.briefs b ON b.id = bo.brief_id \
         JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id \
         WHERE bo.unit_key = $1 \
         ORDER BY vo.created_at DESC, vo.id DESC LIMIT 1",
    )
    .bind(unit_key)
    .fetch_one(sql)
    .await
    .expect("latest output source_path")
}

/// Core regression for the hash fold: editing ONLY a computed brief-output's
/// `source_path` must re-version the brief under a normal (non-backfill)
/// re-ingest, and the new version must carry the edited path. Before folding
/// output config into the brief content hash, the brief hash was unchanged
/// (`{name, units, title_fluent}`), so the brief deduped and the edit silently
/// did not propagate — the bug this closes.
#[tokio::test]
async fn test_brief_output_source_path_edit_reversions_brief() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let brief_name = format!("brief-{short}");
    let unit_key = format!("cand-out-{short}");

    // v1: source_path "/choices".
    let (path_id, _g) = initial_ingest(
        &base_url,
        create_source_path_output_tarball(&suffix, "/choices"),
    )
    .await;
    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        1,
        "exactly one brief version after POST"
    );
    assert_eq!(latest_output_source_path(&sql, &unit_key).await, "/choices");

    // Edit ONLY the output's source_path and re-ingest (Normal, backfill=false).
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_source_path_output_tarball(&suffix, "/results"),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "re-ingest status {status}: {body}"
    );

    // The brief must re-version (the fix) and the latest output must carry the
    // edited path.
    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        2,
        "an output source_path edit must mint a new brief version (hash fold)"
    );
    assert_eq!(
        latest_output_source_path(&sql, &unit_key).await,
        "/results",
        "the edited source_path must be live on the new brief version"
    );
}

/// Companion to the edit test: re-ingesting the SAME source_path must NOT mint a
/// new brief version. Guards that folding output config into the hash stays
/// deterministic (BTreeMap + canonical_json) so unchanged guides still dedup.
#[tokio::test]
async fn test_brief_output_source_path_noop_redups() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let brief_name = format!("brief-{short}");

    let tarball = create_source_path_output_tarball(&suffix, "/choices");
    let (path_id, _g) = initial_ingest(&base_url, tarball.clone()).await;
    assert_eq!(brief_version_count_by_name(&sql, &brief_name).await, 1);

    let (status, body) = ingest_tarball_patch(&base_url, &path_id, tarball, false).await;
    assert_eq!(
        status, 200,
        "no-op re-ingest should refresh in place: {body}"
    );

    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        1,
        "an unchanged source_path must NOT mint a new brief version"
    );
}

// ============================================================================
// Typed brief-output display hint (display / reveal / title)
// ============================================================================

/// Build a guide whose single brief carries a computed `:::brief-output` with a
/// typed `display` hint plus the `reveal` + `title` modifiers, wired into the
/// brief's `units`. Used to prove the display contract survives ingest and that
/// editing the display re-versions the brief.
fn create_display_output_tarball(
    suffix: &str,
    display: &str,
    reveal: bool,
    title: &str,
) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Display Output Guide {suffix}\"\n\
         description = \"Computed brief-output with a typed display hint\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"candidates-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-output {{ name: \"cand-out-{short}\", slot: \"candidates-{short}\", source: \"computed\", source_path: \"/x\", display: \"{display}\", reveal: {reveal}, title: \"{title}\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"cand-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"candidates-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "cand-out-{short} = Candidates output\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Latest (`display_hint` key, `reveal`, `title_text`) for an output by unit_key,
/// resolving to the newest brief version at that path. NULL display hint COALESCEs
/// to 'text', mirroring the read side.
async fn latest_output_display(
    sql: &sqlx::PgPool,
    unit_key: &str,
) -> (String, bool, Option<String>) {
    sqlx::query_as::<_, (String, bool, Option<String>)>(
        "SELECT COALESCE(dh.key, 'text'), bo.reveal, bo.title_text \
         FROM rev.brief_outputs bo \
         JOIN rev.briefs b ON b.id = bo.brief_id \
         JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id \
         LEFT JOIN rev.brief_output_display_hints dh ON dh.id = bo.display_hint_id \
         WHERE bo.unit_key = $1 \
         ORDER BY vo.created_at DESC, vo.id DESC LIMIT 1",
    )
    .bind(unit_key)
    .fetch_one(sql)
    .await
    .expect("latest output display")
}

/// The authored `display` / `reveal` / `title` on a computed `:::brief-output`
/// must persist to `rev.brief_outputs` (display_hint_id resolved via the lookup,
/// reveal, title_text). fetch_one also proves the output materialized at all.
#[tokio::test]
async fn test_brief_output_display_persisted() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;

    ingest_tarball(
        &base_url,
        create_display_output_tarball(&suffix, "checklist", true, "Find these"),
    )
    .await;

    let unit_key = format!("cand-out-{short}");
    let (display, reveal, title) = latest_output_display(&sql, &unit_key).await;

    assert_eq!(display, "checklist", "authored display must survive ingest");
    assert!(reveal, "authored reveal modifier must survive ingest");
    assert_eq!(
        title.as_deref(),
        Some("Find these"),
        "authored title must survive ingest"
    );
}

/// Editing ONLY a computed brief-output's `display` must re-version the brief
/// under a normal re-ingest (the display fields are folded into the brief content
/// hash), and the new version must carry the edited display.
#[tokio::test]
async fn test_brief_output_display_edit_reversions_brief() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let brief_name = format!("brief-{short}");
    let unit_key = format!("cand-out-{short}");

    // v1: display checklist.
    let (path_id, _g) = initial_ingest(
        &base_url,
        create_display_output_tarball(&suffix, "checklist", false, "Find these"),
    )
    .await;
    assert_eq!(brief_version_count_by_name(&sql, &brief_name).await, 1);
    assert_eq!(latest_output_display(&sql, &unit_key).await.0, "checklist");

    // Edit ONLY the display (checklist -> table) and re-ingest (Normal).
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_display_output_tarball(&suffix, "table", false, "Find these"),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "re-ingest status {status}: {body}"
    );

    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        2,
        "a display edit must mint a new brief version (hash fold)"
    );
    assert_eq!(
        latest_output_display(&sql, &unit_key).await.0,
        "table",
        "the edited display must be live on the new brief version"
    );
}

/// Re-ingesting the SAME display must NOT mint a new brief version — the display
/// fold stays deterministic and unchanged guides still dedup. Also guards that a
/// brief whose output has NO authored display (defaults to text) is unaffected:
/// the fold is sparse (only non-default display fields participate).
#[tokio::test]
async fn test_brief_output_display_noop_redups() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let brief_name = format!("brief-{short}");

    let tarball = create_display_output_tarball(&suffix, "checklist", true, "Find these");
    let (path_id, _g) = initial_ingest(&base_url, tarball.clone()).await;
    assert_eq!(brief_version_count_by_name(&sql, &brief_name).await, 1);

    let (status, body) = ingest_tarball_patch(&base_url, &path_id, tarball, false).await;
    assert_eq!(
        status, 200,
        "no-op re-ingest should refresh in place: {body}"
    );

    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        1,
        "an unchanged display must NOT mint a new brief version"
    );
}

// ============================================================================
// Authored pane order + sibling-format grouping (migration 95)
// ============================================================================

/// Build a guide whose single brief authors FOUR units in an order that
/// disagrees with every ordering the platform could otherwise fall back to.
///
/// Authored sequence, and the slot each unit pins:
///
/// | # | unit             | slot               | table          |
/// |---|------------------|--------------------|----------------|
/// | 1 | `zz-src` output  | `zz-src-{short}`   | brief_outputs  |
/// | 2 | `mm-html` output | `mm-html-{short}`  | brief_outputs  |
/// | 3 | `bb-input` input | `bb-input-{short}` | brief_units    |
/// | 4 | `aa-pdf` output  | `aa-pdf-{short}`   | brief_outputs  |
///
/// The prefixes make three orderings mutually distinguishable:
/// - **alphabetical by slot name** (what `/steps` did before): aa, bb, mm, zz —
///   the exact reverse of authored, and the reason a client could not tell which
///   sibling format the author meant to lead.
/// - **units-then-outputs** (any `COALESCE(brief_unit_id, brief_output_id)`
///   shortcut, since the two tables are separate INSERT batches): bb, zz, mm, aa.
/// - **authored** (`rev.brief_units.ord` / `rev.brief_outputs.ord`): zz, mm, bb, aa.
///
/// The input at position 3 is load-bearing: it sits BETWEEN outputs, so nothing
/// that orders one table before the other can reproduce the sequence.
///
/// The three outputs also share a `group`, with `format` on the two visible ones
/// and none on the hidden source slot — the shape a report rendered to HTML and
/// PDF from one markdown source actually has.
fn create_grouped_outputs_tarball(suffix: &str, group: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Grouped Outputs Guide {suffix}\"\n\
         description = \"Sibling-format outputs in authored order\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"zz-src-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"mm-html-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"bb-input-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"aa-pdf-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-output {{ name: \"zz-src-out-{short}\", slot: \"zz-src-{short}\", source: \"computed\", source_path: \"/src\", display: \"hidden\", group: \"{group}\" }}\n:::\n\n\
         :::brief-output {{ name: \"mm-html-out-{short}\", slot: \"mm-html-{short}\", source: \"computed\", source_path: \"/html\", display: \"document\", title: \"Read the report\", group: \"{group}\", format: \"html\" }}\n:::\n\n\
         :::brief-input {{ name: \"bb-input-in-{short}\", control: \"text\", slot: \"bb-input-{short}\" }}\n:::\n\n\
         :::brief-output {{ name: \"aa-pdf-out-{short}\", slot: \"aa-pdf-{short}\", source: \"computed\", source_path: \"/pdf\", display: \"file\", title: \"Download the report\", group: \"{group}\", format: \"pdf\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"zz-src-out-{short}\", \"mm-html-out-{short}\", \"bb-input-in-{short}\", \"aa-pdf-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"zz-src-{short}\", \"mm-html-{short}\", \"bb-input-{short}\", \"aa-pdf-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "zz-src-out-{short} = Source output\n\
         mm-html-out-{short} = HTML output\n\
         bb-input-in-{short} = An input\n\
         aa-pdf-out-{short} = PDF output\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// `/steps` must return a step's exposed slots in the order their brief units
/// were AUTHORED, not sorted by value-slot name.
///
/// Before migration 95 the final `ORDER BY` fell back to `vspd.name`, so the pane
/// rendered alphabetically — arbitrary in general, and specifically unable to say
/// which of several sibling-format outputs leads the group. The fixture's four
/// units are named so alphabetical order is the exact reverse of authored order,
/// and so an input sits between two outputs (defeating any scheme that orders the
/// brief_units table before the brief_outputs table).
#[tokio::test]
async fn test_steps_orders_exposed_slots_by_authored_position() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();

    let group = format!("report-{short}");
    let json = ingest_tarball(&base_url, create_grouped_outputs_tarball(&suffix, &group)).await;
    let guide_id =
        uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&json);

    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("steps-order-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");

    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    let step = steps
        .iter()
        .find(|s| !s.exposed_slots.is_empty())
        .unwrap_or_else(|| panic!("no step exposed any slot: {steps:?}"));

    let order: Vec<&str> = step.exposed_slots.iter().map(|s| s.name.as_str()).collect();
    let authored = [
        format!("zz-src-{short}"),
        format!("mm-html-{short}"),
        format!("bb-input-{short}"),
        format!("aa-pdf-{short}"),
    ];
    let expected: Vec<&str> = authored.iter().map(String::as_str).collect();

    assert_eq!(
        order, expected,
        "exposed slots must arrive in AUTHORED order. Alphabetical (the pre-95 \
         behavior) would give [aa, bb, mm, zz]; ordering brief_units before \
         brief_outputs would give [bb, zz, mm, aa]."
    );
}

/// The authored ordinal must be persisted as ONE sequence spanning both tables.
/// This is the storage-level assertion behind the `/steps` ordering test: units
/// and outputs of a brief share a numbering, so a reader can interleave them.
#[tokio::test]
async fn test_brief_authored_ord_spans_units_and_outputs() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;

    let group = format!("report-{short}");
    ingest_tarball(&base_url, create_grouped_outputs_tarball(&suffix, &group)).await;

    // UNION the two tables and read back the merged sequence, exactly as the
    // /steps ORDER BY does across the operator/computed split.
    let rows: Vec<(String, i32)> = sqlx::query_as(
        "SELECT unit_key, ord FROM rev.brief_units \
           WHERE unit_key LIKE $1 \
         UNION ALL \
         SELECT unit_key, ord FROM rev.brief_outputs \
           WHERE unit_key LIKE $1 \
         ORDER BY 2",
    )
    .bind(format!("%-{short}"))
    .fetch_all(&sql)
    .await
    .expect("read authored ordinals");

    let sequence: Vec<(&str, i32)> = rows.iter().map(|(k, o)| (k.as_str(), *o)).collect();
    assert_eq!(
        sequence,
        vec![
            (format!("zz-src-out-{short}").as_str(), 1),
            (format!("mm-html-out-{short}").as_str(), 2),
            (format!("bb-input-in-{short}").as_str(), 3),
            (format!("aa-pdf-out-{short}").as_str(), 4),
        ],
        "units and outputs must share ONE 1-based authored numbering; a per-table \
         numbering would show two runs of 1..n"
    );
}

/// Build a guide whose ONE step exposes slots pinned by TWO different briefs.
///
/// This is the shape that broke: 68% of real steps expose slots from more than
/// one brief, and a sort key that clusters by brief reorders every one of them.
/// The four slots are named so that alphabetical order **interleaves the two
/// briefs** (`aa` and `mm` from brief one, `bb` and `zz` from brief two) — so any
/// per-brief grouping key placed above the name tiebreak is immediately visible
/// as `[aa, mm, bb, zz]` instead of `[aa, bb, mm, zz]`.
fn create_two_brief_step_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Two Brief Step Guide {suffix}\"\n\
         description = \"One step exposing slots from two briefs\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"aa-one-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"mm-one-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"bb-two-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"zz-two-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-output {{ name: \"aa-one-out-{short}\", slot: \"aa-one-{short}\", source: \"computed\", source_path: \"/a\", display: \"text\" }}\n:::\n\n\
         :::brief-output {{ name: \"mm-one-out-{short}\", slot: \"mm-one-{short}\", source: \"computed\", source_path: \"/m\", display: \"text\" }}\n:::\n\n\
         :::brief-output {{ name: \"bb-two-out-{short}\", slot: \"bb-two-{short}\", source: \"computed\", source_path: \"/b\", display: \"text\" }}\n:::\n\n\
         :::brief-output {{ name: \"zz-two-out-{short}\", slot: \"zz-two-{short}\", source: \"computed\", source_path: \"/z\", display: \"text\" }}\n:::\n\n\
         :::brief {{ name: \"brief-one-{short}\", units: [\"aa-one-out-{short}\", \"mm-one-out-{short}\"] }}\n:::\n\n\
         :::brief {{ name: \"brief-two-{short}\", units: [\"bb-two-out-{short}\", \"zz-two-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-one-{short}\", \"brief-two-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"aa-one-{short}\", \"mm-one-{short}\", \"bb-two-{short}\", \"zz-two-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "aa-one-out-{short} = A output\n\
         mm-one-out-{short} = M output\n\
         bb-two-out-{short} = B output\n\
         zz-two-out-{short} = Z output\n\
         brief-one-{short} = Brief one\n\
         brief-two-{short} = Brief two\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// A brief materialized BEFORE migration 95 carries `ord = 0` on every unit, so
/// its slots must tie on the ordinal and fall through to the historical name
/// ordering — unchanged by this migration, across briefs as well as within one.
///
/// This is the test whose absence let a regression ship: the first cut of the
/// ordering fix sorted by `brief_id` above the name tiebreak, which left
/// single-brief steps untouched (so the other ordering test passed) while
/// silently reordering every legacy multi-brief pane. 68% of real steps expose
/// slots from 2+ briefs.
///
/// Zeroing the ordinals after ingest is the corruption step: it reproduces the
/// exact on-disk state of every brief that existed before the migration ran.
#[tokio::test]
async fn test_steps_legacy_zero_ord_keeps_name_ordering() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();
    let sql = get_db_pool().await;

    let json = ingest_tarball(&base_url, create_two_brief_step_tarball(&suffix)).await;
    let guide_id =
        uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&json);

    // CORRUPT: force every ordinal back to the pre-migration-95 sentinel.
    let zeroed = sqlx::query("UPDATE rev.brief_outputs SET ord = 0 WHERE unit_key LIKE $1")
        .bind(format!("%-{short}"))
        .execute(&sql)
        .await
        .expect("zero the authored ordinals")
        .rows_affected();
    assert_eq!(zeroed, 4, "fixture must have 4 outputs to zero");

    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("legacy-order-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");

    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    let step = steps
        .iter()
        .find(|s| !s.exposed_slots.is_empty())
        .unwrap_or_else(|| panic!("no step exposed any slot: {steps:?}"));

    let order: Vec<&str> = step.exposed_slots.iter().map(|s| s.name.as_str()).collect();
    let by_name = [
        format!("aa-one-{short}"),
        format!("bb-two-{short}"),
        format!("mm-one-{short}"),
        format!("zz-two-{short}"),
    ];
    let expected: Vec<&str> = by_name.iter().map(String::as_str).collect();

    assert_eq!(
        order, expected,
        "with every ord = 0 (a pre-migration-95 brief) slots must fall through to \
         NAME order across both briefs. Any per-brief key above the name tiebreak \
         clusters brief-one's [aa, mm] before brief-two's [bb, zz] (or vice versa) \
         instead of interleaving them."
    );
}

/// Authored `group` / `format` must survive ingest and reach `/steps` on the
/// computed slots' `display`, including the hidden sibling (which carries the
/// group but renders nothing).
#[tokio::test]
async fn test_steps_reports_output_group_and_format() {
    use dapple_data::model::briefs::BriefOutputDisplay;
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();

    let group = format!("report-{short}");
    let json = ingest_tarball(&base_url, create_grouped_outputs_tarball(&suffix, &group)).await;
    let guide_id =
        uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&json);

    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("steps-group-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");

    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    let slots: Vec<_> = steps.iter().flat_map(|s| s.exposed_slots.iter()).collect();

    let by_name = |name: &str| {
        slots
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("slot {name} not exposed: {slots:?}"))
    };

    // All three computed siblings carry the SAME opaque group token.
    for slot_name in [
        format!("zz-src-{short}"),
        format!("mm-html-{short}"),
        format!("aa-pdf-{short}"),
    ] {
        let display = by_name(&slot_name)
            .display
            .as_ref()
            .unwrap_or_else(|| panic!("computed slot {slot_name} must carry a display spec"));
        assert_eq!(
            display.group.as_deref(),
            Some(group.as_str()),
            "{slot_name} must report its authored group"
        );
    }

    let html = by_name(&format!("mm-html-{short}"))
        .display
        .as_ref()
        .expect("html display");
    assert_eq!(html.format.as_deref(), Some("html"));
    assert_eq!(html.kind, BriefOutputDisplay::Document);

    let pdf = by_name(&format!("aa-pdf-{short}"))
        .display
        .as_ref()
        .expect("pdf display");
    assert_eq!(pdf.format.as_deref(), Some("pdf"));
    assert_eq!(pdf.kind, BriefOutputDisplay::File);

    // The hidden source slot is grouped but formatless — a client filters it out
    // before grouping, so it must never become a third switcher entry.
    let src = by_name(&format!("zz-src-{short}"))
        .display
        .as_ref()
        .expect("src display");
    assert_eq!(src.kind, BriefOutputDisplay::Hidden);
    assert!(
        src.format.is_none(),
        "the hidden data-flow slot has no format to switch to"
    );

    // An operator input carries no display spec at all (it renders via `input`).
    assert!(
        by_name(&format!("bb-input-{short}")).display.is_none(),
        "operator slots must not carry a computed display spec"
    );
}

/// Tarball with one stage whose `kind:` attribute is parameterized
/// (`None` ⇒ no attribute authored at all). Minimal brief with one operator
/// input exposed so `/steps` yields slot rows.
fn create_stage_kind_tarball(suffix: &str, kind_attr: Option<&str>) -> Vec<u8> {
    let extra = kind_attr
        .map(|k| format!("kind: \"{k}\", "))
        .unwrap_or_default();
    create_stage_attr_tarball(suffix, &extra)
}

/// Icon twin of [`create_stage_kind_tarball`]. `icon_attr` is spliced verbatim
/// into the `:::stage` attribute block so a test can exercise the array shape,
/// the bare-string shape, and unquoted hyphenated names as an author types them.
fn create_stage_icon_tarball(suffix: &str, icon_attr: Option<&str>) -> Vec<u8> {
    let extra = icon_attr
        .map(|i| format!("icon: {i}, "))
        .unwrap_or_default();
    create_stage_attr_tarball(suffix, &extra)
}

/// One-stage guide whose `:::stage` block carries `stage_attrs` verbatim (must
/// end with `", "` when non-empty). Shared by the kind and icon suites so both
/// exercise the identical materialization path.
fn create_stage_attr_tarball(suffix: &str, stage_attrs: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Stage Kind Guide {suffix}\"\n\
         description = \"Stage kind attribute end-to-end\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let kind_part = stage_attrs;
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"topic-{short}\", kind: \"text\" }}\n:::\n\n\
         :::brief-input {{ name: \"topic-in-{short}\", control: \"text\", slot: \"topic-{short}\" }}\n:::\n\n\
         :::brief {{ name: \"kbrief-{short}\", units: [\"topic-in-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"kgroup-{short}\", briefs: [\"kbrief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"kpack-{short}\", groups: [\"kgroup-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"kstage-{short}\", {kind_part}expose: [\"topic-{short}\"] }}\n\n\
         :::step {{ name: \"kstep-{short}\", task: \"ktask-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"ktask-{short}\" }}\n{{ payload: {{}}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "topic-in-{short} = The topic\n\
         kbrief-{short} = The brief\n\
         kgroup-{short} = The group\n\
         kpack-{short} = The pack\n\
         kstage-{short} = Pull the record\n\
         kstep-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Stored kind key of the LATEST stage version at the given path name.
/// `None` = the row exists with NULL kind_id (or no kind row joined).
async fn latest_stage_kind_by_name(db: &sqlx::PgPool, stage_name: &str) -> Option<String> {
    let row: (Option<String>,) = sqlx::query_as(
        "SELECT sk.key
         FROM rev.stages s
         JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = vo.path_id
         LEFT JOIN rev.stage_kinds sk ON sk.id = s.kind_id
         WHERE spd.name = $1
         ORDER BY s.id DESC
         LIMIT 1",
    )
    .bind(stage_name)
    .fetch_one(db)
    .await
    .expect("latest stage kind lookup failed");
    row.0
}

/// Version count at a stage path (mirrors `brief_version_count_by_name`).
async fn stage_version_count_by_name(db: &sqlx::PgPool, stage_name: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)
         FROM rev.stages s
         JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = vo.path_id
         WHERE spd.name = $1",
    )
    .bind(stage_name)
    .fetch_one(db)
    .await
    .expect("stage version count failed");
    row.0
}

/// An authored `kind:` must persist to `rev.stages.kind_id` (SQL-verified, not
/// response shape) and ride both journey status reads: `/steps` rows carry
/// `stage_kind`/`stage_name`/`stage_description` repeated per step, `/stages`
/// rows carry `name`/`description`/`kind`.
#[tokio::test]
async fn test_stage_kind_persists_and_reaches_journey_status() {
    use dapple_data::model::stages::StageKind;
    use dapple_data::query::journeys::{
        select_stage_binding_status_for_journey, select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();
    let sql = get_db_pool().await;

    let json = ingest_tarball(&base_url, create_stage_kind_tarball(&suffix, Some("fetch"))).await;
    let guide_id =
        uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&json);

    // (a) Persisted: the stage row's kind_id joins back to the authored key.
    assert_eq!(
        latest_stage_kind_by_name(&sql, &format!("kstage-{short}")).await,
        Some("fetch".to_string()),
        "authored kind must persist to rev.stages.kind_id"
    );

    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("stage-kind-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");

    // (b) Every /steps row of the stage repeats the stage identity trio.
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    assert!(!steps.is_empty(), "journey must have steps");
    for step in &steps {
        assert_eq!(step.stage_name, format!("kstage-{short}"));
        assert_eq!(
            step.stage_description, "Pull the record",
            "description must resolve from the stage's Fluent body"
        );
        assert_eq!(step.stage_kind, Some(StageKind::Fetch));
    }

    // (c) /stages carries the same identity per stage entry.
    let stages = select_stage_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("stage status");
    assert!(!stages.is_empty(), "journey must have stages");
    assert_eq!(stages[0].name, format!("kstage-{short}"));
    assert_eq!(stages[0].description, "Pull the record");
    assert_eq!(stages[0].kind, Some(StageKind::Fetch));
}

/// An unknown `kind:` is a graceful downgrade: the ingest response carries a
/// warning naming the stage and the bad key, the row stores NULL kind_id, and
/// the wire serializes with NO `stage_kind` key at all (skip_serializing_if) so
/// clients apply their documented neutral fallback.
#[tokio::test]
async fn test_stage_kind_unknown_warns_and_stores_null() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();
    let sql = get_db_pool().await;

    let json = ingest_tarball(
        &base_url,
        create_stage_kind_tarball(&suffix, Some("shotlist")),
    )
    .await;

    let warnings = json["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("unknown stage kind must surface a warnings array: {json}"));
    assert!(
        warnings.iter().any(|w| {
            let w = w.as_str().unwrap_or_default();
            w.contains(&format!("kstage-{short}")) && w.contains("shotlist")
        }),
        "warning must name the stage and the unrecognized kind: {warnings:?}"
    );

    assert_eq!(
        latest_stage_kind_by_name(&sql, &format!("kstage-{short}")).await,
        None,
        "unknown kind must store NULL, never drop the row or bake the bad key"
    );

    // Wire shape: absent kind means NO stage_kind key (skip_serializing_if).
    let guide_id =
        uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&json);
    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("stage-kind-bogus-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    let wire = serde_json::to_value(&steps[0]).expect("serialize step status");
    assert!(
        wire.get("stage_kind").is_none(),
        "absent kind must serialize with no stage_kind key: {wire}"
    );
    assert_eq!(
        wire["stage_name"],
        serde_json::json!(format!("kstage-{short}")),
        "stage_name rides the wire regardless of kind"
    );
}

/// The hash-fold contract: a kindless guide re-ingests with NO new stage
/// version (the empty-omit fold keeps pre-kind hashes byte-identical), and
/// editing ONLY the kind mints exactly one new version carrying it.
#[tokio::test]
async fn test_stage_kind_edit_reversions_stage_kindless_is_stable() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let stage_name = format!("kstage-{short}");

    let (path_id, _guide_id) =
        initial_ingest(&base_url, create_stage_kind_tarball(&suffix, None)).await;
    assert_eq!(stage_version_count_by_name(&sql, &stage_name).await, 1);

    // Kindless re-ingest -> same hash -> reused version, no re-version.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_stage_kind_tarball(&suffix, None),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "no-op re-ingest status {status}: {body}"
    );
    assert_eq!(
        stage_version_count_by_name(&sql, &stage_name).await,
        1,
        "a kindless re-ingest must NOT mint a new stage version (empty-omit hash fold)"
    );

    // Edit ONLY the kind -> hash forks -> exactly one new version carrying it.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_stage_kind_tarball(&suffix, Some("review")),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "kind-edit re-ingest status {status}: {body}"
    );
    assert_eq!(
        stage_version_count_by_name(&sql, &stage_name).await,
        2,
        "authoring a kind must re-version the stage"
    );
    assert_eq!(
        latest_stage_kind_by_name(&sql, &stage_name).await,
        Some("review".to_string()),
        "the new version must carry the authored kind"
    );
}

/// Stored `rev.stages.icons` of the LATEST stage version at the given path name,
/// projected to the ordered name list. `None` = the column is NULL.
async fn latest_stage_icons_by_name(db: &sqlx::PgPool, stage_name: &str) -> Option<Vec<String>> {
    let row: (Option<serde_json::Value>,) = sqlx::query_as(
        "SELECT s.icons
         FROM rev.stages s
         JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = vo.path_id
         WHERE spd.name = $1
         ORDER BY s.id DESC
         LIMIT 1",
    )
    .bind(stage_name)
    .fetch_one(db)
    .await
    .expect("latest stage icons lookup failed");
    row.0.map(|v| {
        v.as_array()
            .expect("icons must be a JSON array (migration 98 CHECK)")
            .iter()
            .map(|e| {
                e.as_str()
                    .expect("icon element must be a string")
                    .to_string()
            })
            .collect()
    })
}

/// An authored `icon:` must persist to `rev.stages.icons` VERBATIM and in
/// authored order (SQL-verified, not response shape), then ride both journey
/// status reads: `/steps` rows carry `stage_icon` repeated per step, `/stages`
/// rows carry `icon`.
///
/// The attribute is written the way an author actually types it — unquoted,
/// hyphenated — which is question 1 of the dapple-ui ask proven end to end
/// rather than only at the attribute parser.
#[tokio::test]
async fn test_stage_icon_persists_and_reaches_journey_status() {
    use dapple_data::query::journeys::{
        select_stage_binding_status_for_journey, select_step_binding_status_for_journey,
    };

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();
    let sql = get_db_pool().await;

    let json = ingest_tarball(
        &base_url,
        create_stage_icon_tarball(&suffix, Some("[radar, scan-search, id-card]")),
    )
    .await;
    let guide_id =
        uuid::Uuid::parse_str(json["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&json);

    let expected = vec![
        "radar".to_string(),
        "scan-search".to_string(),
        "id-card".to_string(),
    ];

    // (a) Persisted verbatim, in AUTHORED ORDER — a sort or dedup here would
    // silently change which names the client's first-five window sees.
    assert_eq!(
        latest_stage_icons_by_name(&sql, &format!("kstage-{short}")).await,
        Some(expected.clone()),
        "authored icons must persist to rev.stages.icons in authored order"
    );

    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("stage-icon-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");

    // (b) Every /steps row of the stage repeats the icon set identically.
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    assert!(!steps.is_empty(), "journey must have steps");
    for step in &steps {
        assert_eq!(step.stage_icon, expected);
    }

    // (c) /stages carries the same set per stage entry.
    let stages = select_stage_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("stage status");
    assert!(!stages.is_empty(), "journey must have stages");
    assert_eq!(stages[0].icon, expected);

    // (d) Wire shape: a JSON array of strings, in order.
    let wire = serde_json::to_value(&steps[0]).expect("serialize step status");
    assert_eq!(
        wire["stage_icon"],
        serde_json::json!(["radar", "scan-search", "id-card"]),
        "stage_icon must serialize as an ordered JSON array: {wire}"
    );
}

/// The property that makes `icon` the deliberate INVERSE of `kind`: a name this
/// server has never heard of is stored VERBATIM with no warning and no
/// downgrade, because the vocabulary belongs to the drawing client. A guide
/// legitimately authors a name before that client curates it.
///
/// Also pins the bare-string convenience shape and the absent case.
#[tokio::test]
async fn test_stage_icon_unknown_name_is_stored_not_downgraded() {
    use dapple_data::query::journeys::select_step_binding_status_for_journey;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let pool = test_db_pool();
    let sql = get_db_pool().await;

    // A bare string (the one-icon shape), and a name in no vocabulary anywhere.
    let json = ingest_tarball(
        &base_url,
        create_stage_icon_tarball(&suffix, Some("\"totally-invented-glyph\"")),
    )
    .await;

    // No warning: an unrecognized icon name is the NORMAL state, not a defect.
    let warnings = json["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|w| w.as_str().map(String::from))
        .collect::<Vec<_>>();
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("totally-invented-glyph")),
        "an unknown icon NAME must never warn (open vocabulary): {warnings:?}"
    );

    assert_eq!(
        latest_stage_icons_by_name(&sql, &format!("kstage-{short}")).await,
        Some(vec!["totally-invented-glyph".to_string()]),
        "a bare string must normalize to a one-element array, stored verbatim"
    );

    // Absent icons serialize with NO stage_icon key at all — absent and []
    // are the same fact, per the compatibility rule dapple-ui relies on.
    let bare_suffix = uuid::Uuid::now_v7().to_string();
    let bare = ingest_tarball(&base_url, create_stage_icon_tarball(&bare_suffix, None)).await;
    let guide_id =
        uuid::Uuid::parse_str(bare["guide"]["id"].as_str().expect("guide.id")).expect("guide_id");
    let playbook_id = playbook_id_from_ingest(&bare);
    let (status, body) = post_journey_for_playbook(
        &base_url,
        playbook_id,
        guide_id,
        &format!("stage-icon-absent-{short}"),
    )
    .await;
    assert!(status.is_success(), "POST journey status {status}: {body}");
    let journey_id =
        uuid::Uuid::parse_str(body["objs"][0]["id"].as_str().expect("journey id")).expect("uuid");
    let steps = select_step_binding_status_for_journey(&pool, journey_id)
        .await
        .expect("step status");
    let wire = serde_json::to_value(&steps[0]).expect("serialize step status");
    assert!(
        wire.get("stage_icon").is_none(),
        "an icon-less stage must serialize with no stage_icon key: {wire}"
    );
}

/// The hash-fold contract, mirroring the `kind` trio: an icon-less guide
/// re-ingests with NO new stage version (the empty-omit fold keeps pre-icon
/// hashes byte-identical, so the nineteen ingested guides do not mass
/// re-version), and editing ONLY the icons mints exactly one new version.
///
/// Reordering counts as an edit — authored order is load-bearing.
#[tokio::test]
async fn test_stage_icon_edit_reversions_stage_iconless_is_stable() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let stage_name = format!("kstage-{short}");

    let (path_id, _guide_id) =
        initial_ingest(&base_url, create_stage_icon_tarball(&suffix, None)).await;
    assert_eq!(stage_version_count_by_name(&sql, &stage_name).await, 1);

    // Icon-less re-ingest -> same hash -> reused version, no re-version.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_stage_icon_tarball(&suffix, None),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "no-op re-ingest status {status}: {body}"
    );
    assert_eq!(
        stage_version_count_by_name(&sql, &stage_name).await,
        1,
        "an icon-less re-ingest must NOT mint a new stage version (empty-omit hash fold)"
    );

    // Authoring icons forks the hash -> exactly one new version carrying them.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_stage_icon_tarball(&suffix, Some("[radar, globe]")),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "icon-edit re-ingest status {status}: {body}"
    );
    assert_eq!(
        stage_version_count_by_name(&sql, &stage_name).await,
        2,
        "authoring icons must re-version the stage"
    );
    assert_eq!(
        latest_stage_icons_by_name(&sql, &stage_name).await,
        Some(vec!["radar".to_string(), "globe".to_string()])
    );

    // Reordering the SAME names is a real edit: the client picks by position,
    // so a hash that ignored order would strand the guide on the old pick.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_stage_icon_tarball(&suffix, Some("[globe, radar]")),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "icon-reorder re-ingest status {status}: {body}"
    );
    assert_eq!(
        stage_version_count_by_name(&sql, &stage_name).await,
        3,
        "reordering icons must re-version the stage (authored order is load-bearing)"
    );
    assert_eq!(
        latest_stage_icons_by_name(&sql, &stage_name).await,
        Some(vec!["globe".to_string(), "radar".to_string()])
    );
}

/// Authoring more than the five names a client reads WARNS but stores every
/// one: the cap is the drawing client's render window, not a validation rule,
/// and truncating server-side would silently discard authored content.
#[tokio::test]
async fn test_stage_icon_over_cap_warns_but_stores_all() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;

    let json = ingest_tarball(
        &base_url,
        create_stage_icon_tarball(
            &suffix,
            Some("[radar, globe, brain, gauge, compass, telescope, microscope]"),
        ),
    )
    .await;

    let warnings = json["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("over-cap icon list must surface a warnings array: {json}"));
    assert!(
        warnings.iter().any(|w| {
            let w = w.as_str().unwrap_or_default();
            w.contains(&format!("kstage-{short}")) && w.contains("telescope")
        }),
        "warning must name the stage and the names that will never be drawn: {warnings:?}"
    );

    assert_eq!(
        latest_stage_icons_by_name(&sql, &format!("kstage-{short}")).await,
        Some(vec![
            "radar".to_string(),
            "globe".to_string(),
            "brain".to_string(),
            "gauge".to_string(),
            "compass".to_string(),
            "telescope".to_string(),
            "microscope".to_string(),
        ]),
        "all seven names must be stored verbatim — the cap warns, never truncates"
    );
}

/// Editing ONLY an output's `group` must re-version the brief (the grouping
/// fields fold into the brief content hash), and an unchanged group must NOT.
/// Mirrors the display-hint trio — the fold is sparse, so a guide that never
/// authored grouping keeps its hash.
#[tokio::test]
async fn test_output_group_edit_reversions_brief() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let brief_name = format!("brief-{short}");

    let (path_id, _g) = initial_ingest(
        &base_url,
        create_grouped_outputs_tarball(&suffix, &format!("report-{short}")),
    )
    .await;
    assert_eq!(brief_version_count_by_name(&sql, &brief_name).await, 1);

    // Same group -> same hash -> refresh in place, no new version.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_grouped_outputs_tarball(&suffix, &format!("report-{short}")),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "no-op re-ingest status {status}: {body}"
    );
    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        1,
        "an unchanged group must NOT mint a new brief version"
    );

    // Edit ONLY the group token -> new brief version carrying it.
    let edited = format!("deliverable-{short}");
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_grouped_outputs_tarball(&suffix, &edited),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "group-edit re-ingest status {status}: {body}"
    );
    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        2,
        "a group edit must re-version the brief"
    );

    let latest_group: Option<String> = sqlx::query_scalar(
        "SELECT bo.display_group \
         FROM rev.brief_outputs bo \
         JOIN rev.briefs b ON b.id = bo.brief_id \
         JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id \
         WHERE bo.unit_key = $1 \
         ORDER BY vo.created_at DESC, vo.id DESC LIMIT 1",
    )
    .bind(format!("mm-html-out-{short}"))
    .fetch_one(&sql)
    .await
    .expect("read latest display_group");
    assert_eq!(
        latest_group.as_deref(),
        Some(edited.as_str()),
        "the new version must carry the edited group"
    );
}

// ============================================================================
// Deliverable markdown units (computed-output value rendered via $slot
// interpolation in a brief-markdown body)
// ============================================================================

/// Build a guide whose final brief has a computed `:::brief-output` plus a
/// deliverable `:::brief-markdown` that interpolates the output's slot. `heading`
/// is the markdown body's first line, so callers can edit ONLY the markdown body
/// to exercise the brief content-hash fold for markdown text.
fn create_deliverable_markdown_tarball(suffix: &str, heading: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Deliverable Markdown Guide {suffix}\"\n\
         description = \"Computed output rendered via a deliverable brief-markdown\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // Slot uses underscores so `$deliv_<short>` is a clean placeholder. The task
    // returns `{ story }`, and the output lifts `/story`, so the bound value is a
    // bare string. The deliverable markdown interpolates that slot.
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"deliv_{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-output {{ name: \"out-{short}\", slot: \"deliv_{short}\", source: \"computed\", source_path: \"/story\" }}\n:::\n\n\
         :::brief-markdown {{ name: \"md-{short}\", interpolates: [\"deliv_{short}\"] }}\n{heading}\n\n$deliv_{short}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"out-{short}\",\"md-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"deliv_{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{ story: \"hi\" }}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "out-{short} = Deliverable output\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Latest `deliverable_spec` (as text) for a markdown unit by its unit_key,
/// resolving to the newest brief version at that path.
async fn latest_markdown_deliverable_spec(sql: &sqlx::PgPool, unit_key: &str) -> String {
    sqlx::query_scalar(
        "SELECT bum.deliverable_spec::text \
         FROM rev.brief_unit_markdown bum \
         JOIN rev.brief_units bu ON bu.id = bum.brief_unit_id \
         JOIN rev.briefs b ON b.id = bu.brief_id \
         JOIN rev.versioned_objects vo ON vo.id = b.versioned_object_id \
         WHERE bu.unit_key = $1 \
         ORDER BY vo.created_at DESC, vo.id DESC LIMIT 1",
    )
    .bind(unit_key)
    .fetch_one(sql)
    .await
    .expect("latest markdown deliverable_spec")
}

/// A deliverable `:::brief-markdown` (one carrying `interpolates`) must
/// materialize a `rev.brief_unit_markdown` row whose `deliverable_spec` records
/// the resolved slot — the marker the read side and UI key off. (An ordinary
/// markdown with no `interpolates` keeps `{}`; covered implicitly elsewhere.)
#[tokio::test]
async fn test_deliverable_markdown_materializes_spec() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;

    ingest_tarball(
        &base_url,
        create_deliverable_markdown_tarball(&suffix, "## Result"),
    )
    .await;

    let spec = latest_markdown_deliverable_spec(&sql, &format!("md-{short}")).await;
    assert!(
        spec.contains("interpolates") && spec.contains(&format!("deliv_{short}")),
        "deliverable_spec must record the interpolated slot, got: {spec}"
    );
    assert!(
        spec.contains("value_slot_id"),
        "deliverable_spec must resolve the slot name to a value_slot_id, got: {spec}"
    );
}

/// Editing ONLY the deliverable markdown's body must re-version the brief. This
/// proves the markdown body+interpolates are folded into the brief content hash
/// (and closes the pre-existing gap where a markdown body edit did NOT
/// re-version the brief — only its unit name was folded).
#[tokio::test]
async fn test_deliverable_markdown_body_edit_reversions_brief() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let brief_name = format!("brief-{short}");

    let (path_id, _g) = initial_ingest(
        &base_url,
        create_deliverable_markdown_tarball(&suffix, "## Result"),
    )
    .await;
    assert_eq!(brief_version_count_by_name(&sql, &brief_name).await, 1);

    // Edit ONLY the markdown body (heading) and re-ingest.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_deliverable_markdown_tarball(&suffix, "## Your finished result"),
        false,
    )
    .await;
    assert!(
        status.is_success() || status == reqwest::StatusCode::CREATED,
        "re-ingest status {status}: {body}"
    );

    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        2,
        "a markdown body edit must mint a new brief version (markdown hash fold)"
    );
}

/// Companion: re-ingesting the SAME deliverable markdown must NOT mint a new
/// brief version — the markdown hash fold stays deterministic (BTreeMap +
/// canonical_json), so unchanged guides still dedup.
#[tokio::test]
async fn test_deliverable_markdown_noop_redups() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let brief_name = format!("brief-{short}");

    let tarball = create_deliverable_markdown_tarball(&suffix, "## Result");
    let (path_id, _g) = initial_ingest(&base_url, tarball.clone()).await;
    assert_eq!(brief_version_count_by_name(&sql, &brief_name).await, 1);

    let (status, body) = ingest_tarball_patch(&base_url, &path_id, tarball, false).await;
    assert_eq!(
        status, 200,
        "no-op re-ingest should refresh in place: {body}"
    );
    assert_eq!(
        brief_version_count_by_name(&sql, &brief_name).await,
        1,
        "an unchanged deliverable markdown must NOT mint a new brief version"
    );
}

// ============================================================================
// Re-ingest must never drop a sibling step from a multi-step stage
// ============================================================================

/// Build a 2-stage / 5-step guide whose per-step task scripts are caller-chosen,
/// so re-ingests can re-version specific steps (and revert others) to exercise
/// the patch path. Stage Alpha = [a1, a2]; Stage Beta = [b1, b2, b3]. `bodies`
/// supplies the task script payloads in that order: [a1, a2, b1, b2, b3].
fn create_reingest_tarball(suffix: &str, bodies: &[&str; 5]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        r#"title = "Reingest Steps Guide {suffix}"
description = "Two multi-step stages for re-ingest regression"
author = "Test"
classification = "test-{suffix}""#
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);

    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Introduction](ch1.md)\n- [Workflow](ch2.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );
    append_file(
        &mut archive,
        "guide/ch1.md",
        "# Introduction\n\nJust prose.\n",
    );

    let short = &suffix[28..];
    let [a1, a2, b1, b2, b3] = *bodies;
    // Each task script embeds its `bodies` token so a changed token re-versions
    // that task (and thus its step); an unchanged token across ingests reuses,
    // and a reverted token hits the patch conflict arm (older version id).
    let ch2 = format!(
        "# Workflow\n\n\
         :::stage {{ name: \"Stage Alpha {short}\" }}\n\n\
         :::step {{ name: \"Step A1 {short}\", task: \"task-a1-{short}\" }}\n:::\n\n\
         :::step {{ name: \"Step A2 {short}\", task: \"task-a2-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::stage {{ name: \"Stage Beta {short}\" }}\n\n\
         :::step {{ name: \"Step B1 {short}\", task: \"task-b1-{short}\" }}\n:::\n\n\
         :::step {{ name: \"Step B2 {short}\", task: \"task-b2-{short}\" }}\n:::\n\n\
         :::step {{ name: \"Step B3 {short}\", task: \"task-b3-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-a1-{short}\" }}\n{{ payload: {{ v: \"{a1}\" }}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-a2-{short}\" }}\n{{ payload: {{ v: \"{a2}\" }}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-b1-{short}\" }}\n{{ payload: {{ v: \"{b1}\" }}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-b2-{short}\" }}\n{{ payload: {{ v: \"{b2}\" }}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-b3-{short}\" }}\n{{ payload: {{ v: \"{b3}\" }}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch2.md", &ch2);

    let fluent = format!(
        "stage-alpha-{short} = Alpha stage.\n\
         step-a1-{short} = Step A1.\n\
         step-a2-{short} = Step A2.\n\
         stage-beta-{short} = Beta stage.\n\
         step-b1-{short} = Step B1.\n\
         step-b2-{short} = Step B2.\n\
         step-b3-{short} = Step B3.\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// For the LATEST version of each named stage, return `stage_slug -> {step_slug}`
/// resolved through `rev.stage_steps`. This is the authored structure the
/// materializer must preserve; a dropped or cross-wired step shows up as a
/// missing/foreign step_slug here (the live shaggy-dog corruption was exactly
/// `weave-the-distractions` losing `plant-the-running-gags`).
async fn latest_stage_step_map(
    db: &sqlx::PgPool,
    stage_slugs: &[String],
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT ls.slug AS stage_slug, stppd.slug AS step_slug
         FROM (
             SELECT DISTINCT ON (spd.slug) s.id, spd.slug
             FROM rev.stages s
             JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
             JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
             WHERE spd.slug = ANY($1)
             ORDER BY spd.slug, svo.created_at DESC
         ) ls
         JOIN rev.stage_steps ss ON ss.stage_id = ls.id
         JOIN rev.steps st ON st.id = ss.step_id
         JOIN rev.versioned_objects stvo ON stvo.id = st.versioned_object_id
         JOIN rev.step_path_details stppd ON stppd.path_id = stvo.path_id
         ORDER BY stage_slug, step_slug",
    )
    .bind(stage_slugs)
    .fetch_all(db)
    .await
    .expect("stage->step query failed");

    let mut map: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for (stage_slug, step_slug) in rows {
        map.entry(stage_slug).or_default().insert(step_slug);
    }
    map
}

/// Re-ingesting a guide must never drop or cross-wire a step in a multi-step
/// stage. The corrupting case is a MIXED patch batch: when one step reverts to a
/// prior content hash (the patch conflict arm returns its older, smaller id)
/// while siblings get fresh ids, the pre-fix `ORDER BY id` patch SQL +
/// positional Rust assignment re-ordered the result and mis-paired
/// version<->step<->stage — cross-wiring a Beta step into Alpha and dropping a
/// sibling via `insert_stage_steps`' `ON CONFLICT (stage_id, step_id) DO NOTHING`.
#[tokio::test]
async fn test_multi_step_stage_reingest_preserves_steps() {
    use std::collections::{BTreeMap, BTreeSet};

    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;

    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];

    let alpha = format!("stage-alpha-{short}");
    let beta = format!("stage-beta-{short}");
    let stage_slugs = vec![alpha.clone(), beta.clone()];
    let expected: BTreeMap<String, BTreeSet<String>> = BTreeMap::from([
        (
            alpha.clone(),
            BTreeSet::from([format!("step-a1-{short}"), format!("step-a2-{short}")]),
        ),
        (
            beta.clone(),
            BTreeSet::from([
                format!("step-b1-{short}"),
                format!("step-b2-{short}"),
                format!("step-b3-{short}"),
            ]),
        ),
    ]);

    // v1 — initial ingest.
    let (path_id, _guide_id) = initial_ingest(
        &base_url,
        create_reingest_tarball(&suffix, &["v1", "v1", "v1", "v1", "v1"]),
    )
    .await;
    assert_eq!(
        latest_stage_step_map(&db, &stage_slugs).await,
        expected,
        "v1: each stage must link exactly its authored steps"
    );

    // v2 — every step's task changes: homogeneous all-new patch batch.
    let (s2, j2) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_reingest_tarball(&suffix, &["v2", "v2", "v2", "v2", "v2"]),
        false,
    )
    .await;
    assert!(s2.is_success(), "v2 re-ingest failed: {s2} {j2}");
    assert_eq!(
        latest_stage_step_map(&db, &stage_slugs).await,
        expected,
        "v2: no step may be dropped or cross-wired"
    );

    // v3 — MIXED batch: b3 (the LAST step in input order) reverts to its v1
    // content, so the patch returns its older/smaller id; a1/a2/b1/b2 get fresh
    // ids. This is the ordering case that mis-paired the batch pre-fix.
    let (s3, j3) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_reingest_tarball(&suffix, &["v3", "v3", "v3", "v3", "v1"]),
        false,
    )
    .await;
    assert!(s3.is_success(), "v3 re-ingest failed: {s3} {j3}");
    assert_eq!(
        latest_stage_step_map(&db, &stage_slugs).await,
        expected,
        "v3 (mixed batch): every stage must retain exactly its authored steps \
         (no drop, no cross-wire)"
    );
}

/// `?backfill=true` must make a guide whose materialized structure was corrupted
/// resolve, end to end, to a COMPLETE playbook — the user-facing "backfill fixes
/// broken guides" guarantee. This is the path that actually repairs the demo:
/// backfill re-materializes a fresh, correct playbook version and the
/// guide→playbook resolution (E1, latest-non-deprecated-per-path) returns it,
/// with the corrupted prior version deprecated (E2). Corrupt-then-verify per the
/// repair-test discipline (AGENTS.md): DELETE a `stage_step`, backfill, then
/// assert the corruption is gone THROUGH THE RESOLUTION (not just "latest stage
/// by slug", which a fresh version would mask regardless of the resolution fix).
///
/// MECHANISM: under `?backfill=true` the task / value-slot / brief hashes are
/// nonce-rewritten (`force_repair_nonce` / `repair_hash`), so the whole
/// task→step→stage→playbook chain re-versions and a FRESH, correct playbook
/// version is produced (the corrupt prior version is superseded, not reused). The
/// repair is therefore delivered by RESOLUTION — E1 returns the new version and
/// E2 deprecates the old — which is exactly what this asserts.
///
/// Pre-E1 this FAILS: the guide resolves to BOTH the corrupt prior version and
/// the fresh one (no collapse), so a new journey can still pin the broken stage.
#[tokio::test]
async fn test_backfill_repairs_guide_resolution_after_stage_step_drop() {
    use std::collections::BTreeSet;

    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];

    let alpha = format!("stage-alpha-{short}");
    let alpha_steps = BTreeSet::from([format!("step-a1-{short}"), format!("step-a2-{short}")]);

    // Ingest.
    let bodies = ["v1", "v1", "v1", "v1", "v1"];
    let (path_id, guide_id_str) =
        initial_ingest(&base_url, create_reingest_tarball(&suffix, &bodies)).await;
    let guide_id = uuid::Uuid::parse_str(&guide_id_str).unwrap();

    // The playbook a journey would pin right now (the resolved one).
    let p1 = {
        let resolved = dapple_data::query::guide_playbooks::select_guide_playbooks_by_guide_ids(
            &test_db_pool(),
            &[guide_id],
        )
        .await
        .unwrap();
        assert_eq!(resolved.len(), 1, "post-ingest: one playbook resolved");
        resolved[0].playbook_id
    };
    assert_eq!(
        resolved_playbook_stage_steps(&db, p1, &alpha).await,
        alpha_steps,
        "post-ingest: resolved playbook's Alpha links both steps"
    );

    // Corrupt: drop the (P1 Alpha stage, step-a2) link directly in SQL.
    let dropped = sqlx::query(
        "DELETE FROM rev.stage_steps ss
         USING rev.steps st, rev.versioned_objects stvo, rev.step_path_details stppd
         WHERE ss.step_id = st.id
           AND stvo.id = st.versioned_object_id
           AND stppd.path_id = stvo.path_id
           AND stppd.slug = $3
           AND ss.stage_id = (
               SELECT ps.stage_id FROM rev.playbook_stages ps
               JOIN rev.stages s ON s.id = ps.stage_id
               JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
               JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
               WHERE ps.playbook_id = $1 AND spd.slug = $2
           )",
    )
    .bind(p1)
    .bind(&alpha)
    .bind(format!("step-a2-{short}"))
    .execute(&db)
    .await
    .expect("corrupt delete failed");
    assert_eq!(
        dropped.rows_affected(),
        1,
        "setup drops exactly one stage_step"
    );
    assert_eq!(
        resolved_playbook_stage_steps(&db, p1, &alpha).await,
        BTreeSet::from([format!("step-a1-{short}")]),
        "corruption established: the resolved playbook's Alpha now links only step-a1"
    );

    // Backfill with UNCHANGED content.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_reingest_tarball(&suffix, &bodies),
        true,
    )
    .await;
    assert!(status.is_success(), "backfill failed: {status} {body}");

    // The guide must now resolve to exactly ONE playbook, and its Alpha must be
    // complete — i.e., a new journey pins a correct stage. (Fails pre-E1: the
    // guide resolves to both p1 (corrupt) and the fresh version.)
    let resolved = dapple_data::query::guide_playbooks::select_guide_playbooks_by_guide_ids(
        &test_db_pool(),
        &[guide_id],
    )
    .await
    .unwrap();
    assert_eq!(
        resolved.len(),
        1,
        "E1: backfill must leave exactly one resolved playbook, not stack the corrupt prior one"
    );
    let resolved_pb = resolved[0].playbook_id;
    assert_eq!(
        resolved_playbook_stage_steps(&db, resolved_pb, &alpha).await,
        alpha_steps,
        "backfill repair: the resolved playbook's Alpha is complete (a1+a2)"
    );

    // E2: the corrupt prior playbook version is deprecated.
    let p1_dep: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.deprecation_events de
         JOIN rev.playbooks p ON p.versioned_object_id = de.versioned_object_id
         WHERE p.id = $1",
    )
    .bind(p1)
    .fetch_one(&db)
    .await
    .unwrap();
    assert!(
        resolved_pb == p1 || p1_dep >= 1,
        "E2: if backfill superseded p1, the corrupt prior version must be deprecated"
    );
}

/// Steps (by slug) linked to a specific playbook version's stage (by stage slug).
async fn resolved_playbook_stage_steps(
    db: &sqlx::PgPool,
    playbook_id: uuid::Uuid,
    stage_slug: &str,
) -> std::collections::BTreeSet<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT stppd.slug
         FROM rev.playbook_stages ps
         JOIN rev.stages s ON s.id = ps.stage_id
         JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
         JOIN rev.stage_steps ss ON ss.stage_id = s.id
         JOIN rev.steps st ON st.id = ss.step_id
         JOIN rev.versioned_objects stvo ON stvo.id = st.versioned_object_id
         JOIN rev.step_path_details stppd ON stppd.path_id = stvo.path_id
         WHERE ps.playbook_id = $1 AND spd.slug = $2",
    )
    .bind(playbook_id)
    .bind(stage_slug)
    .fetch_all(db)
    .await
    .expect("resolved stage->step query failed");
    rows.into_iter().map(|(s,)| s).collect()
}

/// Ordered step slugs as the real `/steps` rollup
/// (`select_step_binding_status_for_journey`) returns them for a fresh journey
/// pinned to `playbook_id`. A bare journey suffices: slotless steps still yield
/// one ordered row each, so this exercises the rollup's ORDER BY exactly.
async fn rollup_step_order(db: &sqlx::PgPool, playbook_id: uuid::Uuid) -> Vec<String> {
    let principal_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.principals
         WHERE owner_company_id = '019e132e-2610-7a4a-b64a-77a49e286ac0'::uuid
           AND revoked_at IS NULL
         LIMIT 1",
    )
    .fetch_one(db)
    .await
    .expect("a dapple principal must exist");

    let journey_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, name, owner_company_id, principal_id)
         VALUES ($1, 'step-order-test',
                 '019e132e-2610-7a4a-b64a-77a49e286ac0'::uuid, $2)
         RETURNING id",
    )
    .bind(playbook_id)
    .bind(principal_id)
    .fetch_one(db)
    .await
    .expect("journey insert failed");

    let steps = dapple_data::query::journeys::select_step_binding_status_for_journey(
        &test_db_pool(),
        journey_id,
    )
    .await
    .expect("step rollup query failed");

    // Resolve each returned stage_step_id to its step slug, preserving rollup order.
    let mut slugs = Vec::with_capacity(steps.len());
    for s in &steps {
        let slug: String = sqlx::query_scalar(
            "SELECT stppd.slug
             FROM rev.stage_steps ss
             JOIN rev.steps st ON st.id = ss.step_id
             JOIN rev.versioned_objects stvo ON stvo.id = st.versioned_object_id
             JOIN rev.step_path_details stppd ON stppd.path_id = stvo.path_id
             WHERE ss.id = $1",
        )
        .bind(s.stage_step_id)
        .fetch_one(db)
        .await
        .expect("step slug lookup failed");
        slugs.push(slug);
    }
    slugs
}

/// A specific step's `stage_steps` id within a playbook version, by stage+step slug.
async fn stage_step_id_of(
    db: &sqlx::PgPool,
    playbook_id: uuid::Uuid,
    stage_slug: &str,
    step_slug: &str,
) -> uuid::Uuid {
    sqlx::query_scalar(
        "SELECT ss.id
         FROM rev.playbook_stages ps
         JOIN rev.stages s ON s.id = ps.stage_id
         JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
         JOIN rev.stage_steps ss ON ss.stage_id = s.id
         JOIN rev.steps st ON st.id = ss.step_id
         JOIN rev.versioned_objects stvo ON stvo.id = st.versioned_object_id
         JOIN rev.step_path_details stppd ON stppd.path_id = stvo.path_id
         WHERE ps.playbook_id = $1 AND spd.slug = $2 AND stppd.slug = $3",
    )
    .bind(playbook_id)
    .bind(stage_slug)
    .bind(step_slug)
    .fetch_one(db)
    .await
    .expect("stage_step id lookup failed")
}

/// Part A regression. A `stage_step` restored by `?backfill=true` (Model B
/// repair-in-place) gets a FRESH, newest UUIDv7. Under the old GLOBAL
/// `ORDER BY stage_step_id` it then sorted after every earlier-minted step —
/// jumping past LATER stages and stranding itself at the end of the journey.
/// That is the live shaggy-dog stuck-journey: the restored `plant-the-running-gags`
/// sorted after the FINAL stage's steps, so the client (which walks `/steps` order
/// to pick the next step) never reached it, its computed candidates never ran, and
/// the next pick rendered the wrong tiles with no way forward.
///
/// The `/steps` rollup now orders by stored ordinals (playbook_stages.ord, then
/// stage_steps.ord), so a restored step stays in its authored stage. We drop
/// Alpha's LAST step (A2), backfill (restoring A2 with the newest id), and assert
/// the rollup STILL returns A1,A2,B1,B2,B3 — A2 in Alpha, not jumped past Beta.
/// Pre-fix (global `ORDER BY stage_step_id`) this returns A1,B1,B2,B3,A2.
#[tokio::test]
async fn test_backfill_restored_step_keeps_authored_order() {
    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];

    let alpha = format!("stage-alpha-{short}");
    let a2 = format!("step-a2-{short}");
    let authored: Vec<String> = vec![
        format!("step-a1-{short}"),
        format!("step-a2-{short}"),
        format!("step-b1-{short}"),
        format!("step-b2-{short}"),
        format!("step-b3-{short}"),
    ];

    let bodies = ["v1", "v1", "v1", "v1", "v1"];
    let (path_id, guide_id_str) =
        initial_ingest(&base_url, create_reingest_tarball(&suffix, &bodies)).await;
    let guide_id = uuid::Uuid::parse_str(&guide_id_str).unwrap();

    let resolve = || async {
        let resolved = dapple_data::query::guide_playbooks::select_guide_playbooks_by_guide_ids(
            &test_db_pool(),
            &[guide_id],
        )
        .await
        .unwrap();
        assert_eq!(
            resolved.len(),
            1,
            "exactly one playbook resolves for the guide"
        );
        resolved[0].playbook_id
    };

    let p1 = resolve().await;
    assert_eq!(
        rollup_step_order(&db, p1).await,
        authored,
        "baseline: rollup returns steps in authored order"
    );

    let a2_ssid_before = stage_step_id_of(&db, p1, &alpha, &a2).await;

    // Corrupt: drop Alpha's LAST step (A2) from the resolved playbook.
    let dropped = sqlx::query(
        "DELETE FROM rev.stage_steps ss
         USING rev.steps st, rev.versioned_objects stvo, rev.step_path_details stppd
         WHERE ss.step_id = st.id
           AND stvo.id = st.versioned_object_id
           AND stppd.path_id = stvo.path_id
           AND stppd.slug = $3
           AND ss.stage_id = (
               SELECT ps.stage_id FROM rev.playbook_stages ps
               JOIN rev.stages s ON s.id = ps.stage_id
               JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
               JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
               WHERE ps.playbook_id = $1 AND spd.slug = $2
           )",
    )
    .bind(p1)
    .bind(&alpha)
    .bind(&a2)
    .execute(&db)
    .await
    .expect("corrupt delete failed");
    assert_eq!(dropped.rows_affected(), 1, "setup drops exactly A2");

    // Backfill with UNCHANGED content — Model B repair-in-place restores A2.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_reingest_tarball(&suffix, &bodies),
        true,
    )
    .await;
    assert!(status.is_success(), "backfill failed: {status} {body}");

    let p_after = resolve().await;
    let a2_ssid_after = stage_step_id_of(&db, p_after, &alpha, &a2).await;
    assert_ne!(
        a2_ssid_before, a2_ssid_after,
        "backfill must have re-minted A2's stage_step (the out-of-band id this test guards)"
    );
    assert!(
        a2_ssid_after > a2_ssid_before,
        "the restored A2 id is newer (sorts LAST globally) — the exact pre-fix hazard"
    );

    // The fix: A2 stays in Alpha, NOT jumped past Beta.
    assert_eq!(
        rollup_step_order(&db, p_after).await,
        authored,
        "after restore: hierarchical (playbook_stage, stage_step) order keeps A2 in Alpha"
    );
}

/// The case the id-only fix could NOT handle: a restored FIRST/MIDDLE step within a
/// multi-step stage. `?backfill=true` re-mints B2 (Beta's middle step) with the
/// NEWEST id, so within Beta an id sort places it AFTER B3 — rollup A1,A2,B1,B3,B2.
/// Even the hierarchical `(playbook_stage_id, stage_step_id)` fix returns that,
/// because within the stage it is still ordering by id magnitude. Only the STORED
/// `stage_steps.ord` (authored position 2, carried across the re-mint) keeps B2
/// between B1 and B3. We assert BOTH the rollup authored order AND, directly, that
/// the restored row carries ord = 2 (proving the column — not the id sort — does
/// the work).
#[tokio::test]
async fn test_backfill_restored_middle_step_keeps_authored_order() {
    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];

    let beta = format!("stage-beta-{short}");
    let b2 = format!("step-b2-{short}");
    let authored: Vec<String> = vec![
        format!("step-a1-{short}"),
        format!("step-a2-{short}"),
        format!("step-b1-{short}"),
        format!("step-b2-{short}"),
        format!("step-b3-{short}"),
    ];

    let bodies = ["v1", "v1", "v1", "v1", "v1"];
    let (path_id, guide_id_str) =
        initial_ingest(&base_url, create_reingest_tarball(&suffix, &bodies)).await;
    let guide_id = uuid::Uuid::parse_str(&guide_id_str).unwrap();

    let resolve = || async {
        let resolved = dapple_data::query::guide_playbooks::select_guide_playbooks_by_guide_ids(
            &test_db_pool(),
            &[guide_id],
        )
        .await
        .unwrap();
        assert_eq!(
            resolved.len(),
            1,
            "exactly one playbook resolves for the guide"
        );
        resolved[0].playbook_id
    };

    let p1 = resolve().await;
    assert_eq!(
        rollup_step_order(&db, p1).await,
        authored,
        "baseline: rollup returns steps in authored order"
    );

    let b2_ssid_before = stage_step_id_of(&db, p1, &beta, &b2).await;

    // Corrupt: drop Beta's MIDDLE step (B2) from the resolved playbook.
    let dropped = sqlx::query(
        "DELETE FROM rev.stage_steps ss
         USING rev.steps st, rev.versioned_objects stvo, rev.step_path_details stppd
         WHERE ss.step_id = st.id
           AND stvo.id = st.versioned_object_id
           AND stppd.path_id = stvo.path_id
           AND stppd.slug = $3
           AND ss.stage_id = (
               SELECT ps.stage_id FROM rev.playbook_stages ps
               JOIN rev.stages s ON s.id = ps.stage_id
               JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
               JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
               WHERE ps.playbook_id = $1 AND spd.slug = $2
           )",
    )
    .bind(p1)
    .bind(&beta)
    .bind(&b2)
    .execute(&db)
    .await
    .expect("corrupt delete failed");
    assert_eq!(dropped.rows_affected(), 1, "setup drops exactly B2");

    // Backfill with UNCHANGED content — Model B repair-in-place restores B2.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_reingest_tarball(&suffix, &bodies),
        true,
    )
    .await;
    assert!(status.is_success(), "backfill failed: {status} {body}");

    let p_after = resolve().await;
    let b2_ssid_after = stage_step_id_of(&db, p_after, &beta, &b2).await;
    assert!(
        b2_ssid_after > b2_ssid_before,
        "the restored B2 id is newest — under an id sort it would land AFTER B3 within Beta"
    );

    // Direct mechanism check: the restored row carries authored ord = 2 (Beta's
    // middle), NOT an appended position. This is what the id-only fix could not do.
    let b2_ord: i32 = sqlx::query_scalar("SELECT ord FROM rev.stage_steps WHERE id = $1")
        .bind(b2_ssid_after)
        .fetch_one(&db)
        .await
        .expect("ord lookup failed");
    assert_eq!(
        b2_ord, 2,
        "restored middle step carries its authored ord (2), not an appended position"
    );

    // The fix: stored ord keeps B2 between B1 and B3 (id-only ordering returns A1,A2,B1,B3,B2).
    assert_eq!(
        rollup_step_order(&db, p_after).await,
        authored,
        "after restore: stored stage_steps.ord keeps the middle step B2 in authored position"
    );
}

/// The half of the demo bug that backfill alone can't fix: a re-ingest that
/// supersedes a playbook version must (E2) deprecate the prior version and (E1)
/// make guide→playbook resolution return ONLY the latest non-deprecated version
/// per path — so a NEW journey can never resolve/pin a stale (broken) version,
/// even when the guide still links it. Asserted in SQL + via the real resolution
/// query (`select_guide_playbooks_by_guide_ids`), not the response body.
///
/// Pre-fix this FAILS twice: the superseded playbook is left active (no
/// deprecation event), and the resolution returns BOTH the stale and current
/// versions instead of collapsing to the latest.
#[tokio::test]
async fn test_reingest_deprecates_superseded_playbook_and_resolution_collapses() {
    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;
    let suffix = uuid::Uuid::now_v7().to_string();

    // v1 — initial ingest; p1 is the first playbook version.
    let v1 = ingest_tarball(
        &base_url,
        create_reingest_tarball(&suffix, &["v1", "v1", "v1", "v1", "v1"]),
    )
    .await;
    let path_id = v1["guide"]["path_id"]
        .as_str()
        .expect("guide.path_id")
        .to_string();
    let p1 = playbook_id_from_ingest(&v1);

    // v2 — changed content → a NEW playbook version (p2) on the SAME playbook path.
    let (s2, v2) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_reingest_tarball(&suffix, &["v2", "v2", "v2", "v2", "v2"]),
        false,
    )
    .await;
    assert!(s2.is_success(), "v2 ingest failed: {s2} {v2}");
    let guide_id_v2 = uuid::Uuid::parse_str(v2["guide"]["id"].as_str().expect("guide.id")).unwrap();

    // Resolve p2 (newest playbook version on p1's path) via SQL.
    let p2: uuid::Uuid = sqlx::query_scalar(
        "SELECT p.id FROM rev.playbooks p
         JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
         WHERE vo.path_id = (
             SELECT vo1.path_id FROM rev.playbooks p1
             JOIN rev.versioned_objects vo1 ON vo1.id = p1.versioned_object_id
             WHERE p1.id = $1
         )
         ORDER BY vo.created_at DESC
         LIMIT 1",
    )
    .bind(p1)
    .fetch_one(&db)
    .await
    .expect("p2 lookup failed");
    assert_ne!(p1, p2, "v2 must create a new playbook version");

    // E2: the superseded p1 is deprecated; the current p2 is not.
    let deprecated_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.deprecation_events de
         JOIN rev.playbooks p ON p.versioned_object_id = de.versioned_object_id
         WHERE p.id = $1",
    )
    .bind(p1)
    .fetch_one(&db)
    .await
    .expect("p1 deprecation check failed");
    assert!(
        deprecated_count >= 1,
        "E2: superseded playbook version p1 must be deprecated"
    );
    let p2_deprecated_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.deprecation_events de
         JOIN rev.playbooks p ON p.versioned_object_id = de.versioned_object_id
         WHERE p.id = $1",
    )
    .bind(p2)
    .fetch_one(&db)
    .await
    .expect("p2 deprecation check failed");
    assert_eq!(
        p2_deprecated_count, 0,
        "E2: current playbook version p2 must NOT be deprecated"
    );

    // Stack the stale p1 onto v2's guide version (mirrors the demo, where one
    // guide version linked BOTH a stale and the current playbook). E1 must still
    // resolve to exactly p2.
    sqlx::query(
        "INSERT INTO rev.guide_playbooks (guide_id, playbook_id, chapter_index)
         VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
    )
    .bind(guide_id_v2)
    .bind(p1)
    .execute(&db)
    .await
    .expect("stack stale link failed");

    let resolved = dapple_data::query::guide_playbooks::select_guide_playbooks_by_guide_ids(
        &test_db_pool(),
        &[guide_id_v2],
    )
    .await
    .expect("resolution query failed");
    let resolved_ids: Vec<uuid::Uuid> = resolved.iter().map(|r| r.playbook_id).collect();
    assert_eq!(
        resolved_ids,
        vec![p2],
        "E1: guide→playbook resolution must collapse to the latest non-deprecated \
         version (p2), not return the stacked stale p1"
    );
}

/// Fixture: three sibling briefs, each with its own Fluent title message, all in
/// one group/pack. `titles` supplies the en.ftl body for brief-{1,2,3} so a
/// re-ingest can change a subset (a MIXED fluent batch).
fn create_three_brief_fluent_tarball(suffix: &str, titles: &[&str; 3]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];
    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    append_file(
        &mut archive,
        "guide/guide.toml",
        &format!(
            "title = \"Tri Brief Guide {suffix}\"\n\
             description = \"Three briefs, three Fluent titles.\"\n\
             author = \"Test\"\n\
             classification = \"test-{suffix}\"\n"
        ),
    );
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Intro](intro.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let mut intro = String::from("# Intro\n\n");
    for n in 1..=3 {
        intro.push_str(&format!(
            ":::value-slot {{ name: \"slot-{n}-{short}\", kind: \"text\" }}\n:::\n\n\
             :::brief-input {{ name: \"input-{n}-{short}\", control: \"text\", slot: \"slot-{n}-{short}\" }}\n:::\n\n\
             :::brief {{ name: \"brief-{n}-{short}\", units: [\"input-{n}-{short}\"] }}\n:::\n\n"
        ));
    }
    intro.push_str(&format!(
        ":::brief-group {{ name: \"group-{short}\", briefs: [\"brief-1-{short}\", \"brief-2-{short}\", \"brief-3-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n"
    ));
    append_file(&mut archive, "guide/intro.md", &intro);

    let fluent = format!(
        "input-1-{short} = Input one label\n\
         input-2-{short} = Input two label\n\
         input-3-{short} = Input three label\n\
         brief-1-{short} = {}\n\
         brief-2-{short} = {}\n\
         brief-3-{short} = {}\n\
         group-{short} = Group title\n\
         pack-{short} = Pack title\n",
        titles[0], titles[1], titles[2]
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Fluent-title-fidelity invariant (the thing a customer sees): after a MIXED
/// re-ingest where a subset of sibling Fluent titles change, EVERY brief's
/// display title resolves to its OWN latest body — never a sibling's. Asserted
/// via the resolution read (`rev.fluent_body`), not a response flag.
///
/// SCOPE NOTE: this guards the common path; it does NOT by itself reproduce the
/// `materialize_fluent_messages` cross-wire (verified: it passes with and without
/// the name-keying change). The cross-wire needs a REFETCHED existing version to
/// re-enter the upsert batch, which only happens from orphaned-version state —
/// the normal Rust REUSE gate keeps all-new/changed batches in input order. The
/// genuine latent fragility is `upsert_fluent_messages.sql`'s triple-`row_number`
/// join (see the note in `materialize_fluent_messages`). This test still earns
/// its keep as a regression guard on the materializer's name→id mapping.
#[tokio::test]
async fn test_mixed_fluent_batch_brief_title_fidelity() {
    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];

    let brief = |n: u8| format!("brief-{n}-{short}");

    // v1 — three distinct titles.
    let (path_id, _guide_id) = initial_ingest(
        &base_url,
        create_three_brief_fluent_tarball(&suffix, &["Alpha one", "Bravo one", "Charlie one"]),
    )
    .await;
    for (n, want) in [(1u8, "Alpha one"), (2, "Bravo one"), (3, "Charlie one")] {
        let id = latest_brief_id_by_name(&db, &brief(n)).await;
        let (_, title) = resolve_brief_title(&db, id).await;
        assert_eq!(title, want, "v1 brief-{n} title");
    }

    // v2 — MIXED batch: titles 1 and 2 change, title 3 is unchanged. This is the
    // batch shape that mis-pairs name→id if results are assigned positionally.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_three_brief_fluent_tarball(&suffix, &["Alpha TWO", "Bravo TWO", "Charlie one"]),
        false,
    )
    .await;
    assert!(status.is_success(), "v2 re-ingest failed: {status} {body}");

    for (n, want) in [(1u8, "Alpha TWO"), (2, "Bravo TWO"), (3, "Charlie one")] {
        let id = latest_brief_id_by_name(&db, &brief(n)).await;
        let (msg_id, title) = resolve_brief_title(&db, id).await;
        assert!(
            msg_id.is_some(),
            "brief-{n} must pin a Fluent title message"
        );
        assert_eq!(
            title, want,
            "brief-{n} must resolve to its OWN latest title body (no cross-wire)"
        );
    }
}

/// The real P0: `upsert_fluent_messages.sql` used to correlate name↔version↔message
/// by three independent `row_number()` ranks, which mis-paired name→id whenever a
/// REFETCHED existing version re-entered the batch (a refetch carries an OLD id, so
/// ordering versions by id diverges from the input order of names). This test
/// constructs exactly that refetch: it pre-seeds an ORPHANED `versioned_object`
/// (a version row with NO `fluent_message`) on one brief-title's Fluent path whose
/// `content_hash` matches the body that brief is re-ingested with. The Rust REUSE
/// gate only sees versions that HAVE a message, so it misses the orphan and sends
/// that message to the upsert, where `ON CONFLICT (path_id, content_hash)` refetches
/// the old-id orphan version while the sibling titles get fresh (new-id) versions —
/// the precise rank-divergence the old SQL stapled the wrong name onto. With the
/// key-based join every title resolves to its OWN body.
#[tokio::test]
async fn test_fluent_refetch_orphan_version_no_cross_wire() {
    use std::collections::BTreeMap;

    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let brief = |n: u8| format!("brief-{n}-{short}");

    // v1 — three distinct titles.
    let (path_id, _guide_id) = initial_ingest(
        &base_url,
        create_three_brief_fluent_tarball(&suffix, &["Alpha one", "Bravo one", "Charlie one"]),
    )
    .await;

    // Pre-seed an orphaned versioned_object on brief-2's Fluent-title path whose
    // content_hash matches the body we'll re-ingest for brief-2 ("Bravo ORPHAN").
    // No fluent_message row → the Rust REUSE gate cannot see it.
    let orphan_body = "Bravo ORPHAN";
    let orphan_hash = {
        let mut m = BTreeMap::new();
        m.insert("en".to_string(), orphan_body.to_string());
        dapple_data::hash::hash_fluent_message(&m, &BTreeMap::new())
            .as_str()
            .to_string()
    };
    let title2_msg_path: uuid::Uuid =
        sqlx::query_scalar("SELECT path_id FROM rev.fluent_message_path_details WHERE name = $1")
            .bind(brief(2))
            .fetch_one(&db)
            .await
            .expect("brief-2 fluent path must exist");
    let orphan_vo_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) VALUES ($1, $2) RETURNING id",
    )
    .bind(title2_msg_path)
    .bind(&orphan_hash)
    .fetch_one(&db)
    .await
    .expect("seed orphaned versioned_object");

    // Re-ingest: all three titles change; brief-2's new body == the orphan hash, so
    // its version is REFETCHED (old id) while brief-1/brief-3 get fresh versions.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_three_brief_fluent_tarball(&suffix, &["Alpha TWO", orphan_body, "Charlie TWO"]),
        false,
    )
    .await;
    assert!(
        status.is_success(),
        "refetch re-ingest failed: {status} {body}"
    );

    for (n, want) in [(1u8, "Alpha TWO"), (2, orphan_body), (3, "Charlie TWO")] {
        let id = latest_brief_id_by_name(&db, &brief(n)).await;
        let (_, title) = resolve_brief_title(&db, id).await;
        assert_eq!(
            title, want,
            "brief-{n} must resolve to its OWN body even when a sibling refetches an \
             orphaned old-id version (key-join, not rank-join)"
        );
    }

    // Self-validation: confirm the refetch branch actually fired — brief-2's
    // pinned title message must be the one created for the ORPHAN version we
    // seeded (not a fresh version), proving this test exercises the exact
    // old-id-re-enters-the-batch path the rank-join mis-paired.
    let brief2_id = latest_brief_id_by_name(&db, &brief(2)).await;
    let (brief2_msg_id, _) = resolve_brief_title(&db, brief2_id).await;
    let pinned_vo: uuid::Uuid =
        sqlx::query_scalar("SELECT versioned_object_id FROM rev.fluent_messages WHERE id = $1")
            .bind(brief2_msg_id.expect("brief-2 pins a title message"))
            .fetch_one(&db)
            .await
            .expect("pinned fluent message must exist");
    assert_eq!(
        pinned_vo, orphan_vo_id,
        "brief-2's title must pin the message on the REFETCHED orphan version \
         (confirms the refetch branch was exercised)"
    );
}

/// Model B acceptance: a `?backfill=true` on a structurally-HEALTHY guide mints
/// ZERO new playbook/stage/step/task versions — it repairs in place, so the
/// version DAG does not grow on repeated backfills (the unbounded-growth problem
/// the old nonce-re-version behavior caused). Counts entity versions by the
/// fixture's suffix-scoped slugs before and after a no-op backfill.
#[tokio::test]
async fn test_backfill_no_version_growth_on_healthy_playbook() {
    let (base_url, _handle) = spawn_app().await;
    let db = get_db_pool().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];
    let like = format!("%{short}%");

    let bodies = ["v1", "v1", "v1", "v1", "v1"];
    let (path_id, _guide_id) =
        initial_ingest(&base_url, create_reingest_tarball(&suffix, &bodies)).await;

    // Total playbook + stage + step + task versions whose path-slug carries this
    // fixture's suffix (sqlx 0.9 requires a 'static SQL string, so the four
    // tables are UNION'd in one literal rather than built dynamically).
    async fn version_count(db: &sqlx::PgPool, like: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT
               (SELECT count(*) FROM rev.playbooks e
                  JOIN rev.versioned_objects vo ON vo.id = e.versioned_object_id
                  JOIN rev.playbook_path_details pd ON pd.path_id = vo.path_id
                  WHERE pd.slug LIKE $1)
             + (SELECT count(*) FROM rev.stages e
                  JOIN rev.versioned_objects vo ON vo.id = e.versioned_object_id
                  JOIN rev.stage_path_details pd ON pd.path_id = vo.path_id
                  WHERE pd.slug LIKE $1)
             + (SELECT count(*) FROM rev.steps e
                  JOIN rev.versioned_objects vo ON vo.id = e.versioned_object_id
                  JOIN rev.step_path_details pd ON pd.path_id = vo.path_id
                  WHERE pd.slug LIKE $1)
             + (SELECT count(*) FROM rev.tasks e
                  JOIN rev.versioned_objects vo ON vo.id = e.versioned_object_id
                  JOIN rev.task_path_details pd ON pd.path_id = vo.path_id
                  WHERE pd.slug LIKE $1)",
        )
        .bind(like)
        .fetch_one(db)
        .await
        .expect("version count query")
    }

    let before = version_count(&db, &like).await;

    // Backfill UNCHANGED content.
    let (status, body) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_reingest_tarball(&suffix, &bodies),
        true,
    )
    .await;
    assert!(
        status.is_success(),
        "healthy backfill failed: {status} {body}"
    );

    let after = version_count(&db, &like).await;

    assert_eq!(
        before, after,
        "Model B: a healthy backfill must mint ZERO new playbook/stage/step/task \
         versions (before={before:?} after={after:?})"
    );
}

// ===========================================================================
// Phase 0 reproduction: ingest the REAL shaggy-dog guide on a CLEAN, isolated
// DB and check (a) structural correctness and (b) idempotency. This isolates
// the live bug from the polluted dev DB. Skips cleanly if the guide checkout
// is absent (CI), so it never breaks the gate.
// ===========================================================================
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn repro_shaggy_dog_clean_db_idempotency(pool: sqlx::PgPool) {
    use sqlx::Row;

    let guide_dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
        .join("src/dapple/dapple-guides/public/dapple/shaggy-dog-joke-builder");
    if !guide_dir.exists() {
        eprintln!("SKIP repro_shaggy_dog: guide not found at {guide_dir:?}");
        return;
    }

    // Expected per-stage step counts come from the PARSED guide, not a fixture
    // snapshot: this test ingests the live dapple-guides checkout, so hardcoded
    // counts rot every time the guide is re-authored. What's under test is that
    // materialization persists exactly what the plan derived.
    let parsed = dapple_parser::parse_guide(&guide_dir).expect("parse shaggy-dog guide");
    let plan =
        dapple_parser::transform::plan_guide(&parsed, &guide_dir).expect("plan shaggy-dog guide");
    let expected_steps: std::collections::BTreeMap<String, i64> = plan
        .derived_playbooks
        .iter()
        .flat_map(|pb| pb.stages.iter())
        .map(|st| (st.name.clone(), st.steps.len() as i64))
        .collect();
    assert!(
        !expected_steps.is_empty(),
        "shaggy-dog plan derived no stages"
    );

    let state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(dapple_data::DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });

    // Use the SAME tarball for the initial POST and the re-sync PATCH so the
    // directory hash is computed from an identical input both times (the real
    // suite flow ships a tarball both times; a dir-walk vs tarball mismatch would
    // be a test artifact, not the bug under test).
    let tarball = build_dir_tarball(&guide_dir);

    // ---- Ingest #1 (POST from tarball) ----
    let r1 = dapple_prompt_api::handler::guide_ingest::ingest_guide_tarball(
        &state, &tarball, None, None,
    )
    .await
    .expect("ingest #1");
    let guide_id_1 = r1.guide.id;

    // How many DISTINCT playbooks does this guide version expose per chapter?
    let pb_rows = sqlx::query(
        "SELECT chapter_index, count(DISTINCT playbook_id) AS n
         FROM rev.guide_playbooks WHERE guide_id = $1 GROUP BY chapter_index",
    )
    .bind(guide_id_1)
    .fetch_all(&pool)
    .await
    .unwrap();
    for row in &pb_rows {
        let ci: i16 = row.get("chapter_index");
        let n: i64 = row.get("n");
        assert_eq!(
            n, 1,
            "chapter {ci} exposes {n} playbooks (expected 1, no duplicates)"
        );
    }

    // Per-stage step counts for the exposed playbook(s), keyed by stage NAME so
    // they compare directly against the plan-derived StageDef names (no Rust
    // reimplementation of rev.slugify needed).
    let stage_rows = sqlx::query(
        "SELECT spd.name AS name, count(ss.step_id) AS steps
         FROM rev.guide_playbooks gp
         JOIN rev.playbook_stages ps ON ps.playbook_id = gp.playbook_id
         JOIN rev.stages s ON s.id = ps.stage_id
         JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
         LEFT JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
         WHERE gp.guide_id = $1
         GROUP BY spd.name ORDER BY spd.name",
    )
    .bind(guide_id_1)
    .fetch_all(&pool)
    .await
    .unwrap();
    let counts: std::collections::BTreeMap<String, i64> = stage_rows
        .iter()
        .map(|r| (r.get::<String, _>("name"), r.get::<i64, _>("steps")))
        .collect();
    eprintln!("INGEST#1 stage step counts: {counts:?}");
    assert_eq!(
        counts, expected_steps,
        "materialized per-stage step counts must equal the plan-derived counts"
    );

    // Every stage's stored content_hash must match a fresh hash_stage recompute
    // over its PERSISTED steps (no lying hashes).
    let liar_rows = sqlx::query(
        "SELECT spd.slug, svo.content_hash, count(ss.step_id) AS steps
         FROM rev.guide_playbooks gp
         JOIN rev.playbook_stages ps ON ps.playbook_id = gp.playbook_id
         JOIN rev.stages s ON s.id = ps.stage_id
         JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
         LEFT JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
         WHERE gp.guide_id = $1
         GROUP BY spd.slug, svo.content_hash",
    )
    .bind(guide_id_1)
    .fetch_all(&pool)
    .await
    .unwrap();
    eprintln!(
        "INGEST#1 stage hashes: {:?}",
        liar_rows
            .iter()
            .map(|r| (
                r.get::<String, _>("slug"),
                r.get::<String, _>("content_hash"),
                r.get::<i64, _>("steps")
            ))
            .collect::<Vec<_>>()
    );

    let guide_hash_1: String = sqlx::query_scalar(
        "SELECT content_hash FROM rev.versioned_objects vo
                            JOIN rev.guides g ON g.versioned_object_id = vo.id WHERE g.id = $1",
    )
    .bind(guide_id_1)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Re-sync goes through the PATCH path (POST is for NEW guides; its title is
    // namespace-unique). Resolve the guide path_id and build a tarball.
    let path_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT vo.path_id FROM rev.versioned_objects vo
         JOIN rev.guides g ON g.versioned_object_id = vo.id WHERE g.id = $1",
    )
    .bind(guide_id_1)
    .fetch_one(&pool)
    .await
    .unwrap();

    let count_vo = |pool: sqlx::PgPool| async move {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM rev.versioned_objects")
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let stage_step_count = |pool: sqlx::PgPool, gid: uuid::Uuid, stage_name: String| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(ss.step_id) FROM rev.guide_playbooks gp
             JOIN rev.playbook_stages ps ON ps.playbook_id = gp.playbook_id
             JOIN rev.stages s ON s.id = ps.stage_id
             JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
             JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
             LEFT JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
             WHERE gp.guide_id = $1 AND spd.name = $2",
        )
        .bind(gid)
        .bind(stage_name)
        .fetch_one(&pool)
        .await
        .unwrap()
    };

    // Per-entity-type version counts (to see WHICH entities re-version).
    let vo_by_type = |pool: sqlx::PgPool| async move {
        sqlx::query(
            "SELECT et.key AS key, count(*) AS n
             FROM rev.versioned_objects vo
             JOIN rev.object_paths op ON op.id = vo.path_id
             JOIN rev.entity_types et ON et.id = op.entity_type_id
             GROUP BY et.key ORDER BY et.key",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.get::<String, _>("key"), r.get::<i64, _>("n")))
        .collect::<std::collections::BTreeMap<String, i64>>()
    };

    // ---- Ingest #2: PATCH re-sync (Normal) — must be idempotent ----
    let by_type_before = vo_by_type(pool.clone()).await;
    let vo_before = count_vo(pool.clone()).await;
    dapple_prompt_api::handler::guide_ingest::ingest_guide_version_tarball(
        &state,
        path_id,
        &tarball,
        None,
        None,
        dapple_prompt_api::handler::guide_ingest::IngestMode::Normal,
    )
    .await
    .expect("PATCH re-sync (Normal)");
    let vo_after = count_vo(pool.clone()).await;
    let by_type_after = vo_by_type(pool.clone()).await;
    eprintln!(
        "PATCH Normal: vo {vo_before} -> {vo_after} (+{})",
        vo_after - vo_before
    );
    for (k, after) in &by_type_after {
        let before = by_type_before.get(k).copied().unwrap_or(0);
        if *after != before {
            eprintln!(
                "  re-versioned entity_type '{k}': {before} -> {after} (+{})",
                after - before
            );
        }
    }
    assert_eq!(
        vo_before,
        vo_after,
        "idempotency: a Normal PATCH re-sync of identical content must mint ZERO new \
         versioned_objects (minted {})",
        vo_after - vo_before
    );

    // ---- Ingest #3: backfill (ForceRepair) on healthy content — also idempotent ----
    let vo_before = count_vo(pool.clone()).await;
    dapple_prompt_api::handler::guide_ingest::ingest_guide_version_tarball(
        &state,
        path_id,
        &tarball,
        None,
        None,
        dapple_prompt_api::handler::guide_ingest::IngestMode::ForceRepair,
    )
    .await
    .expect("PATCH backfill (ForceRepair)");
    let vo_after = count_vo(pool.clone()).await;
    eprintln!(
        "PATCH ForceRepair: vo {vo_before} -> {vo_after} (+{})",
        vo_after - vo_before
    );
    assert_eq!(
        vo_before,
        vo_after,
        "a ForceRepair backfill of healthy content must mint ZERO new versions (minted {})",
        vo_after - vo_before
    );

    // ---- Corrupt-then-verify: drop one stage_step, backfill, assert repaired ----
    // Victim stage is chosen from the PLAN (first stage with >= 2 steps, so the
    // corruption leaves a partial stage rather than an empty one; any stage as a
    // fallback), keeping this block valid across guide re-authoring.
    let (victim_stage, victim_expected) = expected_steps
        .iter()
        .find(|(_, n)| **n >= 2)
        .or_else(|| expected_steps.iter().find(|(_, n)| **n >= 1))
        .map(|(name, n)| (name.clone(), *n))
        .expect("shaggy-dog plan has no stage with steps");
    eprintln!("corrupt-then-verify victim stage: {victim_stage:?} ({victim_expected} steps)");
    assert_eq!(
        stage_step_count(pool.clone(), guide_id_1, victim_stage.clone()).await,
        victim_expected,
        "precondition: {victim_stage:?} has {victim_expected} steps"
    );
    let victim: uuid::Uuid = sqlx::query_scalar(
        "SELECT ss.id FROM rev.stage_steps ss
         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id
         JOIN rev.guide_playbooks gp ON gp.playbook_id = ps.playbook_id
         JOIN rev.stages s ON s.id = ps.stage_id
         JOIN rev.versioned_objects svo ON svo.id = s.versioned_object_id
         JOIN rev.stage_path_details spd ON spd.path_id = svo.path_id
         WHERE gp.guide_id = $1 AND spd.name = $2
         LIMIT 1",
    )
    .bind(guide_id_1)
    .bind(&victim_stage)
    .fetch_one(&pool)
    .await
    .unwrap();
    // stage_step_overlays FK the stage_step; clear them first, then drop the link.
    sqlx::query("DELETE FROM rev.stage_step_overlays WHERE stage_step_id = $1")
        .bind(victim)
        .execute(&pool)
        .await
        .unwrap();
    let deleted = sqlx::query("DELETE FROM rev.stage_steps WHERE id = $1")
        .bind(victim)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        deleted.rows_affected(),
        1,
        "should have deleted one {victim_stage:?} stage_step"
    );
    assert_eq!(
        stage_step_count(pool.clone(), guide_id_1, victim_stage.clone()).await,
        victim_expected - 1,
        "corrupted {victim_stage:?} to {} steps",
        victim_expected - 1
    );

    dapple_prompt_api::handler::guide_ingest::ingest_guide_version_tarball(
        &state,
        path_id,
        &tarball,
        None,
        None,
        dapple_prompt_api::handler::guide_ingest::IngestMode::ForceRepair,
    )
    .await
    .expect("PATCH repair");
    assert_eq!(
        stage_step_count(pool.clone(), guide_id_1, victim_stage.clone()).await,
        victim_expected,
        "a ForceRepair backfill must restore {victim_stage:?} to {victim_expected} steps"
    );

    let _ = guide_hash_1;
}

/// Build a .tar.gz from a guide directory (root = the dir's own name), mirroring
/// the tarball the HTTP ingest receives.
fn build_dir_tarball(guide_dir: &std::path::Path) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    let mut buf = Vec::new();
    {
        let enc = GzEncoder::new(&mut buf, Compression::default());
        let mut ar = tar::Builder::new(enc);
        ar.append_dir_all(guide_dir.file_name().unwrap(), guide_dir)
            .expect("append guide dir");
        ar.finish().expect("finish tar");
    }
    buf
}

/// Builder for the patch-reingest relink regression: a single stage -> step ->
/// inline nushell task whose `script` body is parameterized (so the task, and
/// therefore its step, re-versions when it changes) and whose guide `nonce` varies
/// the description (so an otherwise-identical re-ingest still forces materialization
/// instead of short-circuiting on the guide content hash).
fn create_oscillation_tarball(suffix: &str, nonce: &str, script: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Oscillation Guide {suffix}\"\n\
         description = \"Patch re-ingest oscillation {nonce}\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Workflow](ch1.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"candidates-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-output {{ name: \"cand-out-{short}\", slot: \"candidates-{short}\", source: \"computed\", source_path: \"/choices\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"cand-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"candidates-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{script}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let fluent = format!(
        "cand-out-{short} = Candidates output\n\
         brief-{short} = The brief\n\
         group-{short} = The group\n\
         pack-{short} = The pack\n\
         stage-{short} = The stage\n\
         step-{short} = The step\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Regression for `step_tasks_step_id_ord_unique` on patch re-ingest.
///
/// Migration 53 added `UNIQUE (step_id, ord)` to `rev.step_tasks`, but the link
/// insert (`insert_step_tasks.sql`) guards only `ON CONFLICT (step_id, task_id)`.
/// The patch path resolves a changed step via `ON CONFLICT (path_id, content_hash)`,
/// which can return a PRE-EXISTING step version that already holds its `step_tasks`;
/// re-linking a DIFFERENT task at the occupied `ord` then violates `(step_id, ord)`
/// (observed in production as an assemble step version carrying two `step_tasks`).
///
/// This proves the failure mechanism deterministically plus the guard the fix uses:
///   1. `insert_step_task_models` collides when a divergent task is linked at an
///      already-occupied ord (the exact 500 the failing sync produced), and
///   2. `select_step_ids_with_tasks` reports an already-linked step, so the patch
///      handler now skips re-linking it (see `guide_ingest.rs`).
#[tokio::test]
async fn test_relink_divergent_task_collides_and_guard_detects_linked_step() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();
    let sql = get_db_pool().await;
    let step_slug = dapple_data::slugify(&format!("step-{short}"));

    // Ingest a guide: one step version V1 linked to its task at ord 1.
    let _ = initial_ingest(
        &base_url,
        create_oscillation_tarball(&suffix, "a", "{ payload: { choices: [1] }, signals: {} }"),
    )
    .await;

    let (v1_step_id, v1_task_id): (uuid::Uuid, uuid::Uuid) = sqlx::query_as(
        "SELECT st.step_id, st.task_id FROM rev.step_tasks st \
         JOIN rev.steps s ON s.id = st.step_id \
         JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id \
         JOIN rev.step_path_details spd ON spd.path_id = vo.path_id \
         WHERE spd.slug = $1 ORDER BY vo.created_at ASC LIMIT 1",
    )
    .bind(&step_slug)
    .fetch_one(&sql)
    .await
    .expect("V1 step and its task");

    // A different existing task: linking it onto V1 lands a divergent task at V1's
    // already-occupied ord 1 -- the production failure mode.
    let other_task_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM rev.tasks WHERE id <> $1 LIMIT 1")
            .bind(v1_task_id)
            .fetch_one(&sql)
            .await
            .expect("some other existing task id");

    let mut tx = sql.begin().await.expect("begin tx");

    // (2) The guard the fix relies on: V1 is reported as already-linked, so the patch
    // handler skips re-linking it.
    let linked = dapple_data::query::steps::select_step_ids_with_tasks(&mut tx, &[v1_step_id])
        .await
        .expect("select_step_ids_with_tasks");
    assert_eq!(
        linked,
        vec![v1_step_id],
        "the guard must report an already-linked step version"
    );

    // (1) The raw relink the patch path performed pre-fix: linking a divergent task
    // onto V1 (already linked at ord 1) must violate (step_id, ord) -- the 500 the
    // failing sync produced. insert_step_tasks only guards (step_id, task_id).
    let res = dapple_data::query::steps::insert_step_task_models(
        &mut tx,
        vec![v1_step_id],
        vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: vec![other_task_id],
        }],
    )
    .await;
    let err = res.expect_err("re-linking a divergent task at an occupied ord must collide");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("step_tasks_step_id_ord_unique") || msg.contains("duplicate key"),
        "expected the (step_id, ord) unique violation, got: {msg}"
    );
    // tx rolls back on drop -- no committed corruption.
}

// ============================================================================
// repeater control type (Migration 060) — ingest materialization
// ============================================================================
//
// These exercise the REAL ingest pipeline for the `repeater` brief-input
// control on a `cardinality:"multi"` json accumulator slot (the packed-panorama
// "Build the Scenes" pane). They assert against LIVE DB state after ingest — the
// brief_unit_input's control_type, its resolved item_fields control_spec, and the
// value_slot's cardinality/min — not the response shape. Require migration 060
// applied to the dev DB.

/// Tarball with a `repeater` brief-input wired to a multi-json accumulator slot,
/// a json candidate-source slot, and an `item_fields` per-item schema. `control`
/// is parameterized so the same fixture drives both the positive (`"repeater"`)
/// and negative (an unknown control) cases.
fn create_repeater_test_tarball(suffix: &str, control: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Repeater Guide {suffix}\"\n\
         description = \"Fixture exercising the repeater control on a multi-json slot.\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Intro](intro.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // pp-scenes: the multi-json accumulator the repeater binds. pp-cands: the
    // json candidate-source slot the control reads its tiles from. item_fields:
    // the per-item schema whose copy ingest resolves from the unit's Fluent attrs.
    let intro = format!(
        "# Intro\n\n\
         :::value-slot {{ name: \"pp-scenes-{short}\", kind: \"json\", cardinality: \"multi\", min: 1 }}\n\
         :::\n\n\
         :::value-slot {{ name: \"pp-cands-{short}\", kind: \"json\" }}\n\
         :::\n\n\
         :::value-slot {{ name: \"pp-refine-{short}\", kind: \"json\" }}\n\
         :::\n\n\
         :::brief-input {{ name: \"scenes-list-{short}\", control: \"{control}\", slot: \"pp-scenes-{short}\", candidate_source: \"pp-cands-{short}\", item_fields: [{{ key: \"beat\", control: \"textarea\" }}, {{ key: \"mix\", control: \"text\" }}], refine_input: \"pp-refine-{short}\" }}\n\
         :::\n\n\
         :::brief {{ name: \"scenes-brief-{short}\", units: [\"scenes-list-{short}\"] }}\n\
         :::\n\n\
         :::brief-group {{ name: \"scenes-group-{short}\", briefs: [\"scenes-brief-{short}\"] }}\n\
         :::\n\n\
         :::brief-pack {{ name: \"scenes-pack-{short}\", groups: [\"scenes-group-{short}\"] }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/intro.md", &intro);

    // The repeater unit carries per-item-field copy as Fluent attributes
    // (`.beat-label` / `.mix-label`). Note the attribute lines use literal
    // newline+indentation (NOT `\`-continuation, which would strip the leading
    // whitespace Fluent needs to recognize a message attribute).
    let fluent = format!(
        "scenes-list-{short} = Build your scenes\n    .beat-label = What is happening here\n    .mix-label = Cast mix\n\
         scenes-brief-{short} = Scenes brief title\n\
         scenes-group-{short} = Scenes group title\n\
         scenes-pack-{short} = Scenes pack title\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// The control-type KEY resolved for a brief's first input unit (joins
/// `brief_unit_input.control_type_id` → `rev.brief_input_control_types`).
async fn brief_input_control_type_key(db: &sqlx::PgPool, brief_id: uuid::Uuid) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT ct.key
         FROM rev.brief_unit_input bui
         JOIN rev.brief_units bu ON bu.id = bui.brief_unit_id
         JOIN rev.brief_input_control_types ct ON ct.id = bui.control_type_id
         WHERE bu.brief_id = $1
         LIMIT 1",
    )
    .bind(brief_id)
    .fetch_optional(db)
    .await
    .expect("brief_unit_input control_type query failed")
}

/// The latest `(cardinality, min_selections)` of a value slot by path-details name.
async fn value_slot_cardinality(db: &sqlx::PgPool, name: &str) -> Option<(String, Option<i32>)> {
    sqlx::query_as::<_, (String, Option<i32>)>(
        "SELECT vs.cardinality, vs.min_selections
         FROM rev.value_slot_path_details vspd
         JOIN rev.object_paths op ON op.id = vspd.path_id
         JOIN rev.versioned_objects vo ON vo.path_id = op.id
         JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id
         WHERE vspd.name = $1
         ORDER BY vo.created_at DESC
         LIMIT 1",
    )
    .bind(name)
    .fetch_optional(db)
    .await
    .expect("value_slot cardinality query failed")
}

/// Ingesting a guide that uses the `repeater` control MATERIALIZES it: the
/// brief-input's control_type resolves to `repeater`, its `item_fields` schema is
/// resolved into control_spec (with per-field copy baked from the unit's Fluent
/// attributes), and the accumulator value slot is `cardinality='multi'` with
/// `min_selections=1`. Asserts live DB state, not the ingest response. (Requires
/// migration 060.)
#[tokio::test]
async fn test_repeater_control_materializes() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let tarball = create_repeater_test_tarball(&suffix, "repeater");
    ingest_tarball(&base_url, tarball).await; // asserts 201 + status:"success"

    let db = get_db_pool().await;

    let brief_id = latest_brief_id_by_name(&db, &format!("scenes-brief-{short}")).await;
    let control = brief_input_control_type_key(&db, brief_id).await;
    assert_eq!(
        control.as_deref(),
        Some("repeater"),
        "the brief-input must materialize with the repeater control type"
    );

    // item_fields resolved into control_spec, with per-field copy from Fluent.
    let spec = brief_input_control_spec(&db, brief_id)
        .await
        .expect("brief-input must have a control_spec");
    let item_fields = spec
        .get("item_fields")
        .and_then(|v| v.as_array())
        .expect("control_spec must carry item_fields for a repeater");
    assert_eq!(
        item_fields.len(),
        2,
        "both declared item fields must materialize"
    );
    assert_eq!(item_fields[0]["key"], "beat");
    assert_eq!(item_fields[0]["control"], "textarea");
    assert_eq!(
        item_fields[0]["label"], "What is happening here",
        "per-field label must resolve from the unit's `.beat-label` Fluent attribute"
    );
    assert_eq!(item_fields[1]["key"], "mix");
    assert_eq!(item_fields[1]["control"], "text");

    // refine_input resolves to the locator slot's value_slot_id in control_spec
    // (the same name→id resolution as candidate_source).
    assert!(
        spec.get("refine_input_value_slot_id")
            .and_then(|v| v.as_str())
            .is_some(),
        "control_spec must carry a resolved refine_input_value_slot_id"
    );

    let (cardinality, min) = value_slot_cardinality(&db, &format!("pp-scenes-{short}"))
        .await
        .expect("pp-scenes value slot must materialize");
    assert_eq!(
        cardinality, "multi",
        "accumulator slot must be cardinality=multi"
    );
    assert_eq!(min, Some(1), "accumulator slot must carry min_selections=1");
}

/// Ingesting a guide that uses the `timeline` control MATERIALIZES it the same way
/// the `repeater` does — it is a repeater with an axis, threading the identical
/// `item_fields` + `refine_input` control_spec. The reserved keys (id/parent_id/
/// kind/anchor) and the flat-arena nesting live entirely in the renderer, so the
/// ingest surface is unchanged: only the control-type string differs, ingestable
/// only because migration 061 seeded `timeline`. Reuses the parameterized repeater
/// fixture with `control = "timeline"`. Asserts live DB state.
#[tokio::test]
async fn test_timeline_control_materializes() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let tarball = create_repeater_test_tarball(&suffix, "timeline");
    ingest_tarball(&base_url, tarball).await; // asserts 201 + status:"success"

    let db = get_db_pool().await;

    let brief_id = latest_brief_id_by_name(&db, &format!("scenes-brief-{short}")).await;
    let control = brief_input_control_type_key(&db, brief_id).await;
    assert_eq!(
        control.as_deref(),
        Some("timeline"),
        "the brief-input must materialize with the timeline control type"
    );

    // Same generic item_fields + refine_input threading as the repeater.
    let spec = brief_input_control_spec(&db, brief_id)
        .await
        .expect("brief-input must have a control_spec");
    let item_fields = spec
        .get("item_fields")
        .and_then(|v| v.as_array())
        .expect("control_spec must carry item_fields for a timeline");
    assert_eq!(
        item_fields.len(),
        2,
        "both declared item fields must materialize"
    );
    assert_eq!(item_fields[0]["key"], "beat");
    assert_eq!(item_fields[0]["control"], "textarea");
    assert!(
        spec.get("refine_input_value_slot_id")
            .and_then(|v| v.as_str())
            .is_some(),
        "control_spec must carry a resolved refine_input_value_slot_id"
    );

    let (cardinality, min) = value_slot_cardinality(&db, &format!("pp-scenes-{short}"))
        .await
        .expect("pp-scenes value slot must materialize");
    assert_eq!(
        cardinality, "multi",
        "accumulator slot must be cardinality=multi"
    );
    assert_eq!(min, Some(1), "accumulator slot must carry min_selections=1");
}

/// Ingesting a guide that uses the `table` control MATERIALIZES it the same way
/// the `repeater` does — a table is a repeater rendered as an aligned grid
/// (rows = records, columns = item_fields), so the ingest surface is unchanged:
/// only the control-type string differs, ingestable only because migration 083
/// seeded `table`. Also asserts the parser recognizes `table` as a subset
/// selector (no spurious multi-cardinality warning). Reuses the parameterized
/// repeater fixture with `control = "table"`. Asserts live DB state.
#[tokio::test]
async fn test_table_control_materializes() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let tarball = create_repeater_test_tarball(&suffix, "table");
    let response = ingest_tarball(&base_url, tarball).await; // asserts 201 + status:"success"

    // `table` is in the parser's SUBSET_SELECTOR_CONTROLS, so binding a multi
    // slot must NOT emit the "only a subset selector collects more than one
    // item" cardinality warning.
    if let Some(warnings) = response["warnings"].as_array() {
        assert!(
            !warnings.iter().any(|w| w
                .as_str()
                .is_some_and(|w| w.contains("collects more than one item"))),
            "table must be recognized as a subset selector; got warnings: {warnings:?}"
        );
    }

    let db = get_db_pool().await;

    let brief_id = latest_brief_id_by_name(&db, &format!("scenes-brief-{short}")).await;
    let control = brief_input_control_type_key(&db, brief_id).await;
    assert_eq!(
        control.as_deref(),
        Some("table"),
        "the brief-input must materialize with the table control type"
    );

    // Same generic item_fields + refine_input threading as the repeater.
    let spec = brief_input_control_spec(&db, brief_id)
        .await
        .expect("brief-input must have a control_spec");
    let item_fields = spec
        .get("item_fields")
        .and_then(|v| v.as_array())
        .expect("control_spec must carry item_fields for a table");
    assert_eq!(
        item_fields.len(),
        2,
        "both declared item fields must materialize"
    );
    assert_eq!(item_fields[0]["key"], "beat");
    assert_eq!(item_fields[0]["control"], "textarea");
    assert!(
        spec.get("refine_input_value_slot_id")
            .and_then(|v| v.as_str())
            .is_some(),
        "control_spec must carry a resolved refine_input_value_slot_id"
    );

    let (cardinality, min) = value_slot_cardinality(&db, &format!("pp-scenes-{short}"))
        .await
        .expect("pp-scenes value slot must materialize");
    assert_eq!(
        cardinality, "multi",
        "accumulator slot must be cardinality=multi"
    );
    assert_eq!(min, Some(1), "accumulator slot must carry min_selections=1");
}

/// The latest value_slot id for a path-details name — to compare a resolved
/// `ref_value_slot_id` against the slot it should point at.
async fn latest_value_slot_id_by_name(db: &sqlx::PgPool, name: &str) -> Option<uuid::Uuid> {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT vs.id
         FROM rev.value_slot_path_details vspd
         JOIN rev.object_paths op ON op.id = vspd.path_id
         JOIN rev.versioned_objects vo ON vo.path_id = op.id
         JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id
         WHERE vspd.name = $1
         ORDER BY vo.created_at DESC
         LIMIT 1",
    )
    .bind(name)
    .fetch_optional(db)
    .await
    .expect("value_slot id query failed")
}

/// A `timeline` control that (a) declares an `axis` config and (b) carries a
/// `timeline-ref` item field pointing at a SEPARATE accumulator slot MATERIALIZES
/// both: `control_spec.axis` is baked verbatim, and the ref field's `ref_slot`
/// NAME is resolved to the referenced slot's `value_slot_id` as
/// `item_fields[].ref_value_slot_id` (the pane-stack nav target). Asserts live DB
/// state. (Requires migrations 061 + 062.)
#[tokio::test]
async fn test_timeline_ref_and_axis_materialize() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let tarball = create_timeline_ref_test_tarball(&suffix);
    ingest_tarball(&base_url, tarball).await; // asserts 201 + status:"success"

    let db = get_db_pool().await;

    let brief_id = latest_brief_id_by_name(&db, &format!("talk-brief-{short}")).await;
    let spec = brief_input_control_spec(&db, brief_id)
        .await
        .expect("brief-input must have a control_spec");

    // (a) axis baked verbatim.
    let axis = spec.get("axis").expect("control_spec must carry axis");
    assert_eq!(
        axis.get("unit").and_then(|v| v.as_str()),
        Some("minutes"),
        "axis.unit must be baked from the directive"
    );
    // duration_field rides the opaque axis JSON verbatim (no resolution needed).
    assert_eq!(
        axis.get("duration_field").and_then(|v| v.as_str()),
        Some("duration_seconds"),
        "axis.duration_field must ride through verbatim"
    );
    // total_slot NAME resolves to the up-front length slot's value_slot_id.
    let resolved_total = axis
        .get("total_value_slot_id")
        .and_then(|v| v.as_str())
        .expect("axis must resolve total_value_slot_id from total_slot");
    let expected_total = latest_value_slot_id_by_name(&db, &format!("talk-len-{short}"))
        .await
        .expect("length slot must materialize");
    assert_eq!(
        resolved_total,
        expected_total.to_string(),
        "total_value_slot_id must point at the up-front length slot"
    );

    // (b) the timeline-ref item field resolved ref_slot NAME -> ref_value_slot_id.
    let item_fields = spec
        .get("item_fields")
        .and_then(|v| v.as_array())
        .expect("control_spec must carry item_fields");
    let ref_field = item_fields
        .iter()
        .find(|f| f["control"] == "timeline-ref")
        .expect("a timeline-ref item field must be present");
    let resolved = ref_field
        .get("ref_value_slot_id")
        .and_then(|v| v.as_str())
        .expect("ref field must resolve ref_value_slot_id");
    let expected = latest_value_slot_id_by_name(&db, &format!("talk-scene-lib-{short}"))
        .await
        .expect("referenced slot must materialize");
    assert_eq!(
        resolved,
        expected.to_string(),
        "ref_value_slot_id must point at the referenced accumulator slot"
    );
}

/// Fixture: a `timeline` brief-input over `talk-scenes` that declares an `axis`
/// and whose `item_fields` carry a `timeline-ref` field pointing at a separate
/// `talk-scene-lib` accumulator slot.
fn create_timeline_ref_test_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Timeline Ref Guide {suffix}\"\n\
         description = \"Fixture exercising timeline axis + a timeline-ref item field.\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Intro](intro.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    // talk-scenes: the parent timeline accumulator. talk-scene-lib: the SEPARATE
    // shared accumulator the `scene` timeline-ref field points at. axis: presentation
    // config ({ unit, allowAbsolute }). The `scene` item field is a `timeline-ref`
    // whose ref_slot names talk-scene-lib.
    let intro = format!(
        "# Intro\n\n\
         :::value-slot {{ name: \"talk-scenes-{short}\", kind: \"json\", cardinality: \"multi\", min: 1 }}\n\
         :::\n\n\
         :::value-slot {{ name: \"talk-scene-lib-{short}\", kind: \"json\", cardinality: \"multi\" }}\n\
         :::\n\n\
         :::value-slot {{ name: \"talk-len-{short}\", kind: \"number\" }}\n\
         :::\n\n\
         :::brief-input {{ name: \"talk-timeline-{short}\", control: \"timeline\", slot: \"talk-scenes-{short}\", axis: {{ unit: \"minutes\", allowAbsolute: true, duration_field: \"duration_seconds\", total_slot: \"talk-len-{short}\" }}, item_fields: [{{ key: \"beat\", control: \"textarea\" }}, {{ key: \"scene\", control: \"timeline-ref\", ref_slot: \"talk-scene-lib-{short}\" }}] }}\n\
         :::\n\n\
         :::brief {{ name: \"talk-brief-{short}\", units: [\"talk-timeline-{short}\"] }}\n\
         :::\n\n\
         :::brief-group {{ name: \"talk-group-{short}\", briefs: [\"talk-brief-{short}\"] }}\n\
         :::\n\n\
         :::brief-pack {{ name: \"talk-pack-{short}\", groups: [\"talk-group-{short}\"] }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/intro.md", &intro);

    let fluent = format!(
        "talk-timeline-{short} = Build your talk\n    .beat-label = What happens here\n    .scene-label = Scene\n\
         talk-brief-{short} = Talk brief title\n\
         talk-group-{short} = Talk group title\n\
         talk-pack-{short} = Talk pack title\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// Ingesting a guide with an UNKNOWN brief-input control fails at the boundary
/// validation with 422 and names the offending key — the guard that makes
/// `repeater` ingestable only because migration 060 seeded it.
#[tokio::test]
async fn test_unknown_control_type_is_rejected() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = &suffix[28..];

    let bad_control = format!("definitely-not-a-control-{short}");
    let tarball = create_repeater_test_tarball(&suffix, &bad_control);

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let response = reqwest::Client::new()
        .post(format!("{}/api/guides/ingest", base_url))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("request failed");

    let status = response.status();
    let body: serde_json::Value = response.json().await.expect("invalid json");
    assert_eq!(
        status, 422,
        "unknown control must be rejected with 422: {body}"
    );
    assert!(
        body.to_string()
            .contains("Unknown brief input control_type"),
        "expected the control-type boundary error, got: {body}"
    );
}

/// Ingesting a guide that uses the `duration` control (a typed min:sec length
/// entry stored in seconds) over a `number` slot MATERIALIZES it: the boundary
/// validation ACCEPTS `duration` (seeded by migration 065 — an unseeded key would
/// 422 here, see `test_unknown_control_type_is_rejected`) and the brief-input's
/// control_type resolves to `duration`. Asserts live DB state. (Requires
/// migration 065.)
#[tokio::test]
async fn test_duration_control_type_accepted() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let short = suffix[28..].to_string();

    let tarball = create_duration_test_tarball(&suffix);
    ingest_tarball(&base_url, tarball).await; // asserts 201 + status:"success"

    let db = get_db_pool().await;
    let brief_id = latest_brief_id_by_name(&db, &format!("len-brief-{short}")).await;
    let control = brief_input_control_type_key(&db, brief_id).await;
    assert_eq!(
        control.as_deref(),
        Some("duration"),
        "the length brief-input must materialize with the duration control type"
    );
}

/// Fixture: a minimal guide with a `number` length slot and a TOP-LEVEL
/// `duration` brief-input over it (the time-centric builder's "how long is the
/// talk" length entry). Proves the seeded `duration` catalog row makes such a
/// control ingestable.
fn create_duration_test_tarball(suffix: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let short = &suffix[28..];

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Duration Guide {suffix}\"\n\
         description = \"Fixture exercising the duration control.\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Intro](intro.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        "# Conduit Context\n\nTest guide frame.\n",
    );

    let intro = format!(
        "# Intro\n\n\
         :::value-slot {{ name: \"talk-len-{short}\", kind: \"number\" }}\n\
         :::\n\n\
         :::brief-input {{ name: \"len-input-{short}\", control: \"duration\", slot: \"talk-len-{short}\" }}\n\
         :::\n\n\
         :::brief {{ name: \"len-brief-{short}\", units: [\"len-input-{short}\"] }}\n\
         :::\n\n\
         :::brief-group {{ name: \"len-group-{short}\", briefs: [\"len-brief-{short}\"] }}\n\
         :::\n\n\
         :::brief-pack {{ name: \"len-pack-{short}\", groups: [\"len-group-{short}\"] }}\n\
         :::\n"
    );
    append_file(&mut archive, "guide/intro.md", &intro);

    let fluent = format!(
        "len-input-{short} = How long is the talk\n\
         len-brief-{short} = Length brief title\n\
         len-group-{short} = Length group title\n\
         len-pack-{short} = Length pack title\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// End-to-end accumulation through the REAL binding HTTP path: POST a growing
/// multi-json list to the SAME slot across two dispatches of one journey, then
/// GET the effective bindings and assert the newest/longest list resolves
/// (latest-wins). This is how the `repeater` control's read-modify-write add
/// reaches the backend. (Requires migration 060 only indirectly — the multi-json
/// slot + latest-wins resolver are what this exercises.)
#[tokio::test]
async fn test_repeater_accumulation_via_api() {
    let (base_url, _handle) = spawn_app().await;
    let db = test_db_pool();
    let pg = db.get_pool();
    let dapple = uuid::Uuid::parse_str("019e132e-2610-7a4a-b64a-77a49e286ac0").unwrap();

    let principal: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.principals (owner_company_id) VALUES ($1) RETURNING id",
    )
    .bind(dapple)
    .fetch_one(pg)
    .await
    .expect("seed principal");
    let playbook: uuid::Uuid = sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
        .fetch_one(pg)
        .await
        .expect("dev DB needs at least one playbook");

    // A multi-json accumulator slot (the pp_scenes shape).
    let slot = {
        use dapple_data::model::value_slots::InsertValueSlotArgs;
        let name = format!("api-accum-slot-{}", uuid::Uuid::now_v7());
        let args = InsertValueSlotArgs {
            names: vec![name.clone()],
            value_kind_keys: vec!["json".to_string()],
            titles: vec![name.clone()],
            descriptions: vec![None],
            constraints: vec![serde_json::json!({})],
            cardinalities: vec!["multi".to_string()],
            min_selections: vec![Some(1)],
            max_selections: vec![None],
            content_hashes: vec![
                dapple_data::hash::hash_value_slot(
                    &name,
                    "",
                    "json",
                    &None,
                    "multi",
                    Some(1),
                    None,
                )
                .as_str()
                .to_string(),
            ],
        };
        dapple_data::query::value_slots::insert_value_slots(&db, args)
            .await
            .expect("insert slot")[0]
            .id
    };

    let journey: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.journeys (playbook_id, owner_company_id, principal_id, name, created_at)
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(playbook)
    .bind(dapple)
    .bind(principal)
    .bind(format!("api-accum-{}", uuid::Uuid::now_v7()))
    .fetch_one(pg)
    .await
    .expect("seed journey");

    let insert_dispatch = "INSERT INTO rev.dispatches (journey_id, user_id, intent, owner_company_id, principal_id, created_at) VALUES ($1, NULL, '{}'::jsonb, $2, $3, now()) RETURNING id";
    let d1: uuid::Uuid = sqlx::query_scalar(insert_dispatch)
        .bind(journey)
        .bind(dapple)
        .bind(principal)
        .fetch_one(pg)
        .await
        .expect("dispatch 1");
    let d2: uuid::Uuid = sqlx::query_scalar(insert_dispatch)
        .bind(journey)
        .bind(dapple)
        .bind(principal)
        .fetch_one(pg)
        .await
        .expect("dispatch 2");

    let client = reqwest::Client::new();
    let json_items = |beats: &[&str]| -> Vec<serde_json::Value> {
        beats
            .iter()
            .map(|b| serde_json::json!({ "value": { "beat": b } }))
            .collect()
    };

    // Add #1 -> [s1] on dispatch 1.
    let r1 = client
        .post(format!("{}/api/dispatch-intent-bindings/", base_url))
        .header("Authorization", test_auth_header())
        .json(&serde_json::json!({
            "objs": [{ "dispatch_id": d1, "value_slot_id": slot, "json_items": json_items(&["s1"]) }]
        }))
        .send()
        .await
        .expect("post binding 1");
    assert!(
        r1.status().is_success(),
        "first bind must succeed, got {}",
        r1.status()
    );

    // Add #2 -> [s1, s2, s3] on dispatch 2 (the full re-emitted list).
    let r2 = client
        .post(format!("{}/api/dispatch-intent-bindings/", base_url))
        .header("Authorization", test_auth_header())
        .json(&serde_json::json!({
            "objs": [{ "dispatch_id": d2, "value_slot_id": slot, "json_items": json_items(&["s1", "s2", "s3"]) }]
        }))
        .send()
        .await
        .expect("post binding 2");
    assert!(
        r2.status().is_success(),
        "second bind must succeed, got {}",
        r2.status()
    );

    // Effective bindings resolve the newest/longest list.
    let resp = client
        .get(format!(
            "{}/api/dispatch-intent-bindings/?query=effective_by_journey_id&journey_id={}",
            base_url, journey
        ))
        .header("Authorization", test_auth_header())
        .send()
        .await
        .expect("get effective");
    let body: serde_json::Value = resp.json().await.expect("effective json");
    let objs = body["objs"]
        .as_array()
        .expect("objs array in effective response");
    let binding = objs
        .iter()
        .find(|o| o["value_slot_id"].as_str() == Some(slot.to_string().as_str()))
        .unwrap_or_else(|| panic!("effective bindings missing the accumulator slot: {body}"));
    let items = binding["json_items"].as_array().expect("json_items array");
    assert_eq!(
        items.len(),
        3,
        "effective binding must hold the newest/longest list (3 scenes), got: {binding}"
    );
}

// ============================================================================
// Dataset (spreadsheet / tabular) value_kind — binding storage, producer,
// readers, and gate coverage (docs/plans/spreadsheet-dataset-ingest.md §11).
// These reuse the computed-output dispatch fixture and flip the computed slot's
// value_kind to `dataset`, then pin a real rev.asset_dataset_extractions row.
// ============================================================================

/// Flip a value slot to the `dataset` value_kind (a test-only in-place mutation;
/// the real re-versioning path is Migration 70's slot re-version on re-sync).
async fn flip_slot_kind_to_dataset(sql: &sqlx::PgPool, slot_id: uuid::Uuid) {
    sqlx::query(
        "UPDATE rev.value_slots \
         SET value_kind_id = (SELECT id FROM rev.value_kinds WHERE key = 'dataset') \
         WHERE id = $1",
    )
    .bind(slot_id)
    .execute(sql)
    .await
    .expect("flip slot kind to dataset");
}

/// Set a value slot's `constraints` JSONB (test-only in-place mutation). Used to
/// attach per-slot dataset overrides (`{ "columns": {...}, "sample": false }`)
/// that the read-time merge (`apply_dataset_slot_constraints`) applies.
async fn set_slot_constraints(
    sql: &sqlx::PgPool,
    slot_id: uuid::Uuid,
    constraints: serde_json::Value,
) {
    sqlx::query("UPDATE rev.value_slots SET constraints = $1 WHERE id = $2")
        .bind(constraints)
        .bind(slot_id)
        .execute(sql)
        .await
        .expect("set slot constraints");
}

/// Create a real asset (draft → chunk → finalize) plus a dataset extraction row.
/// When `mark_ok` is true the extraction carries `envelope` and is `ok`; otherwise
/// it stays `pending`. Returns `(asset_id, extraction_id)`.
async fn create_dataset_asset(
    pool: &dapple_data::DbPool,
    envelope: serde_json::Value,
    mark_ok: bool,
) -> (uuid::Uuid, uuid::Uuid) {
    use dapple_data::model::asset_dataset_extractions::InsertAssetDatasetExtractionArgs;
    use dapple_data::model::drafts::{InsertDraftArgs, InsertDraftChunkArgs};
    use dapple_data::query::asset_dataset_extractions::{
        insert_asset_dataset_extractions, mark_asset_dataset_extraction_ok,
    };
    use dapple_data::query::drafts::{finalize_draft, insert_draft_chunks, insert_drafts};

    let content = b"a,b\n1,2".to_vec();
    let drafts = insert_drafts(
        pool,
        InsertDraftArgs {
            asset_kind_keys: vec!["document".to_string()],
            mime_types: vec!["text/csv".to_string()],
        },
    )
    .await
    .expect("insert draft");
    let draft_id = drafts[0].id;
    insert_draft_chunks(
        pool,
        InsertDraftChunkArgs {
            draft_ids: vec![draft_id],
            data: vec![content.clone()],
        },
    )
    .await
    .expect("insert draft chunk");
    let hash = dapple_data::hash::hash_asset(&content);
    let finalized = finalize_draft(pool, draft_id, hash.as_str())
        .await
        .expect("finalize draft");
    let asset_id = finalized.asset_id;

    let ext = insert_asset_dataset_extractions(
        pool,
        InsertAssetDatasetExtractionArgs {
            asset_ids: vec![asset_id],
            extractor_versions: vec![1],
        },
    )
    .await
    .expect("insert dataset extraction");
    let ext_id = ext[0].id;

    if mark_ok {
        mark_asset_dataset_extraction_ok(pool, ext_id, envelope, Some(1), Some(2))
            .await
            .expect("mark extraction ok");
    }

    (asset_id, ext_id)
}

/// Positive producer path + reader surface + pin stability. A computed `dataset`
/// output whose source_path yields an asset_id pins the current ok extraction;
/// the envelope round-trips through the two-hop read join; and a later
/// re-extraction (a NEW row) does NOT move the existing pin.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_producer_pins_dataset_extraction_and_pin_is_stable() {
    use dapple_data::query::asset_dataset_extractions::{
        insert_asset_dataset_extractions, mark_asset_dataset_extraction_ok,
    };
    use dapple_data::query::dispatch_intent_bindings::select_dispatch_intent_bindings_by_dispatch_ids;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);
    let synth_slot = value_slot_id_by_name(sql, &synth_name).await;
    flip_slot_kind_to_dataset(sql, synth_slot).await;

    let envelope = serde_json::json!({
        "asset_id": "placeholder",
        "tables": [ { "name": "data", "rows": 2, "columns": [ { "name": "a", "type": "int" } ] } ]
    });
    let (asset_id, ext_id) = create_dataset_asset(&pool, envelope.clone(), true).await;

    // ACTION: source_path "" selects the whole payload; the object carries asset_id.
    let outcomes = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        seeded.stage_id,
        seeded.step_id,
        &serde_json::json!({ "asset_id": asset_id.to_string() }),
    )
    .await
    .expect("produce");
    assert!(
        outcomes
            .iter()
            .any(|o| o.name == synth_name && o.outcome == "produced"),
        "dataset computed output must be produced: {outcomes:?}"
    );

    // The dataset_items row pins the exact extraction id.
    let pinned: uuid::Uuid = sqlx::query_scalar(
        "SELECT di.asset_dataset_extraction_id \
         FROM rev.dispatch_intent_binding_dataset_items di \
         JOIN rev.dispatch_intent_bindings b ON b.id = di.dispatch_intent_binding_id \
         WHERE b.dispatch_id = $1 AND b.value_slot_id = $2",
    )
    .bind(dispatch_id)
    .bind(synth_slot)
    .fetch_one(sql)
    .await
    .expect("pinned extraction id");
    assert_eq!(pinned, ext_id, "binding must pin the current ok extraction");

    // producer = computed provenance.
    let producer: Option<String> = sqlx::query_scalar(
        "SELECT vm.producer FROM rev.dispatch_intent_binding_value_meta vm \
         JOIN rev.dispatch_intent_bindings b ON b.id = vm.dispatch_intent_binding_id \
         WHERE b.dispatch_id = $1 AND b.value_slot_id = $2",
    )
    .bind(dispatch_id)
    .bind(synth_slot)
    .fetch_one(sql)
    .await
    .expect("producer meta");
    assert_eq!(producer.as_deref(), Some("computed"));

    // READER: the envelope surfaces through the two-hop join on the folded DTO.
    let bindings = select_dispatch_intent_bindings_by_dispatch_ids(&pool, vec![dispatch_id])
        .await
        .expect("bindings");
    let ds = bindings
        .iter()
        .find(|b| b.value_slot_name == synth_name)
        .expect("dataset binding present");
    assert_eq!(ds.dataset_items.len(), 1, "one pinned dataset item");
    assert_eq!(ds.dataset_items[0].asset_dataset_extraction_id, ext_id);
    assert_eq!(
        ds.dataset_items[0].envelope, envelope,
        "the pinned envelope must surface on the binding row"
    );

    // PIN STABILITY: a re-extraction of the SAME asset inserts a NEW row with a
    // different envelope. The existing binding must keep reading its pinned E1.
    let new_env = serde_json::json!({ "asset_id": "placeholder", "tables": [], "changed": true });
    let ext2 = insert_asset_dataset_extractions(
        &pool,
        dapple_data::model::asset_dataset_extractions::InsertAssetDatasetExtractionArgs {
            asset_ids: vec![asset_id],
            extractor_versions: vec![2],
        },
    )
    .await
    .expect("second extraction");
    mark_asset_dataset_extraction_ok(&pool, ext2[0].id, new_env.clone(), Some(0), Some(0))
        .await
        .expect("mark ext2 ok");

    let bindings_after = select_dispatch_intent_bindings_by_dispatch_ids(&pool, vec![dispatch_id])
        .await
        .expect("bindings after re-extract");
    let ds_after = bindings_after
        .iter()
        .find(|b| b.value_slot_name == synth_name)
        .expect("dataset binding still present");
    assert_eq!(
        ds_after.dataset_items[0].asset_dataset_extraction_id, ext_id,
        "re-extraction must NOT move the existing pin"
    );
    assert_eq!(
        ds_after.dataset_items[0].envelope, envelope,
        "the existing binding must keep reading its ORIGINAL pinned envelope"
    );
}

/// Negative producer path: an asset_id with no `ok` extraction yields
/// `type_mismatch`, writes NO binding, and does NOT fail the dispatch (the
/// producer is warn-and-continue).
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_producer_dataset_no_ok_extraction_is_type_mismatch() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);
    let synth_slot = value_slot_id_by_name(sql, &synth_name).await;
    flip_slot_kind_to_dataset(sql, synth_slot).await;

    // Asset with a PENDING extraction (never marked ok).
    let (asset_id, _ext) = create_dataset_asset(&pool, serde_json::json!({}), false).await;

    let outcomes = dapple_prompt_flow::dispatch_runner::produce_computed_output_bindings(
        &pool,
        dispatch_id,
        seeded.stage_id,
        seeded.step_id,
        &serde_json::json!({ "asset_id": asset_id.to_string() }),
    )
    .await
    .expect("producer runs (warn-and-continue; never errors the dispatch)");

    assert!(
        outcomes
            .iter()
            .any(|o| o.name == synth_name && o.outcome == "type_mismatch"),
        "an asset with no ok extraction must report type_mismatch: {outcomes:?}"
    );
    assert_eq!(
        binding_count_for_slot_name(sql, dispatch_id, &synth_name).await,
        0,
        "no binding may be written when there is no ok extraction"
    );
}

/// Hard-FK integrity (the first binding item table to have it): a dataset item
/// referencing a nonexistent extraction id is rejected by the FK.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_dataset_item_fk_rejects_bogus_extraction() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    // A real binding row already exists (slot-a was bound by the seed).
    let binding_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.dispatch_intent_bindings WHERE dispatch_id = $1 ORDER BY id LIMIT 1",
    )
    .bind(seeded.dispatch.id)
    .fetch_one(sql)
    .await
    .expect("existing binding");

    let bogus = uuid::Uuid::now_v7();
    let result = sqlx::query(
        "INSERT INTO rev.dispatch_intent_binding_dataset_items \
         (dispatch_intent_binding_id, asset_dataset_extraction_id) VALUES ($1, $2)",
    )
    .bind(binding_id)
    .bind(bogus)
    .execute(sql)
    .await;

    assert!(
        result.is_err(),
        "a dataset item pinning a nonexistent extraction must be rejected by the FK"
    );
}

/// Shared reference: two bindings of one dataset pin the SAME extraction row —
/// one envelope, two lightweight FK items ("two copies share a reference").
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_two_bindings_share_one_dataset_extraction() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);
    let synth_slot = value_slot_id_by_name(sql, &synth_name).await;
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{}", seeded.short)).await;
    flip_slot_kind_to_dataset(sql, synth_slot).await;
    flip_slot_kind_to_dataset(sql, slot_a).await;

    let (_asset_id, ext_id) =
        create_dataset_asset(&pool, serde_json::json!({ "tables": [] }), true).await;

    // Bind BOTH slots to the SAME extraction. slot-a already has a binding row
    // from the seed; pin the extraction onto it, and onto a fresh synth binding.
    let slot_a_binding: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.dispatch_intent_bindings WHERE dispatch_id = $1 AND value_slot_id = $2",
    )
    .bind(dispatch_id)
    .bind(slot_a)
    .fetch_one(sql)
    .await
    .expect("slot-a binding");
    let synth_binding: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(dispatch_id)
    .bind(synth_slot)
    .fetch_one(sql)
    .await
    .expect("synth binding");

    for binding in [slot_a_binding, synth_binding] {
        sqlx::query(
            "INSERT INTO rev.dispatch_intent_binding_dataset_items \
             (dispatch_intent_binding_id, asset_dataset_extraction_id) VALUES ($1, $2)",
        )
        .bind(binding)
        .bind(ext_id)
        .execute(sql)
        .await
        .expect("pin shared extraction");
    }

    let pins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.dispatch_intent_binding_dataset_items \
         WHERE asset_dataset_extraction_id = $1",
    )
    .bind(ext_id)
    .fetch_one(sql)
    .await
    .expect("count pins");
    assert_eq!(pins, 2, "two bindings pin one extraction row");

    let distinct_extractions: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT asset_dataset_extraction_id) \
         FROM rev.dispatch_intent_binding_dataset_items di \
         JOIN rev.dispatch_intent_bindings b ON b.id = di.dispatch_intent_binding_id \
         WHERE b.dispatch_id = $1",
    )
    .bind(dispatch_id)
    .fetch_one(sql)
    .await
    .expect("distinct extractions");
    assert_eq!(distinct_extractions, 1, "one shared envelope, two items");
}

/// Read-time, per-slot constraint merge (dataset plan §4.4, Amended 2026-07-20).
/// Two dataset slots pin the SAME shared extraction row but carry DIFFERENT
/// `constraints`: the constrained slot sees its `columns` type override applied
/// and its `sample` stripped, while the plain slot (empty constraints) still sees
/// the raw inferred type and the sample — proving the override is a per-slot
/// read-time merge over a shared write-once envelope, not baked at extraction.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_dataset_slot_constraints_merge_per_slot_at_read_time() {
    use dapple_data::query::briefs::select_bindings_by_journey_of_dispatch;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let synth_name = format!("synth-{}", seeded.short);
    let slot_a_name = format!("slot-a-{}", seeded.short);
    let synth_slot = value_slot_id_by_name(sql, &synth_name).await;
    let slot_a = value_slot_id_by_name(sql, &slot_a_name).await;
    flip_slot_kind_to_dataset(sql, synth_slot).await;
    flip_slot_kind_to_dataset(sql, slot_a).await;
    // The constrained slot is `multi` so the dispatch-scoped query populates
    // item_values for it — that query aggregates the array for multi slots ONLY
    // (single slots deliver via the scalar/envelope columns), and this test
    // asserts the constraint merge reaches the array elements too.
    sqlx::query("UPDATE rev.value_slots SET cardinality = 'multi' WHERE id = $1")
        .bind(synth_slot)
        .execute(sql)
        .await
        .expect("flip constrained slot to multi");

    // The constrained slot overrides `qty` (inferred int) -> number and drops the
    // sample; the plain slot keeps the raw inferred envelope.
    set_slot_constraints(
        sql,
        synth_slot,
        serde_json::json!({ "columns": { "qty": "number" }, "sample": false }),
    )
    .await;
    set_slot_constraints(sql, slot_a, serde_json::json!({})).await;

    // ONE shared extraction whose envelope infers qty as int and carries a sample.
    let envelope = serde_json::json!({
        "asset_id": "placeholder",
        "tables": [ { "name": "data", "rows": 2, "columns": [
            { "name": "qty", "type": "int" },
            { "name": "sku", "type": "text" }
        ] } ],
        "sample": { "data": [ { "qty": 3, "sku": "A1" } ] }
    });
    let (_asset_id, ext_id) = create_dataset_asset(&pool, envelope, true).await;

    // Pin the shared extraction onto both slots' bindings (slot-a already has a
    // seed binding; synth gets a fresh one).
    let slot_a_binding: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM rev.dispatch_intent_bindings WHERE dispatch_id = $1 AND value_slot_id = $2",
    )
    .bind(dispatch_id)
    .bind(slot_a)
    .fetch_one(sql)
    .await
    .expect("slot-a binding");
    let synth_binding: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(dispatch_id)
    .bind(synth_slot)
    .fetch_one(sql)
    .await
    .expect("synth binding");
    for binding in [slot_a_binding, synth_binding] {
        sqlx::query(
            "INSERT INTO rev.dispatch_intent_binding_dataset_items \
             (dispatch_intent_binding_id, asset_dataset_extraction_id) VALUES ($1, $2)",
        )
        .bind(binding)
        .bind(ext_id)
        .execute(sql)
        .await
        .expect("pin shared extraction");
    }

    // READ back the delivered (merged) envelopes.
    let rows = select_bindings_by_journey_of_dispatch(&pool, dispatch_id)
        .await
        .expect("read bindings");
    let qty_type = |env: &serde_json::Value| -> Option<String> {
        env["tables"]
            .as_array()?
            .iter()
            .find(|t| t["name"] == "data")?["columns"]
            .as_array()?
            .iter()
            .find(|c| c["name"] == "qty")?["type"]
            .as_str()
            .map(str::to_owned)
    };

    let constrained = rows
        .iter()
        .find(|r| r.value_slot_name == synth_name)
        .expect("constrained dataset row present");
    let constrained_env = constrained
        .envelope
        .as_ref()
        .expect("constrained envelope present");
    assert_eq!(
        qty_type(constrained_env).as_deref(),
        Some("number"),
        "constrained slot: columns override int -> number"
    );
    assert!(
        constrained_env.get("sample").is_none(),
        "constrained slot: sample:false strips the sample"
    );
    // The override is applied to the item_values array element too.
    let constrained_item = &constrained
        .item_values
        .as_array()
        .expect("item_values array")[0];
    assert_eq!(
        qty_type(constrained_item).as_deref(),
        Some("number"),
        "constrained slot: override also merged into item_values"
    );

    let plain = rows
        .iter()
        .find(|r| r.value_slot_name == slot_a_name)
        .expect("plain dataset row present");
    let plain_env = plain.envelope.as_ref().expect("plain envelope present");
    assert_eq!(
        qty_type(plain_env).as_deref(),
        Some("int"),
        "plain slot (empty constraints) keeps the inferred int over the SHARED row"
    );
    assert!(
        plain_env.get("sample").is_some(),
        "plain slot keeps the sample — the shared row is not mutated by the other slot's merge"
    );
}

/// Gate kind-agnosticism: a required `dataset` operator input blocks the START
/// gate (`is_step_fully_bound`) until bound, then releases — proving the gate
/// never inspects value_kind (no SQL gate change). Reuses the computed-output
/// fixture and flips `slot-a` to the dataset kind.
#[tokio::test]
#[serial_test::serial(guide_overlay_ingest)]
async fn test_required_dataset_input_gates_step_until_bound() {
    use dapple_data::query::journeys::is_step_fully_bound;

    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let seeded = seed_computed_output_dispatch(&base_url, &pool, &suffix).await;
    let dispatch_id = seeded.dispatch.id;
    let slot_a = value_slot_id_by_name(sql, &format!("slot-a-{}", seeded.short)).await;
    flip_slot_kind_to_dataset(sql, slot_a).await;

    // CORRUPT: remove slot-a's seed binding so the required dataset input is unbound.
    sqlx::query(
        "DELETE FROM rev.dispatch_intent_bindings WHERE dispatch_id = $1 AND value_slot_id = $2",
    )
    .bind(dispatch_id)
    .bind(slot_a)
    .execute(sql)
    .await
    .expect("unbind slot-a");

    assert!(
        !is_step_fully_bound(&pool, seeded.journey_id, seeded.stage_id, seeded.step_id)
            .await
            .expect("gate before"),
        "an unbound required dataset input must keep the step not-fully-bound"
    );

    // Bind the dataset input (a real extraction pin) and re-check the gate.
    let (_asset_id, ext_id) =
        create_dataset_asset(&pool, serde_json::json!({ "tables": [] }), true).await;
    let binding_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at) \
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(dispatch_id)
    .bind(slot_a)
    .fetch_one(sql)
    .await
    .expect("rebind slot-a");
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_binding_dataset_items \
         (dispatch_intent_binding_id, asset_dataset_extraction_id) VALUES ($1, $2)",
    )
    .bind(binding_id)
    .bind(ext_id)
    .execute(sql)
    .await
    .expect("pin dataset item");

    assert!(
        is_step_fully_bound(&pool, seeded.journey_id, seeded.stage_id, seeded.step_id)
            .await
            .expect("gate after"),
        "binding the dataset input must satisfy the kind-agnostic START gate"
    );
}

// ============================================================================
// CONDUITS.md guide-frame tests
// ============================================================================
//
// NOTE (per the "Testing Repair Operations" rule): there is no repair path
// that writes `rev.guides.conduits_md` — the hash-matched refresh reuses the
// guide row (identical hash ⇒ identical CONDUITS.md bytes), so there is no
// corrupt-then-verify variant to write here. The PATCH test below is the
// hash-fold proof instead: a CONDUITS.md-only edit MUST mint a new version.

/// Minimal tarball with a caller-controlled CONDUITS.md (or none at all).
fn create_conduits_test_tarball(suffix: &str, conduits: Option<&str>) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    fn append_file(archive: &mut Builder<GzEncoder<&mut Vec<u8>>>, path: &str, content: &str) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_bytes()).unwrap();
    }

    let mut tarball = Vec::new();
    let encoder = GzEncoder::new(&mut tarball, Compression::default());
    let mut archive = Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Conduits Test Guide {suffix}\"\n\
         description = \"Guide-frame storage tests\"\n\
         author = \"Test\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", &guide_toml);
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        "# Summary\n\n- [Chapter 1](ch1.md)\n",
    );
    append_file(&mut archive, "guide/ch1.md", "# Chapter 1\n\nProse.\n");
    if let Some(frame) = conduits {
        append_file(&mut archive, "guide/CONDUITS.md", frame);
    }

    archive.finish().unwrap();
    drop(archive);
    tarball
}

/// POST ingest stores the CONDUITS.md bytes on the guide version row.
#[tokio::test]
async fn test_ingest_stores_conduits_md() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let frame = "# Conduit Context\n\n## Bindings\n\nNone in this fixture.\n";
    let tarball = create_conduits_test_tarball(&suffix, Some(frame));

    let (_path_id, guide_id) = initial_ingest(&base_url, tarball).await;
    let db = get_db_pool().await;
    let guide_uuid: uuid::Uuid = guide_id.parse().unwrap();

    let stored: Option<String> =
        sqlx::query_scalar("SELECT conduits_md FROM rev.guides WHERE id = $1")
            .bind(guide_uuid)
            .fetch_one(&db)
            .await
            .expect("guide row");
    assert_eq!(
        stored.as_deref(),
        Some(frame),
        "ingest must store the CONDUITS.md bytes verbatim"
    );
}

/// A CONDUITS.md-only edit changes the tarball hash, so PATCH mints a NEW
/// guide version carrying the new frame, and the prior version row keeps the
/// old frame untouched (append-only).
#[tokio::test]
async fn test_conduits_only_edit_reversions_guide() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let frame_v1 = "# Conduit Context\n\nFrame v1.\n";
    let frame_v2 = "# Conduit Context\n\nFrame v2 — sharper binding story.\n";

    let (path_id, guide_id_v1) = initial_ingest(
        &base_url,
        create_conduits_test_tarball(&suffix, Some(frame_v1)),
    )
    .await;

    let (status, json) = ingest_tarball_patch(
        &base_url,
        &path_id,
        create_conduits_test_tarball(&suffix, Some(frame_v2)),
        false,
    )
    .await;
    assert_eq!(
        status, 201,
        "CONDUITS-only edit must mint a new version: {json}"
    );
    let guide_id_v2 = json["guide"]["id"]
        .as_str()
        .expect("new guide id")
        .to_string();
    assert_ne!(guide_id_v1, guide_id_v2, "new version row expected: {json}");

    let db = get_db_pool().await;
    let v1: Option<String> = sqlx::query_scalar("SELECT conduits_md FROM rev.guides WHERE id = $1")
        .bind(guide_id_v1.parse::<uuid::Uuid>().unwrap())
        .fetch_one(&db)
        .await
        .expect("v1 row");
    let v2: Option<String> = sqlx::query_scalar("SELECT conduits_md FROM rev.guides WHERE id = $1")
        .bind(guide_id_v2.parse::<uuid::Uuid>().unwrap())
        .fetch_one(&db)
        .await
        .expect("v2 row");
    assert_eq!(v1.as_deref(), Some(frame_v1), "prior version row unchanged");
    assert_eq!(
        v2.as_deref(),
        Some(frame_v2),
        "new version carries new frame"
    );
}

/// A tarball without CONDUITS.md is rejected at parse (422), same layer as a
/// missing SUMMARY.md.
#[tokio::test]
async fn test_ingest_missing_conduits_rejected() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_conduits_test_tarball(&suffix, None);

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tarball)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/guides/ingest", base_url))
        .header("Authorization", test_auth_header())
        .multipart(form)
        .send()
        .await
        .expect("request failed");

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    assert_eq!(status, 422, "missing CONDUITS.md must 422: {body}");
    assert!(
        body.contains("CONDUITS.md"),
        "error must name the missing file: {body}"
    );
}

// ============================================================================
// Windowed embeddings
// ============================================================================

/// A block whose text exceeds the conduit's sequence cap is stored as SEVERAL
/// embedding rows (see dapple-prompt-api `service::text_window`) so its tail
/// stays retrievable. That is only possible if the schema really admits many
/// rows per `block_id` — `guide_content_embeddings_block_idx` is a plain btree,
/// not a UNIQUE index — and if each window carries its own tag/locale rows,
/// since every search query INNER JOINs both.
///
/// This asserts all of that against the live DB rather than trusting the DDL, and
/// needs no conduit: it writes the rows the windowing pipeline would produce and
/// then reads them back through the real search query.
#[tokio::test]
async fn test_multi_window_embeddings_are_independently_retrievable() {
    let (base_url, _handle) = spawn_app().await;
    let suffix = uuid::Uuid::now_v7().to_string();
    let tarball = create_minimal_test_tarball(&suffix);

    // Real guide/chapter/block — every column here is a hard FK.
    let json = ingest_tarball(&base_url, tarball).await;
    let guide_id: uuid::Uuid = json["guide"]["id"]
        .as_str()
        .expect("guide id in response")
        .parse()
        .expect("guide id parses");

    dapple_data::load_env();
    let db_pool = test_db_pool();
    let db = db_pool.get_pool().clone();

    let (chapter_id, block_id): (uuid::Uuid, uuid::Uuid) = sqlx::query_as(
        "SELECT gc.id, gb.id FROM rev.guide_blocks gb
         JOIN rev.guide_chapters gc ON gc.id = gb.chapter_id
         WHERE gc.guide_id = $1 AND gb.directive_type_id =
             (SELECT id FROM rev.guide_block_directive_types WHERE key = 'prose')
         ORDER BY gb.id LIMIT 1",
    )
    .bind(guide_id)
    .fetch_one(&db)
    .await
    .expect("guide should have a prose block");

    // PRECONDITION: no embeddings yet (the test app runs without a conduit).
    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE block_id = $1")
            .bind(block_id)
            .fetch_one(&db)
            .await
            .expect("count embeddings");
    assert_eq!(before, 0, "expected a clean slate for block {block_id}");

    // --- write two windows of ONE block, as the pipeline now would ---------
    let directive_types = dapple_data::query::guides::load_directive_type_lookup_pool(&db_pool)
        .await
        .expect("load directive types");
    let prose_dt_id = *directive_types.get("prose").expect("prose directive type");

    let generation =
        dapple_data::query::guide_content_embeddings::insert_guide_embedding_generation(
            &db_pool, guide_id,
        )
        .await
        .expect("insert embedding generation");

    let head_text = format!("HEAD-{suffix}");
    let tail_text = format!("TAIL-{suffix}");
    let args = dapple_data::model::guide_content_embeddings::InsertGuideContentEmbeddingArgs {
        guide_ids: vec![guide_id; 2],
        chapter_ids: vec![chapter_id; 2],
        block_ids: vec![block_id; 2],
        directive_type_ids: vec![prose_dt_id; 2],
        chunk_texts: vec![head_text.clone(), tail_text.clone()],
        // Distinct vectors so a similarity search can order them.
        embeddings: vec![vec![0.1_f32; 768], vec![0.09_f32; 768]],
        metadata: vec![
            serde_json::json!({"chunk_ordinal": 0, "chunk_count": 2}),
            serde_json::json!({"chunk_ordinal": 1, "chunk_count": 2}),
        ],
        generation_ids: vec![generation.id; 2],
    };

    let inserted = dapple_data::query::guide_content_embeddings::insert_guide_content_embeddings(
        &db_pool, args,
    )
    .await
    .expect("insert two windows of one block");
    assert_eq!(inserted.len(), 2);

    // Each window gets its OWN tag and locale rows — a window missing either
    // silently disappears from every search (both are INNER JOINs).
    let tag_lookup = dapple_data::query::guides::load_tag_lookup_pool(&db_pool)
        .await
        .expect("load tag lookup");
    let universal_tag_id = *tag_lookup.get("universal").expect("universal tag");
    dapple_data::query::guide_content_embeddings::insert_guide_embedding_tags(
        &db_pool,
        dapple_data::model::guide_content_embeddings::InsertGuideEmbeddingTagArgs {
            embedding_ids: vec![inserted[0].id, inserted[1].id],
            tag_ids: vec![universal_tag_id; 2],
        },
    )
    .await
    .expect("insert embedding tags");

    let locale_rows = dapple_data::query::fluent_messages::select_fluent_message_locale_ids(
        &db_pool,
        &["en".to_string()],
    )
    .await
    .expect("select locale ids");
    let en_locale_id = locale_rows.first().expect("en locale seeded").id;
    dapple_data::query::guide_content_embeddings::insert_guide_embedding_locales(
        &db_pool,
        dapple_data::model::guide_content_embeddings::InsertGuideEmbeddingLocaleArgs {
            embedding_ids: vec![inserted[0].id, inserted[1].id],
            locale_ids: vec![en_locale_id; 2],
        },
    )
    .await
    .expect("insert embedding locales");

    // --- VERIFY -----------------------------------------------------------

    // 1. The schema really does admit several rows per block.
    let after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE block_id = $1")
            .bind(block_id)
            .fetch_one(&db)
            .await
            .expect("count embeddings");
    assert_eq!(after, 2, "block {block_id} should carry two window rows");

    // 2. Window identity survives the round-trip. `metadata` is the ONLY thing
    //    distinguishing two windows of a block on the search DTOs, which drop
    //    the embedding row id and expose only `block_id`.
    let ordinals: Vec<String> = sqlx::query_scalar(
        "SELECT metadata->>'chunk_ordinal' FROM rev.guide_content_embeddings
         WHERE block_id = $1 ORDER BY id",
    )
    .bind(block_id)
    .fetch_all(&db)
    .await
    .expect("read chunk ordinals");
    assert_eq!(ordinals, vec!["0".to_string(), "1".to_string()]);

    // 3. Per-window tag rows landed.
    let tag_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM rev.guide_embedding_tags et
         JOIN rev.guide_content_embeddings e ON e.id = et.embedding_id
         WHERE e.block_id = $1",
    )
    .bind(block_id)
    .fetch_one(&db)
    .await
    .expect("count window tags");
    assert_eq!(tag_count, 2, "each window needs its own tag association");

    // 4. THE POINT: both windows come back from a real search, so the tail of a
    //    long block is retrievable. Today, without windowing, that text would
    //    have been truncated away before it was ever embedded.
    let hits =
        dapple_data::query::guide_content_embeddings::search_guide_content_embeddings_by_guide_ids(
            &db_pool,
            &vec![0.1_f32; 768],
            &[guide_id],
            &["universal".to_string()],
            "en",
            50,
        )
        .await
        .expect("search embeddings");

    let texts: Vec<&str> = hits.iter().map(|h| h.chunk_text.as_str()).collect();
    assert!(
        texts.contains(&head_text.as_str()),
        "head window missing: {texts:?}"
    );
    assert!(
        texts.contains(&tail_text.as_str()),
        "TAIL window missing — the block's tail is unreachable: {texts:?}"
    );

    // 5. Deleting one window cascades to its junction rows and leaves the other
    //    window intact, so windows are independently removable.
    sqlx::query("DELETE FROM rev.guide_content_embeddings WHERE id = $1")
        .bind(inserted[0].id)
        .execute(&db)
        .await
        .expect("delete head window");

    let orphan_tags: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rev.guide_embedding_tags WHERE embedding_id = $1")
            .bind(inserted[0].id)
            .fetch_one(&db)
            .await
            .expect("count orphan tags");
    assert_eq!(orphan_tags, 0, "tag rows must cascade with their embedding");

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE block_id = $1")
            .bind(block_id)
            .fetch_one(&db)
            .await
            .expect("count remaining");
    assert_eq!(remaining, 1, "the other window must survive");
}

/// Browse-exclusion regression for the correlated latest-deprecation probe:
/// a deprecated guide version must drop out of the slug-scoped browse read
/// (the page CTE's `WHERE dl.id IS NULL` filter), while a ByIds read — which
/// is NOT a browse filter — must keep returning the version and project its
/// deprecation level. Corrupt-then-verify: assert presence, insert the
/// deprecation event, assert exclusion + projection.
#[tokio::test]
async fn test_deprecated_guide_excluded_from_browse_list() {
    use dapple_data::FilterOptions;
    use dapple_data::query::guides::{select_guides_by_object_ids, select_guides_by_slugs};

    let (base_url, _handle) = spawn_app().await;
    let pool = test_db_pool();
    let sql = pool.get_pool();

    let suffix_a = uuid::Uuid::now_v7().to_string();
    let suffix_b = uuid::Uuid::now_v7().to_string();
    let json_a = ingest_tarball(&base_url, create_minimal_test_tarball(&suffix_a)).await;
    let json_b = ingest_tarball(&base_url, create_minimal_test_tarball(&suffix_b)).await;
    let guide_a: uuid::Uuid = json_a["guide"]["id"].as_str().unwrap().parse().unwrap();
    let guide_b: uuid::Uuid = json_b["guide"]["id"].as_str().unwrap().parse().unwrap();

    // Learn the slugs via ByIds (baseline: both un-deprecated).
    let by_ids =
        select_guides_by_object_ids(&pool, vec![guide_a, guide_b], FilterOptions::default())
            .await
            .expect("guides by ids");
    let slug_of = |id: uuid::Uuid| {
        by_ids
            .iter()
            .find(|g| g.id == id)
            .expect("guide row by id")
            .slug
            .clone()
    };
    let slugs = vec![slug_of(guide_a), slug_of(guide_b)];

    let browsed =
        select_guides_by_slugs(&pool, slugs.clone(), None, None, FilterOptions::default())
            .await
            .expect("by slugs baseline");
    assert!(
        browsed.iter().any(|g| g.id == guide_a) && browsed.iter().any(|g| g.id == guide_b),
        "baseline browse must return both guides"
    );

    // CORRUPT: deprecate guide A's version. Level key 'deprecated' explicitly —
    // the ORDER BY id LIMIT 1 shortcut elsewhere picks 'active'.
    sqlx::query(
        "INSERT INTO rev.deprecation_events (versioned_object_id, to_level_id, reason) \
         SELECT g.versioned_object_id, \
                (SELECT id FROM rev.deprecation_levels WHERE key = 'deprecated'), \
                'test browse exclusion' \
         FROM rev.guides g WHERE g.id = $1",
    )
    .bind(guide_a)
    .execute(sql)
    .await
    .expect("deprecate guide A");

    // VERIFY: the browse read excludes A, keeps B.
    let browsed = select_guides_by_slugs(&pool, slugs, None, None, FilterOptions::default())
        .await
        .expect("by slugs after deprecation");
    assert!(
        !browsed.iter().any(|g| g.id == guide_a),
        "deprecated guide must drop out of the slug browse read"
    );
    assert!(
        browsed.iter().any(|g| g.id == guide_b),
        "undeprecated guide must remain browsable"
    );

    // ByIds keeps the version and projects the level (Group A projection path).
    let a_after = select_guides_by_object_ids(&pool, vec![guide_a], FilterOptions::default())
        .await
        .expect("guide A by id after deprecation");
    assert_eq!(
        a_after.len(),
        1,
        "ByIds is not a browse filter and must keep returning the deprecated version"
    );
    assert_eq!(
        a_after[0].deprecation_level.as_deref(),
        Some("deprecated"),
        "ByIds must project the deprecation level"
    );
}
