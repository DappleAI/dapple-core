-- Select all prompts using unified versioning
--
-- LIMIT/OFFSET apply to distinct prompts (not flat rows) via a page CTE.
-- The flat join (prompt x prompt_fluent_messages x fluent_messages) produces many rows
-- per prompt, so limiting flat rows would silently truncate results.
-- Shared body template for prompt SELECT queries.
-- Callers provide a @page slot defining one or more CTEs
-- that produce a `prompt_page` CTE with at minimum `prompt_page.id`.
WITH latest_deprecation AS (
    SELECT DISTINCT ON (versioned_object_id)
        versioned_object_id,
        to_level_id
    FROM rev.deprecation_events
    ORDER BY versioned_object_id, created_at DESC
),

prompt_page AS (
    SELECT p.id
    FROM rev.prompts p
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
    ORDER BY p.id
    LIMIT $1::BIGINT OFFSET $2::BIGINT
)

SELECT
    p.id,
    p.versioned_object_id,
    op.id AS path_id,
    ppd.name,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    p.description,
    p.created_at,
    -- Fluent message association fields (nullable due to LEFT JOIN)
    pfm.id as prompt_fluent_message_id,
    pfm.fluent_message_id as fluent_message_id,
    pfm.role as fluent_message_role,
    -- Use pfm.id as implicit ordinal since table lacks ordinal column
    ROW_NUMBER() OVER (PARTITION BY p.id ORDER BY pfm.id) - 1 as fluent_message_ordinal
FROM prompt_page prp
JOIN rev.prompts p ON p.id = prp.id
JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
LEFT JOIN latest_deprecation ld ON ld.versioned_object_id = vo.id
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.prompt_fluent_messages pfm ON pfm.prompt_id = p.id
ORDER BY p.id, pfm.id;

