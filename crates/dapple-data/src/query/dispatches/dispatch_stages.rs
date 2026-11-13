use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::dispatches::dispatch_stages::{
    DispatchStageModel, InsertDispatchStagesArgs, InsertedDispatchStageModel,
};
use crate::query::dispatches::dispatch_steps::insert_dispatch_step_models;
use crate::{MergeOrdered, columns_from_tuples};
use itertools::izip;
use sqlx::types::Uuid;
use sqlx::{Postgres, Transaction, query_file_as};

// Select helper not implemented here; stages selection is handled by the principal dispatch select SQL which returns flat rows.

pub(crate) async fn insert_dispatch_stage_models(
    tx: &mut Transaction<'static, Postgres>,
    dispatch_ids: Vec<Uuid>,
    objs: Vec<InsertDispatchStagesArgs>,
) -> Result<Vec<DispatchStageModel>, DataError> {
    validate_insert_dispatch_stage_args(&dispatch_ids, &objs)?;

    let args: (Vec<_>, Vec<_>, Vec<_>, Vec<_>) = columns_from_tuples!(
        dispatch_ids
            .into_iter()
            .zip(objs)
            .flat_map(|(dispatch_id, obj)| {
                let InsertDispatchStagesArgs {
                    playbook_stage_ids,
                    params,
                    steps,
                } = obj;

                izip!(
                    playbook_stage_ids.into_iter(),
                    params.into_iter(),
                    steps.into_iter()
                )
                .map(move |r| (dispatch_id, r.0, r.1, r.2))
            })
    );

    let inserted_stages = query_file_as!(
        InsertedDispatchStageModel,
        "assets/.sql/dispatches/dispatch_stages/insert_dispatch_stages.sql",
        &args.0,
        &args.1,
        &args.2
    )
    .fetch_all(&mut **tx)
    .await?;

    // args.3 contains per-stage InsertDispatchStepsArgs in the same order
    let inserted_steps: Vec<_> = insert_dispatch_step_models(
        tx,
        inserted_stages.iter().map(|r| r.id).collect(),
        args.3.clone(),
    )
    .await?
    .into_iter()
    .collect();

    DispatchStageModel::merge_ordered(&inserted_stages, inserted_steps)
}

fn validate_insert_dispatch_stage_args(
    dispatch_ids: &[Uuid],
    objs: &[InsertDispatchStagesArgs],
) -> Result<(), DataError> {
    if dispatch_ids.len() != objs.len() {
        return Err(InsertArgColumnMismatch::InvalidColumnSize {
            column: "dispatch_ids".to_string(),
            expected: dispatch_ids.len(),
            found: objs.len(),
        }
        .into());
    }

    for (idx, obj) in objs.iter().enumerate() {
        if obj.playbook_stage_ids.len() != obj.params.len() {
            return Err(InsertArgColumnMismatch::InvalidColumnSize {
                column: format!("InsertDispatchStagesArgs[{idx}].stages"),
                expected: obj.playbook_stage_ids.len(),
                found: obj.params.len(),
            }
            .into());
        }
    }

    Ok(())
}
