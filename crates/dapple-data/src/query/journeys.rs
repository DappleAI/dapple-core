use crate::error::DataError;
use crate::model::journeys::InsertedJourneyModel;
use crate::model::journeys::{
    FlatJourneyBriefModel, InsertJourneyBriefPacksArgs, InsertJourneyInferenceProfilesArgs,
    InsertedJourneyBriefPackModel, InsertedJourneyInferenceProfileModel, JourneyBrief,
};
use crate::model::journeys::{FlatJourneyModel, InsertJourneyArgs};
use crate::model::journeys::{Journey, JourneyModel};
use crate::{DbPool, FilterOptions, InsertColumnArgs, columns_from_tuples, get_single_row_by_id};
use itertools::izip;
use log::info;
use sqlx::types::Uuid;

pub async fn select_journey_by_universal_id(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_id: Uuid,
) -> Result<Journey, DataError> {
    let model =
        select_journey_model_by_universal_id(pool, viewer_principal_id, universal_id).await?;
    Ok(model.into())
}

pub(crate) async fn select_journey_model_by_universal_id(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_id: Uuid,
) -> Result<JourneyModel, DataError> {
    let objs = select_journey_models_by_universal_ids(
        pool,
        viewer_principal_id,
        vec![universal_id],
        FilterOptions::default(),
    )
    .await?;

    get_single_row_by_id(universal_id, objs)
}

