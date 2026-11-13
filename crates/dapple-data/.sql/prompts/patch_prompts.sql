-- Patch prompts using unified versioning (create new version with caller-supplied content_hash)
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::UUID[]
    ) WITH ORDINALITY AS o(content_hash, description, id, ord)
),
existing_prompts AS (
    SELECT
        vo.path_id,
        args.id as old_id,
        args.description,
        args.content_hash,
        args.ord
    FROM args
    JOIN rev.prompts p ON p.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
),
-- Create new versioned objects with caller-supplied content_hash
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT
        ep.path_id,
        ep.content_hash
    FROM existing_prompts ep
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
-- Include already-existing versioned objects (conflict hit — content reverted)
all_versions AS (
    SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at
    FROM new_versions_inserted nvi
    UNION ALL
    SELECT vo.id, vo.path_id, vo.content_hash, vo.created_at
    FROM existing_prompts ep
    JOIN rev.versioned_objects vo ON vo.path_id = ep.path_id AND vo.content_hash = ep.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = ep.path_id AND nvi.content_hash = ep.content_hash
    )
),
-- Create new prompts (skip if entity row already exists for this versioned_object)
new_prompts AS (
    INSERT INTO rev.prompts (versioned_object_id, description)
    SELECT
        av.id,
        ep.description
    FROM existing_prompts ep
    JOIN all_versions av ON av.path_id = ep.path_id
    WHERE NOT EXISTS (SELECT 1 FROM rev.prompts p WHERE p.versioned_object_id = av.id)
    RETURNING id, versioned_object_id, description, created_at
),
-- Include already-existing prompts (for reverted versions)
all_prompts AS (
    SELECT np.id, np.versioned_object_id, np.description, np.created_at
    FROM new_prompts np
    UNION ALL
    SELECT p.id, p.versioned_object_id, p.description, p.created_at
    FROM rev.prompts p
    JOIN all_versions av ON p.versioned_object_id = av.id
    WHERE NOT EXISTS (SELECT 1 FROM new_prompts np WHERE np.versioned_object_id = av.id)
)
SELECT
    ap.id AS "id!",
    ap.versioned_object_id AS "versioned_object_id!",
    op.id AS "path_id!",
    ppd.name AS "name!",
    av.content_hash AS "content_hash!",
    ap.description,
    ap.created_at AS "created_at!"
FROM all_prompts ap
JOIN all_versions av ON av.id = ap.versioned_object_id
JOIN rev.object_paths op ON op.id = av.path_id
JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
ORDER BY ap.id;
