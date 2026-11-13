use time::Date;

// ============================================================================
// Insider (Form 4) transaction models
// ============================================================================

/// One cached Form 4 transaction row, as selected by
/// `select_insider_transactions_by_issuer`.
///
/// `trans_code` is the Form 4 code as filed and is NOT interpreted here: the
/// split between discretionary trading (P/S) and compensation flow (A/M/F) lives
/// in `dapple-prompt-flow`'s `edgar_form345`, so the data layer stores what was
/// filed and nothing more.
#[derive(Clone, Debug)]
pub struct InsiderTransactionModel {
    pub issuer_cik: i64,
    pub accession: String,
    pub filed_date: Option<Date>,
    pub trans_date: Option<Date>,
    pub owner_cik: Option<i64>,
    pub owner_name: String,
    pub is_director: bool,
    pub is_officer: bool,
    pub is_ten_pct: bool,
    pub officer_title: Option<String>,
    pub security_title: Option<String>,
    pub trans_code: Option<String>,
    pub acquired_disposed: Option<String>,
    pub shares: Option<f64>,
    pub price_per_share: Option<f64>,
    pub shares_owned_after: Option<f64>,
    pub is_derivative: bool,
    pub is_10b5_1: bool,
}

/// Column-wise insert arguments (all vectors MUST be the same length), mirroring
/// the `InsertHoldingsArgs` UNNEST pattern.
#[derive(Clone, Debug, Default)]
pub struct InsertInsiderTransactionsArgs {
    pub issuer_ciks: Vec<i64>,
    pub accessions: Vec<String>,
    pub filed_dates: Vec<Option<Date>>,
    pub trans_dates: Vec<Option<Date>>,
    pub owner_ciks: Vec<Option<i64>>,
    pub owner_names: Vec<String>,
    pub is_directors: Vec<bool>,
    pub is_officers: Vec<bool>,
    pub is_ten_pcts: Vec<bool>,
    pub officer_titles: Vec<Option<String>>,
    pub security_titles: Vec<Option<String>>,
    pub trans_codes: Vec<Option<String>>,
    pub acquired_disposeds: Vec<Option<String>>,
    pub shares: Vec<Option<f64>>,
    pub prices_per_share: Vec<Option<f64>>,
    pub shares_owned_after: Vec<Option<f64>>,
    pub is_derivatives: Vec<bool>,
    pub is_10b5_1s: Vec<bool>,
}
