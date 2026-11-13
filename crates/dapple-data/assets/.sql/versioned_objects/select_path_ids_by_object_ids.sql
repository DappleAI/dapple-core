WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(object_id, ord)
)
SELECT
    vo.id AS object_id,
    vo.path_id
FROM args
JOIN rev.versioned_objects vo ON vo.id = args.object_id
ORDER BY args.ord;
