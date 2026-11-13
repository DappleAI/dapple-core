args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
hat_page AS (
    SELECT h.id
    FROM args
    JOIN rev.hats h ON h.id = args.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)
