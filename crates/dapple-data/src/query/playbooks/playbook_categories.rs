use crate::columns_from_tuples;
use crate::model::playbooks::playbook_categories::{
    InsertPlaybookCategoryArgs, InsertedPlaybookCategoryModel,
};
use itertools::izip;
use sqlx::types::Uuid;
use sqlx::{Error, Postgres, Transaction};

pub(crate) async fn insert_playbook_categories_models(
    tx: &mut Transaction<'static, Postgres>,
    playbook_ids: Vec<Uuid>,
    objs: Vec<InsertPlaybookCategoryArgs>,
) -> Result<Vec<InsertedPlaybookCategoryModel>, Error> {
    insert_playbook_category_models(tx, playbook_ids, objs).await
}

pub(crate) async fn insert_playbook_category_models(
    tx: &mut Transaction<'static, Postgres>,
    playbook_ids: Vec<Uuid>,
    objs: Vec<InsertPlaybookCategoryArgs>,
) -> Result<Vec<InsertedPlaybookCategoryModel>, Error> {
    let args: (Vec<_>, Vec<_>) = columns_from_tuples!(playbook_ids.into_iter().zip(objs).flat_map(
        |(playbook_id, obj)| {
            let InsertPlaybookCategoryArgs {
                playbook_category_ids,
            } = obj;

            izip!(playbook_category_ids.into_iter(),).map(move |r| (playbook_id, r))
        }
    ));

    let inserted = sqlx::query_file_as!(
        InsertedPlaybookCategoryModel,
        "assets/.sql/playbooks/playbook_categories/insert_playbook_categories.sql",
        &args.0,
        &args.1,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}
