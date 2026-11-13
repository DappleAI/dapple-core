SELECT
    st.id,
    st.service_id,
    st.name,
    st.description,
    st.public_key_pem,
    st.key_algorithm,
    st.created_at,
    ud.object_id AS created_by_user_id,
        (
        SELECT MIN(r.revoked_at)
        FROM web.service_token_revocations r
        WHERE r.service_token_id = st.id
    ) AS revoked_at
FROM web.service_tokens st
LEFT JOIN rev.user_details ud ON ud.user_id = st.created_by_user_id
WHERE st.id = $1;
