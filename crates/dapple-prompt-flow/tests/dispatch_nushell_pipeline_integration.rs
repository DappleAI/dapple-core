use dapple_data::DbPool;
use dapple_data::model::journeys::InsertJourneyArgs;
use dapple_data::model::playbooks::InsertPlaybookArgs;
use dapple_data::model::playbooks::playbook_stages::InsertPlaybookStageArgs;
use dapple_data::model::tasks::InsertTaskArgs;
use dapple_data::query::dispatch_task_attempts::{
    select_attempt_events_by_attempt_object_id, select_task_attempts_by_dispatch_id,
};
use dapple_data::query::{
    dispatches::insert_dispatches, journeys::insert_journeys, tasks::insert_tasks,
};

use dapple_prompt_flow::Step;
use dapple_prompt_flow::dispatch_runner::{execute_dispatch, run_dispatch_from_dto};
use dataflow_rs::Engine;
use dataflow_rs::Workflow;
use serde_json::json;
use sqlx::types::Uuid;
use std::collections::HashMap;
use std::sync::Arc;

use dapple_prompt_flow::func::nu_handler::NuFunction;
use dataflow_rs::AsyncFunctionHandler;

// These tests use full Nushell pipeline capabilities with nu-command crate.
// Commands available: each, where, str, math, wrap, merge, filter, etc.

/// Test that data flows between multiple tasks using real Nushell pipelines.
/// Task 1: Produce data
/// Task 2: Transform using `each` and `str upcase`
/// Task 3: Aggregate using `str join`
#[tokio::test]
async fn test_nushell_multi_task_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    // Task 1: Produce structured data with a list
    let task1_script = r#"
        {
            payload: {
                items: ["apple", "banana", "cherry"],
                count: 3,
                metadata: { source: "test" }
            },
            signals: { produced: true }
        }
    "#;

    // Task 2: Transform using pipeline - each item to uppercase
    let task2_script = r#"
        let data = $in;
        {
            payload: {
                items: ($data.items | each { |it| $it | str upcase }),
                count: ($data.count * 2),
                processed: true
            },
            signals: { transformed: true }
        }
    "#;

    // Task 3: Aggregate using str join
    let task3_script = r#"
        let data = $in;
        {
            payload: {
                summary: ($data.items | str join ", "),
                total: $data.count,
                complete: true
            },
            signals: { finalized: true }
        }
    "#;

    // Build a single workflow with all three tasks chained
    let wf = json!({
        "id": "wf_nushell_pipeline",
        "name": "Nushell Pipeline Test",
        "tasks": [
            {
                "id": "t1_produce",
                "name": "produce_data",
                "function": {
                    "name": "nu",
                    "input": {
                        "script": task1_script,
                        "output_path": "data.step1"
                    }
                }
            },
            {
                "id": "t2_transform",
                "name": "transform_data",
                "function": {
                    "name": "nu",
                    "input": {
                        "script": task2_script,
                        "input_path": "data.step1",
                        "output_path": "data.step2"
                    }
                }
            },
            {
                "id": "t3_finalize",
                "name": "finalize_data",
                "function": {
                    "name": "nu",
                    "input": {
                        "script": task3_script,
                        "input_path": "data.step2",
                        "output_path": "data.final"
                    }
                }
            }
        ]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf)?)?;

    // Register the NuFunction handler
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("nu".into(), Box::new(NuFunction::default()));

    let engine = Engine::new(vec![workflow], Some(functions));
    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    // Execute the pipeline
    engine.process_message(&mut message).await?;

    // Verify Task 1 output (raw data)
    let step1 = message.data().get("step1").expect("step1 should exist");
    assert_eq!(step1["items"], json!(["apple", "banana", "cherry"]));
    assert_eq!(step1["count"], json!(3));
    assert_eq!(step1["metadata"]["source"], json!("test"));

    // Verify Task 2 transformation (uppercase via pipeline)
    let step2 = message.data().get("step2").expect("step2 should exist");
    assert_eq!(
        step2["items"],
        json!(["APPLE", "BANANA", "CHERRY"]),
        "items should be uppercase"
    );
    assert_eq!(step2["count"], json!(6), "count should be doubled");
    assert_eq!(step2["processed"], json!(true));

    // Verify Task 3 aggregation (str join)
    let final_data = message.data().get("final").expect("final should exist");
    assert_eq!(
        final_data["summary"],
        json!("APPLE, BANANA, CHERRY"),
        "should be joined string"
    );
    assert_eq!(final_data["total"], json!(6));
    assert_eq!(final_data["complete"], json!(true));

    // Verify all signals were accumulated
    let signals = message
        .metadata()
        .get("signals")
        .expect("signals should exist");
    assert_eq!(signals["produced"], json!(true));
    assert_eq!(signals["transformed"], json!(true));
    assert_eq!(signals["finalized"], json!(true));

    Ok(())
}

/// Test string operations with pipelines
#[tokio::test]
async fn test_nushell_string_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        let text = "  hello world  ";
        let words = $text | str trim | split row " ";
        {
            payload: {
                original: $text,
                trimmed: ($text | str trim),
                uppercase: ($text | str trim | str upcase),
                words: $words,
                word_count: ($words | length)
            },
            signals: { string_test: true }
        }
    "#;

    let wf = json!({
        "id": "wf_string",
        "name": "String Pipeline Test",
        "tasks": [{
            "id": "t1",
            "name": "string_ops",
            "function": {
                "name": "nu",
                "input": {
                    "script": script,
                    "output_path": "data.result"
                }
            }
        }]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf)?)?;
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("nu".into(), Box::new(NuFunction::default()));

    let engine = Engine::new(vec![workflow], Some(functions));
    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    engine.process_message(&mut message).await?;

    let result = message.data().get("result").expect("result should exist");
    assert_eq!(result["trimmed"], json!("hello world"));
    assert_eq!(result["uppercase"], json!("HELLO WORLD"));
    assert_eq!(result["words"], json!(["hello", "world"]));
    assert_eq!(result["word_count"], json!(2));

    Ok(())
}

/// Test filtering with where command
#[tokio::test]
async fn test_nushell_filter_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        let numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let evens = $numbers | where { |n| ($n mod 2) == 0 };
        let odds = $numbers | where { |n| ($n mod 2) != 0 };
        {
            payload: {
                all: $numbers,
                evens: $evens,
                odds: $odds,
                even_count: ($evens | length),
                sum_evens: ($evens | math sum)
            },
            signals: { filter_test: true }
        }
    "#;

    let wf = json!({
        "id": "wf_filter",
        "name": "Filter Pipeline Test",
        "tasks": [{
            "id": "t1",
            "name": "filter_ops",
            "function": {
                "name": "nu",
                "input": {
                    "script": script,
                    "output_path": "data.result"
                }
            }
        }]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf)?)?;
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("nu".into(), Box::new(NuFunction::default()));

    let engine = Engine::new(vec![workflow], Some(functions));
    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    engine.process_message(&mut message).await?;

    let result = message.data().get("result").expect("result should exist");
    assert_eq!(result["evens"], json!([2, 4, 6, 8, 10]));
    assert_eq!(result["odds"], json!([1, 3, 5, 7, 9]));
    assert_eq!(result["even_count"], json!(5));
    assert_eq!(result["sum_evens"], json!(30));

    Ok(())
}

