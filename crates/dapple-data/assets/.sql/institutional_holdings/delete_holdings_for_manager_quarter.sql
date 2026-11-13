DELETE FROM rev.institutional_holdings
WHERE manager_cik = $1
  AND quarter_end = $2;
