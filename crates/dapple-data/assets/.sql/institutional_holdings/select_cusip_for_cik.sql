SELECT cusip
FROM rev.security_cusip
WHERE cik = $1;