/// Test math operations with pipelines
#[tokio::test]
async fn test_nushell_math_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        let numbers = [10, 20, 30, 40, 50];
        {
            payload: {
                numbers: $numbers,
                sum: ($numbers | math sum),
                avg: ($numbers | math avg),
                min: ($numbers | math min),
                max: ($numbers | math max)
            },
            signals: { math_test: true }
        }
    "#;

    let wf = json!({
        "id": "wf_math",
        "name": "Math Pipeline Test",
        "tasks": [{
            "id": "t1",
            "name": "math_ops",
            "function": {
                "name": "nu",
                "input": {
                    "script": script,
                    "output_path": "data.result"
                }
            }
        }]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf)?)?;
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("nu".into(), Box::new(NuFunction::default()));

    let engine = Engine::new(vec![workflow], Some(functions));
    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    engine.process_message(&mut message).await?;

    let result = message.data().get("result").expect("result should exist");
    assert_eq!(result["sum"], json!(150));
    assert_eq!(result["avg"], json!(30.0)); // avg returns float
    assert_eq!(result["min"], json!(10));
    assert_eq!(result["max"], json!(50));

    Ok(())
}

/// Test data transformation with get, select, wrap
#[tokio::test]
async fn test_nushell_data_transform_pipeline() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        let data = {
            user: {
                name: "Alice",
                email: "alice@example.com",
                age: 30
            },
            settings: {
                theme: "dark",
                notifications: true
            }
        };
        {
            payload: {
                user_name: ($data | get user.name),
                theme: ($data | get settings.theme),
                wrapped: ($data.user.name | wrap username),
                selected: { name: $data.user.name, theme: $data.settings.theme }
            },
            signals: { transform_test: true }
        }
    "#;

    let wf = json!({
        "id": "wf_transform",
        "name": "Data Transform Test",
        "tasks": [{
            "id": "t1",
            "name": "transform_ops",
            "function": {
                "name": "nu",
                "input": {
                    "script": script,
                    "output_path": "data.result"
                }
            }
        }]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf)?)?;
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("nu".into(), Box::new(NuFunction::default()));

    let engine = Engine::new(vec![workflow], Some(functions));
    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    engine.process_message(&mut message).await?;

    let result = message.data().get("result").expect("result should exist");
    assert_eq!(result["user_name"], json!("Alice"));
    assert_eq!(result["theme"], json!("dark"));
    assert_eq!(result["wrapped"]["username"], json!("Alice"));
    assert_eq!(result["selected"]["name"], json!("Alice"));
    assert_eq!(result["selected"]["theme"], json!("dark"));

    Ok(())
}

/// Test null/empty input handling with default command
#[tokio::test]
async fn test_nushell_handles_null_input() -> Result<(), Box<dyn std::error::Error>> {
    // When input_path doesn't exist, $in will be null
    let script = r#"
        let data = $in;
        let has_data = $data != null;
        {
            payload: {
                received_null: ($data == null),
                default_value: (if $data == null { "no data" } else { $data })
            },
            signals: { null_handled: true }
        }
    "#;

    let wf = json!({
        "id": "wf_null_test",
        "name": "Null Input Test",
        "tasks": [{
            "id": "t1",
            "name": "handle_null",
            "function": {
                "name": "nu",
                "input": {
                    "script": script,
                    "input_path": "data.nonexistent",
                    "output_path": "data.result"
                }
            }
        }]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf)?)?;
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("nu".into(), Box::new(NuFunction::default()));

    let engine = Engine::new(vec![workflow], Some(functions));
    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    engine.process_message(&mut message).await?;

    let result = message.data().get("result").expect("result should exist");
    assert_eq!(result["received_null"], json!(true));
    assert_eq!(result["default_value"], json!("no data"));

    Ok(())
}

/// Test list operations: first, last, take, skip, reverse
#[tokio::test]
async fn test_nushell_list_operations() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        let items = ["a", "b", "c", "d", "e"];
        {
            payload: {
                items: $items,
                first: ($items | first),
                last: ($items | last),
                first_two: ($items | take 2),
                skip_two: ($items | skip 2),
                reversed: ($items | reverse)
            },
            signals: { list_ops: true }
        }
    "#;

    let wf = json!({
        "id": "wf_list_ops",
        "name": "List Operations Test",
        "tasks": [{
            "id": "t1",
            "name": "list_ops",
            "function": {
                "name": "nu",
                "input": {
                    "script": script,
                    "output_path": "data.result"
                }
            }
        }]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf)?)?;
    let mut functions: HashMap<String, Box<dyn AsyncFunctionHandler + Send + Sync>> =
        HashMap::new();
    functions.insert("nu".into(), Box::new(NuFunction::default()));

    let engine = Engine::new(vec![workflow], Some(functions));
    let mut message = dataflow_rs::engine::message::Message::from_value(&json!({}));

    engine.process_message(&mut message).await?;

    let result = message.data().get("result").expect("result should exist");
    assert_eq!(result["first"], json!("a"));
    assert_eq!(result["last"], json!("e"));
    assert_eq!(result["first_two"], json!(["a", "b"]));
    assert_eq!(result["skip_two"], json!(["c", "d", "e"]));
    assert_eq!(result["reversed"], json!(["e", "d", "c", "b", "a"]));

    Ok(())
}

