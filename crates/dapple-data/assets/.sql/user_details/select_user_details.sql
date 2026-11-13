SELECT
    objs.object_id AS id,
    u.email::TEXT AS email,
    objs.created_at
FROM rev.user_details objs
JOIN auth.users u ON objs.user_id = u.id
WHERE u.email IS NOT NULL
LIMIT $1 OFFSET $2;