pub async fn select_journeys_by_universal_ids(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Journey>, DataError> {
    let models = select_journey_models_by_universal_ids(
        pool,
        viewer_principal_id,
        universal_ids,
        filter_ops,
    )
    .await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_journey_models_by_universal_ids(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<JourneyModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_object_ids.sql",
        &universal_ids,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        viewer_principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatJourneyModel::fold_models(models)
}

pub async fn select_journeys_by_universal_id_by_playbooks(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    playbook_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Journey>, DataError> {
    let models = select_journey_models_by_universal_id_by_playbooks(
        pool,
        viewer_principal_id,
        universal_ids,
        playbook_ids,
        filter_ops,
    )
    .await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_journey_models_by_universal_id_by_playbooks(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    playbook_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<JourneyModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_object_ids_by_playbooks.sql",
        &universal_ids,
        &playbook_ids,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        viewer_principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatJourneyModel::fold_models(models)
}

pub async fn select_journeys_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    user_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Journey>, DataError> {
    let models =
        select_journey_models_by_user(pool, viewer_principal_id, user_id, filter_ops).await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_journey_models_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    user_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<JourneyModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_user.sql",
        &user_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        viewer_principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatJourneyModel::fold_models(models)
}

/// List the caller's journeys (filtered by `owner_company_id`), overlaid
/// with the caller's per-principal preferences. Used by the external
/// surface's `GET /api/journeys` handler — see
/// `docs/plans/external-api/01-operator-runner-endpoints.md`.
pub async fn select_journeys_by_owner_company_id(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    owner_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Journey>, DataError> {
    let models = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_owner_company_id.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        owner_company_id,
        viewer_principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatJourneyModel::fold_models(models)?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub async fn select_journey_by_universal_id_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    user_id: Uuid,
    universal_id: Uuid,
) -> Result<Journey, DataError> {
    let model = select_journey_model_by_universal_id_by_user(
        pool,
        viewer_principal_id,
        user_id,
        universal_id,
    )
    .await?;
    Ok(model.into())
}

pub(crate) async fn select_journey_model_by_universal_id_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    user_id: Uuid,
    universal_id: Uuid,
) -> Result<JourneyModel, DataError> {
    let objs = select_journey_models_by_universal_ids_by_user(
        pool,
        viewer_principal_id,
        vec![universal_id],
        user_id,
        FilterOptions::default(),
    )
    .await?;

    get_single_row_by_id(universal_id, objs)
}

pub async fn select_journeys_by_universal_ids_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    user_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Journey>, DataError> {
    let models = select_journey_models_by_universal_ids_by_user(
        pool,
        viewer_principal_id,
        universal_ids,
        user_id,
        filter_ops,
    )
    .await?;
    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_journey_models_by_universal_ids_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    user_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<JourneyModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_object_ids_by_user.sql",
        &universal_ids,
        user_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        viewer_principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatJourneyModel::fold_models(models)
}

pub async fn select_journeys_by_universal_id_by_playbooks_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    playbook_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<JourneyModel>, DataError> {
    let models = select_journey_models_by_universal_id_by_playbooks_by_user(
        pool,
        viewer_principal_id,
        universal_ids,
        playbook_ids,
        filter_ops,
    )
    .await?;
    Ok(models.into_iter().collect())
}

pub(crate) async fn select_journey_models_by_universal_id_by_playbooks_by_user(
    pool: &DbPool,
    viewer_principal_id: Uuid,
    universal_ids: Vec<Uuid>,
    playbook_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<JourneyModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatJourneyModel,
        "assets/.sql/journeys/select_journeys_by_object_ids_by_playbooks_by_user.sql",
        &universal_ids,
        &playbook_ids,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        viewer_principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatJourneyModel::fold_models(models)
}

pub async fn insert_journeys(
    pool: &DbPool,
    objs: Vec<InsertJourneyArgs>,
) -> Result<Vec<Journey>, DataError> {
    let models = insert_journey_models(pool, objs).await?;
    Ok(models.into_iter().map(|model| model.into()).collect())
}

pub(crate) async fn insert_journey_models(
    pool: &DbPool,
    objs: Vec<InsertJourneyArgs>,
) -> Result<Vec<JourneyModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;

    validate_insert_journey_args(&objs)?;

    // Flatten the per-arg-block vectors into the column-wise tuples sqlx
    // needs. Column order matches the alphabetical :bind() order in
    // insert_journeys.sqlc: billable_entity_ids, fork_stage_ids, guide_ids,
    // names, owner_company_ids, playbook_ids, predecessor_journey_ids,
    // principal_ids, subject_principal_ids.
    let args: (
        Vec<_>,
        Vec<_>,
        Vec<_>,
        Vec<_>,
        Vec<_>,
        Vec<_>,
        Vec<_>,
        Vec<_>,
        Vec<_>,
    ) = columns_from_tuples!(objs.into_iter().flat_map(|obj| {
        let InsertJourneyArgs {
            playbook_ids,
            principal_ids,
            subject_principal_ids,
            billable_entity_ids,
            names,
            owner_company_ids,
            predecessor_journey_ids,
            guide_ids,
            fork_stage_ids,
        } = obj;

        izip!(
            billable_entity_ids.into_iter(),
            fork_stage_ids.into_iter(),
            guide_ids.into_iter(),
            names.into_iter(),
            owner_company_ids.into_iter(),
            playbook_ids.into_iter(),
            predecessor_journey_ids.into_iter(),
            principal_ids.into_iter(),
            subject_principal_ids.into_iter(),
        )
        .map(move |r| (r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8))
    }));

    info!(
        "insert_journey_models: arg lengths bn/fs/gd/nm/oc/pl/pj/pr/sp = {}/{}/{}/{}/{}/{}/{}/{}/{}",
        args.0.len(),
        args.1.len(),
        args.2.len(),
        args.3.len(),
        args.4.len(),
        args.5.len(),
        args.6.len(),
        args.7.len(),
        args.8.len(),
    );

    // sqlx's array bind needs the `Option<Uuid>` columns passed as slices.
    // We bind by reference so the array variants flow as PostgreSQL UUID[].
    let inserted_objs = sqlx::query_file_as!(
        InsertedJourneyModel,
        "assets/.sql/journeys/insert_journeys.sql",
        &args.0 as &[Option<Uuid>],
        &args.1 as &[Option<Uuid>],
        &args.2 as &[Option<Uuid>],
        &args.3,
        &args.4,
        &args.5,
        &args.6 as &[Option<Uuid>],
        &args.7,
        &args.8 as &[Option<Uuid>],
    )
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(inserted_objs.into_iter().map(Into::into).collect())
}

validate_insert_args!(validate_insert_journey_args, InsertJourneyArgs);

/// Insert journey_brief_packs associations
/// Uses object_ids which are resolved to internal IDs via CTE in SQL
pub async fn insert_journey_brief_packs(
    pool: &DbPool,
    args: InsertJourneyBriefPacksArgs,
) -> Result<Vec<InsertedJourneyBriefPackModel>, DataError> {
    if args.journey_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.journey_ids.len();
    if args.brief_pack_ids.len() != expected_len {
        return Err(DataError::Custom(format!(
            "brief_pack_ids length ({}) does not match journey_ids length ({})",
            args.brief_pack_ids.len(),
            expected_len
        )));
    }

    let inserted = sqlx::query_file_as!(
        InsertedJourneyBriefPackModel,
        "assets/.sql/journeys/insert_journey_brief_packs.sql",
        &args.journey_ids,
        &args.brief_pack_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted)
}

/// Insert journey_inference_profiles associations
/// Uses object_ids which are resolved to internal IDs via CTE in SQL
pub async fn insert_journey_inference_profiles(
    pool: &DbPool,
    args: InsertJourneyInferenceProfilesArgs,
) -> Result<Vec<InsertedJourneyInferenceProfileModel>, DataError> {
    if args.journey_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_len = args.journey_ids.len();
    if args.inference_profile_ids.len() != expected_len {
        return Err(DataError::Custom(format!(
            "inference_profile_ids length ({}) does not match journey_ids length ({})",
            args.inference_profile_ids.len(),
            expected_len
        )));
    }

    let inserted = sqlx::query_file_as!(
        InsertedJourneyInferenceProfileModel,
        "assets/.sql/journeys/insert_journey_inference_profiles.sql",
        &args.journey_ids,
        &args.inference_profile_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(inserted)
}

/// Select all briefs for a journey with their pack/group context
pub async fn select_briefs_by_journey_object_id(
    pool: &DbPool,
    journey_object_id: Uuid,
) -> Result<Vec<JourneyBrief>, DataError> {
    let rows = sqlx::query_file_as!(
        FlatJourneyBriefModel,
        "assets/.sql/journeys/select_briefs_by_journey_object_id.sql",
        journey_object_id
    )
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// True iff `journey_id` exists AND its `owner_company_id` matches the caller.
/// Used by external-API handlers to gate every journey-targeted operation.
pub async fn journey_in_namespace(
    pool: &DbPool,
    journey_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/journeys/journey_in_namespace.sql",
        caller_company_id,
        journey_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// Look up a journey's ownership + guide pin for handler-level chain
/// validation. Returns `Ok(None)` when the journey does not exist (caller
/// treats as 404, matching `journey_in_namespace` semantics).
pub async fn select_journey_chain_ancestor(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Option<crate::model::journeys::JourneyChainAncestor>, DataError> {
    let row = sqlx::query_file_as!(
        crate::model::journeys::JourneyChainAncestor,
        "assets/.sql/journeys/select_journey_chain_ancestor.sql",
        journey_id,
    )
    .fetch_optional(pool.get_pool())
    .await?;
    Ok(row)
}

/// True iff `playbook_stage_id` is a stage of `playbook_id`. Handlers call
/// this to validate a stage-level fork point (`fork_stage_id`, Migration 74)
/// against the predecessor's playbook before insert, so the caller gets a
/// clean 4xx instead of the DB fork-coherence trigger's exception.
pub async fn playbook_stage_in_playbook(
    pool: &DbPool,
    playbook_id: Uuid,
    playbook_stage_id: Uuid,
) -> Result<bool, DataError> {
    // Alphabetical bind order: $1=playbook_id, $2=playbook_stage_id.
    let belongs = sqlx::query_file_scalar!(
        "assets/.sql/journeys/playbook_stage_in_playbook.sql",
        playbook_id,
        playbook_stage_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(belongs)
}

/// Phase 2: chain-summary listing for a `(principal, guide)` pair.
/// Returns flat rows (one per leaf); handler folds via
/// `ChainSummaryRow::fold` into `Vec<ChainSummary>` DTOs.
pub async fn select_chain_summaries_by_principal_and_guide(
    pool: &DbPool,
    principal_id: Uuid,
    guide_id: Uuid,
) -> Result<Vec<crate::model::journeys::ChainSummaryRow>, DataError> {
    let rows = sqlx::query_file_as!(
        crate::model::journeys::ChainSummaryRow,
        "assets/.sql/journeys/select_chain_summaries_by_principal_and_guide.sql",
        guide_id,
        principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

/// Phase 2: walk the chain backward from `journey_id` and return the
/// journeys ordered root → starting journey. Caller is responsible for
/// the ownership / namespace gate (use `select_journey_chain_ancestor`
/// to verify before calling).
///
/// `viewer_principal_id` drives the `hidden_at` LEFT JOIN — each returned
/// `ChainJourneyRow` carries the viewer's per-principal hidden state.
/// Single-get semantics: rows are SURFACED with `hidden_at`, never
/// filtered. A caller who soft-hid a journey can still inspect it here.
pub async fn select_chain_journeys_from_id(
    pool: &DbPool,
    journey_id: Uuid,
    viewer_principal_id: Uuid,
) -> Result<Vec<crate::model::journeys::ChainJourneyRow>, DataError> {
    let rows = sqlx::query_file_as!(
        crate::model::journeys::ChainJourneyRow,
        "assets/.sql/journeys/select_chain_journeys_from_id.sql",
        journey_id,
        viewer_principal_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

/// True iff every value slot the stage NEEDS (its overlay-exposed slots,
/// cascade-resolved stage -> step -> task) is bound by some dispatch in the
/// journey's chain. The required set comes from the stage's overlays, not the
/// brief_packs.
///
/// Strict default: a stage with no overlays has an empty required set and is
/// reported as bound (trivially "done").
pub async fn is_stage_fully_bound(
    pool: &DbPool,
    journey_id: Uuid,
    playbook_stage_id: Uuid,
) -> Result<bool, DataError> {
    // Bind order matches the generated $N: $1 = playbook_stage_id,
    // $2 = starting_journey_id (the journey, shared with journey_chain.sqlc).
    let row = sqlx::query_file_scalar!(
        "assets/.sql/journeys/is_stage_fully_bound.sql",
        playbook_stage_id,
        journey_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// Flat row from `select_stage_binding_status_for_journey.sql`. A private row
/// struct (rather than mapping straight into the DTO) because the TEXT `kind`
/// key needs a Rust-side `StageKind::from_key` to become the typed
/// `Option<StageKind>` the wire carries.
struct StageBindingStatusRow {
    playbook_stage_id: Uuid,
    name: String,
    description: String,
    kind: Option<String>,
    /// Raw `rev.stages.icons` jsonb; projected by `stage_icons_from_json`.
    icon: Option<serde_json::Value>,
    done: bool,
    blocked: bool,
}

/// Per-stage binding status for every stage of the journey's playbook, in
/// authored (UUIDv7) order. `done` is the overlay-driven `is_stage_fully_bound`
/// result evaluated for all stages in a single pass. The DTO is the shared
/// `dapple_types` type (re-exported by the SDK).
pub async fn select_stage_binding_status_for_journey(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<dapple_types::model::journeys::JourneyStageStatus>, DataError> {
    use dapple_types::model::stages::StageKind;

    let rows = sqlx::query_file_as!(
        StageBindingStatusRow,
        "assets/.sql/journeys/select_stage_binding_status_for_journey.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| dapple_types::model::journeys::JourneyStageStatus {
            playbook_stage_id: row.playbook_stage_id,
            name: row.name,
            description: row.description,
            kind: row.kind.as_deref().and_then(StageKind::from_key),
            icon: crate::model::stages::stage_icons_from_json(row.icon.as_ref()),
            done: row.done,
            blocked: row.blocked,
        })
        .collect())
}

/// Step-scoped twin of [`is_stage_fully_bound`]: true iff every value slot the
/// given step exposes (its overlays, cascade-resolved within the given
/// `playbook_stage`) is bound somewhere in the journey's chain. A step with no
/// exposed slots is trivially bound. Used by [`dispatch_ready`] to gate a
/// dispatch on only the steps it actually contains.
pub async fn is_step_fully_bound(
    pool: &DbPool,
    journey_id: Uuid,
    playbook_stage_id: Uuid,
    stage_step_id: Uuid,
) -> Result<bool, DataError> {
    // Generated $N: $1 = playbook_stage_id, $2 = stage_step_id,
    // $3 = starting_journey_id (the journey, shared with journey_chain.sqlc).
    let row = sqlx::query_file_scalar!(
        "assets/.sql/journeys/is_step_fully_bound.sql",
        playbook_stage_id,
        stage_step_id,
        journey_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// One computed value-slot a step CONSUMES (reads but does not produce), with
/// the producing step and whether it is already bound in the journey chain.
/// Returned by [`select_consumed_computed_deps_for_step`].
pub struct ConsumedComputedDep {
    pub value_slot_path_id: Uuid,
    /// The producing step's playbook_stage_id (earliest exposer of the slot).
    pub producer_playbook_stage_id: Uuid,
    /// The producing step's stage_step_id.
    pub producer_stage_step_id: Uuid,
    /// True iff the slot is already bound by some dispatch in the journey chain.
    pub bound: bool,
}

/// Computed value-slot dependencies a step CONSUMES: computed-output slots the
/// step exposes that an EARLIER-STAGE step also exposes (the earliest exposer is
/// the producer; this step is a later consumer). The companion to
/// [`is_step_fully_bound`] — that gate EXCLUDES computed outputs so a producer
/// can spawn on its operator inputs; this re-introduces only the computed
/// outputs a step CONSUMES so [`dispatch_ready`] can refuse to spawn a dispatch
/// whose step would run before its computed input exists (the render-before-
/// shotlist race). The caller skips any dep whose producer step is part of the
/// same dispatch (in-dispatch data flow — gating would deadlock).
pub async fn select_consumed_computed_deps_for_step(
    pool: &DbPool,
    journey_id: Uuid,
    playbook_stage_id: Uuid,
    stage_step_id: Uuid,
) -> Result<Vec<ConsumedComputedDep>, DataError> {
    // Generated $N: $1 = playbook_stage_id, $2 = stage_step_id,
    // $3 = starting_journey_id (alphabetical bind order).
    let rows = sqlx::query_file_as!(
        ConsumedComputedDep,
        "assets/.sql/journeys/select_consumed_computed_deps_for_step.sql",
        playbook_stage_id,
        stage_step_id,
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

/// Flat row from `select_step_binding_status_for_journey.sql`: one per
/// (step, exposed-slot), or one per step with NULL slot columns when the step
/// exposes nothing. Folded into `JourneyStepStatus`.
struct StepBindingStatusRow {
    stage_step_id: Uuid,
    playbook_stage_id: Uuid,
    /// Stage identity, repeated on every row of a stage's steps: authored name
    /// (path-level), fluent-resolved description, and the authored kind key
    /// (`None` when unauthored).
    stage_name: String,
    stage_description: String,
    stage_kind: Option<String>,
    /// Raw `rev.stages.icons` jsonb; projected by `stage_icons_from_json`.
    stage_icon: Option<serde_json::Value>,
    value_slot_id: Option<Uuid>,
    value_slot_name: Option<String>,
    value_kind: Option<String>,
    /// `"operator"` | `"computed"` — NULL only on the all-NULL slotless-step row.
    producer: Option<String>,
    /// Operator-input control columns (NULL for computed slots).
    control_type: Option<String>,
    label_text: Option<String>,
    help_text: Option<String>,
    placeholder_text: Option<String>,
    /// The operator input's brief_unit id — the key for the choices batch-fetch.
    brief_unit_id: Option<Uuid>,
    /// Selection cardinality of the resolved slot ('single'/'multi'); NULL on a
    /// slotless-step row.
    cardinality: Option<String>,
    min_selections: Option<i32>,
    max_selections: Option<i32>,
    /// Dynamic candidate-source slot id + display name (operator pick only).
    candidate_source_value_slot_id: Option<Uuid>,
    candidate_source_name: Option<String>,
    /// Author-controlled optional flag (operator inputs only). `None` (computed
    /// slot / slotless-step row) ⇒ required default.
    optional: Option<bool>,
    /// Per-item field schema for a `repeater` control (raw JSON array of resolved
    /// `{ key, control, label?, help?, placeholder? }`). `None` for every
    /// non-repeater operator input.
    item_fields: Option<serde_json::Value>,
    /// Per-item refine LOCATOR slot for a `repeater` control: the slot the refine
    /// step exposes, which the client matches in `/steps` to find that step.
    /// `None` when the repeater offers no per-item refine.
    refine_input_value_slot_id: Option<Uuid>,
    /// Axis presentation config for a `timeline` control (`{ unit?, allowAbsolute? }`).
    /// `None` for non-timeline controls.
    axis: Option<serde_json::Value>,
    /// Typed display hint for computed slots (NULL for operator slots / slotless
    /// steps). `display_kind` is the lookup key; the fold maps it to the typed
    /// `ExposedSlotDisplay` for computed producers only.
    display_kind: Option<String>,
    display_reveal: Option<bool>,
    display_title: Option<String>,
    /// Sibling-format grouping for computed outputs: same `display_group` ⇒ same
    /// deliverable in a different format, `display_format` names this one. Both
    /// opaque and author-chosen; NULL ⇒ ungrouped.
    display_group: Option<String>,
    display_format: Option<String>,
    bound: Option<bool>,
    /// This pick's candidate source produced a value that was present and EMPTY,
    /// and the pick requires at least one selection — so no operator action can
    /// ever bind it. Distinct from `bound: false`, which merely means "not yet".
    blocked: Option<bool>,
}

/// Per-step exposed-slot status for every step of the journey's playbook, in
/// authored (UUIDv7) order. Powers `GET /api/journeys/{id}/steps`: a client
/// finds the not-`done` step and binds its pending `exposed_slots`.
pub async fn select_step_binding_status_for_journey(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<dapple_types::model::journeys::JourneyStepStatus>, DataError> {
    use crate::query::brief_units::select_brief_unit_input_choices_by_unit_ids;
    use dapple_types::model::briefs::BriefOutputDisplay;
    use dapple_types::model::journey_brief_form::ChoiceView;
    use dapple_types::model::journeys::{
        ExposedSlot, ExposedSlotDisplay, ExposedSlotInput, JourneyStepStatus,
    };
    use std::collections::HashMap;

    let rows = sqlx::query_file_as!(
        StepBindingStatusRow,
        "assets/.sql/journeys/select_step_binding_status_for_journey.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Batch-fetch input choices for operator slots (1:N), keyed by brief_unit_id,
    // mirroring `view::journey_brief_form`. Kept as a separate query (NOT a SQL
    // join) so the 1:N choices don't fan out the step-status rows and corrupt the
    // per-step grouping + `bound` rollup. A value_slot path exposed by several
    // steps repeats its brief_unit_id across rows, so we `.get().cloned()` per
    // occurrence rather than draining the map.
    let unit_ids: Vec<Uuid> = rows.iter().filter_map(|r| r.brief_unit_id).collect();
    let mut choices_by_unit: HashMap<Uuid, Vec<ChoiceView>> = HashMap::new();
    if !unit_ids.is_empty() {
        for c in select_brief_unit_input_choices_by_unit_ids(pool, &unit_ids).await? {
            choices_by_unit
                .entry(c.brief_unit_id)
                .or_default()
                .push(ChoiceView {
                    value: c.value_text,
                    label: c.label_text,
                });
        }
    }

    // Rows are ordered by (playbook_stage_id, stage_step_id) — authored
    // stage-then-step traversal order — so fold consecutive runs into one
    // JourneyStepStatus each. Each stage_step belongs to exactly one stage, so its
    // rows stay contiguous under this ordering (grouping by stage_step_id is still
    // valid). A step with no exposed slot still yields one row (NULL slot columns)
    // and folds to an empty, trivially-done step.
    let mut out: Vec<JourneyStepStatus> = Vec::new();
    for row in rows {
        if out.last().map(|s| s.stage_step_id) != Some(row.stage_step_id) {
            out.push(JourneyStepStatus {
                stage_step_id: row.stage_step_id,
                playbook_stage_id: row.playbook_stage_id,
                stage_name: row.stage_name.clone(),
                stage_description: row.stage_description.clone(),
                stage_kind: row
                    .stage_kind
                    .as_deref()
                    .and_then(dapple_types::model::stages::StageKind::from_key),
                stage_icon: crate::model::stages::stage_icons_from_json(row.stage_icon.as_ref()),
                done: true,
                blocked: false,
                exposed_slots: Vec::new(),
            });
        }
        let current = out.last_mut().expect("pushed above");
        // An exposed-slot row is identified by its name/kind (from
        // value_slot_path_details, populated for EVERY exposed path); the
        // all-NULL row a slotless step yields is skipped. `value_slot_id` is the
        // journey-pinned version, populated for BOTH operator and computed slots;
        // `producer` is the discriminator. A computed slot still counts toward
        // `done` via the path-based `bound` flag.
        if let (Some(name), Some(value_kind)) = (row.value_slot_name, row.value_kind) {
            let bound = row.bound.unwrap_or(false);
            // An OPTIONAL operator input (the author flag, or a
            // min_selections = 0 multi-select) does NOT block the step's `done`
            // while unbound — it stays in `exposed_slots` so the client can still
            // offer it. `optional` is populated only for operator slots (computed
            // -> None), so computed outputs keep gating `done`.
            let optional_skippable = row.optional == Some(true)
                || (row.producer.as_deref() == Some("operator") && row.min_selections == Some(0));
            if !bound && !optional_skippable {
                current.done = false;
            }
            // BLOCKED is the third state, and the whole point of it is that it is
            // NOT recoverable by waiting. The SQL reports it for a required pick
            // whose candidate source produced an empty value; an already-bound
            // slot is never blocked (a good regenerate may have landed after the
            // empty one, or the operator picked before it arrived), so `bound`
            // wins here rather than the two flags contradicting each other on the
            // wire.
            let blocked = row.blocked.unwrap_or(false) && !bound;
            if blocked {
                current.blocked = true;
            }
            let producer = row.producer.unwrap_or_else(|| "operator".to_string());
            let candidate_source_value_slot_id = row.candidate_source_value_slot_id;
            let candidate_source_name = row.candidate_source_name;
            // Render spec only for operator slots that carry a control type.
            let input = if producer == "operator" {
                row.control_type.map(|control_type| ExposedSlotInput {
                    control_type,
                    label_text: row.label_text,
                    help_text: row.help_text,
                    placeholder_text: row.placeholder_text,
                    choices: row
                        .brief_unit_id
                        .and_then(|u| choices_by_unit.get(&u).cloned())
                        .unwrap_or_default(),
                    candidate_source_value_slot_id,
                    candidate_source_name,
                    optional: row.optional.unwrap_or(false),
                    item_fields: row.item_fields,
                    refine_input_value_slot_id: row.refine_input_value_slot_id,
                    axis: row.axis,
                })
            } else {
                None
            };
            // Render spec only for computed slots (the server produces the value;
            // the client renders it per `display`). Operator slots render via
            // `input` and carry no display. An unset display key COALESCEs to
            // `text`.
            let display = if producer == "computed" {
                Some(ExposedSlotDisplay {
                    kind: BriefOutputDisplay::from_key(
                        row.display_kind.as_deref().unwrap_or("text"),
                    ),
                    title: row.display_title,
                    reveal: row.display_reveal.unwrap_or(false),
                    group: row.display_group,
                    format: row.display_format,
                })
            } else {
                None
            };
            current.exposed_slots.push(ExposedSlot {
                value_slot_id: row.value_slot_id,
                name,
                value_kind,
                bound,
                blocked,
                producer,
                cardinality: row.cardinality.unwrap_or_else(|| "single".to_string()),
                min_selections: row.min_selections,
                max_selections: row.max_selections,
                input,
                display,
            });
        }
    }
    Ok(out)
}

/// A computed `:::brief-output` slot a single step exposes, resolved to the
/// journey-pinned value_slot version. Returned by
/// [`select_computed_outputs_for_step`] and consumed by the dispatch runner's
/// computed-output producer.
pub struct ComputedOutputForStep {
    /// The journey-pinned value_slot VERSION to write the binding against (the
    /// same version `/brief-form` and the DONE rollup resolve).
    pub value_slot_id: Uuid,
    /// How to extract the value from the task payload. `.` / empty = the whole
    /// payload root; otherwise an RFC6901 JSON pointer.
    pub source_path: String,
    /// The slot's display name (from `value_slot_path_details`) — surfaced in the
    /// producer's `ComputedOutputOutcome` so a customer can correlate a stuck
    /// computed slot to its production result on `/dispatches/{id}/task_attempts`.
    pub value_slot_name: String,
    /// `text` | `int` | `number` | `bool` | `json` | `asset` — routes the typed
    /// binding item.
    pub value_kind_key: String,
    /// `single` (one item) or `multi` — a multi computed output whose
    /// `source_path` yields an ARRAY produces one typed item per element.
    pub cardinality: String,
}

/// The computed brief-outputs a given step PRODUCES (overlay-exposed,
/// `source_kind = 'computed'`), resolved to the journey's pinned brief_pack
/// versions. The dispatch runner calls this after a task succeeds to learn which
/// value_slot version to bind and how to extract its value. This is the inverse
/// of [`is_step_fully_bound`]'s required set (which EXCLUDES computed outputs so
/// the start gate never waits on a slot the task itself produces); here we
/// INCLUDE only computed outputs so the DONE rollups become satisfiable once the
/// task has run. The journey is resolved from the dispatch.
pub async fn select_computed_outputs_for_step(
    pool: &DbPool,
    dispatch_id: Uuid,
    playbook_stage_id: Uuid,
    stage_step_id: Uuid,
) -> Result<Vec<ComputedOutputForStep>, DataError> {
    // Generated $N: $1 = dispatch_id, $2 = playbook_stage_id,
    // $3 = stage_step_id (alphabetical bind order).
    let rows = sqlx::query_file_as!(
        ComputedOutputForStep,
        "assets/.sql/journeys/select_computed_outputs_for_step.sql",
        dispatch_id,
        playbook_stage_id,
        stage_step_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

/// True iff a dispatch is ready to execute: every step the dispatch contains
/// has its overlay-exposed slots bound ([`is_step_fully_bound`]). A dispatch
/// waits only on the slots its own steps expose, so a generate dispatch runs
/// while a sibling refine dispatch idles until its pick is bound.
///
/// Journey *completion* is deliberately NOT part of this gate. Whether the
/// whole journey's brief form is filled is a UI signal ("all steps done" /
/// confetti — see [`select_stage_binding_status_for_journey`]), unrelated to
/// whether an individual step's dispatch may run; gating execution on it would
/// defeat step-level dispatching (step 1 must run before later steps' inputs
/// exist).
pub async fn dispatch_ready(
    pool: &DbPool,
    dispatch: &crate::model::dispatches::DispatchModel,
) -> Result<bool, DataError> {
    // The (playbook_stage_id, stage_step_id) set this dispatch contains. A
    // consumed computed dependency whose PRODUCER runs within this same
    // dispatch reaches the consumer via in-dispatch accumulated data, not a
    // cross-dispatch binding — so it must NOT block (gating on a binding the
    // in-dispatch producer hasn't written yet would deadlock the dispatch).
    let dispatch_steps: std::collections::HashSet<(Uuid, Uuid)> = dispatch
        .stages
        .iter()
        .flat_map(|stage| {
            stage
                .steps
                .iter()
                .map(move |step| (stage.playbook_stage_id, step.stage_step_id))
        })
        .collect();

    for stage in &dispatch.stages {
        for step in &stage.steps {
            if !is_step_fully_bound(
                pool,
                dispatch.journey_id,
                stage.playbook_stage_id,
                step.stage_step_id,
            )
            .await?
            {
                return Ok(false);
            }

            // A step that CONSUMES a computed output produced by an earlier
            // stage must not start before that value is bound — otherwise it
            // runs on a missing input and spends the user's tokens on a result
            // that cannot honor the direction (the render-before-shotlist race).
            // Deps produced within this same dispatch flow through accumulated
            // data and are skipped to avoid a deadlock.
            for dep in select_consumed_computed_deps_for_step(
                pool,
                dispatch.journey_id,
                stage.playbook_stage_id,
                step.stage_step_id,
            )
            .await?
            {
                if dep.bound {
                    continue;
                }
                if dispatch_steps
                    .contains(&(dep.producer_playbook_stage_id, dep.producer_stage_step_id))
                {
                    continue;
                }
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Value-slot ids exposed by any stage of the journey's playbook (overlay
/// cascade). Empty => the playbook authored no overlays. Drives the brief-form
/// lockstep filter (show an input only if its value_slot is exposed).
pub async fn select_exposed_value_slot_ids_for_journey(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<Uuid>, DataError> {
    let ids = sqlx::query_file_scalar!(
        "assets/.sql/journeys/select_exposed_value_slot_ids_for_journey.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(ids)
}