/// Full dispatch integration test with database persistence
#[tokio::test]
async fn dispatch_nushell_pipeline_integration()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Build DB connection (skip test if DB not available)
    let pool = match DbPool::connect().await {
        Ok(p) => Arc::new(p),
        Err(e) => {
            println!("DB not available: {:?}. Skipping test.", e);
            return Ok(());
        }
    };

    // Script using real Nushell pipeline commands
    let script = r#"
        let items = ["apple", "banana", "cherry"];
        let uppercased = $items | each { |it| $it | str upcase };
        {
            payload: {
                original: $items,
                uppercased: $uppercased,
                joined: ($uppercased | str join ", "),
                count: ($items | length),
                first: ($items | first)
            },
            signals: { db_test_complete: true }
        }
    "#;

    let task_args = InsertTaskArgs {
        names: vec![format!("nushell-db-test-{}", Uuid::now_v7())],
        descriptions: vec!["Test Nushell pipeline with DB persistence".to_string()],
        functions: vec!["nu".to_string()],
        function_inputs: vec![json!({
            "script": script,
            "output_path": "data.result"
        })],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let tasks = insert_tasks(&pool, vec![task_args]).await?;

    // Create step referencing the task
    let step_args = dapple_data::model::steps::InsertStepArgs {
        names: vec![format!("db-test-step-{}", Uuid::now_v7())],
        descriptions: vec![Some("step for db test".to_string())],
        description_fluent_message_ids: Vec::new(),
        tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: vec![tasks[0].id],
        }],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let inserted_steps = dapple_data::query::steps::insert_steps(&pool, vec![step_args]).await?;

    // Create playbook
    let playbook_args = InsertPlaybookArgs {
        names: vec![format!("nu_db_playbook_{}", Uuid::now_v7())],
        descriptions: vec!["playbook for nu db test".to_string()],
        stages: vec![InsertPlaybookStageArgs {
            name: format!("test-stage-{}", Uuid::now_v7()),
            description: Some("stage for nu db test".to_string()),
            description_fluent_message_id: None,
            kind: None,
            icon: Vec::new(),
            step_ids: vec![inserted_steps[0].id],
            stage_id: None,
            content_hash: Some("0000000000000000000000000000000000000000".to_string()),
            reused: false,
        }],
        categories: vec![
            dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs {
                playbook_category_ids: vec![],
            },
        ],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let pbs = dapple_data::query::playbooks::insert_playbooks(&pool, vec![playbook_args]).await?;
    let pb = pbs.into_iter().next().ok_or("no playbook inserted")?;

    // Create user + journey
    let user_detail = dapple_data::test_support::insert_test_user_and_user_detail(
        &pool,
        "nu-db-test@example.com",
    )
    .await?;

    let principal_id = dapple_data::query::principals::select_human_principal_for_user_object_id(
        &pool,
        user_detail.id,
    )
    .await?
    .ok_or("no human_principal for test user")?;

    let jargs = InsertJourneyArgs {
        playbook_ids: vec![pb.id],
        principal_ids: vec![principal_id],
        subject_principal_ids: vec![None],
        billable_entity_ids: vec![None],
        names: vec!["nu-db-test-journey".to_string()],
        owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
        predecessor_journey_ids: vec![None],
        guide_ids: vec![None],
        fork_stage_ids: vec![None],
    };

    let js = insert_journeys(&pool, vec![jargs]).await?;
    let journey = js.into_iter().next().ok_or("no journey")?;

    // Build dispatch args for the single step
    let stage = &pb.stages[0];
    let step = &stage.steps[0];

    let dispatch_args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage.id],
                params: vec![json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step.id],
                        params: vec![json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: vec![step.tasks[0].id],
                            },
                        ],
                    },
                ],
            },
        ],
    };

    let dispatches = insert_dispatches(&pool, vec![journey.id], vec![dispatch_args]).await?;
    let dispatch = dispatches
        .into_iter()
        .next()
        .ok_or("no dispatch inserted")?;

    // Build workflow from task and register nu function
    let wf_value = json!({
        "id": format!("wf_{}", tasks[0].id),
        "name": tasks[0].name,
        "tasks": [{
            "id": format!("t_{}", tasks[0].id),
            "name": tasks[0].name,
            "function": {
                "name": tasks[0].function,
                "input": tasks[0].function_input
            }
        }]
    });

    let workflow = Workflow::from_json(&serde_json::to_string(&wf_value)?)?;
    let step_obj = Step {
        workflows: vec![workflow],
    };

    let mut fmap: HashMap<String, Arc<dyn AsyncFunctionHandler + Send + Sync>> = HashMap::new();
    fmap.insert("nu".to_string(), Arc::new(NuFunction::default()));
    let fmap = Arc::new(fmap);

    // Run the dispatch
    run_dispatch_from_dto(pool.clone(), dispatch.clone(), &step_obj, Some(fmap), 4).await?;

    // Verify in database
    let attempts = select_task_attempts_by_dispatch_id(&pool, dispatch.id).await?;
    assert!(!attempts.is_empty(), "should have at least one attempt");

    let mut found_result = false;
    let mut found_signal = false;
    for attempt in attempts.iter() {
        let events = select_attempt_events_by_attempt_object_id(&pool, attempt.id).await?;

        for event in events.iter() {
            if event.phase_code == "ok" {
                // The runner persists the FULL author signals object under
                // `context_metadata.signals` (not a narrowed token subset) —
                // this is the source the external endpoint projects as `signals`.
                if let Some(meta) = &event.context_metadata
                    && meta.get("signals").and_then(|s| s.get("db_test_complete"))
                        == Some(&json!(true))
                {
                    found_signal = true;
                }
                if let Some(context_data) = &event.context_data
                    && let Some(result) = context_data.get("result")
                {
                    // Verify the Nushell pipeline execution was persisted
                    assert_eq!(
                        result["original"],
                        json!(["apple", "banana", "cherry"]),
                        "original items should match"
                    );
                    assert_eq!(
                        result["uppercased"],
                        json!(["APPLE", "BANANA", "CHERRY"]),
                        "uppercased items should match"
                    );
                    assert_eq!(
                        result["joined"],
                        json!("APPLE, BANANA, CHERRY"),
                        "joined string should match"
                    );
                    assert_eq!(result["count"], json!(3), "count should be 3");
                    assert_eq!(result["first"], json!("apple"), "first should be apple");
                    found_result = true;
                }
            }
        }
    }

    assert!(found_result, "should find result in event context_data");
    assert!(
        found_signal,
        "runner must persist author signal `db_test_complete` under context_metadata.signals"
    );

    Ok(())
}

/// Test that `execute_dispatch()` propagates context between tasks.
///
/// This is the critical test — it verifies the function the API handler actually
/// calls correctly accumulates data across separate Engine invocations.
///
/// Task 1: Produces `{ items: [...], greeting: "hello" }` at `data.nu_output`
/// Task 2: Reads `$in.nu_output.items` (from task 1's accumulated output), transforms them
///
/// Both tasks are in the same step, same stage, same playbook.
#[tokio::test]
async fn execute_dispatch_with_context_accumulation()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {:?}. Skipping test.", e);
            return Ok(());
        }
    };

    // Task 1: Produce structured data (no input needed)
    let task1_script = r#"
        {
            payload: {
                items: ["apple", "banana", "cherry"],
                greeting: "hello"
            },
            signals: { produced: true }
        }
    "#;

    // Task 2: Read accumulated context from task 1 via $in directly.
    // execute_dispatch flattens nu_output contents to top level of accumulated_data,
    // so task 1's payload fields are available as $in.items, $in.greeting, etc.
    let task2_script = r#"
        let transformed = $in.items | each { |it| $it | str upcase };
        {
            payload: {
                uppercased: $transformed,
                received_greeting: $in.greeting,
                count: ($transformed | length)
            },
            signals: { transformed: true }
        }
    "#;

    // Insert both tasks
    let task_args = vec![
        InsertTaskArgs {
            names: vec![format!("ctx-producer-{}", Uuid::now_v7())],
            descriptions: vec!["Produces data for accumulation test".to_string()],
            functions: vec!["nushell".to_string()],
            function_inputs: vec![json!({ "script": task1_script })],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        },
        InsertTaskArgs {
            names: vec![format!("ctx-consumer-{}", Uuid::now_v7())],
            descriptions: vec!["Consumes accumulated context".to_string()],
            functions: vec!["nushell".to_string()],
            function_inputs: vec![json!({ "script": task2_script })],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        },
    ];

    let tasks = insert_tasks(&pool, task_args).await?;
    assert_eq!(tasks.len(), 2, "should have inserted 2 tasks");

    // Create a step referencing BOTH tasks (order matters for context flow)
    let step_args = dapple_data::model::steps::InsertStepArgs {
        names: vec![format!("ctx-test-step-{}", Uuid::now_v7())],
        descriptions: vec![Some(
            "step with two tasks for context accumulation".to_string(),
        )],
        description_fluent_message_ids: Vec::new(),
        tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: vec![tasks[0].id, tasks[1].id],
        }],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let inserted_steps = dapple_data::query::steps::insert_steps(&pool, vec![step_args]).await?;

    // Create playbook with both tasks in the step
    let playbook_args = InsertPlaybookArgs {
        names: vec![format!("ctx_playbook_{}", Uuid::now_v7())],
        descriptions: vec!["playbook for context accumulation test".to_string()],
        stages: vec![InsertPlaybookStageArgs {
            name: format!("ctx-test-stage-{}", Uuid::now_v7()),
            description: Some("stage for context accumulation test".to_string()),
            description_fluent_message_id: None,
            kind: None,
            icon: Vec::new(),
            step_ids: vec![inserted_steps[0].id],
            stage_id: None,
            content_hash: Some("0000000000000000000000000000000000000000".to_string()),
            reused: false,
        }],
        categories: vec![
            dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs {
                playbook_category_ids: vec![],
            },
        ],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let pbs = dapple_data::query::playbooks::insert_playbooks(&pool, vec![playbook_args]).await?;
    let pb = pbs.into_iter().next().ok_or("no playbook inserted")?;

    // Create user + journey
    let user_detail = dapple_data::test_support::insert_test_user_and_user_detail(
        &pool,
        "ctx-accumulation-test@example.com",
    )
    .await?;

    let principal_id = dapple_data::query::principals::select_human_principal_for_user_object_id(
        &pool,
        user_detail.id,
    )
    .await?
    .ok_or("no human_principal for test user")?;

    let jargs = InsertJourneyArgs {
        playbook_ids: vec![pb.id],
        principal_ids: vec![principal_id],
        subject_principal_ids: vec![None],
        billable_entity_ids: vec![None],
        names: vec!["ctx-accumulation-test-journey".to_string()],
        owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
        predecessor_journey_ids: vec![None],
        guide_ids: vec![None],
        fork_stage_ids: vec![None],
    };

    let js = insert_journeys(&pool, vec![jargs]).await?;
    let journey = js.into_iter().next().ok_or("no journey")?;

    // Build dispatch with both tasks
    let stage = &pb.stages[0];
    let step = &stage.steps[0];

    let dispatch_args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage.id],
                params: vec![json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step.id],
                        params: vec![json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: step.tasks.iter().map(|t| t.id).collect(),
                            },
                        ],
                    },
                ],
            },
        ],
    };

    let dispatches = insert_dispatches(&pool, vec![journey.id], vec![dispatch_args]).await?;
    let dispatch = dispatches
        .into_iter()
        .next()
        .ok_or("no dispatch inserted")?;

    // Run execute_dispatch — the function the API handler actually calls
    execute_dispatch(&pool, dispatch.clone(), 4).await?;

    // Verify both tasks completed successfully
    let attempts = select_task_attempts_by_dispatch_id(&pool, dispatch.id).await?;
    assert_eq!(
        attempts.len(),
        2,
        "should have exactly 2 attempts (one per task)"
    );

    let mut task1_ok = false;
    let mut task2_ok = false;
    let mut signal_produced = false;
    let mut signal_transformed = false;

    for attempt in attempts.iter() {
        let events = select_attempt_events_by_attempt_object_id(&pool, attempt.id).await?;

        for event in events.iter() {
            if event.phase_code == "ok" {
                // Primary `execute_dispatch` path: author signals persist in full
                // under `context_metadata.signals` for both tasks in the step.
                if let Some(meta) = &event.context_metadata
                    && let Some(signals) = meta.get("signals")
                {
                    if signals.get("produced") == Some(&json!(true)) {
                        signal_produced = true;
                    }
                    if signals.get("transformed") == Some(&json!(true)) {
                        signal_transformed = true;
                    }
                }
                if let Some(context_data) = &event.context_data {
                    // After flattening, context_data IS the payload directly
                    // (no nu_output wrapper). Task 1 has "items" + "greeting",
                    // Task 2 has "uppercased" + "received_greeting".
                    if context_data.get("items").is_some()
                        && context_data.get("greeting").is_some()
                        && context_data.get("uppercased").is_none()
                    {
                        assert_eq!(
                            context_data["items"],
                            json!(["apple", "banana", "cherry"]),
                            "task 1 should produce original items"
                        );
                        assert_eq!(
                            context_data["greeting"],
                            json!("hello"),
                            "task 1 should produce greeting"
                        );
                        task1_ok = true;
                    }
                    if context_data.get("uppercased").is_some() {
                        assert_eq!(
                            context_data["uppercased"],
                            json!(["APPLE", "BANANA", "CHERRY"]),
                            "task 2 should uppercase items from task 1"
                        );
                        assert_eq!(
                            context_data["received_greeting"],
                            json!("hello"),
                            "task 2 should receive greeting from task 1"
                        );
                        assert_eq!(context_data["count"], json!(3), "task 2 should count items");
                        task2_ok = true;
                    }
                }
            }
        }
    }

    assert!(
        task1_ok,
        "task 1 should have completed with 'ok' phase and correct output"
    );
    assert!(
        task2_ok,
        "task 2 should have completed with 'ok' phase and data from task 1 (proves context accumulation)"
    );
    assert!(
        signal_produced,
        "task 1's `produced` signal must persist under context_metadata.signals"
    );
    assert!(
        signal_transformed,
        "task 2's `transformed` signal must persist under context_metadata.signals"
    );

    Ok(())
}

