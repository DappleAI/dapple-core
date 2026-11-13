use crate::columns_from_tuples;
use crate::model::dispatches::dispatch_tasks::InsertDispatchTaskArgs;
use crate::model::dispatches::dispatch_tasks::InsertedDispatchTasksModel;
use itertools::izip;
use sqlx::types::Uuid;
use sqlx::{Error, Postgres, Transaction};

pub(crate) async fn insert_dispatch_task_models(
    tx: &mut Transaction<'static, Postgres>,
    dispatch_step_ids: Vec<Uuid>,
    objs: Vec<InsertDispatchTaskArgs>,
) -> Result<Vec<InsertedDispatchTasksModel>, Error> {
    let args: (Vec<_>, Vec<_>) =
        columns_from_tuples!(dispatch_step_ids.into_iter().zip(objs).flat_map(
            |(dispatch_id, obj)| {
                let InsertDispatchTaskArgs { step_task_ids } = obj;

                izip!(step_task_ids.into_iter(),).map(move |r| (dispatch_id, r))
            }
        ));

    let task = sqlx::query_file_as!(
        InsertedDispatchTasksModel,
        "assets/.sql/dispatches/dispatch_tasks/insert_dispatch_tasks.sql",
        &args.0,
        &args.1,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(task)
}
