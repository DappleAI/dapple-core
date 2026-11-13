args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
inference_profile_page AS (
    SELECT ip.id, args.ord
    FROM args
    JOIN rev.inference_profiles ip ON ip.id = args.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)
