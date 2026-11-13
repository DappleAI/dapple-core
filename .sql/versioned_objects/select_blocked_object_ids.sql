-- Select IDs that are blocked (retired or quarantined).
-- Returns the id and deprecation level for objects that cannot be referenced.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),
args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
SELECT
    vo.id,
    dl.key as deprecation_level_key
FROM args
JOIN rev.versioned_objects vo ON vo.id = args.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
WHERE dl.key IN ('retired', 'quarantined')
ORDER BY vo.id;
