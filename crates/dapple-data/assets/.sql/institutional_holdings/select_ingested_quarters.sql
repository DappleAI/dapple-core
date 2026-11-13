SELECT quarter_end
FROM rev.institutional_holdings_ingest
WHERE cusip = $1
ORDER BY quarter_end;
