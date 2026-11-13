-- Select value slot paths by IDs using unified versioning
-- Parameters:
--   $1: ids (UUID[])

WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    op.id,
    vspd.name,
    vk.key as value_kind_key,
    op.created_at
FROM args
JOIN rev.object_paths op ON op.id = args.id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vspd.value_kind_id
ORDER BY op.id;
