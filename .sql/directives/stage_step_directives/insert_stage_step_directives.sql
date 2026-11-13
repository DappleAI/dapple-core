-- Attach directives to stage steps
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],  -- stage_step_ids
        $2::UUID[]   -- directive_versioned_object_ids
    ) WITH ORDINALITY AS t(step_id, directive_versioned_object_id, ord)
),
step_refs AS (
    SELECT a.ord, ss.id AS step_id
    FROM args a
    JOIN rev.stage_steps ss ON ss.id = a.step_id
),
directive_refs AS (
    SELECT a.ord, d.id AS directive_id
    FROM args a
    JOIN rev.versioned_objects vo ON vo.id = a.directive_versioned_object_id
    JOIN rev.directives d ON d.versioned_object_id = vo.id
),
new_attachments AS (
    INSERT INTO rev.stage_step_directives (stage_step_id, directive_id)
    SELECT str.step_id, dr.directive_id
    FROM step_refs str
    JOIN directive_refs dr ON str.ord = dr.ord
    ORDER BY str.ord
    RETURNING id, stage_step_id, directive_id, created_at
)
SELECT
    na.id,
    na.stage_step_id,
    na.directive_id,
    na.created_at
FROM new_attachments na
ORDER BY na.id;
