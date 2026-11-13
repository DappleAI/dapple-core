SELECT accession
FROM rev.insider_transactions_ingest
WHERE issuer_cik = $1;
