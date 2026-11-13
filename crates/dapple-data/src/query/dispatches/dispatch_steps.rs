use crate::error::{DataError, InsertArgColumnMismatch};
use crate::model::dispatches::dispatch_steps::{
    DispatchStepModel, InsertDispatchStepsArgs, InsertedDispatchStepModel,
};
use crate::query::dispatches::dispatch_tasks::insert_dispatch_task_models;
use crate::{MergeOrdered, columns_from_tuples};
use itertools::izip;
use sqlx::types::Uuid;
use sqlx::{Postgres, Transaction};

pub(crate) async fn insert_dispatch_step_models(
    tx: &mut Transaction<'static, Postgres>,
    dispatch_ids: Vec<Uuid>,
    objs: Vec<InsertDispatchStepsArgs>,
) -> Result<Vec<DispatchStepModel>, DataError> {
    validate_insert_dispatch_step_args(&dispatch_ids, &objs)?;

    let args: (Vec<_>, Vec<_>, Vec<_>, Vec<_>) = columns_from_tuples!(
        dispatch_ids
            .into_iter()
            .zip(objs)
            .flat_map(|(dispatch_id, obj)| {
                let InsertDispatchStepsArgs {
                    stage_step_ids,
                    params,
                    tasks,
                } = obj;

                izip!(
                    stage_step_ids.into_iter(),
                    params.into_iter(),
                    tasks.into_iter(),
                )
                .map(move |r| (dispatch_id, r.0, r.1, r.2))
            })
    );

    let inserted_steps = sqlx::query_file_as!(
        InsertedDispatchStepModel,
        "assets/.sql/dispatches/dispatch_steps/insert_dispatch_steps.sql",
        &args.0,
        &args.1,
        &args.2
    )
    .fetch_all(&mut **tx)
    .await?;

    let inserted_tasks: Vec<_> = insert_dispatch_task_models(
        tx,
        inserted_steps.iter().map(|r| r.id).collect(),
        args.3.clone(),
    )
    .await?
    .into_iter()
    .map(|t| t.into())
    .collect();

    DispatchStepModel::merge_ordered(&inserted_steps, inserted_tasks)
}
fn validate_insert_dispatch_step_args(
    dispatch_ids: &[Uuid],
    objs: &[InsertDispatchStepsArgs],
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
        if obj.params.len() != obj.tasks.len() {
            return Err(InsertArgColumnMismatch::InvalidColumnSize {
                column: format!("InsertDispatchArgs[{idx}].steps"),
                expected: obj.params.len(),
                found: obj.tasks.len(),
            }
            .into());
        }
    }

    Ok(())
}
