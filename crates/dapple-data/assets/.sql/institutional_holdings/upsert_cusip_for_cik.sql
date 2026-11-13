INSERT INTO rev.security_cusip (cik, cusip)
VALUES ($1, $2)
ON CONFLICT (cik) DO UPDATE SET cusip = EXCLUDED.cusip, resolved_at = now();
