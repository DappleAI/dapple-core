WITH by_object_id AS (
    SELECT
        ud.user_id,
        ud.object_id,
        ud.created_at,
        u.email
    FROM rev.user_details ud
    JOIN auth.users u ON ud.user_id = u.id
    WHERE ud.object_id = $2 AND $2 IS NOT NULL
),
by_auth_id AS (
    SELECT
        ud.user_id,
        ud.object_id,
        ud.created_at,
        u.email
    FROM rev.user_details ud
    JOIN auth.users u ON ud.user_id = u.id
    WHERE ud.user_id = $1 AND $1 IS NOT NULL
),
auth_user AS (
    SELECT id, email FROM auth.users WHERE id = $1 AND $1 IS NOT NULL
),
inserted AS (
        INSERT INTO rev.user_details (user_id)
    SELECT $1
    FROM auth_user
    WHERE $1 IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM by_auth_id)
    ON CONFLICT (user_id) DO NOTHING
    RETURNING user_id, object_id, created_at
)
SELECT
    COALESCE(by_object_id.user_id, by_auth_id.user_id, inserted.user_id) AS user_id,
    COALESCE(by_object_id.object_id, by_auth_id.object_id, inserted.object_id) AS id,
    COALESCE(by_object_id.email, by_auth_id.email, auth_user.email)::TEXT AS email,
    COALESCE(by_object_id.created_at, by_auth_id.created_at, inserted.created_at) AS created_at
FROM (SELECT 1) AS dummy
LEFT JOIN by_object_id ON true
LEFT JOIN by_auth_id ON true
LEFT JOIN auth_user ON true
LEFT JOIN inserted ON true
WHERE by_object_id.user_id IS NOT NULL
   OR by_auth_id.user_id IS NOT NULL
   OR inserted.user_id IS NOT NULL;
