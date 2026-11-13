use dapple_data::MergeOrdered;
use dapple_data::model::playbooks::FlatPlaybookModel;
use dapple_data::model::steps::InsertedStepModel;
use dapple_data::model::steps::StepModel;
use dapple_data::model::steps::step_tasks::StepTaskModel;
use dapple_data::reference::CategoryReference;
use sqlx::types::Uuid;
use time::OffsetDateTime;

#[test]
fn category_reference_conversion_and_serde() {
    let u = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    // consumer-facing CategoryReference no longer exposes internal id
    let r = CategoryReference::from(u);
    assert_eq!(r.id, u);

    // serde round-trip
    let s = serde_json::to_string(&r).unwrap();
    let r2: CategoryReference = serde_json::from_str(&s).unwrap();
    assert_eq!(r, r2);
}

#[test]
fn flat_playbook_fold_categories_ord_and_dedupe() {
    // Create a playbook flat row with category arrays that include a duplicate
    let playbook_oid = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let cat1 = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
    let cat2 = Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();

    let now = OffsetDateTime::from_unix_timestamp(0).unwrap();

    let flat = FlatPlaybookModel {
        id: playbook_oid,
        versioned_object_id: Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap(),
        path_id: Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap(),
        name: "pb".to_string(),
        slug: "pb".to_string(),
        content_hash: "0000000000000000000000000000000000000000".to_string(),
        deprecation_level_key: None,
        description: "desc".to_string(),
        created_at: now,
        playbook_stage_id: None,
        playbook_stage_created_at: None,
        playbook_stage_stage_id: None,
        stage_step_id: None,
        stage_step_created_at: None,
        stage_step_step_id: None,
        step_task_id: None,
        step_task_task_id: None,
        step_task_created_at: None,
        // include duplicate cat2 to test dedupe
        category_ids: Some(vec![cat1, cat2, cat2]),
    };

    let folded = FlatPlaybookModel::fold_models(vec![flat]).expect("fold failed");
    assert_eq!(folded.len(), 1);
    let pb = &folded[0];
    // categories should be deduped and retain ordinality: cat1 then cat2
    assert_eq!(pb.categories.len(), 2);
    // consumer-facing CategoryReference exposes only id
    assert_eq!(pb.categories[0].id, cat1);
    assert_eq!(pb.categories[1].id, cat2);
}

/// Regression: a step's full task set must survive `merge_ordered` even when the
/// backing query returns step_task rows ordered by their OWN id rather than
/// grouped by `step_id`. This reproduces the `insert_step_tasks.sql` +
/// `ON CONFLICT DO NOTHING` interleaving — a re-pulled pre-existing link has an
/// older/smaller id, so a later task of an EARLIER step arrives after a task of
/// a LATER step. The pre-fix streaming fold silently dropped the trailing task
/// (or errored); the order-independent fold groups by `step_id` and keeps it.
#[test]
fn step_tasks_merge_groups_by_step_regardless_of_child_order() {
    let now = OffsetDateTime::from_unix_timestamp(0).unwrap();

    // Parents are id-sorted, exactly as insert_steps returns them. p1 < p2.
    let p1_id = Uuid::parse_str("00000000-0000-0000-0000-0000000000a1").unwrap();
    let p2_id = Uuid::parse_str("00000000-0000-0000-0000-0000000000a2").unwrap();

    let step = |id: Uuid| InsertedStepModel {
        id,
        versioned_object_id: Uuid::parse_str("00000000-0000-0000-0000-0000000000b0").unwrap(),
        path_id: Uuid::parse_str("00000000-0000-0000-0000-0000000000c0").unwrap(),
        name: "step".to_string(),
        slug: "step".to_string(),
        content_hash: "0000000000000000000000000000000000000000".to_string(),
        description: "desc".to_string(),
        created_at: now,
    };
    let parents = vec![step(p1_id), step(p2_id)];

    let task = |row_id: &str, step_id: Uuid, task_id: &str| StepTaskModel {
        id: Uuid::parse_str(row_id).unwrap(),
        step_id,
        task_id: Uuid::parse_str(task_id).unwrap(),
        created_at: now,
    };

    // Ordered by the child's OWN id (what `ORDER BY st.id` yields). p1's SECOND
    // task (row ...03) sorts AFTER p2's only task (row ...02): the interleaving.
    let p1_task_a = "00000000-0000-0000-0000-000000000001";
    let p2_task_c = "00000000-0000-0000-0000-000000000002";
    let p1_task_b = "00000000-0000-0000-0000-000000000003";
    let children = vec![
        task(p1_task_a, p1_id, "00000000-0000-0000-0000-0000000000f1"),
        task(p2_task_c, p2_id, "00000000-0000-0000-0000-0000000000f2"),
        task(p1_task_b, p1_id, "00000000-0000-0000-0000-0000000000f3"),
    ];

    let folded = StepModel::merge_ordered(&parents, children).expect("merge failed");

    assert_eq!(folded.len(), 2);
    // Parent order preserved (parents iterated in slice order).
    assert_eq!(folded[0].id, p1_id);
    assert_eq!(folded[1].id, p2_id);
    // p1 keeps BOTH of its tasks despite the interleaving; none dropped.
    assert_eq!(folded[0].tasks.len(), 2, "step 1 must keep both tasks");
    assert_eq!(folded[1].tasks.len(), 1, "step 2 keeps its task");
    // Within-parent arrival order preserved (a before b).
    assert_eq!(folded[0].tasks[0].id.to_string(), p1_task_a);
    assert_eq!(folded[0].tasks[1].id.to_string(), p1_task_b);
    assert_eq!(folded[1].tasks[0].id.to_string(), p2_task_c);
    // Every child accounted for.
    let total: usize = folded.iter().map(|s| s.tasks.len()).sum();
    assert_eq!(total, 3);
}
