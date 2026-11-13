//! `edgar manager --persist` writes into the SAME cache `edgar holders` reads,
//! from the opposite direction. Corrupt-then-verify against the DB, never
//! against a response field.
//!
//! The assertion that matters most is a NEGATIVE one: a manager-first write must
//! leave `rev.institutional_holdings_ingest` (the CUSIP completeness marker)
//! untouched. `edgar holders` trusts that marker to SKIP its 99.4 MB download, so
//! setting it here would make `edgar holders <cusip>` return the one manager we
//! happened to store and present it as the entire holder base — every row
//! genuine, the coverage claim false. Nothing else in the suite would catch it.

use dapple_data::DbPool;
use dapple_data::model::institutional_holdings::InsertHoldingsArgs;
use dapple_data::query::institutional_holdings::{
    delete_holdings_for_manager_quarter, insert_holdings, mark_manager_quarter_ingested,
    select_ingested_manager_quarters, select_ingested_quarters,
};
use time::Date;
use time::macros::date;

// CIKs no real filer will collide with, so a test only ever claims its own rows
// against a shared dev database. Cargo runs these concurrently, so each test
// gets a DISJOINT namespace — sharing one made test B's cleanup delete test A's
// rows mid-flight.
const CIK_REPLACE: i64 = -987_654_301;
const CIK_ISOLATION: i64 = -987_654_311;
const CIK_ISOLATION_OTHER: i64 = -987_654_312;
const QE: Date = date!(2025 - 03 - 31);
/// Two CUSIPs this manager "holds". 9 chars to satisfy the column's length
/// CHECK, but SYNTHETIC for the same reason the CIKs above are negative: the
/// negative assertion below reads the shared `rev.institutional_holdings_ingest`
/// marker, and a REAL CUSIP (this test originally used Apple's 037833100) is
/// legitimately marked on any dev DB where someone has run `edgar holders` on
/// it — turning the landmine check into a permanent false alarm.
const CUSIP_A: &str = "ZZTEST00A";
const CUSIP_B: &str = "ZZTEST00B";

fn args(cik: i64, rows: &[(&str, f64, f64)]) -> InsertHoldingsArgs {
    let mut a = InsertHoldingsArgs::default();
    for (cusip, shares, value) in rows {
        a.cusips.push((*cusip).to_string());
        a.quarter_ends.push(QE);
        a.manager_ciks.push(Some(cik));
        a.manager_names.push("Persist Test Manager".into());
        a.shares.push(*shares);
        a.values_usd.push(*value);
        a.accessions.push("acc-persist-test".into());
        a.filed_dates.push(None);
    }
    a
}

async fn count_rows(pool: &DbPool, cik: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM rev.institutional_holdings \
         WHERE manager_cik = $1 AND quarter_end = $2",
    )
    .bind(cik)
    .bind(QE)
    .fetch_one(pool.get_pool())
    .await
}

