-- Insert briefs using the unified versioning infrastructure
-- Creates: object_path → brief_path_details → versioned_object → brief
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::TEXT[],
        $4::TEXT[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description,
        name,
        title,
        ord
    )
),
-- Get the entity type ID for briefs
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'brief'
),
-- Migration 33 made (namespace_company_id, scope) NOT NULL on object_paths.
-- This shared CTE is reused by every versioned-entity INSERT (playbooks,
-- stages, steps, tasks, briefs, etc.). Internal ingest writes paths as
-- dapple-namespace + 'public' scope; external ingest writes paths as
-- caller-namespace + 'external' scope.
--
-- Rather than thread these as bind parameters through every entity's
-- insert path, the values are read from per-transaction Postgres session
-- variables (GUCs). Internal callers do NOT set them and inherit the
-- dapple/public defaults via COALESCE. External callers SET LOCAL
-- dapple.namespace_company_id + dapple.scope at txn start; every nested
-- INSERT in the same transaction picks them up automatically.
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
    RETURNING id
),
numbered_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS ord
    FROM new_paths np
),

-- Create path details (name only, no slug for briefs)
path_details AS (
    INSERT INTO rev.brief_path_details (path_id, name)
    SELECT
        np.id,
        args.name
    FROM args
    JOIN numbered_paths np ON np.ord = args.ord
    RETURNING path_id, name
),
numbered_path_details AS (
    SELECT pd.*, row_number() OVER (ORDER BY pd.path_id) AS ord
    FROM path_details pd
),

new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT np.id, args.content_hash
    FROM numbered_paths np
    JOIN args ON args.ord = np.ord
    ORDER BY np.id
    RETURNING id, path_id, content_hash, created_at
),
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS ord
    FROM new_versions nv
),

-- Create briefs
new_briefs AS (
    INSERT INTO rev.briefs (versioned_object_id, title, description)
    SELECT
        nuv.id,
        args.title,
        args.description
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
    ORDER BY args.ord
    RETURNING id, versioned_object_id, title, description, created_at
),
-- Number briefs to preserve ordinality
numbered_briefs AS (
    SELECT nb.*, row_number() OVER (ORDER BY nb.id) AS ord
    FROM new_briefs nb
)
SELECT
    nub.id,
    nub.versioned_object_id,
    nup.id AS path_id,
    npd.name,
    nuv.content_hash,
    nub.title,
    nub.description,
    NULL::UUID AS old_brief_id,
    nub.created_at
FROM numbered_briefs nub
JOIN numbered_versions nuv ON nuv.ord = nub.ord
JOIN numbered_paths nup ON nup.ord = nub.ord
JOIN numbered_path_details npd ON npd.ord = nub.ord
ORDER BY nub.id;
