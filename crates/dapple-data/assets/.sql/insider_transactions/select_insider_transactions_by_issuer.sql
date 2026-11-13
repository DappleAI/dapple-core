SELECT issuer_cik, accession, filed_date, trans_date, owner_cik, owner_name,
       is_director, is_officer, is_ten_pct, officer_title, security_title,
       trans_code, acquired_disposed, shares, price_per_share, shares_owned_after,
       is_derivative, is_10b5_1
FROM rev.insider_transactions
WHERE issuer_cik = $1 AND trans_date >= $2 AND trans_date <= $3
ORDER BY trans_date, owner_name, accession;
