use time::Date;

// ============================================================================
// Institutional 13F holdings models
// ============================================================================

/// One cached holding row: a manager's position in a security for a quarter,
/// as selected by `select_holders_by_cusip`.
#[derive(Clone, Debug)]
pub struct InstitutionalHoldingModel {
    pub cusip: String,
    pub quarter_end: Date,
    pub manager_cik: Option<i64>,
    pub manager_name: String,
    pub shares: f64,
    pub value_usd: f64,
    pub accession: String,
    pub filed_date: Option<Date>,
}

/// Column-wise insert arguments (all vectors MUST be the same length), mirroring
/// the `InsertBackgroundJobArgs` UNNEST pattern.
#[derive(Clone, Debug, Default)]
pub struct InsertHoldingsArgs {
    pub cusips: Vec<String>,
    pub quarter_ends: Vec<Date>,
    pub manager_ciks: Vec<Option<i64>>,
    pub manager_names: Vec<String>,
    pub shares: Vec<f64>,
    pub values_usd: Vec<f64>,
    pub accessions: Vec<String>,
    pub filed_dates: Vec<Option<Date>>,
}
