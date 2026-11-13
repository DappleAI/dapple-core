args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
guide_page AS (
    SELECT g.id
    FROM args
    JOIN rev.guides g ON g.id = args.id
    ORDER BY g.id
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)
