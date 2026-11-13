-- Create new versions of existing directives (PATCH)
-- Takes existing directive ids and creates new versions with updated fields
-- Caller supplies content_hash for idempotent upsert.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::UUID[],
        $4::JSONB[],
        $5::INT[],
        $6::INT[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description,
        id,
        logic,
        max_attempts,
        retry_delay_ms,
        ord
    )
),
-- Look up existing directive info
existing AS (
    SELECT
        args.ord,
        args.content_hash,
        vo.path_id,
        dpd.name
    FROM args
    JOIN rev.directives d_old ON d_old.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = d_old.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.directive_path_details dpd ON dpd.path_id = op.id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        e.path_id,
        e.content_hash
    FROM existing e
    ORDER BY e.ord
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Create new directives
new_directives AS (
    INSERT INTO rev.directives (versioned_object_id, logic, max_attempts, retry_delay_ms, description)
    SELECT
        nv.id,
        args.logic,
        COALESCE(args.max_attempts, 1),
        args.retry_delay_ms,
        args.description
    FROM args
    JOIN existing e ON e.ord = args.ord
    JOIN new_versions nv ON nv.path_id = e.path_id
    RETURNING id, versioned_object_id, logic, max_attempts, retry_delay_ms, description, created_at
)
SELECT
    nd.id,
    nd.versioned_object_id,
    e.path_id,
    e.name,
    nv.content_hash,
    nd.logic,
    nd.max_attempts,
    nd.retry_delay_ms,
    nd.description,
    nd.created_at
FROM new_directives nd
JOIN new_versions nv ON nv.id = nd.versioned_object_id
JOIN existing e ON e.path_id = nv.path_id
ORDER BY nv.id;
