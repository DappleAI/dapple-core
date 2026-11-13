WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],   -- guide_ids
        $2::UUID[],   -- object_path_ids
        $3::TEXT[],     -- ref_keys
        $4::TEXT[]      -- directive_types
    ) WITH ORDINALITY AS o(guide_id, object_path_id, ref_key, directive_type, ord)
)
INSERT INTO rev.guide_shared_refs (guide_id, object_path_id, ref_key, directive_type)
SELECT args.guide_id, args.object_path_id, args.ref_key, args.directive_type
FROM args
ORDER BY args.ord
RETURNING id, guide_id, object_path_id, ref_key, directive_type, created_at;
