new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT (SELECT id FROM entity_type)
    FROM args
    ORDER BY args.ord
    RETURNING id
),
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),
