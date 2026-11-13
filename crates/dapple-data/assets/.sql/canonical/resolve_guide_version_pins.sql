WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],            $2::TEXT[]         ) WITH ORDINALITY AS o(ref_key, content_hash, ord)
)
SELECT
    args.ref_key AS "ref_key!",
    args.content_hash AS "content_hash!",
    vo.id AS "versioned_object_id?",
    vo.content_hash AS "resolved_content_hash?",
    args.ord AS "ord!"
FROM args
LEFT JOIN rev.guides g ON g.source_path = args.ref_key
LEFT JOIN rev.versioned_objects vo
    ON vo.id = g.versioned_object_id
   AND vo.content_hash LIKE (args.content_hash || '%')
ORDER BY args.ord;
