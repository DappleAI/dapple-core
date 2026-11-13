
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::TEXT[],
        $3::JSONB[],
        $4::JSONB[],
        $5::JSONB[],
        $6::INT[]
    ) WITH ORDINALITY AS o(brief_id, unit_key, layout, requires, extensions, authored_ord, ord)
),
objs AS (
    INSERT INTO rev.brief_units (
        brief_id,
        unit_key,
        layout,
        requires,
        extensions,
        ord,
        created_at
    )
    SELECT
        args.brief_id,
        args.unit_key,
        COALESCE(args.layout, '{}'::jsonb),
        COALESCE(args.requires, '{}'::jsonb),
        COALESCE(args.extensions, '{}'::jsonb),
        COALESCE(args.authored_ord, 0),
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
