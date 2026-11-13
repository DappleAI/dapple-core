use dapple_data::DbPool;
use dapple_data::FilterOptions;
use dapple_data::model::dispatches::InsertDispatchArgs;
use dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs;
use dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs;
use dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
use dapple_data::model::journeys::InsertJourneyArgs;
use dapple_data::query::dispatch_task_attempts::select_attempt_events_by_attempt_object_id;
use dapple_data::query::dispatch_task_attempts::select_task_attempts_by_dispatch_id;
use dapple_data::query::dispatches::insert_dispatches;
use dapple_data::query::journeys::insert_journeys;
use dapple_data::query::playbooks::select_playbooks;
use dapple_data::test_support::insert_test_user_and_user_detail;
use dapple_prompt_flow::Step;
use dapple_prompt_flow::dispatch_runner::run_dispatch_from_dto;
use dapple_prompt_flow::func::test_handler::TestFunction;
use dapple_prompt_flow::test_support::build_named_playbook;
use dataflow_rs::AsyncFunctionHandler;
use serde_json::to_string_pretty;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
async fn dispatch_flow_integration_skeleton() -> Result<(), Box<dyn std::error::Error>> {
    // Build a named playbook (no UUIDs/object_ids; dapple-data will generate those)
    let playbook = build_named_playbook("itest");

    // Print generated playbook
    println!("Generated playbook: {}", to_string_pretty(&playbook)?);

    // Attempt to connect to DB using DbPool (uses DAPPLE_PROMPT_DATABASE_URL or .env.test)
    let pool = match DbPool::connect().await {
        Ok(p) => Arc::new(p),
        Err(e) => {
            println!("DB connect failed: {:?}. Skipping DB wiring.", e);
            return Ok(());
        }
    };

    // Query existing playbooks as a light DB smoke test (no mutations).
    match select_playbooks(&pool, FilterOptions::default()).await {
        Ok(pbs) => println!("Found {} playbooks in DB (smoke test)", pbs.len()),
        Err(e) => println!("select_playbooks failed: {:?}", e),
    }

    // Try to find an existing dispatch for the first playbook and run the runner.
    let pbs = select_playbooks(&pool, FilterOptions::default())
        .await
        .unwrap_or_default();

    if let Some(pb) = pbs.into_iter().next() {
        // Create a test user + user_detail required for inserting a Journey
        let user_detail = match insert_test_user_and_user_detail(&pool, "itest@example.com").await {
            Ok(u) => u,
            Err(e) => {
                println!(
                    "insert_test_user_and_user_detail failed: {:?}. Skipping.",
                    e
                );
                return Ok(());
            }
        };

        // Insert a journey that links the selected playbook and our test user
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let principal_id =
            match dapple_data::query::principals::select_human_principal_for_user_object_id(
                &pool,
                user_detail.id,
            )
            .await
            {
                Ok(Some(p)) => p,
                _ => {
                    println!("no human_principal for test user; skipping");
                    return Ok(());
                }
            };

        let jargs = InsertJourneyArgs {
            playbook_ids: vec![pb.id],
            principal_ids: vec![principal_id],
            subject_principal_ids: vec![None],
            billable_entity_ids: vec![None],
            names: vec![format!("itest-journey-{}", ts)],
            owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
            predecessor_journey_ids: vec![None],
            guide_ids: vec![None],
            fork_stage_ids: vec![None],
        };

        let journeys = match insert_journeys(&pool, vec![jargs]).await {
            Ok(js) => js,
            Err(e) => {
                println!("insert_journeys failed: {:?}. Skipping.", e);
                return Ok(());
            }
        };

        let journey = match journeys.into_iter().next() {
            Some(j) => j,
            None => {
                println!("insert_journeys returned no journey; skipping");
                return Ok(());
            }
        };

        // Build a Dispatch insertion payload based on the playbook structure
        let mut stages_args: Vec<InsertDispatchStagesArgs> = Vec::new();
        for stage in pb.stages.iter() {
            let mut steps_args: Vec<InsertDispatchStepsArgs> = Vec::new();
            for step in stage.steps.iter() {
                let mut tasks_args: Vec<InsertDispatchTaskArgs> = Vec::new();
                for task in step.tasks.iter() {
                    tasks_args.push(InsertDispatchTaskArgs {
                        step_task_ids: vec![task.id],
                    });
                }

                steps_args.push(InsertDispatchStepsArgs {
                    stage_step_ids: vec![step.id],
                    params: vec![serde_json::json!({})],
                    tasks: tasks_args,
                });
            }

            stages_args.push(InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage.id],
                params: vec![serde_json::json!({})],
                steps: steps_args,
            });
        }

        let dispatch_args = InsertDispatchArgs {
            intents: vec![serde_json::json!({})],
            stages: stages_args,
        };

        // Insert the dispatch for the created journey
        let inserted = match insert_dispatches(&pool, vec![journey.id], vec![dispatch_args]).await {
            Ok(ds) => ds,
            Err(e) => {
                println!("insert_dispatches failed: {:?}. Skipping.", e);
                return Ok(());
            }
        };

        let dispatch = match inserted.into_iter().next() {
            Some(d) => d,
            None => {
                println!("insert_dispatches returned no dispatch; skipping");
                return Ok(());
            }
        };

        println!("Inserted dispatch {}, running runner", dispatch.id);

        // Build function map with deterministic TestFunction (Arc'd so it can be cloned)
        let mut functions: HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>> =
            HashMap::new();
        functions.insert("test_fn".to_string(), Arc::new(TestFunction));

        let functions = Arc::new(functions);

        // Create a minimal Step (engine/workflows not needed for simulated run)
        let step = Step { workflows: vec![] };

        // Run the dispatcher (this currently simulates task runs)
        let _ =
            run_dispatch_from_dto(pool.clone(), dispatch.clone(), &step, Some(functions), 4).await;

        // Verify attempts were recorded and have events (publisher side-effects).
        // Background publisher/cache tasks run in spawned tasks; poll with backoff to wait for them.
        let mut ok = false;
        let timeout_ms = 5000u128;
        let interval_ms = 200u64;
        let start = std::time::Instant::now();

        while start.elapsed().as_millis() < timeout_ms {
            match select_task_attempts_by_dispatch_id(&pool, dispatch.id).await {
                Ok(attempts) => {
                    if attempts.is_empty() {
                        // no attempts yet; wait and retry
                        tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
                        continue;
                    }

                    // check that every attempt has at least one event
                    let mut all_have_events = true;
                    for attempt in attempts.iter() {
                        match select_attempt_events_by_attempt_object_id(&pool, attempt.id).await {
                            Ok(events) => {
                                if events.is_empty() {
                                    all_have_events = false;
                                    break;
                                }
                            }
                            Err(_) => {
                                all_have_events = false;
                                break;
                            }
                        }
                    }

                    if all_have_events {
                        ok = true;
                        break;
                    }
                }
                Err(_) => {
                    // query failed; wait and retry
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
        }

        assert!(
            ok,
            "timed out waiting for attempt events for dispatch {}",
            dispatch.id
        );
    } else {
        println!("No playbooks found; skipping dispatch runner test");
    }

    Ok(())
}
