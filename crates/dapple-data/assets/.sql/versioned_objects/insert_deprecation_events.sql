WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],             $2::SMALLINT[],          $3::TEXT[],              $4::UUID[]           ) WITH ORDINALITY AS o(
        versioned_object_id,
        to_level_id,
        reason,
        deprecated_by_user_id,
        ord
    )
),
current_levels AS (
    SELECT DISTINCT ON (args.versioned_object_id)
        args.versioned_object_id,
        de.to_level_id AS from_level_id
    FROM args
    LEFT JOIN rev.deprecation_events de ON de.versioned_object_id = args.versioned_object_id
    ORDER BY args.versioned_object_id, de.created_at DESC
),
objs AS (
    INSERT INTO rev.deprecation_events (
        versioned_object_id,
        from_level_id,
        to_level_id,
        reason,
        deprecated_by_user_id
    )
    SELECT
        args.versioned_object_id,
        cl.from_level_id,
        args.to_level_id,
        args.reason,
        args.deprecated_by_user_id
    FROM args
    LEFT JOIN current_levels cl ON cl.versioned_object_id = args.versioned_object_id
    RETURNING *
)
SELECT
    objs.id,
    objs.versioned_object_id,
    objs.from_level_id,
    objs.to_level_id,
    objs.reason,
    objs.deprecated_by_user_id,
    objs.created_at
FROM objs
ORDER BY objs.id;
