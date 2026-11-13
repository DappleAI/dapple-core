SELECT
    vo.id AS versioned_object_id,
    g.id AS guide_id,
    vo.content_hash
FROM rev.versioned_objects vo
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.guides g ON g.versioned_object_id = vo.id
WHERE op.id = $1
  AND vo.content_hash = $2
LIMIT 1;
