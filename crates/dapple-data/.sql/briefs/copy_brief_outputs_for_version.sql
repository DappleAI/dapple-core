-- Copy brief outputs from old brief to new brief (for PATCH carry-forward)
-- Parameters:
--   $1: old_brief_id (UUID)
--   $2: new_brief_id (UUID)

WITH copy_outputs AS (
    INSERT INTO rev.brief_outputs (brief_id, value_slot_id, unit_key, source_kind_id, source_path, transform)
    SELECT $2, bo.value_slot_id, bo.unit_key, bo.source_kind_id, bo.source_path, bo.transform
    FROM rev.brief_outputs bo
    WHERE bo.brief_id = $1
    RETURNING id
)
SELECT count(*) AS copied_outputs FROM copy_outputs;
