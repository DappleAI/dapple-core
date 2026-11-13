-- Insert object paths with entity type IDs
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::SMALLINT[]  -- entity_type_ids
    ) WITH ORDINALITY AS o(
        entity_type_id,
        ord
    )
)
INSERT INTO rev.object_paths (entity_type_id)
SELECT args.entity_type_id
FROM args
RETURNING
    id,
    entity_type_id,
    created_at;
