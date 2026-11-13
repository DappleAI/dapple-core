WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],             $2::UUID[],             $3::TEXT[],              $4::TEXT[]           ) WITH ORDINALITY AS o(guide_id, asset_id, role, relative_path, ord)
),
objs AS (
    INSERT INTO rev.guide_assets (guide_id, asset_id, role, relative_path)
    SELECT args.guide_id, args.asset_id, args.role, args.relative_path
    FROM args
    ORDER BY args.ord
    RETURNING id, guide_id, asset_id, role, relative_path, created_at
)
SELECT * FROM objs ORDER BY objs.id;
