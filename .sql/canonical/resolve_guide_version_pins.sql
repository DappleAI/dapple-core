-- Resolve (ref_key, content_hash) pairs to versioned_object_id via rev.guides + rev.versioned_objects.
-- Supports both full 40-char hashes (exact match) and short 8+ char hashes (prefix match).
-- Returns one row per input, with versioned_object_id NULL if not resolved.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],    -- ref_keys
        $2::TEXT[]     -- content_hashes (full or short prefix)
    ) WITH ORDINALITY AS o(ref_key, content_hash, ord)
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