/// Build a single-stage / single-step playbook whose step contains `scripts`
/// as ordered nushell tasks, wire a journey + dispatch, and run it through
/// `execute_dispatch`. Returns the dispatch so the caller can inspect the
/// resulting task attempts. Returns `Ok(None)` if the DB is unavailable.
async fn run_single_step_with_scripts(
    label: &str,
    scripts: &[&str],
) -> Result<
    Option<(DbPool, dapple_data::model::dispatches::DispatchModel)>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {:?}. Skipping test.", e);
            return Ok(None);
        }
    };

    let task_args: Vec<InsertTaskArgs> = scripts
        .iter()
        .enumerate()
        .map(|(i, script)| InsertTaskArgs {
            names: vec![format!("{label}-task{i}-{}", Uuid::now_v7())],
            descriptions: vec![format!("{label} task {i}")],
            functions: vec!["nushell".to_string()],
            function_inputs: vec![json!({ "script": script })],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        })
        .collect();
    let tasks = insert_tasks(&pool, task_args).await?;

    let step_args = dapple_data::model::steps::InsertStepArgs {
        names: vec![format!("{label}-step-{}", Uuid::now_v7())],
        descriptions: vec![Some(format!("{label} step"))],
        description_fluent_message_ids: Vec::new(),
        tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: tasks.iter().map(|t| t.id).collect(),
        }],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };
    let inserted_steps = dapple_data::query::steps::insert_steps(&pool, vec![step_args]).await?;

    let playbook_args = InsertPlaybookArgs {
        names: vec![format!("{label}_pb_{}", Uuid::now_v7())],
        descriptions: vec![format!("{label} playbook")],
        stages: vec![InsertPlaybookStageArgs {
            name: format!("{label}-stage-{}", Uuid::now_v7()),
            description: Some(format!("{label} stage")),
            description_fluent_message_id: None,
            kind: None,
            icon: Vec::new(),
            step_ids: vec![inserted_steps[0].id],
            stage_id: None,
            content_hash: Some("0000000000000000000000000000000000000000".to_string()),
            reused: false,
        }],
        categories: vec![
            dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs {
                playbook_category_ids: vec![],
            },
        ],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };
    let pbs = dapple_data::query::playbooks::insert_playbooks(&pool, vec![playbook_args]).await?;
    let pb = pbs.into_iter().next().ok_or("no playbook inserted")?;

    let user_detail = dapple_data::test_support::insert_test_user_and_user_detail(
        &pool,
        &format!("{label}-{}@example.com", Uuid::now_v7()),
    )
    .await?;
    let principal_id = dapple_data::query::principals::select_human_principal_for_user_object_id(
        &pool,
        user_detail.id,
    )
    .await?
    .ok_or("no human_principal for test user")?;

    let jargs = InsertJourneyArgs {
        playbook_ids: vec![pb.id],
        principal_ids: vec![principal_id],
        subject_principal_ids: vec![None],
        billable_entity_ids: vec![None],
        names: vec![format!("{label}-journey")],
        owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
        predecessor_journey_ids: vec![None],
        guide_ids: vec![None],
        fork_stage_ids: vec![None],
    };
    let js = insert_journeys(&pool, vec![jargs]).await?;
    let journey = js.into_iter().next().ok_or("no journey")?;

    let stage = &pb.stages[0];
    let step = &stage.steps[0];
    let dispatch_args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage.id],
                params: vec![json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step.id],
                        params: vec![json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: step.tasks.iter().map(|t| t.id).collect(),
                            },
                        ],
                    },
                ],
            },
        ],
    };
    let dispatches = insert_dispatches(&pool, vec![journey.id], vec![dispatch_args]).await?;
    let dispatch = dispatches
        .into_iter()
        .next()
        .ok_or("no dispatch inserted")?;

    execute_dispatch(&pool, dispatch.clone(), 4).await?;
    Ok(Some((pool, dispatch)))
}

