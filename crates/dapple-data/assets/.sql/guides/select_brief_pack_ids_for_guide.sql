WITH candidates AS (
    SELECT bp.id AS brief_pack_id, vo.path_id, vo.created_at
    FROM rev.guide_brief_packs gbp
    JOIN rev.brief_packs bp ON bp.id = gbp.brief_pack_id
    JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
    LEFT JOIN LATERAL (
        SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

    ) ld ON TRUE
    WHERE gbp.guide_id = $1::UUID
      AND ld.to_level_id IS NULL
),
latest AS (
    SELECT DISTINCT ON (path_id) brief_pack_id
    FROM candidates
        ORDER BY path_id, created_at DESC, brief_pack_id DESC
)
SELECT brief_pack_id AS "brief_pack_id!"
FROM latest
ORDER BY brief_pack_id;
