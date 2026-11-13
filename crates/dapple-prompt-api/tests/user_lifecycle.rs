//! # User Lifecycle Tests
//!
//! Integration tests that exercise the full API lifecycle for all principal objects.
//!
//! ## Architecture
//!
//! Tests are organized as "trials" that run **sequentially** via `libtest_mimic`
//! (`test_threads = 1`). Each trial:
//! 1. Creates objects via POST using the `Endpoint` trait
//! 2. Registers created objects in `ScenarioContext` for subsequent trials
//! 3. Validates POST responses match GET results
//!
//! Trials are ordered topologically so dependencies are always satisfied by the
//! time a trial runs — no polling loops or timeouts needed.
//!
//! ## Dependency Order
//!
//! ```text
//! categories  hats  fluent_messages  value_slots  assets  directives  inference_profiles  guides
//!     │        │         │                │                                │
//!     │        │    prompts (→ fluent)  briefs (→ value_slots)      verify/deprecate guides
//!     │        │         │                │
//!     │        │    tasks (→ prompts)  brief_groups (→ briefs)  brief_units (→ briefs)
//!     │        │         │                │
//!     │        │    steps (→ tasks)    brief_packs (→ brief_groups)
//!     │        │    stages (→ steps)       │
//!     └────────┴─────────┘                 │
//!         playbooks (→ steps,cats,hats)    │
//!              │                            │
//!         journeys (→ playbooks, brief_packs)
//!              │
//!         dispatches (→ playbooks, journeys)
//!              │
//!       attempts (→ dispatches)
//!       dispatch_intent_bindings (→ dispatches, value_slots)
//! ```
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --test user-lifecycle --package dapple-prompt-api
//! RUST_LOG=info cargo test --test user-lifecycle  # with logging
//! ```
//!
//! ## See Also
//!
//! - `consumer/mod.rs` - ScenarioContext, Endpoint trait
//! - `tests/README.md` - Detailed documentation
//! - `dapple-data/README.md` - Data layer concepts

use reqwest::Url;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use uuid::Uuid;

mod consumer;
use crate::consumer::error::ConsumerError;
use crate::consumer::test_auth_header;
use consumer::assets::AssetEndpoint;
use consumer::brief_groups::BriefGroupEndpoint;
use consumer::brief_packs::BriefPackEndpoint;
use consumer::brief_units::BriefUnitEndpoint;
use consumer::briefs::BriefEndpoint;
use consumer::categories::CategoryEndpoint;
use consumer::companies as companies_consumer;
use consumer::directives::DirectiveEndpoint;
use consumer::dispatch_intent_bindings::DispatchIntentBindingEndpoint;
use consumer::dispatches::DispatchEndpoint;
use consumer::external_service_tokens as external_service_tokens_consumer;
use consumer::fluent_messages::{FluentMessageEndpoint, FluentMessagePathEndpoint};
use consumer::guide_search::GuideSearchEndpoint;
use consumer::guides::GuideEndpoint;
use consumer::hats::HatEndpoint;
use consumer::inference_profiles::InferenceProfileEndpoint;
use consumer::journeys::JourneyEndpoint;
use consumer::playbooks::PlaybookEndpoint;
use consumer::prompts::PromptEndpoint;
use consumer::spawn_app;
use consumer::stages::StageEndpoint;
use consumer::steps::StepEndpoint;
use consumer::tasks::TaskEndpoint;
use consumer::value_slots::ValueSlotEndpoint;
use consumer::{ScenarioContext, async_trial};

use crate::consumer::Endpoint;
use dapple_data::model::tasks::Task;
use dapple_prompt_api::schema::guides::GetGuideSetSchema;
use dapple_prompt_api::schema::tasks::GetTaskSetSchema;
use libtest_mimic::{Arguments, run};
use redis::AsyncCommands;

static RT: once_cell::sync::Lazy<Arc<Runtime>> =
    once_cell::sync::Lazy::new(|| Arc::new(Runtime::new().expect("rt")));

