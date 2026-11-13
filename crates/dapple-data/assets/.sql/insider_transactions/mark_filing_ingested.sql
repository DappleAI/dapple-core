INSERT INTO rev.insider_transactions_ingest (issuer_cik, accession)
VALUES ($1, $2)
ON CONFLICT (issuer_cik, accession) DO UPDATE SET ingested_at = now();
