WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],                 $2::TEXT[],                 $3::TEXT[],                 $4::TEXT[],                 $5::TEXT[],                 $6::UUID[]              ) WITH ORDINALITY AS o(
        service_id,
        name,
        description,
        public_key_pem,
        key_algorithm,
        created_by_user_object_id,
        ord
    )
),
objs AS (
    INSERT INTO web.service_tokens (
        service_id,
        name,
        description,
        public_key_pem,
        key_algorithm,
        created_by_user_id
    )
    SELECT
        args.service_id,
        args.name,
        args.description,
        args.public_key_pem,
        args.key_algorithm,
        ud.user_id
    FROM args
    LEFT JOIN rev.user_details ud ON ud.object_id = args.created_by_user_object_id
    ORDER BY args.ord
    RETURNING *
)
SELECT
    objs.id,
    objs.service_id,
    objs.name,
    objs.created_at
FROM objs
ORDER BY objs.id;
