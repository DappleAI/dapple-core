INSERT INTO rev.institutional_holdings
    (cusip, quarter_end, manager_cik, manager_name, shares, value_usd, accession, filed_date)
SELECT * FROM UNNEST(
    $1::TEXT[],
    $2::DATE[],
    $3::BIGINT[],
    $4::TEXT[],
    $5::DOUBLE PRECISION[],
    $6::DOUBLE PRECISION[],
    $7::TEXT[],
    $8::DATE[]
);