fn main() {
    dapple_data::load_env();
    env_logger::init();

    // Diagnostic: report whether Redis env vars are present (mask password).
    match std::env::var("DAPPLE_REDIS_HOST") {
        Ok(h) => println!("DAPPLE_REDIS_HOST loaded: {}", h),
        Err(_) => println!("DAPPLE_REDIS_HOST not set"),
    }
    if std::env::var("DAPPLE_REDIS_PASSWORD").is_ok() {
        println!("DAPPLE_REDIS_PASSWORD is set (masked)");
    } else {
        println!("DAPPLE_REDIS_PASSWORD not set");
    }

    // Initialize Redis integrations early so write-through caching/publishing
    // is active during API operations. Use `DAPPLE_REDIS_HOST` or fall back
    // to the cluster default. If `DAPPLE_REDIS_PASSWORD` is set, inject it
    // into the URL so authentication succeeds against secured clusters.
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

    // Try to initialize Redis integrations with connectivity check. Tests
    // should fail-fast when `DAPPLE_REDIS_STRICT` is set; otherwise fall
    // back to noop integrations so local development is resilient.
    let init_res = RT.block_on(async {
        dapple_data::integrations::try_init_redis_with_url_async(&redis_url).await
    });

    match init_res {
        Ok(()) => (),
        Err(e) => {
            if std::env::var("DAPPLE_REDIS_STRICT").is_ok() {
                panic!("failed to initialize redis integrations: {:?}", e);
            } else {
                log::warn!(
                    "Redis init failed, falling back to noop integrations: {}",
                    e
                );
                dapple_data::integrations::init_noop();
            }
        }
    }
    let rt = RT.clone();

    let (base_url, _handle) = rt.block_on(spawn_app());
    let base_url = Url::parse(&base_url).expect("base_url parse");

    let scenario = Arc::new(Mutex::new(ScenarioContext::default()));

    let categories_trial = async_trial(&rt, "create_categories", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let categories_ep = CategoryEndpoint {
                    base_url: base_url.clone(),
                };
                let created_categories = categories_ep.post_fake_object_set(1..3).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_category_batch("categories-init", created_categories);
                }
                Ok(())
            }
        }
    });

    let hats_trial = async_trial(&rt, "create_hats", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let hats_ep = HatEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created_hats = hats_ep.post_fake_object_set(1..3).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_hat_batch("hats-init", created_hats);
                }
                Ok(())
            }
        }
    });

    let tasks_trial = async_trial(&rt, "create_tasks", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let tasks_ep = TaskEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created_tasks = tasks_ep.post_fake_object_set(4..7).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_task_batch("tasks-init", created_tasks);
                }
                Ok(())
            }
        }
    });

    let steps_trial = async_trial(&rt, "create_steps", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let steps_ep = StepEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };

                let created_steps = steps_ep.post_fake_object_set(4..7).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_step_batch("steps-init", created_steps);
                }

                Ok(())
            }
        }
    });

    let playbooks_trial = async_trial(&rt, "create_playbooks", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let playbooks_ep = PlaybookEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created_playbooks = playbooks_ep.post_fake_object_set(2..5).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_playbook_batch("init-playbooks", created_playbooks);
                }
                Ok(())
            }
        }
    });

    let stages_trial = async_trial(&rt, "create_stages", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let stages_ep = StageEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };

                let created_stages = stages_ep.post_fake_object_set(1..3).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_stage_batch("stages-init", created_stages);
                }

                Ok(())
            }
        }
    });

    let journeys_trial = async_trial(&rt, "create_journeys", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let journeys_ep = JourneyEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };

                // Journey-POST now REQUIRES a guide_id and validates that the
                // guide exposes the playbook (400 GUIDE_PLAYBOOK_MISMATCH
                // otherwise) — there is no auto-pin. The consumer picks a
                // random (guide, playbook) pair from the registered batches,
                // so seed `rev.guide_playbooks` links for the full cross-product
                // of registered guides × registered playbooks first; then any
                // pair the consumer picks is guaranteed linked.
                let (guide_ids, playbook_ids): (Vec<Uuid>, Vec<Uuid>) = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let guides: Vec<Uuid> =
                        ctx.guide_batches.iter().flatten().map(|g| g.id).collect();
                    let playbooks: Vec<Uuid> = ctx
                        .playbook_batches
                        .iter()
                        .flatten()
                        .map(|p| p.id)
                        .collect();
                    (guides, playbooks)
                };
                {
                    let pool = consumer::test_db_pool();
                    let sql = pool.get_pool();
                    for g in &guide_ids {
                        for p in &playbook_ids {
                            sqlx::query(
                                "INSERT INTO rev.guide_playbooks \
                                     (guide_id, playbook_id, chapter_index) \
                                 VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
                            )
                            .bind(g)
                            .bind(p)
                            .execute(sql)
                            .await
                            .map_err(|e| {
                                ConsumerError::Custom(format!("seed guide_playbooks: {e}"))
                            })?;
                        }
                    }
                }

                let created_journeys = journeys_ep.post_fake_object_set(2..5).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_journey_batch("init-journeys", created_journeys);
                }
                Ok(())
            }
        }
    });

    let verify_journeys_recent_trial = async_trial(&rt, "verify_journeys_recent", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Get the created journey ids for verification
                let expected_journey_ids: Vec<Uuid> = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let handle = ctx.journey_named_batches.get("init-journeys").unwrap();
                    ctx.journey_batches[handle.index]
                        .iter()
                        .map(|j| j.id)
                        .collect()
                };

                // Call GET /api/journeys/recent
                let client = reqwest::Client::new();
                let recent_url = base_url.join("api/journeys/recent").unwrap();
                let resp = client
                    .get(recent_url)
                    .header("authorization", test_auth_header())
                    .send()
                    .await?;

                let status = resp.status();
                let body_text = resp.text().await?;
                if !status.is_success() {
                    panic!(
                        "GET /journeys/recent: unexpected status {} body: {}",
                        status, body_text
                    );
                }

                // Parse response as IndexResponse<Journey>
                let response: serde_json::Value = serde_json::from_str(&body_text)
                    .expect("failed to parse /journeys/recent response");
                let objs = response
                    .get("objs")
                    .and_then(|v| v.as_array())
                    .expect("response should have 'objs' array");

                // Verify the expected journeys appear in the response
                let returned_ids: Vec<Uuid> = objs
                    .iter()
                    .filter_map(|j| j.get("id").and_then(|v| v.as_str()))
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();

                for expected_id in &expected_journey_ids {
                    assert!(
                        returned_ids.contains(expected_id),
                        "Expected journey {} not found in /journeys/recent response",
                        expected_id
                    );
                }

                log::info!(
                    "verify_journeys_recent: found {} of {} expected journeys",
                    expected_journey_ids.len(),
                    returned_ids.len()
                );

                Ok(())
            }
        }
    });

    // Regression for issue #484: `GET /api/journeys/favorites` must resolve
    // the viewer's per-principal display-name override (the same resolution
    // `GET /api/journeys/{id}` applies) instead of the base `rev.journeys.name`.
    // Before the fix, step 3 fails because the favorites list returns the
    // pre-rename base name.
    //
    // Deliberately self-contained: it seeds its own journey via the DB pool
    // (reusing a migration-seeded playbook) and never touches the shared
    // `ScenarioContext` mutex, so it is unaffected by unrelated lifecycle
    // trials and the exact #484 path (favorite -> PATCH rename -> list) is
    // exercised end-to-end through the real HTTP handlers.
    let verify_favorite_reflects_rename_trial =
        async_trial(&rt, "verify_favorite_reflects_rename", {
            let base_url = base_url.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    let pool = consumer::test_db_pool();
                    let sql = pool.get_pool();

                    // Resolve the spawn_app test user's user_details object_id
                    // and human principal (seeded by spawn_app).
                    let (user_object_id, principal_id): (Uuid, Uuid) = sqlx::query_as(
                        "SELECT ud.object_id, hp.principal_id
                         FROM rev.user_details ud
                         JOIN rev.human_principals hp ON hp.user_id = ud.user_id
                         WHERE ud.user_id = $1",
                    )
                    .bind(crate::consumer::test_auth_user_id())
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("resolve test principal: {e}")))?;

                    // Reuse any existing playbook (migrations seed at least one;
                    // a polluted dev DB has many — either works).
                    let playbook_id: Uuid =
                        sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
                            .fetch_optional(sql)
                            .await
                            .map_err(|e| ConsumerError::Custom(format!("pick playbook: {e}")))?
                            .ok_or_else(|| {
                                ConsumerError::Custom(
                                    "no playbook available to seed a journey".into(),
                                )
                            })?;

                    // dapple company UUID (seeded by Migration 31; mirrors
                    // Mig 37's principal owner_company_id).
                    let dapple_company =
                        Uuid::parse_str("019e132e-2610-7a4a-b64a-77a49e286ac0").unwrap();
                    let base_name = format!("Base-484-{}", Uuid::now_v7());
                    let journey_id: Uuid = sqlx::query_scalar(
                        "INSERT INTO rev.journeys
                             (playbook_id, owner_company_id, principal_id, name, created_at)
                         VALUES ($1, $2, $3, $4, now())
                         RETURNING id",
                    )
                    .bind(playbook_id)
                    .bind(dapple_company)
                    .bind(principal_id)
                    .bind(&base_name)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("seed journey: {e}")))?;
                    let _ = user_object_id; // resolved for clarity; favorites keys on the JWT user

                    let client = reqwest::Client::new();

                    // 1. Favorite the journey.
                    let fav_url = base_url
                        .join(&format!("api/journeys/{journey_id}/favorite"))
                        .unwrap();
                    let resp = client
                        .post(fav_url)
                        .header("authorization", test_auth_header())
                        .send()
                        .await?;
                    let status = resp.status();
                    let body = resp.text().await?;
                    assert!(
                        status.is_success(),
                        "POST favorite: unexpected status {status} body: {body}"
                    );

                    // 2. Rename it (writes a per-principal preference override;
                    //    the base rev.journeys.name is untouched).
                    let renamed = format!("Renamed-484-{}", Uuid::now_v7());
                    let patch_url = base_url
                        .join(&format!("api/journeys/{journey_id}"))
                        .unwrap();
                    let resp = client
                        .patch(patch_url)
                        .header("authorization", test_auth_header())
                        .json(&serde_json::json!({ "name": renamed }))
                        .send()
                        .await?;
                    let status = resp.status();
                    let body = resp.text().await?;
                    assert!(
                        status.is_success(),
                        "PATCH journey rename: unexpected status {status} body: {body}"
                    );
                    let patched: serde_json::Value =
                        serde_json::from_str(&body).expect("parse PATCH journey response");
                    assert_eq!(
                        patched
                            .get("obj")
                            .and_then(|o| o.get("name"))
                            .and_then(|v| v.as_str()),
                        Some(renamed.as_str()),
                        "single-GET path should reflect the rename: {body}"
                    );

                    // 3. The favorites list must now show the renamed name, not
                    //    the base name (this is the #484 regression assertion).
                    //
                    // Migration 46 made favorites chain-keyed: the response carries
                    // `chain_root` (id + name) rather than the per-journey `journey`
                    // ref. This seeded journey has no predecessor so it IS its own
                    // chain root, and the per-principal rename keyed on
                    // `journey_id` overlays its chain_root_name 1:1.
                    let list_url = base_url.join("api/journeys/favorites").unwrap();
                    let resp = client
                        .get(list_url)
                        .header("authorization", test_auth_header())
                        .send()
                        .await?;
                    let status = resp.status();
                    let body = resp.text().await?;
                    assert!(
                        status.is_success(),
                        "GET favorites: unexpected status {status} body: {body}"
                    );
                    let listed: serde_json::Value =
                        serde_json::from_str(&body).expect("parse favorites response");
                    let entry = listed
                        .get("objs")
                        .and_then(|v| v.as_array())
                        .expect("favorites response should have 'objs' array")
                        .iter()
                        .find(|e| {
                            e.get("chain_root")
                                .and_then(|j| j.get("id"))
                                .and_then(|v| v.as_str())
                                == Some(journey_id.to_string().as_str())
                        })
                        .unwrap_or_else(|| {
                            panic!("favorited chain root {journey_id} missing from list: {body}")
                        });
                    let listed_name = entry
                        .get("chain_root")
                        .and_then(|j| j.get("name"))
                        .and_then(|v| v.as_str())
                        .expect("favorite entry should carry chain_root.name");
                    assert_eq!(
                        listed_name, renamed,
                        "favorites list must reflect the per-principal rename \
                         (got {listed_name:?}, base name was {base_name:?})"
                    );

                    log::info!(
                        "verify_favorite_reflects_rename: journey {journey_id} favorites \
                         name correctly resolved to override {renamed:?}"
                    );

                    Ok(())
                }
            }
        });

    // Regression for the bug Migration 46 was written to fix: a favorite set
    // on a chain must remain valid after the user advances ("next chapter") or
    // forks ("start over"). The favorite is anchored to the chain root; the
    // child journey is created with `predecessor_journey_id = root`, so the
    // chain-walk in check_favorite_chain.sql must resolve back through it.
    //
    // Self-contained: seeds two journeys in the same chain via direct DB
    // inserts and exercises POST/GET /favorite through HTTP, like the #484
    // trial above.
    let verify_favorite_follows_advance_trial = async_trial(
        &rt,
        "verify_favorite_follows_advance",
        {
            let base_url = base_url.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    let pool = consumer::test_db_pool();
                    let sql = pool.get_pool();

                    let (user_object_id, principal_id): (Uuid, Uuid) = sqlx::query_as(
                        "SELECT ud.object_id, hp.principal_id
                         FROM rev.user_details ud
                         JOIN rev.human_principals hp ON hp.user_id = ud.user_id
                         WHERE ud.user_id = $1",
                    )
                    .bind(crate::consumer::test_auth_user_id())
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("resolve test principal: {e}")))?;
                    let _ = user_object_id;
                    let _ = principal_id;

                    let playbook_id: Uuid =
                        sqlx::query_scalar("SELECT id FROM rev.playbooks LIMIT 1")
                            .fetch_optional(sql)
                            .await
                            .map_err(|e| ConsumerError::Custom(format!("pick playbook: {e}")))?
                            .ok_or_else(|| {
                                ConsumerError::Custom(
                                    "no playbook available to seed a journey".into(),
                                )
                            })?;

                    let dapple_company =
                        Uuid::parse_str("019e132e-2610-7a4a-b64a-77a49e286ac0").unwrap();

                    // Chained journeys require a guide pin (Migration 44
                    // trigger). Skip the chain assertions when no guide is
                    // available — clean test DBs may not have one and the
                    // chain-walk behavior is independently covered by
                    // journey_chain_test against pre-seeded fixtures.
                    let guide_id: Option<Uuid> =
                        sqlx::query_scalar("SELECT id FROM rev.guides LIMIT 1")
                            .fetch_optional(sql)
                            .await
                            .map_err(|e| ConsumerError::Custom(format!("pick guide: {e}")))?;
                    let Some(guide_id) = guide_id else {
                        log::info!(
                            "verify_favorite_follows_advance: no guide seeded; skipping \
                             chain assertions (single-journey case is covered by \
                             verify_favorite_reflects_rename)"
                        );
                        return Ok(());
                    };

                    // Seed the chain root.
                    let root_name = format!("Chain-Root-{}", Uuid::now_v7());
                    let root_id: Uuid = sqlx::query_scalar(
                        "INSERT INTO rev.journeys
                             (playbook_id, owner_company_id, principal_id, name,
                              guide_id, created_at)
                         VALUES ($1, $2, $3, $4, $5, now())
                         RETURNING id",
                    )
                    .bind(playbook_id)
                    .bind(dapple_company)
                    .bind(principal_id)
                    .bind(&root_name)
                    .bind(guide_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("seed root: {e}")))?;

                    let client = reqwest::Client::new();

                    // Favorite the root.
                    let resp = client
                        .post(
                            base_url
                                .join(&format!("api/journeys/{root_id}/favorite"))
                                .unwrap(),
                        )
                        .header("authorization", test_auth_header())
                        .send()
                        .await?;
                    assert!(
                        resp.status().is_success(),
                        "POST favorite on root: status {}",
                        resp.status()
                    );

                    // Seed a chained child (advance). Pinned to the same
                    // guide_id (the Migration 44 trigger enforces this).
                    let child_name = format!("Chain-Child-{}", Uuid::now_v7());
                    let child_id: Uuid = sqlx::query_scalar(
                        "INSERT INTO rev.journeys
                             (playbook_id, owner_company_id, principal_id, name,
                              predecessor_journey_id, guide_id, created_at)
                         VALUES ($1, $2, $3, $4, $5, $6, now())
                         RETURNING id",
                    )
                    .bind(playbook_id)
                    .bind(dapple_company)
                    .bind(principal_id)
                    .bind(&child_name)
                    .bind(root_id)
                    .bind(guide_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("seed child: {e}")))?;

                    // Check favorite via the CHILD's id. The server must walk
                    // back to the root and report favorited = true.
                    let resp = client
                        .get(
                            base_url
                                .join(&format!("api/journeys/{child_id}/favorite"))
                                .unwrap(),
                        )
                        .header("authorization", test_auth_header())
                        .send()
                        .await?;
                    let status = resp.status();
                    let body = resp.text().await?;
                    assert!(
                        status.is_success(),
                        "GET favorite on child: unexpected {status} body: {body}"
                    );
                    let resolved: serde_json::Value =
                        serde_json::from_str(&body).expect("parse check_favorite response");
                    assert_eq!(
                        resolved.get("is_favorited").and_then(|v| v.as_bool()),
                        Some(true),
                        "child journey {child_id} should be favorited via chain root {root_id}: {body}"
                    );
                    assert_eq!(
                        resolved
                            .get("chain_root_journey_id")
                            .and_then(|v| v.as_str()),
                        Some(root_id.to_string().as_str()),
                        "check response should expose the resolved chain_root_journey_id: {body}"
                    );

                    // Favoriting the child again is a no-op against the same chain.
                    let resp = client
                        .post(
                            base_url
                                .join(&format!("api/journeys/{child_id}/favorite"))
                                .unwrap(),
                        )
                        .header("authorization", test_auth_header())
                        .send()
                        .await?;
                    assert!(
                        resp.status().is_success(),
                        "POST favorite on child (idempotent): status {}",
                        resp.status()
                    );

                    // List should contain exactly one favorite for this chain
                    // (no duplicate row from the second POST).
                    let resp = client
                        .get(base_url.join("api/journeys/favorites").unwrap())
                        .header("authorization", test_auth_header())
                        .send()
                        .await?;
                    let body = resp.text().await?;
                    let listed: serde_json::Value =
                        serde_json::from_str(&body).expect("parse favorites list");
                    let matches: usize = listed
                        .get("objs")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter(|e| {
                                    e.get("chain_root")
                                        .and_then(|j| j.get("id"))
                                        .and_then(|v| v.as_str())
                                        == Some(root_id.to_string().as_str())
                                })
                                .count()
                        })
                        .unwrap_or(0);
                    assert_eq!(
                        matches, 1,
                        "exactly one favorite row should exist for chain rooted at {root_id} \
                         (second favorite POST via child must dedup): {body}"
                    );

                    Ok(())
                }
            }
        },
    );

    let verify_tasks_by_slugs_trial = async_trial(&rt, "verify_tasks_by_slugs", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Get slugs from created tasks
                let (expected_slugs, expected_tasks): (Vec<String>, Vec<Task>) = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let handle = ctx.task_named_batches.get("tasks-init").unwrap();
                    let tasks = &ctx.task_batches[handle.index];
                    (
                        tasks.iter().map(|t| t.slug.clone()).collect(),
                        tasks.clone(),
                    )
                };

                // Query tasks by slugs
                let tasks_ep = TaskEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let found = tasks_ep
                    .get_object_set(GetTaskSetSchema::BySlugs {
                        slugs: expected_slugs.clone(),
                        options: Some(dapple_data::FilterOptions {
                            limit: Some(100),
                            page: Some(1),
                        }),
                    })
                    .await?;

                // Verify all expected tasks were returned
                let found_ids: Vec<Uuid> = found.iter().map(|t| t.id).collect();
                for expected in &expected_tasks {
                    assert!(
                        found_ids.contains(&expected.id),
                        "Task with slug '{}' (id {}) not found in by_slugs response",
                        expected.slug,
                        expected.id,
                    );
                }

                log::info!(
                    "verify_tasks_by_slugs: found {}/{} tasks by slug lookup",
                    found.len(),
                    expected_tasks.len(),
                );

                Ok(())
            }
        }
    });

    let dispatches_trial = async_trial(&rt, "create_dispatches", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let dispatches_ep = DispatchEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };

                let created_dispatches = dispatches_ep.post_fake_object_set(2..4).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_dispatch_batch("dispatches-init", created_dispatches);
                }

                Ok(())
            }
        }
    });

    let attempts_trial = async_trial(&rt, "attempts_flow", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // pick a dispatch and a nested step/task
                let (dispatch_obj, step_obj, task_obj) = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let handle = ctx
                        .dispatch_named_batches
                        .get("dispatches-init")
                        .expect("dispatches-init handle");
                    let batch = ctx
                        .dispatch_batches
                        .get(handle.index)
                        .expect("dispatch batch");
                    let dispatch = batch.first().expect("no dispatch created");
                    let stage = dispatch.stages.first().expect("no stage");
                    let step = stage.steps.first().expect("no step");
                    let task = step.tasks.first().expect("no task");
                    (dispatch.id, step.id, task.task.id)
                };

                let client = reqwest::Client::new();

                // create attempt
                let attempts_url = base_url
                    .join(&format!("api/dispatches/{}/task_attempt/", dispatch_obj))
                    .unwrap();
                let create_body = serde_json::json!({
                    "dispatch_step_id": step_obj,
                    "task_id": task_obj,
                    "status": "queued",
                    "context_data": {},
                    "context_metadata": {}
                });

                let resp = client
                    .post(attempts_url)
                    .header("authorization", test_auth_header())
                    .json(&create_body)
                    .send()
                    .await?;

                let status = resp.status();
                let body_text = resp.text().await?;
                if !status.is_success() || body_text.trim().is_empty() {
                    panic!(
                        "create attempt: unexpected response status {} body: {}",
                        status, body_text
                    );
                }
                let created: serde_json::Value = match serde_json::from_str(&body_text) {
                    Ok(v) => v,
                    Err(e) => panic!("json decode created: {} body: {}", e, body_text),
                };
                // expect PostResponse with objs[0]
                // debug logging removed
                let obj = created
                    .get("objs")
                    .and_then(|v| v.get(0))
                    .expect("missing created attempt");
                let attempt_id = obj.get("id").and_then(|v| v.as_str()).expect("missing id");

                // post an event (use id UUID in path)
                let events_url = base_url
                    .join(&format!(
                        "api/dispatches/{}/task_attempt/{}/events",
                        dispatch_obj, attempt_id
                    ))
                    .unwrap();
                let event_body = serde_json::json!({
                    "phase_code": "running",
                    "context_data": {},
                    "context_metadata": {},
                    "context_temp_data": {}
                });

                let resp2 = client
                    .post(events_url)
                    .header("authorization", test_auth_header())
                    .json(&event_body)
                    .send()
                    .await?;

                let status2 = resp2.status();
                let body_text2 = resp2.text().await?;
                if !status2.is_success() || body_text2.trim().is_empty() {
                    panic!(
                        "create event: unexpected response status {} body: {}",
                        status2, body_text2
                    );
                }
                let created_ev: serde_json::Value = match serde_json::from_str(&body_text2) {
                    Ok(v) => v,
                    Err(e) => panic!("json decode event: {} body: {}", e, body_text2),
                };
                assert!(
                    created_ev.get("objs").and_then(|v| v.get(0)).is_some(),
                    "event not created"
                );

                // fetch attempt by id and ensure status is derived from latest event
                let get_url = base_url
                    .join(&format!(
                        "api/dispatches/{}/task_attempt/{}",
                        dispatch_obj, attempt_id
                    ))
                    .unwrap();
                let get_resp = client
                    .get(get_url)
                    .header("authorization", test_auth_header())
                    .send()
                    .await?;

                let status3 = get_resp.status();
                let got_text = get_resp.text().await?;
                if !status3.is_success() {
                    panic!(
                        "get attempt: unexpected response status {} body: {}",
                        status3, got_text
                    );
                }
                let got: serde_json::Value = match serde_json::from_str(&got_text) {
                    Ok(v) => v,
                    Err(e) => panic!("json decode get: {} body: {}", e, got_text),
                };
                let status = got
                    .get("obj")
                    .and_then(|o| o.get("status"))
                    .and_then(|s| s.as_str())
                    .expect("status missing");
                assert_eq!(
                    status, "running",
                    "attempt status should reflect latest event"
                );

                Ok(())
            }
        }
    });

    let fluent_messages_trial = async_trial(&rt, "create_fluent_messages", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let ep = FluentMessageEndpoint {
                    base_url: base_url.clone(),
                };
                // Creating fluent messages automatically creates their paths (first version)
                let created = ep.post_fake_object_set(4..6).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_fluent_message_batch("init-fluent-messages", created);
                }
                Ok(())
            }
        }
    });

    // fluent-message-paths is a GET-only surface (paths are created as a
    // side-effect of fluent-message creation, never POSTed directly). Exercise
    // the GET-by-id and index handlers against a path pinned by a message that
    // `create_fluent_messages` produced.
    let fluent_message_paths_trial = async_trial(&rt, "verify_fluent_message_paths", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let ep = FluentMessagePathEndpoint {
                    base_url: base_url.clone(),
                };
                let fm = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.random_fluent_message_from_named_batch("init-fluent-messages")
                        .expect("test setup: create_fluent_messages must run first")
                };
                // GET by id round-trips (test_fetched_object_id asserts the id match).
                let fetched = ep.get_object(fm.path_id).await?;
                assert_eq!(fetched.id, fm.path_id, "fluent message path id round-trip");
                // The index handler returns the same path when queried by object id.
                let set = ep
                    .get_object_set(
                        dapple_prompt_api::schema::fluent_messages::GetFluentMessagePathSetSchema::ByObjectIds {
                            ids: vec![fm.path_id],
                        },
                    )
                    .await?;
                assert!(
                    set.iter().any(|p| p.id == fm.path_id),
                    "index should return the requested fluent message path"
                );
                Ok(())
            }
        }
    });

    let value_slots_trial = async_trial(&rt, "create_value_slots", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Value slots are created atomically with their paths
                let ep = ValueSlotEndpoint {
                    base_url: base_url.clone(),
                };
                let created = ep.post_fake_object_set(2..4).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_value_slot_batch("value-slots-init", created);
                }
                Ok(())
            }
        }
    });

    let assets_trial = async_trial(&rt, "create_assets", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Assets are created atomically with their paths
                let ep = AssetEndpoint {
                    base_url: base_url.clone(),
                };
                let created = ep.post_fake_object_set(2..4).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_asset_batch("assets-init", created);
                }
                Ok(())
            }
        }
    });

    let dispatch_intent_bindings_trial = async_trial(&rt, "create_dispatch_intent_bindings", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                use consumer::Endpoint as _;
                use dapple_prompt_api::schema::value_slots::{
                    CreateValueSlotSchema, CreateValueSlotSetSchema,
                };

                // Multi-item bindings require multi (keep-N) slots — a single
                // slot now rejects >1 item. Create dedicated unbounded multi
                // slots for the storage round-trip below.
                let vs_ep = ValueSlotEndpoint {
                    base_url: base_url.clone(),
                };
                let multi_schema = CreateValueSlotSetSchema {
                    objs: (0..3)
                        .map(|i| CreateValueSlotSchema {
                            name: format!("binding-multi-{}-{}", i, Uuid::new_v4()),
                            value_kind: "text".to_string(),
                            title: "binding multi slot".to_string(),
                            description: None,
                            constraints: serde_json::json!({}),
                            cardinality: "multi".to_string(),
                            min_selections: None,
                            max_selections: None,
                        })
                        .collect(),
                };
                let value_slot_ids: Vec<Uuid> = vs_ep
                    .post_object_set(multi_schema)
                    .await?
                    .iter()
                    .map(|s| s.id)
                    .collect();

                let ep = DispatchIntentBindingEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                    value_slot_ids,
                };
                let created = ep.post_fake_object_set(1..3).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_dispatch_intent_binding_batch(
                        "dispatch-intent-bindings-init",
                        created,
                    );
                }
                Ok(())
            }
        }
    });

    let directives_trial = async_trial(&rt, "create_directives", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Directives have no dependencies, can be created immediately
                let ep = DirectiveEndpoint {
                    base_url: base_url.clone(),
                };
                let created = ep.post_fake_object_set(2..4).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_directive_batch("directives-init", created);
                }
                Ok(())
            }
        }
    });

    let briefs_trial = async_trial(&rt, "create_briefs", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let ep = BriefEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created = ep.post_fake_object_set(4..7).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_brief_batch("briefs-init", created);
                }
                Ok(())
            }
        }
    });

    let brief_units_trial = async_trial(&rt, "verify_brief_units", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Get a brief that has units (created inline with the brief)
                let brief_id = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.random_brief_from_named_batch("briefs-init")
                        .expect("should have briefs")
                        .id
                };

                let ep = BriefUnitEndpoint {
                    base_url: base_url.clone(),
                    brief_id,
                };

                // Verify units are accessible via GET
                let units = ep.get_units().await?;
                log::info!(
                    "Verified {} brief units for brief {}",
                    units.len(),
                    brief_id
                );

                // Should have at least 1 unit since briefs are created with inline units
                assert!(
                    !units.is_empty(),
                    "Brief {} should have at least one unit (created inline)",
                    brief_id
                );

                // Verify unit structure
                for unit in &units {
                    assert!(!unit.unit_key.is_empty(), "unit_key should not be empty");
                }

                // Register the units in the scenario context
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_brief_unit_batch("brief-units-init", units);
                }
                Ok(())
            }
        }
    });

    let brief_groups_trial = async_trial(&rt, "create_brief_groups", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let ep = BriefGroupEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created = ep.post_fake_object_set(4..7).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_brief_group_batch("brief-groups-init", created);
                }
                Ok(())
            }
        }
    });

    let brief_packs_trial = async_trial(&rt, "create_brief_packs", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let ep = BriefPackEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created = ep.post_fake_object_set(4..7).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_brief_pack_batch("brief-packs-init", created);
                }
                Ok(())
            }
        }
    });

    let prompts_trial = async_trial(&rt, "create_prompts", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let ep = PromptEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created = ep.post_fake_object_set(4..7).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_prompt_batch("prompts-init", created);
                }
                Ok(())
            }
        }
    });

    // Inference profiles: no dependencies, creates profiles without configs
    let inference_profiles_trial = async_trial(&rt, "create_inference_profiles", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let ep = InferenceProfileEndpoint {
                    base_url: base_url.clone(),
                };
                let created = ep.post_fake_object_set(4..6).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_inference_profile_batch("inference-profiles-init", created);
                }
                Ok(())
            }
        }
    });

    let guides_trial = async_trial(&rt, "create_guides", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let guides_ep = GuideEndpoint {
                    base_url: base_url.clone(),
                };
                let created_guides = guides_ep.post_fake_object_set(2..4).await?;
                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_guide_batch("guides-init", created_guides);
                }
                Ok(())
            }
        }
    });

    let verify_guides_by_slugs_trial = async_trial(&rt, "verify_guides_by_slugs", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Get slugs from created guides
                let (expected_slugs, expected_guides): (
                    Vec<String>,
                    Vec<dapple_data::model::guides::Guide>,
                ) = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let handle = ctx.guide_named_batches.get("guides-init").unwrap();
                    let guides = &ctx.guide_batches[handle.index];
                    (
                        guides.iter().map(|g| g.slug.clone()).collect(),
                        guides.clone(),
                    )
                };

                // Query guides by slugs
                let guides_ep = GuideEndpoint {
                    base_url: base_url.clone(),
                };
                let found = guides_ep
                    .get_object_set(GetGuideSetSchema::BySlugs {
                        slugs: expected_slugs.clone(),
                        scope: None,
                        environment: None,
                        options: Some(dapple_data::FilterOptions {
                            limit: Some(100),
                            page: Some(1),
                        }),
                    })
                    .await?;

                // Verify all expected guides were returned
                let found_ids: Vec<Uuid> = found.iter().map(|g| g.id).collect();
                for expected in &expected_guides {
                    assert!(
                        found_ids.contains(&expected.id),
                        "Guide with slug '{}' (id {}) not found in by_slugs response",
                        expected.slug,
                        expected.id,
                    );
                }

                log::info!(
                    "verify_guides_by_slugs: found {}/{} guides by slug lookup",
                    found.len(),
                    expected_guides.len(),
                );

                Ok(())
            }
        }
    });

    let deprecate_guides_trial = async_trial(&rt, "deprecate_guides", {
        let base_url = base_url.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Create a guide specifically for deprecation testing
                let ep = GuideEndpoint {
                    base_url: base_url.clone(),
                };
                let created = ep.post_fake_object_set(1..2).await?;
                let guide = &created[0];

                // Deprecate the guide
                let deprecated = ep
                    .delete_object(guide.path_id, "deprecated", Some("Test deprecation"))
                    .await?;

                // Verify the deprecation was recorded
                assert_eq!(
                    deprecated.deprecation_level.as_deref(),
                    Some("deprecated"),
                    "deprecation level should be 'deprecated'"
                );

                // Verify GET returns the deprecated guide with deprecation info
                let fetched = ep.get_object(deprecated.id).await?;
                assert_eq!(
                    fetched.deprecation_level.as_deref(),
                    Some("deprecated"),
                    "fetched guide should have deprecation level"
                );

                log::info!("deprecate_guides trial completed successfully");
                Ok(())
            }
        }
    });

    // Deprecation test: create briefs, deprecate them, verify deprecation
    let deprecate_briefs_trial = async_trial(&rt, "deprecate_briefs", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                // Create a brief specifically for deprecation testing
                let ep = BriefEndpoint {
                    base_url: base_url.clone(),
                    context: scenario.clone(),
                };
                let created = ep.post_fake_object_set(1..2).await?;
                let brief = &created[0];

                // Deprecate the brief
                let deprecated = ep
                    .delete_object(brief.path_id, "deprecated", Some("Test deprecation"))
                    .await?;

                // Verify the deprecation was recorded
                assert_eq!(
                    deprecated.deprecation_level.as_deref(),
                    Some("deprecated"),
                    "deprecation level should be 'deprecated'"
                );

                // Verify GET returns the deprecated brief with deprecation info
                let fetched = ep.get_object(deprecated.id).await?;
                assert_eq!(
                    fetched.deprecation_level.as_deref(),
                    Some("deprecated"),
                    "fetched brief should have deprecation level"
                );

                log::info!("deprecate_briefs trial completed successfully");
                Ok(())
            }
        }
    });

    // Dispatch execution trial: creates a self-contained entity tree with a real
    // nushell task, dispatches via HTTP, then polls for background execution completion.
    let dispatch_execution_trial = async_trial(&rt, "dispatch_execution", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let client = reqwest::Client::new();
                let auth = test_auth_header();

                // 1. Create a task with nushell function and a simple script
                let task_body = serde_json::json!({
                    "objs": [{
                        "name": format!("dispatch-exec-task-{}", Uuid::now_v7()),
                        "description": "Task for dispatch execution test",
                        "function": "nushell",
                        "function_input": {
                            "script": "{ payload: { result: \"dispatch_executed\", items: [1, 2, 3] }, signals: { done: true } }"
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
                let status = resp.status();
                let body: serde_json::Value = resp.json().await?;
                assert!(
                    status.is_success(),
                    "create task failed: {} {:?}",
                    status,
                    body
                );
                let task_oid: Uuid = body["objs"][0]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("task id");

                // 2. Create a step referencing that task
                let step_body = serde_json::json!({
                    "objs": [{
                        "name": format!("dispatch-exec-step-{}", Uuid::now_v7()),
                        "description": "Step for dispatch execution test",
                        "tasks": [{ "task_id": task_oid }]
                    }]
                });

                let resp = client
                    .post(base_url.join("api/steps/").unwrap())
                    .header("authorization", &auth)
                    .json(&step_body)
                    .send()
                    .await?;
                let status = resp.status();
                let body: serde_json::Value = resp.json().await?;
                assert!(
                    status.is_success(),
                    "create step failed: {} {:?}",
                    status,
                    body
                );
                let step_oid: Uuid = body["objs"][0]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("step id");

                // 3. Get a category from earlier trials
                let category_oid = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.random_category_from_named_batch("categories-init")
                        .expect("should have categories")
                        .id
                };

                // 4. Create a playbook with that step + task
                let playbook_body = serde_json::json!({
                    "objs": [{
                        "name": format!("dispatch-exec-playbook-{}", Uuid::now_v7()),
                        "description": "Playbook for dispatch execution test",
                        "stages": [{
                            "steps": [{
                                "step_id": step_oid,
                                "params": {},
                                "tasks": [{ "task_id": task_oid }]
                            }]
                        }],
                        "categories": [category_oid]
                    }]
                });

                let resp = client
                    .post(base_url.join("api/playbooks/").unwrap())
                    .header("authorization", &auth)
                    .json(&playbook_body)
                    .send()
                    .await?;
                let status = resp.status();
                let body: serde_json::Value = resp.json().await?;
                assert!(
                    status.is_success(),
                    "create playbook failed: {} {:?}",
                    status,
                    body
                );
                let pb = &body["objs"][0];
                let pb_oid: Uuid = pb["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("playbook id");
                let pb_step_oid: Uuid = pb["stages"][0]["steps"][0]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("playbook step id");
                let pb_task_oid: Uuid = pb["stages"][0]["steps"][0]["tasks"][0]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("playbook task id");

                // 5. Create a journey from that playbook. This playbook is built
                //    via the REST API (not guide ingest), so no guide exposes it
                //    yet. Journey-POST now validates that the pinned guide
                //    exposes the playbook (400 GUIDE_PLAYBOOK_MISMATCH otherwise),
                //    so pin a registered guide and seed the guide_playbooks link
                //    first, exactly as the real ingest pipeline would establish it.
                let guide_oid = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.random_guide_from_any_batch()
                        .expect("should have guides")
                        .id
                };
                {
                    let pool = consumer::test_db_pool();
                    sqlx::query(
                        "INSERT INTO rev.guide_playbooks \
                             (guide_id, playbook_id, chapter_index) \
                         VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
                    )
                    .bind(guide_oid)
                    .bind(pb_oid)
                    .execute(pool.get_pool())
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("seed guide_playbooks: {e}")))?;
                }
                let journey_body = serde_json::json!({
                    "objs": [{
                        "name": format!("dispatch-exec-journey-{}", Uuid::now_v7()),
                        "playbook_id": pb_oid,
                        "guide_id": guide_oid,
                        "brief_pack_ids": [],
                        "inference_profile_ids": []
                    }]
                });

                let resp = client
                    .post(base_url.join("api/journeys/").unwrap())
                    .header("authorization", &auth)
                    .json(&journey_body)
                    .send()
                    .await?;
                let status = resp.status();
                let body: serde_json::Value = resp.json().await?;
                assert!(
                    status.is_success(),
                    "create journey failed: {} {:?}",
                    status,
                    body
                );
                let journey_oid: Uuid = body["objs"][0]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("journey id");

                // 6. Create a dispatch (triggers background execute_dispatch())
                // Steps are submitted directly; the backend derives stages from each step's
                // stage_step_id via stage_steps -> playbook_stages.
                let dispatch_body = serde_json::json!({
                    "objs": [{
                        "journey_id": journey_oid,
                        "intent": {},
                        "steps": [{
                            "stage_step_id": pb_step_oid,
                            "params": {},
                            "tasks": [{ "step_task_id": pb_task_oid }]
                        }]
                    }]
                });

                let resp = client
                    .post(base_url.join("api/dispatches/").unwrap())
                    .header("authorization", &auth)
                    .json(&dispatch_body)
                    .send()
                    .await?;
                let status = resp.status();
                let body: serde_json::Value = resp.json().await?;
                assert!(
                    status.is_success(),
                    "create dispatch failed: {} {:?}",
                    status,
                    body
                );
                let dispatch_oid: Uuid = body["objs"][0]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("dispatch id");

                // 7. Poll for task attempt completion (background execution)
                let attempts_url = base_url
                    .join(&format!("api/dispatches/{}/task_attempt/", dispatch_oid))
                    .unwrap();

                let mut found_ok = false;
                for attempt_num in 0..50 {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                    let resp = client
                        .get(attempts_url.clone())
                        .header("authorization", &auth)
                        .send()
                        .await?;

                    if !resp.status().is_success() {
                        continue;
                    }

                    let body: serde_json::Value = resp.json().await?;
                    if let Some(objs) = body.get("objs").and_then(|v| v.as_array()) {
                        for attempt in objs {
                            let status =
                                attempt.get("status").and_then(|s| s.as_str()).unwrap_or("");
                            if status == "ok" {
                                found_ok = true;
                                log::info!(
                                    "dispatch_execution: attempt {} completed with status 'ok' after {} polls",
                                    attempt.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                                    attempt_num + 1,
                                );
                                break;
                            }
                        }
                    }

                    if found_ok {
                        break;
                    }
                }

                assert!(
                    found_ok,
                    "dispatch {} should have a task attempt with status 'ok' within 10s",
                    dispatch_oid
                );

                Ok(())
            }
        }
    });

    // Computed-output completion (shared-core): ingest a guide whose single-step
    // stage exposes a computed `:::brief-output`, run the dispatch through the
    // REAL runner, and assert the step AND stage reach `done == true` once the
    // task produces the output. This is the internal twin of the external
    // `computed_output_reaches_done_after_dispatch` test — dapple-ui/dapple-suite
    // poll exactly these `/steps` + `/stages` `done` flags, so without the
    // runtime producer the journey would stall here, internally, before any
    // external consumer saw it. The stage exposes ONLY the computed slot (slot-a
    // is an unexposed brief input), so the dispatch is start-ready immediately
    // (computed outputs are excluded from the start gate) and auto-spawns on
    // create — no operator binding needed; `done` flips purely on production.
    let computed_output_reaches_done_trial = async_trial(
        &rt,
        "computed_output_reaches_done_after_dispatch",
        {
            let base_url = base_url.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    let client = reqwest::Client::new();
                    let auth = test_auth_header();
                    let suffix = Uuid::now_v7().to_string();
                    let short = suffix[28..].to_string();

                    // 1. Ingest the computed-output guide via the real HTTP surface.
                    let form = reqwest::multipart::Form::new().part(
                        "file",
                        reqwest::multipart::Part::bytes(computed_output_done_guide_tarball(
                            &suffix,
                        ))
                        .file_name("guide.tar.gz")
                        .mime_str("application/gzip")
                        .unwrap(),
                    );
                    let resp = client
                        .post(base_url.join("api/guides/ingest").unwrap())
                        .header("authorization", &auth)
                        .multipart(form)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(status.is_success(), "ingest failed: {status} {body}");
                    let playbook_id = body["derived_objects"]["playbook_ids"][0]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("playbook id");
                    let guide_id = body["guide"]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("guide id");

                    // 2. Resolve stage/step/task ids.
                    let pool = consumer::test_db_pool();
                    let sql = pool.get_pool();
                    let stage_id: Uuid = sqlx::query_scalar(
                        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY id LIMIT 1",
                    )
                    .bind(playbook_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("stage: {e}")))?;
                    let step_id: Uuid = sqlx::query_scalar(
                        "SELECT ss.id FROM rev.stage_steps ss \
                         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
                         WHERE ps.id = $1 ORDER BY ss.id LIMIT 1",
                    )
                    .bind(stage_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("step: {e}")))?;
                    let step_task_id: Uuid = sqlx::query_scalar(
                        "SELECT stk.id FROM rev.step_tasks stk \
                         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
                         WHERE ss.id = $1 ORDER BY stk.id LIMIT 1",
                    )
                    .bind(step_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("step_task: {e}")))?;

                    // 3. POST journey — empty brief_pack_ids auto-derives the
                    //    guide's pack (so the producer can resolve synth's version).
                    let journey_body = serde_json::json!({ "objs": [{
                        "name": format!("computed-done-journey-{short}"),
                        "playbook_id": playbook_id,
                        "guide_id": guide_id,
                        "brief_pack_ids": [],
                        "inference_profile_ids": []
                    }]});
                    let resp = client
                        .post(base_url.join("api/journeys/").unwrap())
                        .header("authorization", &auth)
                        .json(&journey_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create journey failed: {status} {body}"
                    );
                    let journey_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("journey id");

                    // 4. POST dispatch — start-ready (no exposed input), so it
                    //    auto-spawns execute_dispatch in the background.
                    let dispatch_body = serde_json::json!({ "objs": [{
                        "journey_id": journey_oid,
                        "intent": {},
                        "steps": [{
                            "stage_step_id": step_id,
                            "params": {},
                            "tasks": [{ "step_task_id": step_task_id }]
                        }]
                    }]});
                    let resp = client
                        .post(base_url.join("api/dispatches/").unwrap())
                        .header("authorization", &auth)
                        .json(&dispatch_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create dispatch failed: {status} {body}"
                    );
                    let dispatch_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("dispatch id");

                    // 5. Poll for task `ok` (background execution).
                    let attempts_url = base_url
                        .join(&format!("api/dispatches/{}/task_attempt/", dispatch_oid))
                        .unwrap();
                    let mut found_ok = false;
                    for _ in 0..50 {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        let resp = client
                            .get(attempts_url.clone())
                            .header("authorization", &auth)
                            .send()
                            .await?;
                        if !resp.status().is_success() {
                            continue;
                        }
                        let body: serde_json::Value = resp.json().await?;
                        if let Some(objs) = body.get("objs").and_then(|v| v.as_array())
                            && objs
                                .iter()
                                .any(|a| a.get("status").and_then(|s| s.as_str()) == Some("ok"))
                        {
                            found_ok = true;
                            break;
                        }
                    }
                    assert!(found_ok, "computed-output dispatch must reach task ok");

                    // 6. Assert the step AND stage report done == true (the exact
                    //    signal dapple-ui/dapple-suite poll).
                    let steps: serde_json::Value = client
                        .get(
                            base_url
                                .join(&format!("api/journeys/{}/steps", journey_oid))
                                .unwrap(),
                        )
                        .header("authorization", &auth)
                        .send()
                        .await?
                        .json()
                        .await?;
                    let step_done = steps["objs"].as_array().unwrap().iter().any(|o| {
                        o["stage_step_id"].as_str() == Some(step_id.to_string().as_str())
                            && o["done"].as_bool() == Some(true)
                    });
                    assert!(
                        step_done,
                        "step must be done after computed output produced: {steps}"
                    );

                    let stages: serde_json::Value = client
                        .get(
                            base_url
                                .join(&format!("api/journeys/{}/stages", journey_oid))
                                .unwrap(),
                        )
                        .header("authorization", &auth)
                        .send()
                        .await?
                        .json()
                        .await?;
                    let stage_done = stages["objs"].as_array().unwrap().iter().any(|o| {
                        o["playbook_stage_id"].as_str() == Some(stage_id.to_string().as_str())
                            && o["done"].as_bool() == Some(true)
                    });
                    assert!(
                        stage_done,
                        "stage must be done once its only step is done: {stages}"
                    );

                    Ok(())
                }
            }
        },
    );

    // A no-input step (exposes only a computed output) must AUTO-dispatch: the
    // runtime creates + runs it once its earlier stages are done, with no consumer
    // POSTing it. Two computed-output stages; the test POSTs ONLY stage A and
    // asserts stage B reaches done and carries the `auto_dispatch` intent marker.
    let auto_dispatch_pending_step_trial =
        async_trial(&rt, "auto_dispatch_pending_no_input_step", {
            let base_url = base_url.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    let client = reqwest::Client::new();
                    let auth = test_auth_header();
                    let suffix = Uuid::now_v7().to_string();
                    let short = suffix[28..].to_string();

                    // 1. Ingest the two-stage computed-output guide.
                    let form = reqwest::multipart::Form::new().part(
                        "file",
                        reqwest::multipart::Part::bytes(auto_dispatch_two_stage_guide_tarball(
                            &suffix, false,
                        ))
                        .file_name("guide.tar.gz")
                        .mime_str("application/gzip")
                        .unwrap(),
                    );
                    let resp = client
                        .post(base_url.join("api/guides/ingest").unwrap())
                        .header("authorization", &auth)
                        .multipart(form)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(status.is_success(), "ingest failed: {status} {body}");
                    let playbook_id = body["derived_objects"]["playbook_ids"][0]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("playbook id");
                    let guide_id = body["guide"]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("guide id");

                    // 2. Resolve stage A (first) + stage B (second) + their step ids.
                    let pool = consumer::test_db_pool();
                    let sql = pool.get_pool();
                    let stage_ids: Vec<Uuid> = sqlx::query_scalar(
                        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY ord",
                    )
                    .bind(playbook_id)
                    .fetch_all(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("stages: {e}")))?;
                    assert_eq!(stage_ids.len(), 2, "guide must have two stages");
                    let stage_a_id = stage_ids[0];
                    let stage_b_id = stage_ids[1];

                    let step_a_id: Uuid = sqlx::query_scalar(
                        "SELECT ss.id FROM rev.stage_steps ss \
                         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
                         WHERE ps.id = $1 ORDER BY ss.ord LIMIT 1",
                    )
                    .bind(stage_a_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("step_a: {e}")))?;
                    let step_b_id: Uuid = sqlx::query_scalar(
                        "SELECT ss.id FROM rev.stage_steps ss \
                         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
                         WHERE ps.id = $1 ORDER BY ss.ord LIMIT 1",
                    )
                    .bind(stage_b_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("step_b: {e}")))?;
                    let task_a_id: Uuid = sqlx::query_scalar(
                        "SELECT stk.id FROM rev.step_tasks stk \
                         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
                         WHERE ss.id = $1 ORDER BY stk.ord LIMIT 1",
                    )
                    .bind(step_a_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("task_a: {e}")))?;

                    // 3. Create the journey.
                    let journey_body = serde_json::json!({ "objs": [{
                        "name": format!("auto-dispatch-journey-{short}"),
                        "playbook_id": playbook_id,
                        "guide_id": guide_id,
                        "brief_pack_ids": [],
                        "inference_profile_ids": []
                    }]});
                    let resp = client
                        .post(base_url.join("api/journeys/").unwrap())
                        .header("authorization", &auth)
                        .json(&journey_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create journey failed: {status} {body}"
                    );
                    let journey_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("journey id");

                    // 4. POST a dispatch for STAGE A ONLY. Stage B is never POSTed —
                    //    if it runs, the runtime auto-dispatched it.
                    let dispatch_body = serde_json::json!({ "objs": [{
                        "journey_id": journey_oid,
                        "intent": {},
                        "steps": [{
                            "stage_step_id": step_a_id,
                            "params": {},
                            "tasks": [{ "step_task_id": task_a_id }]
                        }]
                    }]});
                    let resp = client
                        .post(base_url.join("api/dispatches/").unwrap())
                        .header("authorization", &auth)
                        .json(&dispatch_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create dispatch failed: {status} {body}"
                    );

                    // 5. Poll the per-stage rollup until STAGE B reports done — it can
                    //    only get there if its task ran, which only happens if the
                    //    runtime auto-created its dispatch after stage A finished.
                    let stages_url = base_url
                        .join(&format!("api/journeys/{}/stages", journey_oid))
                        .unwrap();
                    let mut stage_b_done = false;
                    for _ in 0..50 {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        let resp = client
                            .get(stages_url.clone())
                            .header("authorization", &auth)
                            .send()
                            .await?;
                        if !resp.status().is_success() {
                            continue;
                        }
                        let body: serde_json::Value = resp.json().await?;
                        if let Some(objs) = body.get("objs").and_then(|v| v.as_array())
                            && objs.iter().any(|o| {
                                o["playbook_stage_id"].as_str()
                                    == Some(stage_b_id.to_string().as_str())
                                    && o["done"].as_bool() == Some(true)
                            })
                        {
                            stage_b_done = true;
                            break;
                        }
                    }
                    assert!(
                        stage_b_done,
                        "stage B must reach done via auto-dispatch (never POSTed)"
                    );

                    // 6. Prove it was AUTO-created: a dispatch for stage B's step
                    //    carrying the `auto_dispatch` intent marker exists.
                    let auto_count: i64 = sqlx::query_scalar(
                        "SELECT count(*) FROM rev.dispatches d \
                         JOIN rev.dispatch_stages dst ON dst.dispatch_id = d.id \
                         JOIN rev.dispatch_steps ds ON ds.dispatch_stage_id = dst.id \
                         WHERE d.journey_id = $1 AND ds.stage_step_id = $2 \
                           AND d.intent ->> 'source' = 'auto_dispatch'",
                    )
                    .bind(journey_oid)
                    .bind(step_b_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("auto count: {e}")))?;
                    assert!(
                        auto_count >= 1,
                        "stage B's dispatch must be auto-created (intent source=auto_dispatch)"
                    );

                    Ok(())
                }
            }
        });

    // A picker-feed generate step (its computed output is a brief-input's
    // candidate_source) auto-dispatches like any other pure-compute step: the
    // RUNTIME owns computed production, which is what `/steps` promises when it
    // marks a slot `producer: "computed"`.
    //
    // This trial used to assert the opposite. Picker feeds were excluded on the
    // rationale that the consumer dispatches them on pane open, because two owners
    // double-ran a non-deterministic generate (two candidate arrays bound ~1s
    // apart, flapping the picker; seen live on run-the-10k "Lay Out the Business").
    // The flap was real, but its cause was two owners — and opting the runtime out
    // made every consumer responsible for advancing the journey. A consumer that
    // merely waits, exactly as documented, then stalls with no runtime-side
    // recovery: dapple-ui's only advancement path was a poll loop bounded by a 20s
    // deadline, so a 238s EDGAR pull returned before advancing and nothing created
    // the successor until the operator happened to submit again (live: 20 minutes
    // later, long after they gave up).
    // The fix is ONE owner (the runtime), enforced by asserting EXACTLY one
    // dispatch below — zero catches the exclusion coming back, two catches a
    // consumer self-dispatching again.
    //
    // Same two-stage guide, but stage B's output is a candidate_source; after
    // stage A completes, exactly one dispatch must exist for B.
    let auto_dispatch_skips_candidate_feed_trial =
        async_trial(&rt, "auto_dispatch_runs_candidate_feed_step", {
            let base_url = base_url.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    let client = reqwest::Client::new();
                    let auth = test_auth_header();
                    let suffix = Uuid::now_v7().to_string();
                    let short = suffix[28..].to_string();

                    // 1. Ingest the picker variant of the two-stage guide.
                    let form = reqwest::multipart::Form::new().part(
                        "file",
                        reqwest::multipart::Part::bytes(auto_dispatch_two_stage_guide_tarball(
                            &suffix, true,
                        ))
                        .file_name("guide.tar.gz")
                        .mime_str("application/gzip")
                        .unwrap(),
                    );
                    let resp = client
                        .post(base_url.join("api/guides/ingest").unwrap())
                        .header("authorization", &auth)
                        .multipart(form)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(status.is_success(), "ingest failed: {status} {body}");
                    let playbook_id = body["derived_objects"]["playbook_ids"][0]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("playbook id");
                    let guide_id = body["guide"]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("guide id");

                    // 2. Resolve stage/step/task ids.
                    let pool = consumer::test_db_pool();
                    let sql = pool.get_pool();
                    let stage_ids: Vec<Uuid> = sqlx::query_scalar(
                        "SELECT id FROM rev.playbook_stages WHERE playbook_id = $1 ORDER BY ord",
                    )
                    .bind(playbook_id)
                    .fetch_all(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("stages: {e}")))?;
                    assert_eq!(stage_ids.len(), 2, "guide must have two stages");
                    let stage_a_id = stage_ids[0];
                    let stage_b_id = stage_ids[1];

                    let step_a_id: Uuid = sqlx::query_scalar(
                        "SELECT ss.id FROM rev.stage_steps ss \
                         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
                         WHERE ps.id = $1 ORDER BY ss.ord LIMIT 1",
                    )
                    .bind(stage_a_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("step_a: {e}")))?;
                    let step_b_id: Uuid = sqlx::query_scalar(
                        "SELECT ss.id FROM rev.stage_steps ss \
                         JOIN rev.playbook_stages ps ON ps.stage_id = ss.stage_id \
                         WHERE ps.id = $1 ORDER BY ss.ord LIMIT 1",
                    )
                    .bind(stage_b_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("step_b: {e}")))?;
                    let task_a_id: Uuid = sqlx::query_scalar(
                        "SELECT stk.id FROM rev.step_tasks stk \
                         JOIN rev.stage_steps ss ON ss.step_id = stk.step_id \
                         WHERE ss.id = $1 ORDER BY stk.ord LIMIT 1",
                    )
                    .bind(step_a_id)
                    .fetch_one(sql)
                    .await
                    .map_err(|e| ConsumerError::Custom(format!("task_a: {e}")))?;

                    // 3. Create the journey.
                    let journey_body = serde_json::json!({ "objs": [{
                        "name": format!("candidate-feed-journey-{short}"),
                        "playbook_id": playbook_id,
                        "guide_id": guide_id,
                        "brief_pack_ids": [],
                        "inference_profile_ids": []
                    }]});
                    let resp = client
                        .post(base_url.join("api/journeys/").unwrap())
                        .header("authorization", &auth)
                        .json(&journey_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create journey failed: {status} {body}"
                    );
                    let journey_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("journey id");

                    // 4. POST a dispatch for STAGE A only.
                    let dispatch_body = serde_json::json!({ "objs": [{
                        "journey_id": journey_oid,
                        "intent": {},
                        "steps": [{
                            "stage_step_id": step_a_id,
                            "params": {},
                            "tasks": [{ "step_task_id": task_a_id }]
                        }]
                    }]});
                    let resp = client
                        .post(base_url.join("api/dispatches/").unwrap())
                        .header("authorization", &auth)
                        .json(&dispatch_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create dispatch failed: {status} {body}"
                    );

                    // 5. Wait for stage A to reach done (its dispatch ran + retrigger
                    //    drain swept the journey), polling the per-stage rollup.
                    let stages_url = base_url
                        .join(&format!("api/journeys/{}/stages", journey_oid))
                        .unwrap();
                    let mut stage_a_done = false;
                    for _ in 0..50 {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        let resp = client
                            .get(stages_url.clone())
                            .header("authorization", &auth)
                            .send()
                            .await?;
                        if !resp.status().is_success() {
                            continue;
                        }
                        let body: serde_json::Value = resp.json().await?;
                        if let Some(objs) = body.get("objs").and_then(|v| v.as_array())
                            && objs.iter().any(|o| {
                                o["playbook_stage_id"].as_str()
                                    == Some(stage_a_id.to_string().as_str())
                                    && o["done"].as_bool() == Some(true)
                            })
                        {
                            stage_a_done = true;
                            break;
                        }
                    }
                    assert!(
                        stage_a_done,
                        "stage A must complete via its POSTed dispatch"
                    );

                    // 6. Stage A is done, so the runtime must now create stage B's
                    //    dispatch by itself. Poll rather than a single sleep: the drain
                    //    runs on the finishing dispatch's retrigger and on every poll,
                    //    so the row lands promptly but not synchronously.
                    let mut b_dispatch_count: i64 = 0;
                    for _ in 0..20 {
                        b_dispatch_count = sqlx::query_scalar(
                            "SELECT count(*) FROM rev.dispatches d \
                             JOIN rev.dispatch_stages dst ON dst.dispatch_id = d.id \
                             JOIN rev.dispatch_steps ds ON ds.dispatch_stage_id = dst.id \
                             WHERE d.journey_id = $1 AND ds.stage_step_id = $2",
                        )
                        .bind(journey_oid)
                        .bind(step_b_id)
                        .fetch_one(sql)
                        .await
                        .map_err(|e| ConsumerError::Custom(format!("b count: {e}")))?;
                        if b_dispatch_count > 0 {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                    assert_eq!(
                        b_dispatch_count, 1,
                        "picker-feed step B must be auto-dispatched EXACTLY ONCE by the \
                         runtime. Zero means the old consumer-driven exclusion is back, \
                         which dead-ends any consumer that merely waits — the documented \
                         behaviour for a `producer: \"computed\"` slot (seen live: \
                         run-the-10k stalled at stage 1 on a 238s pull). More than one \
                         means two owners are creating dispatches again, which is what \
                         flapped the candidate feed (two non-deterministic generates \
                         binding two different arrays to one slot)"
                    );

                    Ok(())
                }
            }
        });

    // =========================================================================
    // Phase 5: pgvector search integration trials
    // =========================================================================

    // Tier 1: verify_search_endpoint_validation (Level 0, no deps)
    let verify_search_endpoint_validation_trial =
        async_trial(&rt, "verify_search_endpoint_validation", {
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
                        "Empty search query should return 400 Bad Request, got {}",
                        status
                    );

                    // If conduit is not configured, search should return 503
                    if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                        let status = search_ep.search_status("test query").await?;
                        assert_eq!(
                            status, 503,
                            "Search without DAPPLE_CONDUIT_URL should return 503, got {}",
                            status
                        );
                    }

                    log::info!("verify_search_endpoint_validation: passed");
                    Ok(())
                }
            }
        });

    // Tier 0: verify_guides_by_classification (self-contained, no dependencies)
    // Uses a unique per-run classification to avoid flaky failures from
    // accumulated "workflow" guides in the shared test database.
    let verify_guides_by_classification_trial = async_trial(
        &rt,
        "verify_guides_by_classification",
        {
            let base_url = base_url.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    let guides_ep = GuideEndpoint {
                        base_url: base_url.clone(),
                    };

                    // Create guides with a unique classification for this run
                    let run_classification = format!("test-{}", Uuid::now_v7());
                    let mut schema = guides_ep.fake_create_set_schema(2..3);
                    for obj in &mut schema.objs {
                        obj.classification = Some(run_classification.clone());
                    }
                    let created = guides_ep.post_object_set(schema).await?;
                    let expected_ids: Vec<Uuid> = created.iter().map(|g| g.id).collect();

                    // Query by the unique classification
                    let found = guides_ep
                        .get_object_set(GetGuideSetSchema::ByClassification {
                            classification: run_classification.clone(),
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
                    for expected_id in &expected_ids {
                        assert!(
                            found_ids.contains(expected_id),
                            "Guide {} should be in ByClassification('{}') results",
                            expected_id,
                            run_classification,
                        );
                    }

                    // Query by a classification that no guide has
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
                        "verify_guides_by_classification: found {}/{} '{}' guides, 0 nonexistent",
                        expected_ids.len(),
                        found.len(),
                        run_classification,
                    );

                    Ok(())
                }
            }
        },
    );

    // Tier 1: verify_embedding_insert_delete (Level 1, depends on guides)
    let verify_embedding_insert_delete_trial = async_trial(
        &rt,
        "verify_embedding_insert_delete",
        {
            let scenario = scenario.clone();
            move || async move {
                // Get a guide from the scenario context
                let guide = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.random_guide_from_named_batch("guides-init")
                        .expect("should have guides")
                };

                // Connect to the database directly
                let pool = consumer::test_db_pool();

                // Get a chapter and block from the guide
                let chapter = guide.chapters.first().expect("guide should have chapters");
                let block = chapter.blocks.first().expect("chapter should have blocks");

                // Look up the directive type ID for "prose"
                let directive_types =
                    dapple_data::query::guides::load_directive_type_lookup_pool(&pool)
                        .await
                        .expect("load directive types");
                let prose_dt_id = *directive_types.get("prose").expect("prose directive type");

                // Create an embedding generation for append-only versioning
                let generation =
                    dapple_data::query::guide_content_embeddings::insert_guide_embedding_generation(
                        &pool, guide.id,
                    )
                    .await
                    .expect("insert embedding generation");

                // Insert a test embedding (768-dim vector)
                let fake_embedding = vec![0.1_f32; 768];
                let args =
                    dapple_data::model::guide_content_embeddings::InsertGuideContentEmbeddingArgs {
                        guide_ids: vec![guide.id],
                        chapter_ids: vec![chapter.id],
                        block_ids: vec![block.id],
                        directive_type_ids: vec![prose_dt_id],
                        chunk_texts: vec!["Test embedding content".to_string()],
                        embeddings: vec![fake_embedding],
                        metadata: vec![serde_json::json!({"test": true})],
                        generation_ids: vec![generation.id],
                    };

                let inserted =
                    dapple_data::query::guide_content_embeddings::insert_guide_content_embeddings(
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

                let tag_args =
                    dapple_data::model::guide_content_embeddings::InsertGuideEmbeddingTagArgs {
                        embedding_ids: vec![emb_id],
                        tag_ids: vec![universal_tag_id],
                    };
                let tag_inserted =
                    dapple_data::query::guide_content_embeddings::insert_guide_embedding_tags(
                        &pool, tag_args,
                    )
                    .await
                    .expect("insert embedding tags");
                assert_eq!(tag_inserted.len(), 1, "should insert one tag association");

                // Insert locale associations
                let locale_rows =
                    dapple_data::query::fluent_messages::select_fluent_message_locale_ids(
                        &pool,
                        &["en".to_string()],
                    )
                    .await
                    .expect("select locale ids");

                if !locale_rows.is_empty() {
                    let locale_args =
                        dapple_data::model::guide_content_embeddings::InsertGuideEmbeddingLocaleArgs {
                            embedding_ids: vec![emb_id],
                            locale_ids: vec![locale_rows[0].id],
                        };
                    let locale_inserted =
                        dapple_data::query::guide_content_embeddings::insert_guide_embedding_locales(
                            &pool,
                            locale_args,
                        )
                        .await
                        .expect("insert embedding locales");
                    assert_eq!(
                        locale_inserted.len(),
                        1,
                        "should insert one locale association"
                    );
                }

                // Verify count reflects latest generation
                let count = dapple_data::query::guide_content_embeddings::count_guide_embeddings(
                    &pool, guide.id,
                )
                .await
                .expect("count embeddings");
                assert_eq!(count, 1, "should have 1 embedding in latest generation");

                // Create a new generation (simulates re-embedding)
                let gen2 =
                    dapple_data::query::guide_content_embeddings::insert_guide_embedding_generation(
                        &pool, guide.id,
                    )
                    .await
                    .expect("insert second generation");
                assert_ne!(generation.id, gen2.id, "generations should differ");

                // New generation has no embeddings — count should be 0
                let count_after =
                    dapple_data::query::guide_content_embeddings::count_guide_embeddings(
                        &pool, guide.id,
                    )
                    .await
                    .expect("count embeddings after new generation");
                assert_eq!(count_after, 0, "new generation should have 0 embeddings");

                log::info!(
                    "verify_embedding_generations: inserted {} embedding(s) for guide {}, verified generation filtering",
                    inserted.len(),
                    guide.id
                );

                Ok(())
            }
        },
    );

    // Tier 2: verify_guide_embeddings_generated (Level 2, depends on guides)
    let verify_guide_embeddings_generated_trial = async_trial(
        &rt,
        "verify_guide_embeddings_generated",
        {
            let base_url = base_url.clone();
            let scenario = scenario.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    // Skip if DAPPLE_CONDUIT_URL is not set
                    if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                        log::info!(
                            "verify_guide_embeddings_generated: skipping (DAPPLE_CONDUIT_URL not set)"
                        );
                        return Ok(());
                    }

                    // Create a fresh guide specifically for embedding testing
                    let guides_ep = GuideEndpoint {
                        base_url: base_url.clone(),
                    };
                    let created = guides_ep.post_fake_object_set(1..2).await?;
                    let guide = &created[0];

                    // Register for downstream trials
                    {
                        let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        ctx.register_guide_batch("guides-with-embeddings", created.clone());
                    }

                    // Poll for embeddings to appear (async generation via tokio::spawn)
                    let pool = consumer::test_db_pool();
                    let mut embedding_count = 0i64;

                    for attempt in 0..30 {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                        let row = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1",
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
                         WHERE e.guide_id = $1",
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
                         WHERE e.guide_id = $1",
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

                    // Verify every embeddable block is represented.
                    // The test guide has 3 chapters x 2 prose blocks = 6 embeddable blocks
                    let expected_blocks = guide
                        .chapters
                        .iter()
                        .flat_map(|ch| ch.blocks.iter())
                        .filter(|b| {
                            b.directive_type == "prose"
                                && b.content.is_some()
                                && !b.content.as_ref().unwrap().trim().is_empty()
                        })
                        .count() as i64;

                    // Assert on DISTINCT blocks, not row count. A block longer
                    // than EMBED_WINDOW_CHARS (dapple-prompt-api
                    // service::text_window) is deliberately stored as SEVERAL
                    // rows so its tail stays retrievable, so `COUNT(*)` is not
                    // the block count in general. It happens to be here — these
                    // fixture blocks are tiny — but asserting equality on it
                    // would fail confusingly the day someone grows the fixture,
                    // pointing at embeddings rather than at the window size.
                    let distinct_block_count = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(DISTINCT block_id) FROM rev.guide_content_embeddings \
                         WHERE guide_id = $1",
                    )
                    .bind(guide.id)
                    .fetch_one(pool.get_pool())
                    .await
                    .expect("count distinct embedded blocks");

                    assert_eq!(
                        distinct_block_count, expected_blocks,
                        "Embedded block count ({}) should match embeddable block count ({})",
                        distinct_block_count, expected_blocks
                    );
                    assert!(
                        embedding_count >= expected_blocks,
                        "Embedding rows ({}) must be at least one per embeddable block ({})",
                        embedding_count,
                        expected_blocks
                    );

                    log::info!(
                        "verify_guide_embeddings_generated: guide {} has {} embeddings, {} tags, {} locales",
                        guide.id,
                        embedding_count,
                        tag_count,
                        locale_count
                    );

                    Ok(())
                }
            }
        },
    );

    // Tier 2: verify_guide_search_round_trip (Level 5, after embeddings verified)
    let verify_guide_search_round_trip_trial = async_trial(
        &rt,
        "verify_guide_search_round_trip",
        {
            let base_url = base_url.clone();
            let scenario = scenario.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                        log::info!(
                            "verify_guide_search_round_trip: skipping (DAPPLE_CONDUIT_URL not set)"
                        );
                        return Ok(());
                    }

                    // Get the guide that was created with embeddings
                    let guide = {
                        let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        match ctx.guide_named_batches.get("guides-with-embeddings") {
                            Some(handle) => ctx.guide_batches[handle.index][0].clone(),
                            None => {
                                log::info!(
                                    "verify_guide_search_round_trip: skipping (no embedded guide)"
                                );
                                return Ok(());
                            }
                        }
                    };

                    // Verify embeddings exist before searching
                    let pool = consumer::test_db_pool();
                    let emb_count = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1",
                    )
                    .bind(guide.id)
                    .fetch_one(pool.get_pool())
                    .await
                    .expect("count embeddings");

                    if emb_count == 0 {
                        log::warn!(
                            "verify_guide_search_round_trip: guide {} has no embeddings, skipping",
                            guide.id
                        );
                        return Ok(());
                    }

                    let search_ep = GuideSearchEndpoint {
                        base_url: base_url.clone(),
                    };

                    // Extract a text snippet from the guide's first block as search query
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

                    log::info!(
                        "verify_guide_search_round_trip: all assertions passed ({} default, {} scoped, {} threshold, {} limited)",
                        results.len(),
                        scoped_results.len(),
                        threshold_results.len(),
                        limited_results.len()
                    );

                    Ok(())
                }
            }
        },
    );

    // Tier 2: verify_guide_reingest_replaces_embeddings (Level 5)
    let verify_guide_reingest_replaces_embeddings_trial = async_trial(
        &rt,
        "verify_guide_reingest_replaces_embeddings",
        {
            let base_url = base_url.clone();
            let scenario = scenario.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                        log::info!(
                            "verify_guide_reingest_replaces_embeddings: skipping (DAPPLE_CONDUIT_URL not set)"
                        );
                        return Ok(());
                    }

                    // Get the guide created for embedding testing
                    let guide = {
                        let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        match ctx.guide_named_batches.get("guides-with-embeddings") {
                            Some(handle) => ctx.guide_batches[handle.index][0].clone(),
                            None => {
                                log::info!(
                                    "verify_guide_reingest_replaces_embeddings: skipping (no embedded guide)"
                                );
                                return Ok(());
                            }
                        }
                    };

                    let pool = consumer::test_db_pool();

                    // Record the current embedding IDs
                    let old_embedding_ids: Vec<Uuid> = sqlx::query_scalar(
                        "SELECT id FROM rev.guide_content_embeddings WHERE guide_id = $1 ORDER BY id",
                    )
                    .bind(guide.id)
                    .fetch_all(pool.get_pool())
                    .await
                    .expect("fetch old embedding ids");

                    assert!(
                        !old_embedding_ids.is_empty(),
                        "Guide should have existing embeddings before re-ingest"
                    );

                    // Re-ingest: PATCH guide via the same path_id to create a new version
                    let guides_ep = GuideEndpoint {
                        base_url: base_url.clone(),
                    };
                    let new_guide_schema = guides_ep.fake_create_schema();

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

                    let client = reqwest::Client::new();
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

                    // Poll for new embeddings to appear on the new version
                    let mut new_embedding_count = 0i64;
                    for attempt in 0..30 {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                        new_embedding_count = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1",
                        )
                        .bind(new_guide_id)
                        .fetch_one(pool.get_pool())
                        .await
                        .expect("count new embeddings");

                        if new_embedding_count > 0 {
                            log::info!(
                                "verify_guide_reingest_replaces_embeddings: new guide {} has {} embeddings after {} polls",
                                new_guide_id,
                                new_embedding_count,
                                attempt + 1
                            );
                            break;
                        }
                    }

                    assert!(
                        new_embedding_count > 0,
                        "New guide version {} should have embeddings within 15s",
                        new_guide_id
                    );

                    log::info!(
                        "verify_guide_reingest_replaces_embeddings: old guide {} had {} embeddings, new guide {} has {}",
                        guide.id,
                        old_embedding_ids.len(),
                        new_guide_id,
                        new_embedding_count
                    );

                    Ok(())
                }
            }
        },
    );

    // Tier 2: verify_full_search_pipeline (Level 7)
    let verify_full_search_pipeline_trial = async_trial(&rt, "verify_full_search_pipeline", {
        let base_url = base_url.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                    log::info!(
                        "verify_full_search_pipeline: skipping (DAPPLE_CONDUIT_URL not set)"
                    );
                    return Ok(());
                }

                let client = reqwest::Client::new();
                let auth = test_auth_header();

                // 1. Create a guide with a unique nonsense term in content
                let unique_term = format!(
                    "xyzzy{}",
                    Uuid::now_v7()
                        .to_string()
                        .replace('-', "")
                        .chars()
                        .take(8)
                        .collect::<String>()
                );

                let guide_body = serde_json::json!({
                    "objs": [{
                        "title": format!("Search Pipeline Test Guide {}", unique_term),
                        "description": format!("A guide about {} for testing the full search pipeline", unique_term),
                        "author": "Test Author",
                        "classification": "workflow",
                        "source_path": format!("docs/books/search-test-{}", unique_term),
                        "chapters": [{
                            "title": "Introduction",
                            "source_file": "intro.md",
                            "parent_index": null,
                            "blocks": [{
                                "directive_type": "prose",
                                "name": null,
                                "content": format!(
                                    "This guide explains {} in detail. It covers the fundamentals of {} and provides practical examples.",
                                    unique_term, unique_term
                                ),
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
                assert!(
                    resp.status().is_success(),
                    "create guide failed: {}",
                    resp.status()
                );
                let body: serde_json::Value = resp.json().await?;
                let guide_id: Uuid = body["objs"][0]["id"]
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .expect("guide id");

                // 2. Wait for embeddings
                let pool = consumer::test_db_pool();
                let mut has_embeddings = false;
                for _ in 0..30 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let count = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM rev.guide_content_embeddings WHERE guide_id = $1",
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

                assert!(
                    has_embeddings,
                    "Guide {} should have embeddings within 15s",
                    guide_id
                );

                // 3. Search using the unique term
                let search_ep = GuideSearchEndpoint {
                    base_url: base_url.clone(),
                };

                let results = search_ep
                    .search(
                        &unique_term,
                        Some(&["universal"]),
                        Some("en"),
                        Some(5),
                        None,
                        None,
                    )
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

    // Tier 2: verify_rag_graceful_degradation (Level 7)
    let verify_rag_graceful_degradation_trial = async_trial(
        &rt,
        "verify_rag_graceful_degradation",
        {
            let base_url = base_url.clone();
            let scenario = scenario.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    if std::env::var("DAPPLE_CONDUIT_URL").is_err() {
                        log::info!(
                            "verify_rag_graceful_degradation: skipping (DAPPLE_CONDUIT_URL not set)"
                        );
                        return Ok(());
                    }

                    let client = reqwest::Client::new();
                    let auth = test_auth_header();

                    // 1. Create a task with rag_guide_ids pointing to a non-existent guide
                    let nonexistent_guide_id = Uuid::now_v7();
                    let task_body = serde_json::json!({
                        "objs": [{
                            "name": format!("rag-degrade-task-{}", Uuid::now_v7()),
                            "description": "Task for RAG degradation test",
                            "function": "nushell",
                            "function_input": {
                                "script": "{ payload: { result: 'rag_degradation_ok' }, signals: { done: true } }",
                                "rag_guide_ids": [nonexistent_guide_id.to_string()]
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
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create task failed: {} {:?}",
                        status,
                        body
                    );
                    let task_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("task id");

                    // 2. Create step
                    let step_body = serde_json::json!({
                        "objs": [{
                            "name": format!("rag-degrade-step-{}", Uuid::now_v7()),
                            "description": "Step for RAG degradation test",
                            "tasks": [{ "task_id": task_oid }]
                        }]
                    });
                    let resp = client
                        .post(base_url.join("api/steps/").unwrap())
                        .header("authorization", &auth)
                        .json(&step_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create step failed: {} {:?}",
                        status,
                        body
                    );
                    let step_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("step id");

                    // 3. Get a category from earlier trials
                    let category_oid = {
                        let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        ctx.random_category_from_named_batch("categories-init")
                            .expect("should have categories")
                            .id
                    };

                    // 4. Create playbook
                    let playbook_body = serde_json::json!({
                        "objs": [{
                            "name": format!("rag-degrade-playbook-{}", Uuid::now_v7()),
                            "description": "Playbook for RAG degradation test",
                            "stages": [{
                                "steps": [{
                                    "step_id": step_oid,
                                    "params": {},
                                    "tasks": [{ "task_id": task_oid }]
                                }]
                            }],
                            "categories": [category_oid]
                        }]
                    });
                    let resp = client
                        .post(base_url.join("api/playbooks/").unwrap())
                        .header("authorization", &auth)
                        .json(&playbook_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create playbook failed: {} {:?}",
                        status,
                        body
                    );
                    let pb = &body["objs"][0];
                    let pb_oid: Uuid = pb["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("playbook id");
                    let pb_step_oid: Uuid = pb["stages"][0]["steps"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("playbook step id");
                    let pb_task_oid: Uuid = pb["stages"][0]["steps"][0]["tasks"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("playbook task id");

                    // 5. Create journey. The playbook is REST-built so no guide
                    //    exposes it; journey-POST now validates the pin exposes
                    //    the playbook, so pin a registered guide and seed the
                    //    guide_playbooks link first.
                    let guide_oid = {
                        let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        ctx.random_guide_from_any_batch()
                            .expect("should have guides")
                            .id
                    };
                    {
                        let pool = consumer::test_db_pool();
                        sqlx::query(
                            "INSERT INTO rev.guide_playbooks \
                                 (guide_id, playbook_id, chapter_index) \
                             VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
                        )
                        .bind(guide_oid)
                        .bind(pb_oid)
                        .execute(pool.get_pool())
                        .await
                        .map_err(|e| ConsumerError::Custom(format!("seed guide_playbooks: {e}")))?;
                    }
                    let journey_body = serde_json::json!({
                        "objs": [{
                            "name": format!("rag-degrade-journey-{}", Uuid::now_v7()),
                            "playbook_id": pb_oid,
                            "guide_id": guide_oid,
                            "brief_pack_ids": [],
                            "inference_profile_ids": []
                        }]
                    });
                    let resp = client
                        .post(base_url.join("api/journeys/").unwrap())
                        .header("authorization", &auth)
                        .json(&journey_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create journey failed: {} {:?}",
                        status,
                        body
                    );
                    let journey_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("journey id");

                    // 6. Create dispatch (triggers background execution)
                    let dispatch_body = serde_json::json!({
                        "objs": [{
                            "journey_id": journey_oid,
                            "intent": {},
                            "steps": [{
                                "stage_step_id": pb_step_oid,
                                "params": {},
                                "tasks": [{ "step_task_id": pb_task_oid }]
                            }]
                        }]
                    });
                    let resp = client
                        .post(base_url.join("api/dispatches/").unwrap())
                        .header("authorization", &auth)
                        .json(&dispatch_body)
                        .send()
                        .await?;
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().await?;
                    assert!(
                        status.is_success(),
                        "create dispatch failed: {} {:?}",
                        status,
                        body
                    );
                    let dispatch_oid: Uuid = body["objs"][0]["id"]
                        .as_str()
                        .and_then(|s| Uuid::parse_str(s).ok())
                        .expect("dispatch id");

                    // 7. Poll for task attempt completion
                    let attempts_url = base_url
                        .join(&format!("api/dispatches/{}/task_attempt/", dispatch_oid))
                        .unwrap();

                    let mut found_ok = false;
                    for attempt_num in 0..50 {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                        let resp = client
                            .get(attempts_url.clone())
                            .header("authorization", &auth)
                            .send()
                            .await?;

                        if !resp.status().is_success() {
                            continue;
                        }

                        let body: serde_json::Value = resp.json().await?;
                        if let Some(objs) = body.get("objs").and_then(|v| v.as_array()) {
                            for attempt in objs {
                                let status =
                                    attempt.get("status").and_then(|s| s.as_str()).unwrap_or("");
                                if status == "ok" {
                                    found_ok = true;
                                    log::info!(
                                        "verify_rag_graceful_degradation: attempt completed with 'ok' after {} polls",
                                        attempt_num + 1
                                    );
                                    break;
                                }
                            }
                        }

                        if found_ok {
                            break;
                        }
                    }

                    assert!(
                        found_ok,
                        "Dispatch {} should complete successfully despite non-existent RAG guide {}",
                        dispatch_oid, nonexistent_guide_id
                    );

                    log::info!(
                        "verify_rag_graceful_degradation: dispatch completed despite non-existent RAG guide"
                    );

                    Ok(())
                }
            }
        },
    );

    // -----------------------------------------------------------------
    // Admin: B2B partner (companies) + external service tokens lifecycle
    // -----------------------------------------------------------------
    //
    // Create company → list → fetch by id (empty tokens) → register token →
    // fetch by id (one token) → revoke token → fetch by id (revoked_at
    // populated) → revoke company. Also exercises 403 for non-admin and
    // the constraint-violation paths (duplicate slug → 409, duplicate
    // service_id → 409, invalid PEM → 400).

    let companies_create_trial = async_trial(&rt, "create_company", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                use dapple_prompt_api::schema::companies::CreateCompanySchema;

                // Use a unique slug per test run so the trial is rerunnable
                // without dropping the dev DB.
                let slug_suffix = Uuid::now_v7().to_string().split_at(13).0.replace('-', "");
                let slug = format!("c{}", slug_suffix);

                let created = companies_consumer::create_company(
                    &base_url,
                    CreateCompanySchema {
                        slug: slug.clone(),
                        name: "Acme Test".to_string(),
                    },
                )
                .await?;

                assert_eq!(created.status, "success");
                assert_eq!(created.slug, slug);

                // Listing should include the new company.
                let list = companies_consumer::list_companies(&base_url, false).await?;
                assert!(
                    list.objs.iter().any(|c| c.id == created.id),
                    "newly-created company not in list"
                );

                // Fetch by id — no tokens yet.
                let detail = companies_consumer::get_company(&base_url, created.id).await?;
                assert_eq!(detail.id, created.id);
                assert!(detail.external_service_tokens.is_empty());
                assert!(detail.revoked_at.is_none());

                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_company_batch(
                        "companies-init",
                        vec![dapple_prompt_api::schema::companies::CompanyView {
                            id: created.id,
                            slug: created.slug,
                            name: created.name,
                            created_at: created.created_at,
                            revoked_at: None,
                        }],
                    );
                }

                Ok(())
            }
        }
    });

    let companies_non_admin_403_trial = async_trial(&rt, "create_company_non_admin_403", {
        let base_url = base_url.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                use dapple_prompt_api::schema::companies::CreateCompanySchema;

                let status = companies_consumer::create_company_as_non_admin(
                    &base_url,
                    CreateCompanySchema {
                        slug: "non-admin-slug".to_string(),
                        name: "should never land".to_string(),
                    },
                )
                .await?;
                assert_eq!(
                    status.as_u16(),
                    403,
                    "non-admin POST /api/companies/ should be 403, got {}",
                    status
                );
                Ok(())
            }
        }
    });

    let companies_dup_slug_trial = async_trial(&rt, "create_company_dup_slug_409", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                use dapple_prompt_api::schema::companies::CreateCompanySchema;

                let existing_slug = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let handle = ctx
                        .companies_named_batches
                        .get("companies-init")
                        .expect("companies-init not registered");
                    ctx.companies_batches[handle.index][0].slug.clone()
                };

                // Re-using the same slug should hit the UNIQUE constraint.
                let url = base_url.join("api/companies/").unwrap();
                let resp = reqwest::Client::new()
                    .post(url.clone())
                    .header("Authorization", crate::consumer::test_admin_auth_header())
                    .json(&CreateCompanySchema {
                        slug: existing_slug,
                        name: "Duplicate".to_string(),
                    })
                    .send()
                    .await?;
                let status = resp.status();
                let text = resp.text().await?;
                assert_eq!(
                    status.as_u16(),
                    409,
                    "duplicate slug should be 409, got {}: {}",
                    status,
                    text
                );

                Ok(())
            }
        }
    });

    let external_service_tokens_create_trial = async_trial(&rt, "create_external_service_token", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                use dapple_prompt_api::schema::external_service_tokens::CreateExternalServiceTokenSchema;

                let company_id = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let handle = ctx
                        .companies_named_batches
                        .get("companies-init")
                        .expect("companies-init not registered");
                    ctx.companies_batches[handle.index][0].id
                };

                let service_id_suffix = Uuid::now_v7().to_string().split_at(13).0.replace('-', "");
                let service_id = format!("svc_{}", service_id_suffix);

                let resp = external_service_tokens_consumer::create_token(
                    &base_url,
                    company_id,
                    CreateExternalServiceTokenSchema {
                        service_id: service_id.clone(),
                        name: "Production credential".to_string(),
                        description: Some("Smoke test".to_string()),
                        public_key_pem: external_service_tokens_consumer::SAMPLE_PUBLIC_KEY_PEM_A
                            .to_string(),
                    },
                )
                .await?;

                assert_eq!(resp.service_id, service_id);
                assert_eq!(resp.public_key_pem_sha256.len(), 64);

                // Fetch the company detail; should now report one token.
                let detail = companies_consumer::get_company(&base_url, company_id).await?;
                assert_eq!(detail.external_service_tokens.len(), 1);
                let view = &detail.external_service_tokens[0];
                assert_eq!(view.id, resp.id);
                assert!(view.revoked_at.is_none());
                assert_eq!(view.principal_id, resp.principal_id);

                // List via the dedicated endpoint.
                let list =
                    external_service_tokens_consumer::list_tokens(&base_url, company_id).await?;
                assert_eq!(list.results, 1);

                {
                    let mut ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    ctx.register_external_service_token_batch(
                        "external-service-tokens-init",
                        vec![view.clone()],
                    );
                }

                Ok(())
            }
        }
    });

    let external_service_token_dup_id_trial = async_trial(
        &rt,
        "create_external_service_token_dup_409",
        {
            let base_url = base_url.clone();
            let scenario = scenario.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    use dapple_prompt_api::schema::external_service_tokens::CreateExternalServiceTokenSchema;

                    let (company_id, existing_service_id) = {
                        let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        let c_handle = ctx
                            .companies_named_batches
                            .get("companies-init")
                            .expect("companies-init not registered");
                        let t_handle = ctx
                            .external_service_tokens_named_batches
                            .get("external-service-tokens-init")
                            .expect("external-service-tokens-init not registered");
                        (
                            ctx.companies_batches[c_handle.index][0].id,
                            ctx.external_service_tokens_batches[t_handle.index][0]
                                .service_id
                                .clone(),
                        )
                    };

                    let (status, body) = external_service_tokens_consumer::create_token_status(
                        &base_url,
                        company_id,
                        CreateExternalServiceTokenSchema {
                            service_id: existing_service_id,
                            name: "Duplicate id".to_string(),
                            description: None,
                            public_key_pem:
                                external_service_tokens_consumer::SAMPLE_PUBLIC_KEY_PEM_B
                                    .to_string(),
                        },
                        true,
                    )
                    .await?;
                    assert_eq!(
                        status.as_u16(),
                        409,
                        "duplicate service_id should be 409, got {}: {}",
                        status,
                        body
                    );
                    Ok(())
                }
            }
        },
    );

    let external_service_token_invalid_pem_trial = async_trial(
        &rt,
        "create_external_service_token_invalid_pem_400",
        {
            let base_url = base_url.clone();
            let scenario = scenario.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    use dapple_prompt_api::schema::external_service_tokens::CreateExternalServiceTokenSchema;

                    let company_id = {
                        let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        let handle = ctx
                            .companies_named_batches
                            .get("companies-init")
                            .expect("companies-init not registered");
                        ctx.companies_batches[handle.index][0].id
                    };

                    let service_id_suffix =
                        Uuid::now_v7().to_string().split_at(13).0.replace('-', "");
                    let service_id = format!("badpem_{}", service_id_suffix);

                    let (status, body) = external_service_tokens_consumer::create_token_status(
                        &base_url,
                        company_id,
                        CreateExternalServiceTokenSchema {
                            service_id,
                            name: "Invalid PEM".to_string(),
                            description: None,
                            public_key_pem: "not a real PEM".to_string(),
                        },
                        true,
                    )
                    .await?;
                    assert_eq!(
                        status.as_u16(),
                        400,
                        "invalid PEM should be 400, got {}: {}",
                        status,
                        body
                    );
                    Ok(())
                }
            }
        },
    );

    let external_service_token_non_admin_403_trial = async_trial(
        &rt,
        "create_external_service_token_non_admin_403",
        {
            let base_url = base_url.clone();
            let scenario = scenario.clone();
            move || {
                let base_url = base_url.clone();
                async move {
                    use dapple_prompt_api::schema::external_service_tokens::CreateExternalServiceTokenSchema;

                    let company_id = {
                        let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                        let handle = ctx
                            .companies_named_batches
                            .get("companies-init")
                            .expect("companies-init not registered");
                        ctx.companies_batches[handle.index][0].id
                    };

                    let (status, _) = external_service_tokens_consumer::create_token_status(
                        &base_url,
                        company_id,
                        CreateExternalServiceTokenSchema {
                            service_id: "should_not_land".to_string(),
                            name: "Non-admin attempt".to_string(),
                            description: None,
                            public_key_pem:
                                external_service_tokens_consumer::SAMPLE_PUBLIC_KEY_PEM_A
                                    .to_string(),
                        },
                        false,
                    )
                    .await?;
                    assert_eq!(status.as_u16(), 403, "non-admin should be 403");
                    Ok(())
                }
            }
        },
    );

    let external_service_token_revoke_trial = async_trial(&rt, "revoke_external_service_token", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let (company_id, token_id) = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let c_handle = ctx
                        .companies_named_batches
                        .get("companies-init")
                        .expect("companies-init not registered");
                    let t_handle = ctx
                        .external_service_tokens_named_batches
                        .get("external-service-tokens-init")
                        .expect("external-service-tokens-init not registered");
                    (
                        ctx.companies_batches[c_handle.index][0].id,
                        ctx.external_service_tokens_batches[t_handle.index][0].id,
                    )
                };

                // A non-admin caller must not be able to revoke the token. Assert
                // this BEFORE the admin revoke, while the token is still active.
                let non_admin_status = external_service_tokens_consumer::revoke_token_status(
                    &base_url, token_id, false,
                )
                .await?;
                assert!(
                    matches!(non_admin_status.as_u16(), 401 | 403),
                    "non-admin revoke must be rejected, got {non_admin_status}"
                );

                let resp = external_service_tokens_consumer::revoke_token(
                    &base_url,
                    token_id,
                    Some("smoke test cleanup"),
                )
                .await?;
                assert_eq!(resp.status, "success");

                let detail = companies_consumer::get_company(&base_url, company_id).await?;
                let view = &detail.external_service_tokens[0];
                assert!(
                    view.revoked_at.is_some(),
                    "token should have revoked_at populated"
                );

                Ok(())
            }
        }
    });

    let companies_revoke_trial = async_trial(&rt, "revoke_company", {
        let base_url = base_url.clone();
        let scenario = scenario.clone();
        move || {
            let base_url = base_url.clone();
            async move {
                let company_id = {
                    let ctx = scenario.lock().unwrap_or_else(|p| p.into_inner());
                    let handle = ctx
                        .companies_named_batches
                        .get("companies-init")
                        .expect("companies-init not registered");
                    ctx.companies_batches[handle.index][0].id
                };

                let resp = companies_consumer::revoke_company(
                    &base_url,
                    company_id,
                    Some("smoke test cleanup"),
                )
                .await?;
                assert_eq!(resp.status, "success");

                // Default list should now not include the revoked company.
                let live = companies_consumer::list_companies(&base_url, false).await?;
                assert!(
                    !live.objs.iter().any(|c| c.id == company_id),
                    "revoked company should be hidden from default list"
                );

                // Explicit ?revoked=true should bring it back.
                let all = companies_consumer::list_companies(&base_url, true).await?;
                assert!(
                    all.objs.iter().any(|c| c.id == company_id),
                    "revoked company should appear when revoked=true"
                );

                Ok(())
            }
        }
    });

    // Run all trials sequentially in topological order.
    // Each trial's dependencies are guaranteed to be satisfied by prior trials.
    let mut args = Arguments::from_args();
    args.test_threads = Some(1);
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
            verify_search_endpoint_validation_trial, // Phase 5: search endpoint validation
            verify_guides_by_classification_trial,   // Phase 5: self-contained (own guides)
            // Level 1: Depend on Level 0
            fluent_message_paths_trial, // → fluent_messages (GET-only path surface)
            briefs_trial,               // → value_slots (for inline outputs)
            verify_guides_by_slugs_trial, // → guides
            deprecate_guides_trial,     // → guides
            verify_embedding_insert_delete_trial, // Phase 5: → guides
            // Level 2: Depend on Level 1
            brief_groups_trial,                      // → briefs
            brief_units_trial, // → briefs (read-only verification of inline units)
            prompts_trial,     // → fluent_messages
            verify_guide_embeddings_generated_trial, // Phase 5 Tier 2: → guides
            // Level 3: Depend on Level 2
            tasks_trial,       // → prompts (tasks now embed prompt_ids)
            brief_packs_trial, // → brief_groups
            // Level 4: Depend on Level 3
            steps_trial,                 // → tasks
            verify_tasks_by_slugs_trial, // → tasks
            // Level 5: Depend on Level 4
            playbooks_trial, // → steps, categories, hats
            // Standalone "add stage to existing playbook" — needs BOTH a playbook
            // (Level 5) and steps (Level 4), so it must run after playbooks, not
            // at Level 3. Nothing downstream consumes these standalone stages.
            stages_trial,
            journeys_trial,                       // → playbooks, brief_packs
            deprecate_briefs_trial,               // → briefs
            verify_guide_search_round_trip_trial, // Phase 5 Tier 2: → embeddings
            verify_guide_reingest_replaces_embeddings_trial, // Phase 5 Tier 2: → embeddings
            // Level 6: Depend on Level 5
            dispatches_trial,                      // → playbooks, journeys
            verify_journeys_recent_trial,          // → journeys
            verify_favorite_reflects_rename_trial, // → journeys (issue #484)
            verify_favorite_follows_advance_trial, // → journeys (chain favorites, Mig 46)
            // Level 7: Depend on Level 6
            attempts_trial,                           // → dispatches
            dispatch_intent_bindings_trial,           // → dispatches, value_slots
            dispatch_execution_trial,                 // → categories (creates own entity tree)
            computed_output_reaches_done_trial,       // → self-contained (ingests own guide)
            auto_dispatch_pending_step_trial, // → self-contained (auto-dispatch no-input step)
            auto_dispatch_skips_candidate_feed_trial, // → self-contained (picker feed NOT auto-dispatched)
            verify_rag_graceful_degradation_trial,    // Phase 5 Tier 2: → categories
            verify_full_search_pipeline_trial,        // Phase 5 Tier 2: → embeddings
            // Admin B2B partner endpoints (independent of other lifecycle).
            companies_create_trial,
            companies_non_admin_403_trial,
            companies_dup_slug_trial,
            external_service_tokens_create_trial,
            external_service_token_dup_id_trial,
            external_service_token_invalid_pem_trial,
            external_service_token_non_admin_403_trial,
            external_service_token_revoke_trial,
            companies_revoke_trial,
        ],
    );
    conclusion.exit_if_failed();

    // --- Redis verification: ensure write-through cache keys were produced ---
    // Initialize Redis-backed integrations (uses DAPPLE_REDIS_HOST or default cluster)
    let _ = dapple_data::integrations::init_redis_from_env();

    // Run async verification on the existing runtime
    let verify_rt = rt.clone();
    let scenario_clone = scenario.clone();
    verify_rt.block_on(async move {
        // Connect directly to the same Redis used by integrations for verification
        // Reuse the `redis_url` computed earlier (it includes injected password
        // when `DAPPLE_REDIS_PASSWORD` is set) so verification uses the same
        // authenticated URL the integrations were initialized with.
        let url = redis_url.clone();

        let client = match redis::Client::open(url.as_str()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "skipping Redis verification: failed to construct client: {}",
                    e
                );
                return;
            }
        };

        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("skipping Redis verification: failed to connect: {}", e);
                return;
            }
        };

        // Helper to check existence with retries (increased to allow background
        // write-through tasks to complete). Total wait ~10s (50 * 200ms).
        async fn check_key_with_retries<C>(conn: &mut C, key: &str) -> bool
        where
            C: AsyncCommands + Unpin,
        {
            let mut attempts = 0u8;
            while attempts < 50 {
                let exists: redis::RedisResult<i32> = conn.exists(key).await;
                match exists {
                    Ok(n) if n > 0 => return true,
                    _ => {
                        attempts = attempts.saturating_add(1);
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
            }
            false
        }

        // Lock scenario, collect the ids to verify, then DROP the guard before
        // any await. Holding a std::sync::Mutex guard across `.await` is a clippy
        // lint (await_holding_lock) and a real foot-gun.
        let (playbook_ids, dispatch_ids): (Vec<Uuid>, Vec<Uuid>) = {
            let ctx = scenario_clone.lock().unwrap_or_else(|p| p.into_inner());
            let playbook_ids = ctx
                .named_playbook_batches
                .get("init-playbooks")
                .and_then(|handle| ctx.playbook_batches.get(handle.index))
                .map(|batch| batch.iter().map(|pb| pb.id).collect())
                .unwrap_or_default();
            let dispatch_ids = ctx
                .dispatch_named_batches
                .get("dispatches-init")
                .and_then(|handle| ctx.dispatch_batches.get(handle.index))
                .map(|batch| batch.iter().map(|d| d.id).collect())
                .unwrap_or_default();
            (playbook_ids, dispatch_ids)
        };

        // Give background tasks a short head-start before polling Redis.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Verify playbooks created under the named batch `init-playbooks`
        for pb_id in playbook_ids {
            let key = format!("dto:playbook:{}", pb_id);
            let ok = check_key_with_retries(&mut conn, &key).await;
            if !ok {
                // Diagnostic: list dto keys and try to GET the key to aid debugging.
                let keys_res: redis::RedisResult<Vec<String>> = conn.keys("dto:*").await;
                match keys_res {
                    Ok(keys) => println!("redis keys (dto:*): {:?}", keys),
                    Err(e) => println!("failed to list dto keys: {}", e),
                }
                let val_res: redis::RedisResult<Option<String>> = conn.get(&key).await;
                match val_res {
                    Ok(val) => println!("GET {} => {:?}", key, val),
                    Err(e) => println!("GET {} error: {}", key, e),
                }

                panic!("redis verification failed: key missing: {}", key);
            }
        }

        // Verify dispatches created under the named batch `dispatches-init`
        for d_id in dispatch_ids {
            let key = format!("dto:dispatch:{}", d_id);
            let ok = check_key_with_retries(&mut conn, &key).await;
            if !ok {
                panic!("redis verification failed: key missing: {}", key);
            }
        }
    });
}

