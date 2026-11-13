use crate::error::DataError;
use crate::model::categories::InsertCategoryArgs;
use crate::model::categories::InsertedCategoryModel;
use crate::model::categories::{Category, CategoryModel, FlatCategoryModel};
use crate::{DbPool, FilterOptions, InsertColumnArgs, columns_from_tuples};
use itertools::izip;
use log::debug;
use sqlx::types::Uuid;

pub async fn select_category_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<Category, DataError> {
    let model = select_category_model_by_universal_id(pool, universal_id).await?;

    Ok(model.into())
}

pub(crate) async fn select_category_model_by_universal_id(
    pool: &DbPool,
    universal_id: Uuid,
) -> Result<CategoryModel, DataError> {
    let objs =
        select_category_models_by_universal_ids(pool, vec![universal_id], FilterOptions::default())
            .await?;

    let obj = objs
        .into_iter()
        .next()
        .ok_or(DataError::MissingIdLookup(format!(
            "category_id {} not found",
            universal_id
        )))?;

    Ok(obj)
}

pub async fn select_categories_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<Category>, DataError> {
    let models = select_category_models_by_universal_ids(pool, universal_ids, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_category_models_by_universal_ids(
    pool: &DbPool,
    universal_ids: Vec<Uuid>,
    filter_ops: FilterOptions,
) -> Result<Vec<CategoryModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatCategoryModel,
        "assets/.sql/categories/select_categories_by_object_ids.sql",
        filter_ops.sql_values().0,
        &universal_ids,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatCategoryModel::fold_models(models)
}

pub async fn select_categories_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<Category>, DataError> {
    let models = select_category_models_by_slugs(pool, slugs, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn select_category_models_by_slugs(
    pool: &DbPool,
    slugs: Vec<String>,
    filter_ops: FilterOptions,
) -> Result<Vec<CategoryModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatCategoryModel,
        "assets/.sql/categories/select_categories_by_slugs.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
        &slugs,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatCategoryModel::fold_models(models)
}

pub async fn select_categories(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<Category>, DataError> {
    let models = select_category_models(pool, filter_ops).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub async fn select_category_models(
    pool: &DbPool,
    filter_ops: FilterOptions,
) -> Result<Vec<CategoryModel>, DataError> {
    let models = sqlx::query_file_as!(
        FlatCategoryModel,
        "assets/.sql/categories/select_categories.sql",
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    FlatCategoryModel::fold_models(models)
}

pub async fn insert_categories(
    pool: &DbPool,
    objs: Vec<InsertCategoryArgs>,
) -> Result<Vec<Category>, DataError> {
    let models = insert_category_models(pool, objs).await?;

    Ok(models.into_iter().map(Into::into).collect())
}

pub(crate) async fn insert_category_models(
    pool: &DbPool,
    args: Vec<InsertCategoryArgs>,
) -> Result<Vec<CategoryModel>, DataError> {
    let p = pool.get_pool();
    let mut tx = p.begin().await?;

    validate_insert_category_args(&args)?;

    debug!("Inserting category models: {:?}", args);

    let args: (Vec<_>, Vec<_>, Vec<_>) = columns_from_tuples!(args.into_iter().flat_map(|obj| {
        let InsertCategoryArgs {
            names,
            descriptions,
            content_hashes,
        } = obj;

        izip!(
            names.into_iter(),
            descriptions.into_iter(),
            content_hashes.into_iter(),
        )
        .map(move |r| (r.0, r.1, r.2))
    }));

    debug!("Inserting category models (processed args): {:?}", args);

    let inserted_objs = sqlx::query_file_as!(
        InsertedCategoryModel,
        "assets/.sql/categories/insert_categories.sql",
        &args.2,
        &args.1,
        &args.0,
    )
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    debug!("Inserted category models: {:#?}", inserted_objs);

    Ok(inserted_objs.into_iter().map(Into::into).collect())
}

validate_insert_args!(validate_insert_category_args, InsertCategoryArgs);

// ============================================================================
// Deprecation queries
// ============================================================================

/// Arguments for deprecating categories
#[derive(Clone, Debug)]
pub struct DeprecateCategoriesArgs {
    pub path_ids: Vec<Uuid>,
    pub reasons: Vec<Option<String>>,
    pub to_level_id: i16,
    pub deprecated_by_user_id: Option<Uuid>,
}

/// Deprecate categories by their path IDs
pub async fn deprecate_categories(
    pool: &DbPool,
    args: DeprecateCategoriesArgs,
) -> Result<Vec<Category>, DataError> {
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

    let flat_models = sqlx::query_file_as!(
        FlatCategoryModel,
        "assets/.sql/categories/deprecate_categories.sql",
        args.deprecated_by_user_id,
        &args.path_ids,
        &args.reasons as &[Option<String>],
        args.to_level_id,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let models = FlatCategoryModel::fold_models(flat_models)?;
    Ok(models
        .into_iter()
        .map(|m: CategoryModel| m.into())
        .collect())
}

/// True iff `category_id` exists AND its path lives in `caller_company_id`'s
/// namespace. Used by external-API handlers to gate `GET /api/categories/{id}`.
pub async fn category_in_namespace(
    pool: &DbPool,
    category_id: Uuid,
    caller_company_id: Uuid,
) -> Result<bool, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/categories/category_in_namespace.sql",
        caller_company_id,
        category_id,
    )
    .fetch_one(pool.get_pool())
    .await?;
    Ok(row.unwrap_or(false))
}

/// External-API category listing. Returns categories whose path lives in
/// `caller_company_id`'s namespace and whose latest version is not deprecated.
pub async fn select_categories_by_external_caller(
    pool: &DbPool,
    caller_company_id: Uuid,
    filter_ops: FilterOptions,
) -> Result<Vec<Category>, DataError> {
    // Bind order is alphabetical by sql-composer convention:
    // caller_company_id, limit, offset
    let models = sqlx::query_file_as!(
        FlatCategoryModel,
        "assets/.sql/categories/select_categories_by_external_caller.sql",
        caller_company_id,
        filter_ops.sql_values().0,
        filter_ops.sql_values().1,
    )
    .fetch_all(pool.get_pool())
    .await?;

    let folded = FlatCategoryModel::fold_models(models)?;
    Ok(folded.into_iter().map(Into::into).collect())
}
