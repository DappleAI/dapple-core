use crate::DbPool;
use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::dispatch_intent_bindings::binding_items::{
    BindingAssetItemModel, BindingBoolItemModel, BindingDatasetItemModel, BindingIntItemModel,
    BindingJsonItemModel, BindingNumberItemModel, BindingTextItemModel, BindingValueMetaModel,
};
use crate::model::dispatch_intent_bindings::{
    DispatchIntentBinding, FlatDispatchIntentBindingModel, InsertDispatchIntentBindingArgs,
    InsertedDispatchIntentBindingModel,
};
use rust_decimal::Decimal;
use sqlx::types::Uuid;

// ============================================================================
// DispatchIntentBinding queries
// ============================================================================

/// Validate that each operator-submitted binding respects its value slot's
/// selection cardinality before insert. `submissions` is `(value_slot_id,
/// item_count)` where `item_count` is the total number of typed items the
/// binding carries (across all kinds — a well-formed binding fills exactly the
/// slot's `value_kind`).
///
/// * `single` slot → exactly **1** item (the historical default).
/// * `multi` slot → `min_selections ≤ count ≤ max_selections` (NULL bounds are
///   no floor / no ceiling; `min = 0` permits a deliberate keep-none).
///
/// Slots not found in the DB are skipped here so the INSERT's own FK error
/// surfaces the real problem. Returns [`DataError::InvalidInput`] (HTTP 400) on
/// the first violation. This is the operator-write guard; the computed-output
/// producer enforces nothing here (it shapes its own multi arrays).
pub async fn validate_binding_cardinality(
    pool: &DbPool,
    submissions: &[(Uuid, usize)],
) -> Result<(), DataError> {
    use std::collections::HashMap;

    if submissions.is_empty() {
        return Ok(());
    }

    let ids: Vec<Uuid> = submissions.iter().map(|(id, _)| *id).collect();
    let slots = crate::query::value_slots::select_value_slots_by_object_ids(pool, ids).await?;
    let by_id: HashMap<Uuid, (String, Option<i32>, Option<i32>)> = slots
        .into_iter()
        .map(|s| (s.id, (s.cardinality, s.min_selections, s.max_selections)))
        .collect();

    for (slot_id, count) in submissions {
        let Some((cardinality, min, max)) = by_id.get(slot_id) else {
            continue;
        };
        if cardinality == "single" {
            if *count != 1 {
                return Err(DataError::InvalidInput(format!(
                    "value slot {slot_id} is single-valued; a binding must carry exactly \
                     1 item, got {count}"
                )));
            }
            continue;
        }
        // multi (keep-N)
        let min_n = min.unwrap_or(0).max(0) as usize;
        if *count < min_n {
            return Err(DataError::InvalidInput(format!(
                "value slot {slot_id} is multi (keep-N) with min_selections {min_n}; a \
                 binding must keep at least {min_n} item(s), got {count}"
            )));
        }
        if let Some(max_n) = max
            && *count > *max_n as usize
        {
            return Err(DataError::InvalidInput(format!(
                "value slot {slot_id} is multi (keep-N) with max_selections {max_n}; a \
                 binding may keep at most {max_n} item(s), got {count}"
            )));
        }
    }
    Ok(())
}

