WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],  -- hat_id[]
        $2::UUID[]   -- category_id[]
    ) WITH ORDINALITY AS o(hat_id, category_id, ord)
),
returned AS (
    INSERT INTO rev.hat_categories (hat_id, category_id)
    SELECT a.hat_id, a.category_id
    FROM args a
    JOIN rev.categories t ON a.category_id = t.id
    ORDER BY a.ord
    RETURNING *
)
SELECT
    returned.id,
    returned.hat_id,
    returned.category_id,
    returned.created_at
FROM returned
JOIN args ON returned.hat_id = args.hat_id AND returned.category_id = args.category_id
ORDER BY returned.id;
