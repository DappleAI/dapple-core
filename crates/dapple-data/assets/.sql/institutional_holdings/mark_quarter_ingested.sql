INSERT INTO rev.institutional_holdings_ingest (cusip, quarter_end)
VALUES ($1, $2)
ON CONFLICT (cusip, quarter_end) DO UPDATE SET ingested_at = now();
