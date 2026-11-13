use crate::error::DataError;
use crate::model::hats::hat_categories::InsertedHatCategoryModel;
use sqlx::types::Uuid;

pub async fn insert_hat_categories(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    hat_ids: Vec<Uuid>,
    category_ids: Vec<Uuid>,
) -> Result<Vec<InsertedHatCategoryModel>, DataError> {
    let inserted = sqlx::query_file_as!(
        InsertedHatCategoryModel,
        "assets/.sql/hats/hat_categories/insert_hat_categories.sql",
        &hat_ids,
        &category_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(inserted)
}
