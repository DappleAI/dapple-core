-- Migration 88: Institutional 13F holdings (on-demand, per-CUSIP cache)
--
-- A lean cache of quarterly 13F-HR holdings extracted from the SEC DERA "Form 13F
-- Data Sets", populated ON DEMAND for a company the first time it is reviewed (the
-- run-the-10k Ownership and Control pass), and queried by `edgar holders`. This is
-- NOT a full-universe index: only CUSIPs that have been reviewed are stored, one
-- row per (manager, quarter) for the target security. The ingest job replaces a
-- (cusip, quarter) slice wholesale (DELETE then INSERT), so no upsert conflict
-- handling is needed and re-ingest is idempotent.

CREATE TABLE rev.institutional_holdings (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    cusip TEXT NOT NULL CHECK (LENGTH(cusip) = 9),
    quarter_end DATE NOT NULL,           -- period of report (quarter-end date)
    manager_cik BIGINT,                  -- filer CIK; null when absent in the data set
    manager_name TEXT NOT NULL,
    shares DOUBLE PRECISION NOT NULL,
    value_usd DOUBLE PRECISION NOT NULL, -- normalized to whole dollars (pre-2023 was thousands)
    accession TEXT NOT NULL,
    filed_date DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The one hot query: a security's holders across the window's quarters.
CREATE INDEX institutional_holdings_cusip_quarter_idx
    ON rev.institutional_holdings (cusip, quarter_end);

-- Which (cusip, quarter) slices are cached, so a repeat review skips the download
-- and a widened window ingests only the missing quarters.
CREATE TABLE rev.institutional_holdings_ingest (
    cusip TEXT NOT NULL CHECK (LENGTH(cusip) = 9),
    quarter_end DATE NOT NULL,
    status TEXT NOT NULL DEFAULT 'ok',
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (cusip, quarter_end)
);

-- The resolved common-stock CUSIP for a company (from its SC 13D/13G cover pages),
-- cached so repeat reviews skip the resolution fetch.
CREATE TABLE rev.security_cusip (
    cik BIGINT PRIMARY KEY,
    cusip TEXT NOT NULL CHECK (LENGTH(cusip) = 9),
    resolved_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
