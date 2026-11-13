WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    objs.id,
    objs.playbook_id,
    objs.user_id,
    objs.name,
    objs.created_at
FROM rev.journeys objs
JOIN args a ON a.id = objs.id
ORDER BY objs.id
LIMIT $2 OFFSET $3;
