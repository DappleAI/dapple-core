-- Migration 94: manager-scoped 13F ingest marker
--
-- Migration 88 built rev.institutional_holdings as a per-CUSIP cache. Its marker
-- table rev.institutional_holdings_ingest means "every manager holding this CUSIP
-- in this quarter is stored", and `edgar holders` TRUSTS that marker to skip the
-- 99.4 MB DERA download for quarters it believes complete.
--
-- `edgar manager --persist` fills the same rows table from the opposite
-- direction: one manager, every security it held, read off the Archives route at
-- ~50 KB/quarter with amendments resolved. That is a different completeness
-- claim about the same rows, so it needs its own marker.
--
-- THE RULE: a manager-first write MUST NEVER touch the CUSIP marker. Marking a
-- CUSIP from a manager-first ingest would make `edgar holders <cusip>` skip the
-- download and return the one manager we happened to store, presenting it as the
-- whole holder base — every row genuine, the coverage claim false.
--
-- The two writers agree on what a row means: only plain long share positions are
-- stored (sshPrnamtType = SH, no putCall), matching holdings_for_cusip's filter.

CREATE TABLE rev.institutional_holdings_manager_ingest (
    manager_cik BIGINT NOT NULL,
    quarter_end DATE NOT NULL,
    -- The accession the stored rows came from, so a re-persist can tell a
    -- refetch of the same filing from a genuinely new one (an amendment lands
    -- later and changes the quarter without changing its period).
    accession TEXT,
    status TEXT NOT NULL DEFAULT 'ok',
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (manager_cik, quarter_end)
);

-- The manager-first delete scope. Without this the replace scans the whole cache.
CREATE INDEX institutional_holdings_manager_quarter_idx
    ON rev.institutional_holdings (manager_cik, quarter_end);
