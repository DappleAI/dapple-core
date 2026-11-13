WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::SMALLINT[]      ) WITH ORDINALITY AS o(
        entity_type_id,
        ord
    )
)
INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope)
SELECT
    args.entity_type_id,
    COALESCE(
        NULLIF(current_setting('dapple.namespace_company_id', true), ''),
        '019e132e-2610-7a4a-b64a-77a49e286ac0'
    )::UUID,
    COALESCE(
        NULLIF(current_setting('dapple.scope', true), ''),
        'public'
    )::TEXT
FROM args
RETURNING
    id,
    entity_type_id,
    created_at;
