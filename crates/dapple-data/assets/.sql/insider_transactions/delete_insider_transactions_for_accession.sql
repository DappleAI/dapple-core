DELETE FROM rev.insider_transactions
WHERE issuer_cik = $1 AND accession = $2;
