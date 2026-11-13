args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
category_page AS (
    SELECT c.id, args.ord
    FROM args
    JOIN rev.categories c ON c.id = args.id
    ORDER BY args.ord
    LIMIT $1::BIGINT OFFSET $3::BIGINT
)
