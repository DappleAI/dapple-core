WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],             $2::TEXT[]           ) WITH ORDINALITY AS o(
        path_id,
        content_hash,
        ord
    )
),
objs AS (
    INSERT INTO rev.versioned_objects (
        path_id,
        content_hash
    )
    SELECT
        args.path_id,
        args.content_hash
    FROM args
    ORDER BY args.ord
    RETURNING *
),
numbered_objs AS (
    SELECT o.*, row_number() OVER (ORDER BY o.id) AS ord
    FROM objs o
)
SELECT
    no.id,
    no.path_id,
    no.content_hash,
    no.created_at
FROM numbered_objs no
JOIN rev.object_paths op ON op.id = no.path_id
ORDER BY no.id;
