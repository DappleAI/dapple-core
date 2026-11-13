WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(object_id, ord)
)
SELECT
    objs.object_id AS id,
    u.email::TEXT AS email,
    objs.created_at
FROM rev.user_details objs
     JOIN auth.users u ON objs.user_id = u.id
JOIN args a ON a.object_id = objs.object_id
WHERE u.email IS NOT NULL
ORDER BY objs.object_id
LIMIT $2 OFFSET $3;
