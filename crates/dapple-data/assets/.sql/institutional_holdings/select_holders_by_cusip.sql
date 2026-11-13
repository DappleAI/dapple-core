SELECT cusip, quarter_end, manager_cik, manager_name, shares, value_usd, accession, filed_date
FROM rev.institutional_holdings
WHERE cusip = $1 AND quarter_end >= $2 AND quarter_end <= $3
ORDER BY quarter_end, value_usd DESC;