/// Insert bindings and their typed value items.
///
/// **The parent rows and their items MUST commit together.** A binding row is
/// the signal that a value slot is bound: `is_step_fully_bound` /
/// `is_stage_fully_bound` test for the row's existence alone and never join the
/// item tables. So a parent row visible without its items reads as "bound" while
/// carrying no value, and the runtime launches the next dispatch into that
/// window. Its context seed
/// (`select_bindings_by_journey_of_dispatch`) then reads the item value with a
/// scalar subquery, gets SQL NULL, and `binding_row_to_json` drops the slot from
/// `$in.bindings` — the guide runs against a pool that is silently missing a
/// value it was told is present.
///
/// That is not hypothetical: it shipped. Writing the items as separate
/// autocommitted statements left a measured 508 ms window on an 11 MB paired
/// payload, during which `weigh-the-m-and-a` read an empty binding for a company
/// whose filing was sitting in the database, and reported to the operator that
/// the company had no IPO prospectus on file. The window scales with payload
/// size and is value-kind-blind, so any slot in any guide can be blanked this
/// way. Do not move these inserts back out of the transaction.
///
/// Committing together also stops a failed item insert from leaving orphaned
/// parent rows behind, which readiness would otherwise count as bound forever.
pub async fn insert_dispatch_intent_bindings(
    pool: &DbPool,
    args: InsertDispatchIntentBindingArgs,
) -> Result<Vec<DispatchIntentBinding>, DataError> {
    if args.dispatch_ids.is_empty() {
        return Err(InsertArgColumnMismatch::EmptyColumnSet {}.into());
    }

    if args.dispatch_ids.len() != args.value_slot_ids.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "value_slot_ids".into(),
            expected: args.dispatch_ids.len(),
            found: args.value_slot_ids.len(),
        }
        .into());
    }

    // Operator writes leave `produced_empty` unset (the concept is the computed
    // producer's alone), so an empty vec means all-false rather than a length
    // mismatch. A non-empty vec must still be in lockstep, or a binding would
    // silently inherit its neighbour's verdict.
    let produced_empty: Vec<bool> = if args.produced_empty.is_empty() {
        vec![false; args.dispatch_ids.len()]
    } else if args.produced_empty.len() != args.dispatch_ids.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "produced_empty".into(),
            expected: args.dispatch_ids.len(),
            found: args.produced_empty.len(),
        }
        .into());
    } else {
        args.produced_empty.clone()
    };

    // One transaction for Steps 1 and 2 — see this function's doc comment for
    // why a parent row must never be visible ahead of its items.
    let p = pool.get_pool();
    let mut tx = p.begin().await?;

    // Step 1: Insert the binding rows. Their items follow in Step 2, inside this
    // same transaction, so nothing observes a valueless binding.
    let inserted_models = sqlx::query_file_as!(
        InsertedDispatchIntentBindingModel,
        "assets/.sql/dispatch_intent_bindings/insert_dispatch_intent_bindings.sql",
        &args.dispatch_ids,
        &args.value_slot_ids,
        &produced_empty,
    )
    .fetch_all(&mut *tx)
    .await?;

    // Step 2: Flatten and insert items for each type (no ordinals)
    // Text items
    let mut text_binding_ids = Vec::new();
    let mut text_values = Vec::new();

    for (idx, item_args) in args.text_items.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for value in item_args.values.iter() {
                text_binding_ids.push(binding_id);
                text_values.push(value.clone());
            }
        }
    }

    if !text_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingTextItemModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_text_items.sql",
            &text_binding_ids,
            &text_values,
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Int items
    let mut int_binding_ids = Vec::new();
    let mut int_values = Vec::new();

    for (idx, item_args) in args.int_items.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for value in item_args.values.iter() {
                int_binding_ids.push(binding_id);
                int_values.push(*value);
            }
        }
    }

    if !int_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingIntItemModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_int_items.sql",
            &int_binding_ids,
            &int_values,
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Number items
    let mut number_binding_ids = Vec::new();
    let mut number_values: Vec<Decimal> = Vec::new();

    for (idx, item_args) in args.number_items.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for value in item_args.values.iter() {
                number_binding_ids.push(binding_id);
                number_values.push(*value);
            }
        }
    }

    if !number_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingNumberItemModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_number_items.sql",
            &number_binding_ids,
            &number_values,
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Bool items
    let mut bool_binding_ids = Vec::new();
    let mut bool_values = Vec::new();

    for (idx, item_args) in args.bool_items.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for value in item_args.values.iter() {
                bool_binding_ids.push(binding_id);
                bool_values.push(*value);
            }
        }
    }

    if !bool_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingBoolItemModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_bool_items.sql",
            &bool_binding_ids,
            &bool_values,
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Json items
    let mut json_binding_ids = Vec::new();
    let mut json_values = Vec::new();

    for (idx, item_args) in args.json_items.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for value in item_args.values.iter() {
                json_binding_ids.push(binding_id);
                json_values.push(value.clone());
            }
        }
    }

    if !json_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingJsonItemModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_json_items.sql",
            &json_binding_ids,
            &json_values,
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Asset items
    let mut asset_binding_ids = Vec::new();
    let mut asset_ids = Vec::new();

    for (idx, item_args) in args.asset_items.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for asset_id in item_args.asset_ids.iter() {
                asset_binding_ids.push(binding_id);
                asset_ids.push(*asset_id);
            }
        }
    }

    if !asset_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingAssetItemModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_asset_items.sql",
            &asset_binding_ids,
            &asset_ids,
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Dataset items — pin the resolved extraction row by FK (no envelope copy).
    let mut dataset_binding_ids = Vec::new();
    let mut dataset_extraction_ids = Vec::new();

    for (idx, item_args) in args.dataset_items.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for extraction_id in item_args.asset_dataset_extraction_ids.iter() {
                dataset_binding_ids.push(binding_id);
                dataset_extraction_ids.push(*extraction_id);
            }
        }
    }

    if !dataset_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingDatasetItemModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_dataset_items.sql",
            &dataset_binding_ids,
            &dataset_extraction_ids,
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Value meta
    let mut meta_binding_ids = Vec::new();
    let mut meta_ui_specs = Vec::new();
    let mut meta_producers: Vec<Option<String>> = Vec::new();

    for (idx, item_args) in args.value_meta.iter().enumerate() {
        if idx < inserted_models.len() {
            let binding_id = inserted_models[idx].id;
            for i in 0..item_args.ui_specs.len() {
                meta_binding_ids.push(binding_id);
                meta_ui_specs.push(item_args.ui_specs[i].clone());
                meta_producers.push(item_args.producers[i].clone());
            }
        }
    }

    if !meta_binding_ids.is_empty() {
        let _rows = sqlx::query_file_as!(
            BindingValueMetaModel,
            "assets/.sql/dispatch_intent_bindings/items/insert_value_meta.sql",
            &meta_binding_ids,
            &meta_ui_specs,
            &meta_producers as &[Option<String>],
        )
        .fetch_all(&mut *tx)
        .await?;
    }

    // Bindings and their values become visible here, together. Every reader
    // downstream — readiness gates, `$in.bindings` seeding, DONE rollups,
    // pollers, the UI — now sees both or neither.
    tx.commit().await?;

    // Step 3: Fetch the full bindings with items. Runs on the pool after the
    // commit, so it reads the same state everyone else does.
    // Deduplicate dispatch_object_ids to avoid duplicate rows in the select
    let dispatch_ids: Vec<Uuid> = inserted_models
        .iter()
        .map(|m| m.dispatch_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    select_dispatch_intent_bindings_by_dispatch_ids(pool, dispatch_ids).await
}

