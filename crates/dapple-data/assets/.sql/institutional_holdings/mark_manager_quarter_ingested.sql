INSERT INTO rev.institutional_holdings_manager_ingest (manager_cik, quarter_end, accession)
VALUES ($1, $2, $3)
ON CONFLICT (manager_cik, quarter_end)
DO UPDATE SET ingested_at = now(), accession = EXCLUDED.accession;
