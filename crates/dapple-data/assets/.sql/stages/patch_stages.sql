WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::UUID[],
        $3::TEXT[],
                        $4::JSONB[],
        $5::TEXT[],
        $6::UUID[]
    ) WITH ORDINALITY AS o(
        content_hash,
        description_fluent_message_id,
        description,
        icons,
        kind,
        path_id,
        ord
    )
),
paths AS (
    SELECT
        op.id AS path_id,
        spd.name,
        spd.slug,
        args.ord
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.stage_path_details spd ON spd.path_id = op.id
),
new_versions_inserted AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT paths.path_id, args.content_hash
    FROM args
    JOIN paths ON paths.ord = args.ord
    ORDER BY paths.ord
    ON CONFLICT (path_id, content_hash) DO NOTHING
    RETURNING id, path_id, content_hash, created_at
),
new_versions AS (
            SELECT nvi.id, nvi.path_id, nvi.content_hash, nvi.created_at, p.ord
    FROM new_versions_inserted nvi
    JOIN paths p ON p.path_id = nvi.path_id
    UNION ALL
        SELECT vo.id, paths.path_id, vo.content_hash, vo.created_at, args.ord
    FROM args
    JOIN paths ON paths.ord = args.ord
    JOIN rev.versioned_objects vo ON vo.path_id = paths.path_id AND vo.content_hash = args.content_hash
    WHERE NOT EXISTS (
        SELECT 1 FROM new_versions_inserted nvi
        WHERE nvi.path_id = paths.path_id AND nvi.content_hash = args.content_hash
    )
),
numbered_versions AS (
    SELECT nv.id, nv.path_id, nv.content_hash, nv.created_at, nv.ord
    FROM new_versions nv
),

new_stages AS (
    INSERT INTO rev.stages (versioned_object_id, description, description_fluent_message_id, kind_id, icons)
    SELECT
        nuv.id,
        args.description,
        args.description_fluent_message_id,
        sk.id,
                        NULLIF(args.icons, '[]'::JSONB)
    FROM args
    JOIN numbered_versions nuv ON nuv.ord = args.ord
                    LEFT JOIN rev.stage_kinds sk ON sk.key = args.kind
    WHERE NOT EXISTS (SELECT 1 FROM rev.stages s WHERE s.versioned_object_id = nuv.id)
    ORDER BY args.ord
    RETURNING id, versioned_object_id, description, description_fluent_message_id, kind_id, icons, created_at
),
all_stages AS (
            SELECT ns.id, ns.versioned_object_id, ns.description, ns.description_fluent_message_id, ns.kind_id, ns.icons, ns.created_at, nuv.ord
    FROM new_stages ns
    JOIN numbered_versions nuv ON nuv.id = ns.versioned_object_id
    UNION ALL
        SELECT s.id, s.versioned_object_id, s.description, s.description_fluent_message_id, s.kind_id, s.icons, s.created_at, nuv.ord
    FROM rev.stages s
    JOIN numbered_versions nuv ON s.versioned_object_id = nuv.id
    WHERE NOT EXISTS (SELECT 1 FROM new_stages ns WHERE ns.versioned_object_id = nuv.id)
),
numbered_stages AS (
    SELECT ast.* FROM all_stages ast
)
SELECT
    nus.id AS "id!",
    nus.versioned_object_id AS "versioned_object_id!",
    nuv.path_id AS "path_id!",
    paths.name AS "name!",
    paths.slug AS "slug!",
    nuv.content_hash AS "content_hash!",
    COALESCE(rev.fluent_body(nus.description_fluent_message_id, 'en'), nus.description, paths.name) AS "description!",
    sk.key AS "kind?",
    nus.icons AS "icons?",
    nus.created_at AS "created_at!"
FROM numbered_stages nus
JOIN numbered_versions nuv ON nuv.ord = nus.ord
JOIN paths ON paths.ord = nus.ord
LEFT JOIN rev.stage_kinds sk ON sk.id = nus.kind_id
ORDER BY nus.ord;
