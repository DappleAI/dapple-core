use crate::DbPool;
use crate::error::DataError;
use crate::model::insider_transactions::{InsertInsiderTransactionsArgs, InsiderTransactionModel};
use time::Date;

// ============================================================================
// Insider transaction cache (per-issuer, per-filing)
// ============================================================================

/// Replace one filing's rows: the ingest deletes then re-inserts, so a re-ingest
/// (an amended 4/A restating the same accession) is idempotent and no upsert
/// conflict handling is needed.
pub async fn delete_insider_transactions_for_accession(
    pool: &DbPool,
    issuer_cik: i64,
    accession: &str,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/insider_transactions/delete_insider_transactions_for_accession.sql",
        issuer_cik,
        accession,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(())
}

/// Batch-insert the transaction rows parsed from one Form 4.
pub async fn insert_insider_transactions(
    pool: &DbPool,
    args: InsertInsiderTransactionsArgs,
) -> Result<u64, DataError> {
    if args.issuer_ciks.is_empty() {
        return Ok(0);
    }
    let res = sqlx::query_file!(
        "assets/.sql/insider_transactions/insert_insider_transactions.sql",
        &args.issuer_ciks,
        &args.accessions,
        &args.filed_dates as &[Option<Date>],
        &args.trans_dates as &[Option<Date>],
        &args.owner_ciks as &[Option<i64>],
        &args.owner_names,
        &args.is_directors,
        &args.is_officers,
        &args.is_ten_pcts,
        &args.officer_titles as &[Option<String>],
        &args.security_titles as &[Option<String>],
        &args.trans_codes as &[Option<String>],
        &args.acquired_disposeds as &[Option<String>],
        &args.shares as &[Option<f64>],
        &args.prices_per_share as &[Option<f64>],
        &args.shares_owned_after as &[Option<f64>],
        &args.is_derivatives,
        &args.is_10b5_1s,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(res.rows_affected())
}

/// Every cached insider transaction for an issuer within a transaction-date range
/// (inclusive), oldest first.
pub async fn select_insider_transactions_by_issuer(
    pool: &DbPool,
    issuer_cik: i64,
    from_date: Date,
    to_date: Date,
) -> Result<Vec<InsiderTransactionModel>, DataError> {
    let rows = sqlx::query_file_as!(
        InsiderTransactionModel,
        "assets/.sql/insider_transactions/select_insider_transactions_by_issuer.sql",
        issuer_cik,
        from_date,
        to_date,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}

// ============================================================================
// Ingest state
// ============================================================================

/// Mark a Form 4 accession as parsed (idempotent). Written for every parsed
/// filing, including one that carried no transactions, so it is not refetched.
pub async fn mark_filing_ingested(
    pool: &DbPool,
    issuer_cik: i64,
    accession: &str,
) -> Result<(), DataError> {
    sqlx::query_file!(
        "assets/.sql/insider_transactions/mark_filing_ingested.sql",
        issuer_cik,
        accession,
    )
    .execute(pool.get_pool())
    .await?;
    Ok(())
}

/// The accessions already parsed for an issuer, so the fetch skips them.
pub async fn select_ingested_accessions(
    pool: &DbPool,
    issuer_cik: i64,
) -> Result<Vec<String>, DataError> {
    let rows = sqlx::query_file_scalar!(
        "assets/.sql/insider_transactions/select_ingested_accessions.sql",
        issuer_cik,
    )
    .fetch_all(pool.get_pool())
    .await?;
    Ok(rows)
}
