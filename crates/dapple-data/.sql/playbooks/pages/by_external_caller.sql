-- External-API playbook page CTE: list playbooks scoped to the caller's
-- company namespace.
--
-- The customer's surface filters by `namespace_company_id = caller's UUID`
-- at the object_paths layer. Non-deprecated only — playbooks whose latest
-- versioned_object carries a non-NULL deprecation_levels row are skipped.
playbook_latest AS (
    SELECT DISTINCT ON (op.id)
        p.id
    FROM rev.playbooks p
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
    LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
    WHERE dl.id IS NULL
      AND op.namespace_company_id = $1::UUID
    ORDER BY op.id, vo.created_at DESC
),
playbook_page AS (
    SELECT id FROM playbook_latest
    ORDER BY id
    LIMIT $2::BIGINT OFFSET $3::BIGINT
)
