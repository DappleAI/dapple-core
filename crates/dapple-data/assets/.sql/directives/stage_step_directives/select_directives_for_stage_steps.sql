WITH step_ids AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS t(step_id, ord)
)
SELECT
    ssd.id,
    ss.id AS stage_step_id,
    ssd.directive_id,
    op.id AS directive_path_id,
    dpd.name AS directive_name,
    vo.content_hash,
    d.logic,
    d.max_attempts,
    d.retry_delay_ms,
    d.description,
    ssd.created_at
FROM step_ids sti
JOIN rev.stage_steps ss ON ss.id = sti.step_id
LEFT JOIN rev.stage_step_directives ssd ON ssd.stage_step_id = ss.id
LEFT JOIN rev.directives d ON d.id = ssd.directive_id
LEFT JOIN rev.versioned_objects vo ON vo.id = d.versioned_object_id
LEFT JOIN rev.object_paths op ON op.id = vo.path_id
LEFT JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
ORDER BY ss.id, ssd.id;
