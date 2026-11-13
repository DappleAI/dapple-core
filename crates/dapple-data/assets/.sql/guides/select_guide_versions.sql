SELECT
    gpd.slug,
    gpd.title,
    g.source_path,
    op.id AS path_id,
    vo.id AS versioned_object_id,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    g.created_at
FROM rev.guides g
JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.guide_path_details gpd ON gpd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
WHERE ($1::TEXT IS NULL OR gpd.slug = $1)
  AND ($2::TEXT IS NULL OR g.source_path = $2)
ORDER BY gpd.slug, g.created_at DESC
LIMIT $3 OFFSET $4;
