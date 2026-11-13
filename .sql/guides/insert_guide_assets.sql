-- Insert guide asset associations.
-- Links guide versions to assets with role classification and original relative path.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],     -- guide_id[]
        $2::UUID[],     -- asset_id[]
        $3::TEXT[],      -- role[]
        $4::TEXT[]       -- relative_path[]
    ) WITH ORDINALITY AS o(guide_id, asset_id, role, relative_path, ord)
),
objs AS (
    INSERT INTO rev.guide_assets (guide_id, asset_id, role, relative_path)
    SELECT args.guide_id, args.asset_id, args.role, args.relative_path
    FROM args
    ORDER BY args.ord
    RETURNING id, guide_id, asset_id, role, relative_path, created_at
)
SELECT * FROM objs ORDER BY objs.id;
