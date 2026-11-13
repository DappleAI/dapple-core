-- Migration 89: Insider (Form 4) transactions, on-demand per-issuer cache
--
-- A lean cache of Section 16 insider transactions parsed from Form 4/4-A XML,
-- populated ON DEMAND the first time a company is reviewed (the run-the-10k
-- annual pull) and queried by `edgar insiders`. Like rev.institutional_holdings
-- (Migration 88) this is NOT a full-universe index: only issuers that have been
-- reviewed are stored.
--
-- WHY PER-FILING RATHER THAN PER-QUARTER. The 13F cache keys its ingest state on
-- (cusip, quarter) because 13F data is filed by thousands of managers and only
-- reaches us through the SEC DERA quarterly data sets. Form 4s are filed by the
-- issuer's own insiders and are indexed under the ISSUER's CIK, so the company's
-- own submissions feed lists every one of them and each document is ~5 KB. We
-- therefore fetch the filings directly, which is both far smaller (a full window
-- is single-digit MB against hundreds for DERA) and far fresher (a Form 4 is
-- readable within days of filing, where a DERA quarter publishes in arrears).
-- Ingest state is keyed on the accession because that is the unit we fetch.
--
-- A filing is replaced wholesale (DELETE its rows, then INSERT), so re-ingesting
-- an accession is idempotent and the insert needs no conflict handling. A Form 4
-- carrying no transactions still gets an ingest row, so it is not refetched.

CREATE TABLE rev.insider_transactions (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    issuer_cik BIGINT NOT NULL,
    accession TEXT NOT NULL,
    filed_date DATE,
    trans_date DATE,
    -- The reporting owner. One Form 4 has exactly one owner in practice, but the
    -- schema allows several, so the owner is carried per transaction row.
    owner_cik BIGINT,
    owner_name TEXT NOT NULL,
    is_director BOOLEAN NOT NULL DEFAULT false,
    is_officer BOOLEAN NOT NULL DEFAULT false,
    is_ten_pct BOOLEAN NOT NULL DEFAULT false,
    officer_title TEXT,
    security_title TEXT,
    -- Form 4 Table I/II transaction code: P purchase, S sale, A award, M option
    -- exercise, F shares withheld for taxes, G gift, C conversion, D disposition
    -- to the issuer. The report MUST separate discretionary (P/S) from
    -- compensation flow (A/M/F); counting withheld-for-tax shares as selling
    -- would misstate the record.
    trans_code TEXT,
    acquired_disposed TEXT CHECK (acquired_disposed IN ('A', 'D')),
    shares DOUBLE PRECISION,
    price_per_share DOUBLE PRECISION,
    shares_owned_after DOUBLE PRECISION,
    -- Table II (derivative) rather than Table I (non-derivative).
    is_derivative BOOLEAN NOT NULL DEFAULT false,
    -- The Form 4 <aff10b5One> flag: the transaction was made under a pre-arranged
    -- Rule 10b5-1 plan, i.e. scheduled ahead of time rather than decided on the
    -- day. Kept so a scheduled sale is never presented as an opportunistic one.
    is_10b5_1 BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The one hot query: an issuer's transactions across a date window.
CREATE INDEX insider_transactions_issuer_date_idx
    ON rev.insider_transactions (issuer_cik, trans_date);

-- Replacing a filing's rows needs this one.
CREATE INDEX insider_transactions_accession_idx
    ON rev.insider_transactions (accession);

-- Which Form 4 accessions have been parsed, so a repeat review refetches nothing
-- and a widened window fetches only the filings it has not seen.
CREATE TABLE rev.insider_transactions_ingest (
    issuer_cik BIGINT NOT NULL,
    accession TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'ok',
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (issuer_cik, accession)
);
