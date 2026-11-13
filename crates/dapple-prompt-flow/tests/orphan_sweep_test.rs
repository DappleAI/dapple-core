//! Startup orphan sweep: attempts whose latest event is still non-terminal
//! when the executing process boots are restart-stranded (execution is
//! in-process) and must be marked `failed` so status is honest and the
//! operator can re-dispatch. Corrupt-then-verify: seed the stranded state
//! directly in the DB, run the sweep, assert against the DB — never against a
//! response flag.

use dapple_data::DbPool;
use dapple_data::model::journeys::InsertJourneyArgs;
use dapple_data::model::playbooks::InsertPlaybookArgs;
use dapple_data::model::playbooks::playbook_stages::InsertPlaybookStageArgs;
use dapple_data::model::tasks::InsertTaskArgs;
use dapple_data::query::dispatch_task_attempts::{
    insert_task_attempt_event, insert_task_attempt_with_object_ids,
    select_attempt_events_by_attempt_object_id,
};
use dapple_data::query::{
    dispatches::insert_dispatches, journeys::insert_journeys, tasks::insert_tasks,
};
use dapple_prompt_flow::dispatch_runner::fail_orphaned_attempts;
use serde_json::json;
use sqlx::types::Uuid;

/// Seed a journey + dispatch and return the attempt id of a freshly inserted
/// task attempt whose only event is `phase_code` stamped with `executor`
/// (None = no executor key in the metadata, like pre-stamping binaries).
async fn seed_attempt_with_latest_event(
    pool: &DbPool,
    label: &str,
    phase_code: &str,
    executor: Option<&str>,
) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let task_args = InsertTaskArgs {
        names: vec![format!("{label}-task-{}", Uuid::now_v7())],
        descriptions: vec![format!("{label} task")],
        functions: vec!["nushell".to_string()],
        function_inputs: vec![json!({ "script": "{ payload: {}, signals: {} }" })],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };
    let tasks = insert_tasks(pool, vec![task_args]).await?;

    let step_args = dapple_data::model::steps::InsertStepArgs {
        names: vec![format!("{label}-step-{}", Uuid::now_v7())],
        descriptions: vec![Some(format!("{label} step"))],
        description_fluent_message_ids: Vec::new(),
        tasks: vec![dapple_data::model::steps::step_tasks::InsertStepTaskArgs {
            task_ids: tasks.iter().map(|t| t.id).collect(),
        }],
        content_hashes: vec!["0000000000000000000000000000000000000000".to_string()],
    };
    let inserted_steps = dapple_data::query::steps::insert_steps(pool, vec![step_args]).await?;

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
    let pbs = dapple_data::query::playbooks::insert_playbooks(pool, vec![playbook_args]).await?;
    let pb = pbs.into_iter().next().ok_or("no playbook inserted")?;

    let user_detail = dapple_data::test_support::insert_test_user_and_user_detail(
        pool,
        &format!("{label}-{}@example.com", Uuid::now_v7()),
    )
    .await?;
    let principal_id = dapple_data::query::principals::select_human_principal_for_user_object_id(
        pool,
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
    let js = insert_journeys(pool, vec![jargs]).await?;
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
    let dispatches = insert_dispatches(pool, vec![journey.id], vec![dispatch_args]).await?;
    let dispatch = dispatches
        .into_iter()
        .next()
        .ok_or("no dispatch inserted")?;
    let dispatch_step_id = dispatch.stages[0].steps[0].id;

    // The corrupt state: an attempt whose latest (only) event never reached a
    // terminal phase — exactly what a process restart mid-task leaves behind.
    let (_attempt_num, attempt_dto) = insert_task_attempt_with_object_ids(
        pool,
        dispatch_step_id,
        tasks[0].id,
        "running",
        json!({}),
        json!({}),
    )
    .await?;
    let metadata = executor.map(|e| json!({ "executor": e }));
    insert_task_attempt_event(
        pool,
        attempt_dto.id,
        phase_code,
        Some(json!({ "dispatch_intent": {} })),
        metadata,
        None,
    )
    .await?;

    Ok(attempt_dto.id)
}

async fn latest_phase(
    pool: &DbPool,
    attempt_id: Uuid,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let events = select_attempt_events_by_attempt_object_id(pool, attempt_id).await?;
    Ok(events
        .last()
        .map(|e| e.phase_code.clone())
        .unwrap_or_default())
}

/// The sweep marks a restart-stranded attempt (latest event `running`, stamped
/// with this executor) failed — verified against the DB, not a response flag —
/// while leaving alone (a) attempts owned by a DIFFERENT executor and (b)
/// attempts that already reached a terminal phase.
#[tokio::test]
async fn sweep_fails_own_orphans_and_spares_others()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {:?}. Skipping test.", e);
            return Ok(());
        }
    };

    // Unique identities so this test only ever claims its own seeded rows,
    // even against a shared dev database.
    let mine = format!("sweep-test-{}", Uuid::now_v7());
    let other = format!("sweep-test-other-{}", Uuid::now_v7());

    let orphaned = seed_attempt_with_latest_event(&pool, "orphan", "running", Some(&mine)).await?;
    let foreign = seed_attempt_with_latest_event(&pool, "foreign", "running", Some(&other)).await?;
    let finished = seed_attempt_with_latest_event(&pool, "finished", "ok", Some(&mine)).await?;

    let marked = fail_orphaned_attempts(&pool, &mine).await;
    assert!(marked >= 1, "sweep should mark at least the seeded orphan");

    // Corrupt-then-verify: the stranded attempt gained a terminal failed event...
    assert_eq!(latest_phase(&pool, orphaned).await?, "failed");
    let events = select_attempt_events_by_attempt_object_id(&pool, orphaned).await?;
    let failed_event = events.last().expect("failed event present");
    assert_eq!(
        failed_event
            .context_data
            .as_ref()
            .and_then(|d| d.get("error"))
            .and_then(|e| e.as_str()),
        Some("interrupted by service restart"),
    );

    // ...while another executor's live attempt and a finished attempt are untouched.
    assert_eq!(latest_phase(&pool, foreign).await?, "running");
    assert_eq!(latest_phase(&pool, finished).await?, "ok");

    // Idempotence: a second boot sweep finds this orphan already terminal.
    fail_orphaned_attempts(&pool, &mine).await;
    let events_after = select_attempt_events_by_attempt_object_id(&pool, orphaned).await?;
    assert_eq!(
        events_after.len(),
        events.len(),
        "second sweep must not append another failed event"
    );

    Ok(())
}
