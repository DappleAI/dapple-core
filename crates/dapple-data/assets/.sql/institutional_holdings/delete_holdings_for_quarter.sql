DELETE FROM rev.institutional_holdings
WHERE cusip = $1 AND quarter_end = $2;
