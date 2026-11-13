INSERT INTO rev.insider_transactions
    (issuer_cik, accession, filed_date, trans_date, owner_cik, owner_name,
     is_director, is_officer, is_ten_pct, officer_title, security_title,
     trans_code, acquired_disposed, shares, price_per_share, shares_owned_after,
     is_derivative, is_10b5_1)
SELECT * FROM UNNEST(
    $1::BIGINT[],
    $2::TEXT[],
    $3::DATE[],
    $4::DATE[],
    $5::BIGINT[],
    $6::TEXT[],
    $7::BOOLEAN[],
    $8::BOOLEAN[],
    $9::BOOLEAN[],
    $10::TEXT[],
    $11::TEXT[],
    $12::TEXT[],
    $13::TEXT[],
    $14::DOUBLE PRECISION[],
    $15::DOUBLE PRECISION[],
    $16::DOUBLE PRECISION[],
    $17::BOOLEAN[],
    $18::BOOLEAN[]
);
