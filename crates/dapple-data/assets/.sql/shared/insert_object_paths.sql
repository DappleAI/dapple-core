new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope)
    SELECT
        (SELECT id FROM entity_type),
        COALESCE(
            NULLIF(current_setting('dapple.namespace_company_id', true), ''),
            '019e132e-2610-7a4a-b64a-77a49e286ac0'
        )::UUID,
        COALESCE(
            NULLIF(current_setting('dapple.scope', true), ''),
            'public'
        )::TEXT
    FROM args
    ORDER BY args.ord
                    RETURNING id, namespace_company_id
),
numbered_paths AS (
    SELECT np.id, np.namespace_company_id, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