/// A minimal guide whose single-step stage exposes ONLY a computed
/// `:::brief-output` (`synth`). `slot-a` is an UNEXPOSED brief input, so the
/// brief is non-empty yet nothing gates the dispatch start (the computed output
/// is excluded from the start gate, and slot-a is not exposed) — the dispatch is
/// start-ready on create and auto-spawns. The single nushell task returns a
/// payload; the runtime producer binds `synth` from it, flipping the step/stage
/// `done` rollups. Used by the `computed_output_reaches_done_after_dispatch`
/// trial.
fn computed_output_done_guide_tarball(suffix: &str) -> Vec<u8> {
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
        "title = \"Computed Done Guide {suffix}\"\n\
         description = \"Single-step stage exposing one computed output\"\n\
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

    // Stage exposes ONLY `synth` (computed) — slot-a stays an unexposed input so
    // the brief is valid but nothing gates the start.
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"slot-a-{short}\", kind: \"text\" }}\n:::\n\n\
         :::value-slot {{ name: \"synth-{short}\", kind: \"json\" }}\n:::\n\n\
         :::brief-input {{ name: \"input-a-{short}\", control: \"text\", slot: \"slot-a-{short}\" }}\n:::\n\n\
         :::brief-output {{ name: \"synth-out-{short}\", slot: \"synth-{short}\", source: \"computed\" }}\n:::\n\n\
         :::brief {{ name: \"brief-{short}\", units: [\"input-a-{short}\", \"synth-out-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-{short}\", briefs: [\"brief-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-{short}\", expose: [\"synth-{short}\"] }}\n\n\
         :::step {{ name: \"step-{short}\", task: \"task-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-{short}\" }}\n{{ payload: {{ result: \"done\" }}, signals: {{}} }}\n:::\n"
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

/// Two stages, each exposing ONLY a computed output (no operator input). The
/// auto-dispatch test POSTs stage A and asserts the runtime auto-dispatches stage
/// B (which no consumer ever POSTs) once stage A is done.
/// Two computed-output stages (A then B). With `picker_on_b`, stage B's computed
/// output is additionally wired as a brief-input's `candidate_source`, making
/// step B a picker-feed generate step — which must NOT auto-dispatch (the
/// consumer dispatches picker feeds on pane open; an auto-dispatch would
/// double-run the generate).
fn auto_dispatch_two_stage_guide_tarball(suffix: &str, picker_on_b: bool) -> Vec<u8> {
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
        "title = \"Auto Dispatch Guide {suffix}\"\n\
         description = \"Two computed-output stages; the second must auto-dispatch\"\n\
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

    let picker_blocks = if picker_on_b {
        format!(
            ":::value-slot {{ name: \"run-b-{short}\", kind: \"json\" }}\n:::\n\n\
             :::brief-input {{ name: \"pick-b-{short}\", control: \"select\", slot: \"run-b-{short}\", candidate_source: \"out-b-{short}\" }}\n:::\n\n"
        )
    } else {
        String::new()
    };
    let brief_b_units = if picker_on_b {
        format!("[\"out-b-output-{short}\", \"pick-b-{short}\"]")
    } else {
        format!("[\"out-b-output-{short}\"]")
    };
    let ch1 = format!(
        "# Workflow\n\n\
         :::value-slot {{ name: \"out-a-{short}\", kind: \"json\" }}\n:::\n\n\
         :::value-slot {{ name: \"out-b-{short}\", kind: \"json\" }}\n:::\n\n\
         {picker_blocks}\
         :::brief-output {{ name: \"out-a-output-{short}\", slot: \"out-a-{short}\", source: \"computed\" }}\n:::\n\n\
         :::brief-output {{ name: \"out-b-output-{short}\", slot: \"out-b-{short}\", source: \"computed\" }}\n:::\n\n\
         :::brief {{ name: \"brief-a-{short}\", units: [\"out-a-output-{short}\"] }}\n:::\n\n\
         :::brief {{ name: \"brief-b-{short}\", units: {brief_b_units} }}\n:::\n\n\
         :::brief-group {{ name: \"group-a-{short}\", briefs: [\"brief-a-{short}\"] }}\n:::\n\n\
         :::brief-group {{ name: \"group-b-{short}\", briefs: [\"brief-b-{short}\"] }}\n:::\n\n\
         :::brief-pack {{ name: \"pack-{short}\", groups: [\"group-a-{short}\", \"group-b-{short}\"] }}\n:::\n\n\
         :::stage {{ name: \"stage-a-{short}\", expose: [\"out-a-{short}\"] }}\n\n\
         :::step {{ name: \"step-a-{short}\", task: \"task-a-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::stage {{ name: \"stage-b-{short}\", expose: [\"out-b-{short}\"] }}\n\n\
         :::step {{ name: \"step-b-{short}\", task: \"task-b-{short}\" }}\n:::\n\n\
         :::\n\n\
         :::nushell {{ name: \"task-a-{short}\" }}\n{{ payload: {{ a: 1 }}, signals: {{}} }}\n:::\n\n\
         :::nushell {{ name: \"task-b-{short}\" }}\n{{ payload: {{ b: 2 }}, signals: {{}} }}\n:::\n"
    );
    append_file(&mut archive, "guide/ch1.md", &ch1);

    let picker_fluent = if picker_on_b {
        format!("pick-b-{short} = Pick B\n")
    } else {
        String::new()
    };
    let fluent = format!(
        "out-a-output-{short} = Output A\n\
         out-b-output-{short} = Output B\n\
         {picker_fluent}\
         brief-a-{short} = Brief A\n\
         brief-b-{short} = Brief B\n\
         group-a-{short} = Group A\n\
         group-b-{short} = Group B\n\
         pack-{short} = The pack\n\
         stage-a-{short} = Stage A\n\
         stage-b-{short} = Stage B\n\
         step-a-{short} = Step A\n\
         step-b-{short} = Step B\n"
    );
    append_file(&mut archive, "guide/locales/en.ftl", &fluent);

    archive.finish().unwrap();
    drop(archive);
    tarball
}
