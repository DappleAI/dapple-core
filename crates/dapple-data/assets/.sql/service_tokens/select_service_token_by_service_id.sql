SELECT
    st.id,
    st.service_id,
    st.public_key_pem,
    st.key_algorithm,
        (
        SELECT MIN(r.revoked_at)
        FROM web.service_token_revocations r
        WHERE r.service_token_id = st.id
    ) AS revoked_at
FROM web.service_tokens st
WHERE st.service_id = $1;
