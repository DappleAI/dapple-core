-- Insert brief units (parent only, subtypes inserted separately)
-- Parameters:
--   $1: brief_ids (UUID[])
--   $2: unit_keys (TEXT[])
--   $3: layouts (JSONB[])
--   $4: requires (JSONB[])
--   $5: extensions (JSONB[])

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::TEXT[],
        $3::JSONB[],
        $4::JSONB[],
        $5::JSONB[]
    ) WITH ORDINALITY AS o(brief_id, unit_key, layout, requires, extensions, ord)
),
objs AS (
    INSERT INTO rev.brief_units (
        brief_id,
        unit_key,
        layout,
        requires,
        extensions,
        created_at
    )
    SELECT
        args.brief_id,
        args.unit_key,
        COALESCE(args.layout, '{}'::jsonb),
        COALESCE(args.requires, '{}'::jsonb),
        COALESCE(args.extensions, '{}'::jsonb),
        now()
    FROM args
    ORDER BY args.ord
    RETURNING id, brief_id, unit_key, layout, requires, extensions, created_at
)
SELECT
    objs.id,
    objs.brief_id,
    objs.unit_key,
    objs.layout,
    objs.requires,
    objs.extensions,
    objs.created_at
FROM objs
ORDER BY objs.id;
