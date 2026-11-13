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
resolved AS (
    SELECT
        args.ord,
        ROW_NUMBER() OVER (PARTITION BY st.id ORDER BY args.ord) AS pos,
        st.id AS stage_id,
        s.id AS step_id
    FROM args
    JOIN rev.stages st ON args.stage_id = st.id
    JOIN rev.steps s ON args.step_id = s.id
),
objs AS (
    INSERT INTO rev.stage_steps (stage_id, step_id, ord)
    SELECT r.stage_id, r.step_id, r.pos
    FROM resolved r
    ORDER BY r.ord
    ON CONFLICT (stage_id, step_id) DO NOTHING
    RETURNING id, stage_id, step_id, ord, created_at
),
all_rows AS (
    SELECT id, stage_id, step_id, ord, created_at
    FROM objs
    UNION ALL
    SELECT ss.id, ss.stage_id, ss.step_id, ss.ord, ss.created_at
    FROM resolved r
    JOIN rev.stage_steps ss ON ss.stage_id = r.stage_id AND ss.step_id = r.step_id
    WHERE NOT EXISTS (
        SELECT 1 FROM objs o
        WHERE o.stage_id = r.stage_id AND o.step_id = r.step_id
    )
)
SELECT
    ar.id AS "id!",
    ar.stage_id AS "stage_id!",
    ar.step_id AS "step_id!",
    ar.created_at AS "created_at!"
FROM all_rows ar
ORDER BY ar.stage_id, ar.ord;