/// Count terminal phase events of a given code across all of a dispatch's task
/// attempts.
async fn count_phase_events(
    pool: &DbPool,
    dispatch_id: Uuid,
    phase: &str,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let attempts = select_task_attempts_by_dispatch_id(pool, dispatch_id).await?;
    let mut n = 0;
    for attempt in &attempts {
        for event in select_attempt_events_by_attempt_object_id(pool, attempt.id).await? {
            if event.phase_code == phase {
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Fail-together: when a task in a step errors, the step STOPS — later tasks in
/// the step never run (no attempt row), while the prior successful task is still
/// recorded as `ok` (history is never rolled back).
#[tokio::test]
async fn execute_dispatch_stops_step_on_task_failure()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ok_script = r#"{ payload: { n: 1 }, signals: {} }"#;
    let fail_script = r#"error make { msg: "intentional task failure" }"#;
    // task2 fails; task3 must never run.
    let scripts = [ok_script, fail_script, ok_script];
    let Some((pool, dispatch)) = run_single_step_with_scripts("failstop", &scripts).await? else {
        return Ok(());
    };

    let attempts = select_task_attempts_by_dispatch_id(&pool, dispatch.id).await?;
    assert_eq!(
        attempts.len(),
        2,
        "only the first two tasks should have attempt rows; the third must not run"
    );
    assert_eq!(
        count_phase_events(&pool, dispatch.id, "ok").await?,
        1,
        "the first task succeeded and is recorded ok (history kept)"
    );
    assert_eq!(
        count_phase_events(&pool, dispatch.id, "failed").await?,
        1,
        "the second task failed"
    );
    assert_eq!(
        count_phase_events(&pool, dispatch.id, "unsafe").await?,
        0,
        "a plain error is `failed`, not `unsafe`"
    );
    Ok(())
}

/// A task that emits `signals.unsafe = true` vetoes the step: its attempt ends in
/// the terminal `unsafe` phase and later tasks never run.
#[tokio::test]
async fn execute_dispatch_unsafe_signal_vetoes_step()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let unsafe_script =
        r#"{ payload: { tainted: true }, signals: { unsafe: true, reason: "poisoned" } }"#;
    let ok_script = r#"{ payload: { n: 2 }, signals: {} }"#;
    // task1 vetoes; task2 must never run.
    let scripts = [unsafe_script, ok_script];
    let Some((pool, dispatch)) = run_single_step_with_scripts("unsafeveto", &scripts).await? else {
        return Ok(());
    };

    let attempts = select_task_attempts_by_dispatch_id(&pool, dispatch.id).await?;
    assert_eq!(
        attempts.len(),
        1,
        "the unsafe veto stops the step; the second task must not run"
    );
    assert_eq!(
        count_phase_events(&pool, dispatch.id, "unsafe").await?,
        1,
        "the vetoing task ends in the terminal `unsafe` phase"
    );
    assert_eq!(
        count_phase_events(&pool, dispatch.id, "ok").await?,
        0,
        "an unsafe veto is not an `ok`"
    );
    Ok(())
}

/// Test that `execute_dispatch()` seeds journey context from prior dispatches.
///
/// Dispatch 1: runs task A which produces `{ items: [...], greeting: "hello" }`
/// Dispatch 2: runs task B in the same journey, which reads `$in.items` from prior context
///
/// This verifies that dispatch N can see completed task outputs from dispatches 1..N-1
/// via journey context seeding, without the client shuttling data between calls.
#[tokio::test]
async fn execute_dispatch_multi_dispatch_journey_context()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {:?}. Skipping test.", e);
            return Ok(());
        }
    };

    // Task A: Produce structured data
    let task_a_script = r#"
        {
            payload: {
                items: ["apple", "banana", "cherry"],
                greeting: "hello from dispatch 1"
            },
            signals: { produced: true }
        }
    "#;

    // Task B: Read accumulated context from task A via $in (seeded from prior dispatch)
    let task_b_script = r#"
        let transformed = $in.items | each { |it| $it | str upcase };
        {
            payload: {
                uppercased: $transformed,
                received_greeting: $in.greeting,
                count: ($transformed | length)
            },
            signals: { transformed: true }
        }
    "#;

    // Insert task A
    let task_a_args = InsertTaskArgs {
        names: vec![format!("journey-ctx-producer-{}", Uuid::now_v7())],
        descriptions: vec!["Produces data for journey context test".to_string()],
        functions: vec!["nushell".to_string()],
        function_inputs: vec![json!({ "script": task_a_script })],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    // Insert task B
    let task_b_args = InsertTaskArgs {
        names: vec![format!("journey-ctx-consumer-{}", Uuid::now_v7())],
        descriptions: vec!["Consumes journey context from prior dispatch".to_string()],
        functions: vec!["nushell".to_string()],
        function_inputs: vec![json!({ "script": task_b_script })],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let tasks = insert_tasks(&pool, vec![task_a_args, task_b_args]).await?;
    assert_eq!(tasks.len(), 2, "should have inserted 2 tasks");

    // Create separate steps for each task (each dispatch targets a different step)
    let step_a_args = dapple_data::model::steps::InsertStepArgs {
        names: vec![format!("journey-ctx-step-a-{}", Uuid::now_v7())],
        descriptions: vec![Some("step for task A".to_string())],
        description_fluent_message_ids: Vec::new(),
        tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: vec![tasks[0].id],
        }],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let step_b_args = dapple_data::model::steps::InsertStepArgs {
        names: vec![format!("journey-ctx-step-b-{}", Uuid::now_v7())],
        descriptions: vec![Some("step for task B".to_string())],
        description_fluent_message_ids: Vec::new(),
        tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: vec![tasks[1].id],
        }],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let inserted_steps =
        dapple_data::query::steps::insert_steps(&pool, vec![step_a_args, step_b_args]).await?;
    assert_eq!(inserted_steps.len(), 2, "should have inserted 2 steps");

    // Create playbook with both steps in separate stages
    let stage_a = InsertPlaybookStageArgs {
        name: format!("journey-ctx-stage-a-{}", Uuid::now_v7()),
        description: Some("stage for step A".to_string()),
        description_fluent_message_id: None,
        kind: None,
        icon: Vec::new(),
        step_ids: vec![inserted_steps[0].id],
        stage_id: None,
        content_hash: Some("0000000000000000000000000000000000000000".to_string()),
        reused: false,
    };

    let stage_b = InsertPlaybookStageArgs {
        name: format!("journey-ctx-stage-b-{}", Uuid::now_v7()),
        description: Some("stage for step B".to_string()),
        description_fluent_message_id: None,
        kind: None,
        icon: Vec::new(),
        step_ids: vec![inserted_steps[1].id],
        stage_id: None,
        content_hash: Some("0000000000000000000000000000000000000000".to_string()),
        reused: false,
    };

    let playbook_args = InsertPlaybookArgs {
        names: vec![format!("journey_ctx_playbook_{}", Uuid::now_v7())],
        descriptions: vec!["playbook for journey context test".to_string()],
        stages: vec![stage_a, stage_b],
        categories: vec![
            dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs {
                playbook_category_ids: vec![],
            },
        ],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };

    let pbs = dapple_data::query::playbooks::insert_playbooks(&pool, vec![playbook_args]).await?;
    let pb = pbs.into_iter().next().ok_or("no playbook inserted")?;
    assert_eq!(pb.stages.len(), 2, "playbook should have 2 stages");

    // Create user + journey
    let user_detail = dapple_data::test_support::insert_test_user_and_user_detail(
        &pool,
        "journey-ctx-test@example.com",
    )
    .await?;

    let principal_id = dapple_data::query::principals::select_human_principal_for_user_object_id(
        &pool,
        user_detail.id,
    )
    .await?
    .ok_or("no human_principal for test user")?;

    let jargs = InsertJourneyArgs {
        playbook_ids: vec![pb.id],
        principal_ids: vec![principal_id],
        subject_principal_ids: vec![None],
        billable_entity_ids: vec![None],
        names: vec!["journey-ctx-test-journey".to_string()],
        owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
        predecessor_journey_ids: vec![None],
        guide_ids: vec![None],
        fork_stage_ids: vec![None],
    };

    let js = insert_journeys(&pool, vec![jargs]).await?;
    let journey = js.into_iter().next().ok_or("no journey")?;

    // --- Dispatch 1: targets stage A / step A / task A ---
    let stage_a_ref = &pb.stages[0];
    let step_a_ref = &stage_a_ref.steps[0];

    let dispatch1_args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage_a_ref.id],
                params: vec![json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step_a_ref.id],
                        params: vec![json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: vec![step_a_ref.tasks[0].id],
                            },
                        ],
                    },
                ],
            },
        ],
    };

    let dispatches1 = insert_dispatches(&pool, vec![journey.id], vec![dispatch1_args]).await?;
    let dispatch1 = dispatches1
        .into_iter()
        .next()
        .ok_or("no dispatch 1 inserted")?;

    // Execute dispatch 1 (task A runs)
    execute_dispatch(&pool, dispatch1.clone(), 4).await?;

    // Verify dispatch 1 completed
    let attempts1 = select_task_attempts_by_dispatch_id(&pool, dispatch1.id).await?;
    assert!(!attempts1.is_empty(), "dispatch 1 should have attempts");

    let mut dispatch1_ok = false;
    for attempt in attempts1.iter() {
        let events = select_attempt_events_by_attempt_object_id(&pool, attempt.id).await?;
        for event in events.iter() {
            if event.phase_code == "ok" {
                dispatch1_ok = true;
            }
        }
    }
    assert!(dispatch1_ok, "dispatch 1 should have completed with 'ok'");

    // --- Dispatch 2: targets stage B / step B / task B (same journey) ---
    let stage_b_ref = &pb.stages[1];
    let step_b_ref = &stage_b_ref.steps[0];

    let dispatch2_args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage_b_ref.id],
                params: vec![json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step_b_ref.id],
                        params: vec![json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: vec![step_b_ref.tasks[0].id],
                            },
                        ],
                    },
                ],
            },
        ],
    };

    let dispatches2 = insert_dispatches(&pool, vec![journey.id], vec![dispatch2_args]).await?;
    let dispatch2 = dispatches2
        .into_iter()
        .next()
        .ok_or("no dispatch 2 inserted")?;

    // Execute dispatch 2 (task B runs with journey context from dispatch 1)
    execute_dispatch(&pool, dispatch2.clone(), 4).await?;

    // Verify dispatch 2 completed and task B received data from task A
    let attempts2 = select_task_attempts_by_dispatch_id(&pool, dispatch2.id).await?;
    assert!(!attempts2.is_empty(), "dispatch 2 should have attempts");

    let mut task_b_ok = false;
    for attempt in attempts2.iter() {
        let events = select_attempt_events_by_attempt_object_id(&pool, attempt.id).await?;
        for event in events.iter() {
            if event.phase_code == "ok"
                && let Some(context_data) = &event.context_data
            {
                // After flattening, context_data IS the payload directly
                // (no nu_output wrapper).
                if context_data.get("uppercased").is_some() {
                    assert_eq!(
                        context_data["uppercased"],
                        json!(["APPLE", "BANANA", "CHERRY"]),
                        "task B should uppercase items from task A (cross-dispatch)"
                    );
                    assert_eq!(
                        context_data["received_greeting"],
                        json!("hello from dispatch 1"),
                        "task B should receive greeting from task A (cross-dispatch)"
                    );
                    assert_eq!(context_data["count"], json!(3), "task B should count items");
                    task_b_ok = true;
                }
            }
        }
    }

    assert!(
        task_b_ok,
        "task B should have completed with 'ok' and data from task A (proves cross-dispatch journey context seeding)"
    );

    Ok(())
}

