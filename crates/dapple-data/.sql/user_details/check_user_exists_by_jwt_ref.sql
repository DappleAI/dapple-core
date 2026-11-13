-- Check if a user exists by JWTUserIdRef (no auto-creation)
-- $1 = auth_user_id (nullable i64) - if provided, check by auth.users.id
-- $2 = object_id (nullable UUID) - if provided, check by object_id
-- Returns user_id and object_id if found, empty result if not found
SELECT
    ud.user_id,
    ud.object_id AS id
FROM rev.user_details ud
WHERE
    (ud.object_id = $2 AND $2 IS NOT NULL)
    OR (ud.user_id = $1 AND $1 IS NOT NULL);
