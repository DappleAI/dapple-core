use crate::DbPool;
use crate::error::DataError;
use crate::model::institutional_holdings::{InsertHoldingsArgs, InstitutionalHoldingModel};
use time::Date;

// ============================================================================
// Holdings cache (per-CUSIP, per-quarter)
// ============================================================================

/// Replace a (cusip, quarter) slice: the ingest job deletes then re-inserts, so a
/// re-ingest is idempotent and no upsert conflict handling is needed.
pub async fn delete_holdings_for_quarter(
    pool: &DbPool,
    cusip: &str,
    quarter_end: Date,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/institutional_holdings/delete_holdings_for_quarter.sql",
        cusip,
        quarter_end,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(())
}

/// Batch-insert deduped holding rows for one (cusip, quarter) slice.
pub async fn insert_holdings(pool: &DbPool, args: InsertHoldingsArgs) -> Result<u64, DataError> {
    if args.cusips.is_empty() {
        return Ok(0);
    }
    let res = sqlx::query_file!(
        "assets/.sql/institutional_holdings/insert_holdings.sql",
        &args.cusips,
        &args.quarter_ends,
        &args.manager_ciks as &[Option<i64>],
        &args.manager_names,
        &args.shares,
        &args.values_usd,
        &args.accessions,
        &args.filed_dates as &[Option<Date>],
    )
    .execute(pool.get_pool())
    .await?;
    Ok(res.rows_affected())
}

/// All cached holders of a security within a quarter-end range (inclusive),
/// newest position first within a quarter.
pub async fn select_holders_by_cusip(
    pool: &DbPool,
    cusip: &str,
    from_quarter: Date,
    to_quarter: Date,
) -> Result<Vec<InstitutionalHoldingModel>, DataError> {
    let rows = sqlx::query_file_as!(
        InstitutionalHoldingModel,
        "assets/.sql/institutional_holdings/select_holders_by_cusip.sql",
        cusip,
        from_quarter,
        to_quarter,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

// ============================================================================
// Ingest state
// ============================================================================

/// Mark a (cusip, quarter) slice as ingested (idempotent).
pub async fn mark_quarter_ingested(
    pool: &DbPool,
    cusip: &str,
    quarter_end: Date,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/institutional_holdings/mark_quarter_ingested.sql",
        cusip,
        quarter_end,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(())
}

/// The quarters already cached for a security, so the ingest fills only the gap.
pub async fn select_ingested_quarters(pool: &DbPool, cusip: &str) -> Result<Vec<Date>, DataError> {
    let rows = sqlx::query_file_scalar!(
        "assets/.sql/institutional_holdings/select_ingested_quarters.sql",
        cusip,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

// ============================================================================
// Manager-first ingest state
// ============================================================================
//
// The same rows table filled from the opposite direction: one manager, every
// security it held. ⚠️ These functions must NEVER be mixed with the CUSIP-scoped
// ones above — see migration 94. A manager-first write that marked a CUSIP
// ingested would make `edgar holders <cusip>` skip its download and report one
// manager as the entire holder base.

/// Replace one MANAGER's quarter: delete then re-insert, so a re-persist is
/// idempotent. Scoped by `(manager_cik, quarter_end)`, never by cusip.
pub async fn delete_holdings_for_manager_quarter(
    pool: &DbPool,
    manager_cik: i64,
    quarter_end: Date,
) -> Result<u64, DataError> {
    let res = sqlx::query_file!(
        "assets/.sql/institutional_holdings/delete_holdings_for_manager_quarter.sql",
        manager_cik,
        quarter_end,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(res.rows_affected())
}

/// Mark a `(manager, quarter)` slice as stored (idempotent).
///
/// Asserts "every security this manager reported that quarter is stored" — NOT
/// anything about any CUSIP's holder base.
pub async fn mark_manager_quarter_ingested(
    pool: &DbPool,
    manager_cik: i64,
    quarter_end: Date,
    accession: Option<&str>,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/institutional_holdings/mark_manager_quarter_ingested.sql",
        manager_cik,
        quarter_end,
        accession,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(())
}

/// The quarters already stored for a manager, so a re-persist writes only the gap.
pub async fn select_ingested_manager_quarters(
    pool: &DbPool,
    manager_cik: i64,
) -> Result<Vec<Date>, DataError> {
    let rows = sqlx::query_file_scalar!(
        "assets/.sql/institutional_holdings/select_ingested_manager_quarters.sql",
        manager_cik,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

// ============================================================================
// Resolved CUSIP cache (per company CIK)
// ============================================================================

/// The cached common-stock CUSIP resolved for a company CIK, if any.
pub async fn select_cusip_for_cik(pool: &DbPool, cik: i64) -> Result<Option<String>, DataError> {
    let row = sqlx::query_file_scalar!(
        "assets/.sql/institutional_holdings/select_cusip_for_cik.sql",
        cik,
    )
    .fetch_optional(pool.get_pool())
    .await?;
    Ok(row)
}

/// Cache a company's resolved CUSIP (idempotent).
pub async fn upsert_cusip_for_cik(pool: &DbPool, cik: i64, cusip: &str) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/institutional_holdings/upsert_cusip_for_cik.sql",
        cik,
        cusip,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(())
}
