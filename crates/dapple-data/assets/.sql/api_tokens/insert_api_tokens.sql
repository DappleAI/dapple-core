WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],                 $2::TEXT[],                 $3::BYTEA[],                $4::TEXT[],                 $5::TIMESTAMPTZ[]       ) WITH ORDINALITY AS o(
        user_object_id,
        name,
        token_hash,
        token_prefix,
        expires_at,
        ord
    )
),
user_lookup AS (
    SELECT ud.user_id, ud.object_id, args.ord
    FROM args
    JOIN rev.user_details ud ON ud.object_id = args.user_object_id
),
objs AS (
    INSERT INTO web.api_tokens (user_id, name, token_hash, token_prefix, expires_at)
    SELECT
        ul.user_id,
        args.name,
        args.token_hash,
        args.token_prefix,
        args.expires_at
    FROM args
    JOIN user_lookup ul ON ul.ord = args.ord
    ORDER BY args.ord
    RETURNING *
)
SELECT
    objs.id,
    ud.object_id AS user_id,
    objs.name,
    objs.token_hash,
    objs.token_prefix,
    objs.expires_at,
    objs.created_at
FROM objs
JOIN rev.user_details ud ON ud.user_id = objs.user_id
ORDER BY objs.id;
