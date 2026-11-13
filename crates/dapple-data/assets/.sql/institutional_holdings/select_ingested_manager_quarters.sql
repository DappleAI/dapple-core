SELECT quarter_end
FROM rev.institutional_holdings_manager_ingest
WHERE manager_cik = $1
  AND status = 'ok'
ORDER BY quarter_end;