/// Regression (double-billing): two concurrent `execute_dispatch` calls on ONE
/// dispatch must produce ONE attempt per declared task, not two.
///
/// Two entry points reach a single dispatch independently: the API's post-binding
/// delegation (`execute_dispatch_and_retrigger` → straight to `execute_dispatch`)
/// and a poll-triggered `reconcile_journey_dispatches` drain. `drains_in_flight`
/// only excludes drains from EACH OTHER, and the drain's "unstarted" filter
/// (`select_unstarted_dispatch_ids_for_journey`) keys off task-attempt ROWS — so
/// between a direct execute starting and its first attempt row landing, the
/// dispatch still looks unstarted and the drain runs it a second time. The UI polls
/// `/steps` every ~2s and each poll spawns a reconcile, so that window is sampled
/// constantly.
///
/// Observed live on run-the-10k "Fetch the Annual Record": 2 declared tasks → 4
/// attempts ~100ms apart, every EDGAR fetch and LLM call billed twice. That also
/// makes this the guard for non-deterministic generates now that the runtime
/// auto-dispatches picker feeds — a duplicated execution is exactly the
/// two-different-candidate-arrays flap.
#[tokio::test]
async fn execute_dispatch_is_not_run_twice_concurrently()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {:?}. Skipping test.", e);
            return Ok(());
        }
    };

    // Two trivial tasks in one step. The scripts only need to succeed; what is
    // under test is how many ATTEMPT ROWS exist afterwards.
    let task_args = vec![
        InsertTaskArgs {
            names: vec![format!("dup-guard-a-{}", Uuid::now_v7())],
            descriptions: vec!["first task of the double-run guard test".to_string()],
            functions: vec!["nushell".to_string()],
            function_inputs: vec![json!({ "script": "{ payload: { a: 1 }, signals: {} }" })],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        },
        InsertTaskArgs {
            names: vec![format!("dup-guard-b-{}", Uuid::now_v7())],
            descriptions: vec!["second task of the double-run guard test".to_string()],
            functions: vec!["nushell".to_string()],
            function_inputs: vec![json!({ "script": "{ payload: { b: 2 }, signals: {} }" })],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        },
    ];
    let tasks = insert_tasks(&pool, task_args).await?;

    let step_args = dapple_data::model::steps::InsertStepArgs {
        names: vec![format!("dup-guard-step-{}", Uuid::now_v7())],
        descriptions: vec![Some(
            "step with two tasks for the double-run guard".to_string(),
        )],
        description_fluent_message_ids: Vec::new(),
        tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: vec![tasks[0].id, tasks[1].id],
        }],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };
    let inserted_steps = dapple_data::query::steps::insert_steps(&pool, vec![step_args]).await?;

    let playbook_args = InsertPlaybookArgs {
        names: vec![format!("dup_guard_playbook_{}", Uuid::now_v7())],
        descriptions: vec!["playbook for the double-run guard test".to_string()],
        stages: vec![InsertPlaybookStageArgs {
            name: format!("dup-guard-stage-{}", Uuid::now_v7()),
            description: Some("stage for the double-run guard test".to_string()),
            description_fluent_message_id: None,
            kind: None,
            icon: Vec::new(),
            step_ids: vec![inserted_steps[0].id],
            stage_id: None,
            content_hash: Some("0000000000000000000000000000000000000000".to_string()),
            reused: false,
        }],
        categories: vec![
            dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs {
                playbook_category_ids: vec![],
            },
        ],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };
    let pbs = dapple_data::query::playbooks::insert_playbooks(&pool, vec![playbook_args]).await?;
    let pb = pbs.into_iter().next().ok_or("no playbook inserted")?;

    let user_detail = dapple_data::test_support::insert_test_user_and_user_detail(
        &pool,
        "dup-guard-test@example.com",
    )
    .await?;
    let principal_id = dapple_data::query::principals::select_human_principal_for_user_object_id(
        &pool,
        user_detail.id,
    )
    .await?
    .ok_or("no human_principal for test user")?;

    let jargs = InsertJourneyArgs {
        playbook_ids: vec![pb.id],
        principal_ids: vec![principal_id],
        subject_principal_ids: vec![None],
        billable_entity_ids: vec![None],
        names: vec!["dup-guard-test-journey".to_string()],
        owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
        predecessor_journey_ids: vec![None],
        guide_ids: vec![None],
        fork_stage_ids: vec![None],
    };
    let js = insert_journeys(&pool, vec![jargs]).await?;
    let journey = js.into_iter().next().ok_or("no journey")?;

    let stage = &pb.stages[0];
    let step = &stage.steps[0];
    let dispatch_args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage.id],
                params: vec![json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step.id],
                        params: vec![json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: step.tasks.iter().map(|t| t.id).collect(),
                            },
                        ],
                    },
                ],
            },
        ],
    };
    let dispatches = insert_dispatches(&pool, vec![journey.id], vec![dispatch_args]).await?;
    let dispatch = dispatches
        .into_iter()
        .next()
        .ok_or("no dispatch inserted")?;

    // The race: both callers fire at once, exactly as the direct `/execute` and a
    // poll-triggered drain do in production.
    let (first, second) = tokio::join!(
        execute_dispatch(&pool, dispatch.clone(), 4),
        execute_dispatch(&pool, dispatch.clone(), 4),
    );
    first?;
    second?;

    let attempts = select_task_attempts_by_dispatch_id(&pool, dispatch.id).await?;
    assert_eq!(
        attempts.len(),
        2,
        "one attempt per declared task — the loser of the race must skip, not re-run \
         the step (got {} attempts for 2 tasks, i.e. every task billed twice)",
        attempts.len(),
    );

    Ok(())
}

