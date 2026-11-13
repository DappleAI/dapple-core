
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::JSONB[]
    ) WITH ORDINALITY AS o(brief_pack_id, brief_group_id, ui_overrides, ord)
)
INSERT INTO rev.brief_pack_groups (brief_pack_id, brief_group_id, ui_overrides, created_at)
SELECT args.brief_pack_id, args.brief_group_id, COALESCE(args.ui_overrides, '{}'::jsonb), now()
FROM args
ORDER BY args.ord
RETURNING id, brief_pack_id, brief_group_id, ui_overrides, created_at;
