args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
step_page AS (
    SELECT s.id
    FROM args
    JOIN rev.steps s ON s.id = args.id
    ORDER BY s.id
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)