// =============================================================================
// C1: drain-scoped SeedCache (journey-context seed reuse + watermark)
// =============================================================================

/// Build the single-step fixture WITHOUT executing anything, returning the
/// pool, journey id, and playbook so a test can insert several dispatches on
/// the same journey and drive them through the seed cache itself.
async fn build_seed_cache_fixture(
    label: &str,
    script: &str,
) -> Result<
    Option<(DbPool, Uuid, dapple_data::model::playbooks::Playbook)>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {:?}. Skipping test.", e);
            return Ok(None);
        }
    };

    let tasks = insert_tasks(
        &pool,
        vec![InsertTaskArgs {
            names: vec![format!("{label}-task-{}", Uuid::now_v7())],
            descriptions: vec![format!("{label} task")],
            functions: vec!["nushell".to_string()],
            function_inputs: vec![json!({ "script": script })],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        }],
    )
    .await?;

    let inserted_steps = dapple_data::query::steps::insert_steps(
        &pool,
        vec![dapple_data::model::steps::InsertStepArgs {
            names: vec![format!("{label}-step-{}", Uuid::now_v7())],
            descriptions: vec![Some(format!("{label} step"))],
            description_fluent_message_ids: Vec::new(),
            tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
                task_ids: vec![tasks[0].id],
            }],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        }],
    )
    .await?;

    let pbs = dapple_data::query::playbooks::insert_playbooks(
        &pool,
        vec![InsertPlaybookArgs {
            names: vec![format!("{label}_pb_{}", Uuid::now_v7())],
            descriptions: vec![format!("{label} playbook")],
            stages: vec![InsertPlaybookStageArgs {
                name: format!("{label}-stage-{}", Uuid::now_v7()),
                description: Some(format!("{label} stage")),
                description_fluent_message_id: None,
                kind: None,
                icon: Vec::new(),
                step_ids: vec![inserted_steps[0].id],
                stage_id: None,
                content_hash: Some("0000000000000000000000000000000000000000".to_string()),
                reused: false,
            }],
            categories: vec![
                dapple_data::model::playbooks::playbook_categories::InsertPlaybookCategoryArgs {
                    playbook_category_ids: vec![],
                },
            ],
            content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
        }],
    )
    .await?;
    let pb = pbs.into_iter().next().ok_or("no playbook inserted")?;

    let user_detail = dapple_data::test_support::insert_test_user_and_user_detail(
        &pool,
        &format!("{label}-{}@example.com", Uuid::now_v7()),
    )
    .await?;
    let principal_id = dapple_data::query::principals::select_human_principal_for_user_object_id(
        &pool,
        user_detail.id,
    )
    .await?
    .ok_or("no human_principal for test user")?;

    let js = insert_journeys(
        &pool,
        vec![InsertJourneyArgs {
            playbook_ids: vec![pb.id],
            principal_ids: vec![principal_id],
            subject_principal_ids: vec![None],
            billable_entity_ids: vec![None],
            names: vec![format!("{label}-journey")],
            owner_company_ids: vec![dapple_data::DAPPLE_COMPANY_UUID],
            predecessor_journey_ids: vec![None],
            guide_ids: vec![None],
            fork_stage_ids: vec![None],
        }],
    )
    .await?;
    let journey = js.into_iter().next().ok_or("no journey")?;

    Ok(Some((pool, journey.id, pb)))
}

async fn insert_one_dispatch(
    pool: &DbPool,
    journey_id: Uuid,
    pb: &dapple_data::model::playbooks::Playbook,
) -> Result<dapple_data::model::dispatches::DispatchModel, Box<dyn std::error::Error + Send + Sync>>
{
    let stage = &pb.stages[0];
    let step = &stage.steps[0];
    let dispatch_args = dapple_data::model::dispatches::InsertDispatchArgs {
        intents: vec![json!({})],
        stages: vec![
            dapple_data::model::dispatches::dispatch_stages::InsertDispatchStagesArgs {
                playbook_stage_ids: vec![stage.id],
                params: vec![json!({})],
                steps: vec![
                    dapple_data::model::dispatches::dispatch_steps::InsertDispatchStepsArgs {
                        stage_step_ids: vec![step.id],
                        params: vec![json!({})],
                        tasks: vec![
                            dapple_data::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs {
                                step_task_ids: step.tasks.iter().map(|t| t.id).collect(),
                            },
                        ],
                    },
                ],
            },
        ],
    };
    let dispatches = insert_dispatches(pool, vec![journey_id], vec![dispatch_args]).await?;
    Ok(dispatches
        .into_iter()
        .next()
        .ok_or("no dispatch inserted")?)
}

/// The slot names a dispatch's task saw under `$in.bindings` (echoed into the
/// ok event's flattened context_data by the fixture script).
async fn seen_binding_slots(
    pool: &DbPool,
    dispatch_id: Uuid,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut seen = Vec::new();
    for attempt in select_task_attempts_by_dispatch_id(pool, dispatch_id).await? {
        for event in select_attempt_events_by_attempt_object_id(pool, attempt.id).await? {
            if event.phase_code == "ok"
                && let Some(ctx) = &event.context_data
                && let Some(arr) = ctx.get("seen").and_then(|v| v.as_array())
            {
                seen.extend(arr.iter().filter_map(|v| v.as_str().map(str::to_owned)));
            }
        }
    }
    Ok(seen)
}

