WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY
    AS o(
        stage_id,
        step_id,
        ord
    )
),
-- Resolve IDs with ordinality preserved
resolved AS (
    SELECT
        args.ord,
        st.id AS stage_id,
        s.id AS step_id
    FROM args
    JOIN rev.stages st ON args.stage_id = st.id
    JOIN rev.steps s ON args.step_id = s.id
),
objs AS (
    INSERT INTO rev.stage_steps (stage_id, step_id)
    SELECT r.stage_id, r.step_id
    FROM resolved r
    ORDER BY r.ord
    ON CONFLICT (stage_id, step_id) DO NOTHING
    RETURNING *
),
-- Return inserted rows (conflicts silently skipped).
-- Also include pre-existing rows for the same (stage_id, step_id) pairs
-- so callers always get a complete result set.
all_rows AS (
    SELECT ss.id, ss.stage_id, ss.step_id, ss.created_at
    FROM rev.stage_steps ss
    JOIN resolved r ON ss.stage_id = r.stage_id AND ss.step_id = r.step_id
)
SELECT
    ar.id,
    ar.stage_id,
    ar.step_id,
    ar.created_at
FROM all_rows ar
ORDER BY ar.id;