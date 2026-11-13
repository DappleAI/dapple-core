-- External-API value_slot listing: scoped to the caller's company namespace.
-- Used by GET /api/value-slots on external-api.dapple.x.
-- Shared body template for value_slot SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `value_slot_page` CTE with at minimum `value_slot_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

-- External-API value_slot page CTE: list value_slots scoped to the caller's
-- company namespace. Non-deprecated only.
value_slot_latest AS (
    SELECT DISTINCT ON (op.id)
        vs.id
    FROM rev.value_slots vs
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
value_slot_page AS (
    SELECT id FROM value_slot_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)

SELECT
    vs.id,
    vs.versioned_object_id,
    op.id AS path_id,
    vspd.name,
    vk.key as value_kind_key,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    vs.title,
    vs.description,
    vs.constraints,
    vs.created_at
FROM value_slot_page vskp
JOIN rev.value_slots vs ON vs.id = vskp.id
JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.value_kinds vk ON vk.id = vspd.value_kind_id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
ORDER BY vs.id;

