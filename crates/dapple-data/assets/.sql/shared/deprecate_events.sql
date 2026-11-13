events AS (
    INSERT INTO rev.deprecation_events (
        versioned_object_id, from_level_id, to_level_id, reason, deprecated_by_user_id
    )
    SELECT
        resolved.versioned_object_id,
        resolved.from_level_id,
        $2::SMALLINT,
        resolved.reason,
        $1::UUID
    FROM resolved
    RETURNING *
)
