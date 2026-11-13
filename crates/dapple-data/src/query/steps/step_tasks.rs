use crate::columns_from_tuples;
use crate::model::steps::step_tasks::InsertStepTaskArgs;
use crate::model::steps::step_tasks::InsertedStepTaskModel;
use crate::model::steps::step_tasks::StepTaskModel;
use itertools::izip;
use log::debug;
use sqlx::types::Uuid;
use sqlx::{Error, Postgres, Transaction};

pub(crate) async fn insert_step_tasks(
    tx: &mut Transaction<'static, Postgres>,
    step_ids: Vec<Uuid>,
    args: Vec<InsertStepTaskArgs>,
) -> Result<Vec<StepTaskModel>, Error> {
    let inserted_models = insert_step_task_models(tx, step_ids, args).await?;

    let objs = inserted_models
        .into_iter()
        .map(|model| model.into())
        .map(|model: StepTaskModel| model)
        .collect();

    Ok(objs)
}

pub async fn insert_step_task_models(
    tx: &mut Transaction<'static, Postgres>,
    step_ids: Vec<Uuid>,
    args: Vec<InsertStepTaskArgs>,
) -> Result<Vec<InsertedStepTaskModel>, Error> {
    debug!("Inserting step_task models: {:?}", args);

    let args: (Vec<_>, Vec<_>) =
        columns_from_tuples!(step_ids.into_iter().zip(args).flat_map(|(step_id, obj)| {
            let InsertStepTaskArgs { task_ids } = obj;

            izip!(task_ids.into_iter(),).map(move |r| (step_id, r))
        }));

    debug!("Inserting step_task models (processed args): {:?}", args);

    let task = sqlx::query_file_as!(
        InsertedStepTaskModel,
        "assets/.sql/steps/step_tasks/insert_step_tasks.sql",
        &args.0,
        &args.1
    )
    .fetch_all(&mut **tx)
    .await?;

    debug!("Inserted step_task models: {:?}", task);

    Ok(task)
}

/// Which of the given step ids already have at least one `step_tasks` link.
///
/// Patch ingest uses this to skip re-linking immutable, already-materialized step
/// versions: a content-addressed step's task links are written once when the
/// version is first created, and the `(step_id, ord)` UNIQUE constraint added by
/// migration 53 makes re-linking a different task at an occupied ord collide.
pub async fn select_step_ids_with_tasks(
    tx: &mut Transaction<'static, Postgres>,
    step_ids: &[Uuid],
) -> Result<Vec<Uuid>, Error> {
    let rows = sqlx::query_file_scalar!(
        "assets/.sql/steps/step_tasks/select_step_ids_with_tasks.sql",
        step_ids
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}
