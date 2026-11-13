pub mod hat_categories;

use crate::error::DataError;
use crate::model::hats::{FlatHatModel, Hat, HatModel, InsertHatArgs, InsertedHatModel};
use crate::{
    DbPool, FilterOptions, InsertColumnArgs, Keyed, columns_from_tuples, get_single_row_by_id,
};
use itertools::izip;
use sqlx::types::Uuid;

pub async fn select_hats(pool: &DbPool, filter_ops: FilterOptions) -> Result<Vec<Hat>, DataError> {
    let models = select_hat_models(pool, filter_ops).await?;

    let hats = models.into_iter().map(|m| m.into()).collect();

    Ok(hats)
}

pub(crate) async fn select_hat_models(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<HatModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatHatModel,
        "assets/.sql/hats/select_hats.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatHatModel::fold_models(models)
}

pub async fn select_hats_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<Hat>, DataError> {
    let models = select_hat_models_by_slugs(pool, slugs, filter_ops).await?;

    let hats = models.into_iter().map(|m| m.into()).collect();

    Ok(hats)
}

pub async fn select_hat_models_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<HatModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatHatModel,
        "assets/.sql/hats/select_hats_by_slugs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatHatModel::fold_models(models)
}

pub async fn select_hat_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<Hat, DataError> {
    let obj = select_hat_model_by_universal_id(pool, universal_id).await?;
    Ok(obj.into())
}

pub(crate) async fn select_hat_model_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<HatModel, DataError> {
    let objs =
        select_hat_models_by_universal_ids(pool, vec![universal_id], FilterOptions::default())
            .await?;
    get_single_row_by_id(universal_id, objs)
}

pub async fn select_hat_models_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<HatModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatHatModel,
        "assets/.sql/hats/select_hats_by_object_ids.sql",
        filter_ops.sql_values().0,
        &universal_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatHatModel::fold_models(models)
}

pub async fn insert_hats(pool: &DbPool, objs: Vec<InsertHatArgs>) -> Result<Vec<Hat>, DataError> {
    let hat_models = insert_hat_models(pool, objs).await?;

    let hats = hat_models.into_iter().map(|model| model.into()).collect();

    Ok(hats)
}

pub(crate) async fn insert_hat_models(
    pool: &DbPool,
    objs: Vec<InsertHatArgs>,
) -> Result<Vec<HatModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;

    // validate input sizes
    validate_insert_hat_args(&objs)?;

    let args: (Vec<_>, Vec<_>, Vec<_>, Vec<_>) =
        columns_from_tuples!(objs.into_iter().flat_map(|obj| {
            let InsertHatArgs {
                names,
                descriptions,
                categories,
                content_hashes,
            } = obj;
            izip!(
                names.into_iter(),
                descriptions.into_iter(),
                categories.into_iter(),
                content_hashes.into_iter(),
            )
            .map(move |r| (r.0, r.1, r.2, r.3))
        }));

    let inserted_objs = sqlx::query_file_as!(
        InsertedHatModel,
        "assets/.sql/hats/insert_hats.sql",
        &args.3,
        &args.1,
        &args.0,
    )
    .fetch_all(&mut *tx)
    .await?;

    // insert categories associated with the created hats (one-to-many)

    // Build pairs of (hat_id, category_id) where hat_ids are repeated for each category
    // This ensures parallel array alignment in the SQL UNNEST
    let (hat_ids, category_args): (Vec<Uuid>, Vec<Uuid>) = inserted_objs
        .iter()
        .zip(args.2.iter())
        .flat_map(|(hat, cat_args)| {
            cat_args
                .hat_category_ids
                .iter()
                .map(move |cat_id| (hat.id, *cat_id))
        })
        .unzip();

    // each hat's categories are expected to be provided per hat in corresponding order
    let _inserted_categories =
        crate::query::hats::hat_categories::insert_hat_categories(&mut tx, hat_ids, category_args)
            .await?;

    tx.commit().await?;

    // build HatModel structs and attach categories mapping
    let mut hat_models: Vec<HatModel> = inserted_objs
        .into_iter()
        .map(|p| HatModel {
            id: p.id,
            versioned_object_id: p.versioned_object_id,
            path_id: p.path_id,
            name: p.name,
            slug: p.slug,
            content_hash: p.content_hash,
            deprecation_level_key: None,
            description: p.description,
            created_at: p.created_at,
            categories: vec![],
        })
        .collect();

    // collect categories from inserted categories
    use std::collections::HashMap;
    let mut categories_map: HashMap<Uuid, Vec<crate::reference::CategoryReference>> =
        HashMap::new();
    for c in _inserted_categories.into_iter() {
        categories_map
            .entry(c.hat_id)
            .or_default()
            .push(crate::reference::CategoryReference::from(c.category_id));
    }

    for hm in hat_models.iter_mut() {
        hm.categories = categories_map.remove(hm.id()).unwrap_or_default();
    }

    Ok(hat_models)
}

validate_insert_args!(validate_insert_hat_args, InsertHatArgs);

pub async fn select_hats_by_path_ids(
    pool: &DbPool,
    path_ids: Vec<Uuid>,
) -> Result<Vec<Hat>, DataError> {
    if path_ids.is_empty() {
        return Ok(Vec::new());
    }

    let models = sqlx::query_file_as!(
        FlatHatModel,
        "assets/.sql/hats/select_hats_by_path_ids.sql",
        &path_ids,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let hat_models = FlatHatModel::fold_models(models)?;
    Ok(hat_models.into_iter().map(Into::into).collect())
}

// ============================================================================
// Deprecation queries
// ============================================================================

/// Row returned from deprecate_hats SQL
#[derive(Debug)]
struct DeprecatedPathRow {
    path_id: Uuid,
}

/// Arguments for deprecating hats
#[derive(Clone, Debug)]
pub struct DeprecateHatsArgs {
    pub path_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate hats by their path IDs
pub async fn deprecate_hats(pool: &DbPool, args: DeprecateHatsArgs) -> Result<Vec<Hat>, DataError> {
    if args.path_ids.is_empty() {
        return Ok(Vec::new());
    }

    if args.path_ids.len() != args.reasons.len() {
        return Err(DataError::Custom(format!(
            "reasons length {} does not match path_ids length {}",
            args.reasons.len(),
            args.path_ids.len()
        )));
    }

    // Execute deprecation and get back the path IDs
    let deprecated_rows = sqlx::query_file_as!(
        DeprecatedPathRow,
        "assets/.sql/hats/deprecate_hats.sql",
        args.deprecated_by_user_id,
        &args.path_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    // Re-fetch the full hats with associations
    let path_ids: Vec<Uuid> = deprecated_rows.iter().map(|r| r.path_id).collect();
    select_hats_by_path_ids(pool, path_ids).await
}
