WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],  -- playbook_id[]
        $2::UUID[]   -- category_id[]
    ) WITH ORDINALITY AS o(playbook_id, category_id, ord)
),
returned AS (
    INSERT INTO rev.playbook_categories (playbook_id, category_id)
    SELECT a.playbook_id, a.category_id
    FROM args a
    JOIN rev.categories t ON a.category_id = t.id
    ORDER BY a.ord
    RETURNING *
)
SELECT
    returned.id,
    returned.playbook_id,
    returned.category_id,
    returned.created_at
FROM returned
JOIN args ON returned.playbook_id = args.playbook_id AND returned.category_id = args.category_id
ORDER BY returned.id;