pub async fn select_dispatch_intent_bindings_by_dispatch_ids(
    pool: &DbPool,
    dispatch_object_ids: Vec<Uuid>,
) -> Result<Vec<DispatchIntentBinding>, DataError> {
    if dispatch_object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flat_models = sqlx::query_file_as!(
        FlatDispatchIntentBindingModel,
        "assets/.sql/dispatch_intent_bindings/select_dispatch_intent_bindings_by_dispatch_ids.sql",
        &dispatch_object_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDispatchIntentBindingModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// Every binding ever attached to any dispatch of `journey_id`, ordered by
/// `created_at ASC` (tie-broken by `id ASC`). Powers
/// `GET /api/journeys/{id}/binding-history`, which dapple-ui's history /
/// timeline-replay route consumes in lieu of
/// `dispatch-intent-bindings?query=by_dispatch_ids&dispatch_ids=...`.
///
/// Unlike `select_bindings_by_journey_id` (latest-per-slot), every binding
/// is returned so the consumer can partition by `dispatch_id`.
pub async fn select_dispatch_intent_bindings_by_journey_id(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<DispatchIntentBinding>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatDispatchIntentBindingModel,
        "assets/.sql/dispatch_intent_bindings/select_dispatch_intent_bindings_by_journey_id.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDispatchIntentBindingModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}

/// Chain-aware effective-binding lookup. Walks back through
/// `predecessor_journey_id` and returns one DispatchIntentBinding per
/// value_slot_id using the journey-local-first rule (Migration 44). For
/// chain-root journeys this is equivalent to "latest binding per slot on
/// this journey's dispatches"; for chained children, slots not bound at
/// this level inherit the most recent upstream binding.
pub async fn select_effective_bindings_for_journey(
    pool: &DbPool,
    journey_id: Uuid,
) -> Result<Vec<DispatchIntentBinding>, DataError> {
    let flat_models = sqlx::query_file_as!(
        FlatDispatchIntentBindingModel,
        "assets/.sql/dispatch_intent_bindings/select_effective_bindings_for_journey.sql",
        journey_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatDispatchIntentBindingModel::fold_models(flat_models)?;
    Ok(models.into_iter().map(|m| m.into()).collect())
}