/// The correctness half of the seed cache: a binding written between two
/// executions (exactly what an operator POST from the api pod looks like to a
/// flow-pod drain) MUST reach the next dispatch even though the cache is
/// populated — the append-only watermark mismatch forces a reload. Asserted on
/// what the TASK actually saw in `$in.bindings`, not on cache internals.
#[tokio::test]
async fn seed_cache_watermark_reloads_on_new_binding()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use dapple_prompt_flow::dispatch_runner::{SeedCache, execute_dispatch_with_seed_cache};

    let script = r#"let b = ($in.bindings? | default {}); { payload: { seen: ($b | columns) }, signals: {} }"#;
    let Some((pool, journey_id, pb)) = build_seed_cache_fixture("seedcache-wm", script).await?
    else {
        return Ok(());
    };
    let d1 = insert_one_dispatch(&pool, journey_id, &pb).await?;
    let d2 = insert_one_dispatch(&pool, journey_id, &pb).await?;

    let mut cache = SeedCache::new(journey_id);
    let out1 = execute_dispatch_with_seed_cache(&pool, d1.clone(), 4, Some(&mut cache)).await?;
    assert!(out1.any_task_ok, "echo task must succeed");
    assert!(cache.is_populated(), "first run populates the cache");
    assert_eq!(
        seen_binding_slots(&pool, d1.id).await?,
        Vec::<String>::new(),
        "no bindings exist yet — d1 sees an empty $in.bindings"
    );

    // A binding lands from "elsewhere" (operator POST / other pod): raw insert,
    // not through this process's execute path, so any_task_ok never fires.
    let slot_name = format!("seedcache-slot-{}", Uuid::now_v7());
    let slot = dapple_data::query::value_slots::insert_value_slots(
        &pool,
        dapple_data::model::value_slots::InsertValueSlotArgs {
            names: vec![slot_name.clone()],
            value_kind_keys: vec!["text".to_string()],
            titles: vec![slot_name.clone()],
            descriptions: vec![None],
            constraints: vec![json!({})],
            cardinalities: vec!["single".to_string()],
            min_selections: vec![None],
            max_selections: vec![None],
            content_hashes: vec![
                dapple_data::hash::hash_value_slot(
                    &slot_name, "", "text", &None, "single", None, None,
                )
                .as_str()
                .to_string(),
            ],
        },
    )
    .await?[0]
        .id;
    let binding_id: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at)
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(d1.id)
    .bind(slot)
    .fetch_one(pool.get_pool())
    .await?;
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_binding_text_items
             (dispatch_intent_binding_id, value) VALUES ($1, $2)",
    )
    .bind(binding_id)
    .bind("hello")
    .execute(pool.get_pool())
    .await?;

    let out2 = execute_dispatch_with_seed_cache(&pool, d2.clone(), 4, Some(&mut cache)).await?;
    assert!(out2.any_task_ok);
    assert_eq!(
        seen_binding_slots(&pool, d2.id).await?,
        vec![slot_name],
        "the watermark must catch the out-of-band binding and reload the seed — \
         a stale cache here would hand the task an empty $in.bindings"
    );
    Ok(())
}

/// The retention half: a dispatch that only FAILED (no ok task, no binding)
/// changes nothing the seed reads, so the cache survives for the retry — the
/// exact loop that re-seeded three times back-to-back and OOMed flow.
#[tokio::test]
async fn seed_cache_survives_failed_dispatch()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use dapple_prompt_flow::dispatch_runner::{SeedCache, execute_dispatch_with_seed_cache};

    let script = r#"error make { msg: "deliberate failure for seed-cache test" }"#;
    let Some((pool, journey_id, pb)) = build_seed_cache_fixture("seedcache-fail", script).await?
    else {
        return Ok(());
    };
    let d1 = insert_one_dispatch(&pool, journey_id, &pb).await?;
    let d2 = insert_one_dispatch(&pool, journey_id, &pb).await?;

    let mut cache = SeedCache::new(journey_id);
    let out1 = execute_dispatch_with_seed_cache(&pool, d1.clone(), 4, Some(&mut cache)).await?;
    assert!(!out1.any_task_ok, "failing task must not report ok");
    assert!(cache.is_populated(), "seed loaded despite the failure");

    // The retry (a fresh dispatch, same journey): nothing changed, so the
    // populated cache is served — and stays populated for further retries.
    let out2 = execute_dispatch_with_seed_cache(&pool, d2.clone(), 4, Some(&mut cache)).await?;
    assert!(!out2.any_task_ok);
    assert!(
        cache.is_populated(),
        "cache survives an all-failed dispatch"
    );
    Ok(())
}

/// C2 envelope delivery: a json slot authored `constraints.delivery =
/// "envelope"` seeds `$in.bindings.<slot>` as a HANDLE (no payload rides the
/// seed), and `binding open <binding_id>` resolves the full value on demand.
/// Asserted end-to-end from inside a task script — the exact author experience.
/// Multi-thread flavor: `binding open` resolves its DB read via
/// `tokio::task::block_in_place`, which the current-thread test runtime forbids
/// (production runtimes are always multi-threaded).
#[tokio::test(flavor = "multi_thread")]
async fn envelope_slot_seeds_handle_and_binding_open_resolves()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let slot_name = format!("env-slot-{}", Uuid::now_v7().simple());
    let script = format!(
        r#"let h = ($in.bindings | get "{slot_name}")
let v = (binding open $h.binding_id)
{{ payload: {{ handle_delivery: $h.delivery, opened: $v }}, signals: {{}} }}"#
    );
    let Some((pool, journey_id, pb)) = build_seed_cache_fixture("envelope", &script).await? else {
        return Ok(());
    };
    let d1 = insert_one_dispatch(&pool, journey_id, &pb).await?;

    // The slot opts into envelope delivery at authoring time.
    let slot = dapple_data::query::value_slots::insert_value_slots(
        &pool,
        dapple_data::model::value_slots::InsertValueSlotArgs {
            names: vec![slot_name.clone()],
            value_kind_keys: vec!["json".to_string()],
            titles: vec![slot_name.clone()],
            descriptions: vec![None],
            constraints: vec![json!({ "delivery": "envelope" })],
            cardinalities: vec!["single".to_string()],
            min_selections: vec![None],
            max_selections: vec![None],
            content_hashes: vec![
                dapple_data::hash::hash_value_slot(
                    &slot_name, "", "json", &None, "single", None, None,
                )
                .as_str()
                .to_string(),
            ],
        },
    )
    .await?[0]
        .id;
    let binding_id: Uuid = sqlx::query_scalar(
        "INSERT INTO rev.dispatch_intent_bindings (dispatch_id, value_slot_id, created_at)
         VALUES ($1, $2, now()) RETURNING id",
    )
    .bind(d1.id)
    .bind(slot)
    .fetch_one(pool.get_pool())
    .await?;
    let payload = json!({ "big": "value", "rows": [1, 2, 3] });
    sqlx::query(
        "INSERT INTO rev.dispatch_intent_binding_json_items
             (dispatch_intent_binding_id, value) VALUES ($1, $2)",
    )
    .bind(binding_id)
    .bind(&payload)
    .execute(pool.get_pool())
    .await?;

    execute_dispatch(&pool, d1.clone(), 4).await?;

    let mut checked = false;
    for attempt in select_task_attempts_by_dispatch_id(&pool, d1.id).await? {
        for event in select_attempt_events_by_attempt_object_id(&pool, attempt.id).await? {
            if event.phase_code == "ok"
                && let Some(ctx) = &event.context_data
            {
                assert_eq!(
                    ctx.get("handle_delivery").and_then(|v| v.as_str()),
                    Some("envelope"),
                    "the seed must carry the handle, not the payload"
                );
                assert_eq!(
                    ctx.get("opened"),
                    Some(&payload),
                    "binding open must resolve the exact bound value"
                );
                checked = true;
            }
        }
    }
    assert!(checked, "expected an ok event carrying the assertions");
    Ok(())
}