#[tokio::test]
async fn a_manager_write_replaces_its_own_quarter_and_never_claims_a_cusip()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {e:?}. Skipping test.");
            return Ok(());
        }
    };

    // Leave nothing behind from a previous run.
    delete_holdings_for_manager_quarter(&pool, CIK_REPLACE, QE).await?;

    // 1. First write.
    let n = insert_holdings(
        &pool,
        args(
            CIK_REPLACE,
            &[(CUSIP_A, 100.0, 1000.0), (CUSIP_B, 50.0, 500.0)],
        ),
    )
    .await?;
    assert_eq!(n, 2);
    mark_manager_quarter_ingested(&pool, CIK_REPLACE, QE, Some("acc-persist-test")).await?;
    assert_eq!(count_rows(&pool, CIK_REPLACE).await?, 2, "both rows landed");

    // The manager marker is set...
    let marked = select_ingested_manager_quarters(&pool, CIK_REPLACE).await?;
    assert!(marked.contains(&QE), "the manager-quarter must be marked");

    // ...and the CUSIP marker is NOT. THE LANDMINE. If this ever flips, every
    // `edgar holders` call for these securities silently under-reports its
    // holder base to whatever this manager happened to hold.
    for cusip in [CUSIP_A, CUSIP_B] {
        let cusip_marked = select_ingested_quarters(&pool, cusip).await?;
        assert!(
            !cusip_marked.contains(&QE),
            "a manager-first write marked {cusip} complete for {QE}; \
             `edgar holders {cusip}` would now skip its download and report one manager \
             as the whole holder base"
        );
    }

    // 2. Re-persist is idempotent AND replaces: the manager dropped B and
    //    doubled A. Corrupt the expectation by writing a different slice, then
    //    verify the old row is gone rather than accumulated.
    let removed = delete_holdings_for_manager_quarter(&pool, CIK_REPLACE, QE).await?;
    assert_eq!(removed, 2, "the replace clears the manager's whole quarter");
    insert_holdings(&pool, args(CIK_REPLACE, &[(CUSIP_A, 200.0, 2000.0)])).await?;
    mark_manager_quarter_ingested(&pool, CIK_REPLACE, QE, Some("acc-persist-test-2")).await?;

    assert_eq!(
        count_rows(&pool, CIK_REPLACE).await?,
        1,
        "the dropped position is gone"
    );
    let shares: f64 = sqlx::query_scalar::<_, f64>(
        "SELECT shares FROM rev.institutional_holdings \
         WHERE manager_cik = $1 AND quarter_end = $2 AND cusip = $3",
    )
    .bind(CIK_REPLACE)
    .bind(QE)
    .bind(CUSIP_A)
    .fetch_one(pool.get_pool())
    .await?;
    assert_eq!(
        shares, 200.0,
        "the surviving position was restated, not doubled"
    );

    // The marker stays single-rowed rather than accumulating per re-persist.
    let marked = select_ingested_manager_quarters(&pool, CIK_REPLACE).await?;
    assert_eq!(marked.iter().filter(|d| **d == QE).count(), 1);

    // 3. Clean up so a shared dev DB does not accumulate test rows.
    delete_holdings_for_manager_quarter(&pool, CIK_REPLACE, QE).await?;
    sqlx::query("DELETE FROM rev.institutional_holdings_manager_ingest WHERE manager_cik = $1")
        .bind(CIK_REPLACE)
        .execute(pool.get_pool())
        .await?;
    assert_eq!(count_rows(&pool, CIK_REPLACE).await?, 0);
    Ok(())
}

/// A manager-first delete must not touch other managers' rows for the same
/// security — that is what makes the two writers able to share one table.
#[tokio::test]
async fn a_manager_write_leaves_other_managers_alone()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = match DbPool::connect().await {
        Ok(p) => p,
        Err(e) => {
            println!("DB not available: {e:?}. Skipping test.");
            return Ok(());
        }
    };
    delete_holdings_for_manager_quarter(&pool, CIK_ISOLATION, QE).await?;
    delete_holdings_for_manager_quarter(&pool, CIK_ISOLATION_OTHER, QE).await?;

    // Another manager already holds the same security — as a per-CUSIP ingest
    // would have left it.
    let mut other = args(CIK_ISOLATION_OTHER, &[(CUSIP_A, 999.0, 9990.0)]);
    other.manager_names = vec!["Someone Else".into()];
    insert_holdings(&pool, other).await?;

    insert_holdings(&pool, args(CIK_ISOLATION, &[(CUSIP_A, 100.0, 1000.0)])).await?;
    // Replace OUR quarter.
    let removed = delete_holdings_for_manager_quarter(&pool, CIK_ISOLATION, QE).await?;
    assert_eq!(removed, 1, "only our row");

    let survivors: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM rev.institutional_holdings \
         WHERE manager_cik = $1 AND quarter_end = $2 AND cusip = $3",
    )
    .bind(CIK_ISOLATION_OTHER)
    .bind(QE)
    .bind(CUSIP_A)
    .fetch_one(pool.get_pool())
    .await?;
    assert_eq!(survivors, 1, "the other manager's row survived our replace");

    delete_holdings_for_manager_quarter(&pool, CIK_ISOLATION_OTHER, QE).await?;
    Ok(())
}
