-- Upsert fluent messages using the unified versioning infrastructure.
-- Handles BOTH truly new names AND orphaned path_details (where
-- object_path + path_details exist but versioned_object/fluent_message don't).
--
-- For each input name:
--   - If fluent_message_path_details already has that name -> reuse its path_id
--   - Otherwise -> create new object_path + path_details
-- Then unconditionally create versioned_object + fluent_message for all.
--
-- NOTE: Data-modifying CTEs cannot see each other's effects on target tables,
-- so all data flows through CTE references, never re-reading the table.

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(content_hash, description, name, ord)
),
entity_type AS (
    SELECT id FROM rev.entity_types WHERE key = 'fluent_message'
),
-- Find names that already have path_details (orphaned or not)
existing_details AS (
    SELECT fmpd.path_id, op.id AS path_id_val, fmpd.name
    FROM rev.fluent_message_path_details fmpd
    JOIN rev.object_paths op ON op.id = fmpd.path_id
    WHERE fmpd.name = ANY($3::TEXT[])
),
-- Names that need brand-new paths
new_name_args AS (
    SELECT args.*
    FROM args
    LEFT JOIN existing_details ed ON ed.name = args.name
    WHERE ed.name IS NULL
),
-- Create object_paths only for truly new names
new_paths AS (
    INSERT INTO rev.object_paths (entity_type_id)
    SELECT (SELECT id FROM entity_type)
    FROM new_name_args
    ORDER BY new_name_args.ord
    RETURNING id
),
numbered_new_paths AS (
    SELECT np.*, row_number() OVER (ORDER BY np.id) AS rn
    FROM new_paths np
),
numbered_new_name_args AS (
    SELECT nna.*, row_number() OVER (ORDER BY nna.ord) AS rn
    FROM new_name_args nna
),
-- Create path_details only for truly new names
new_path_details AS (
    INSERT INTO rev.fluent_message_path_details (path_id, name, description)
    SELECT nnp.id, nna.name, nna.description
    FROM numbered_new_paths nnp
    JOIN numbered_new_name_args nna ON nna.rn = nnp.rn
    RETURNING path_id, name, description
),
-- Merge: every input name now has a path_id
-- Flows entirely through CTEs, no table re-reads
all_paths AS (
    SELECT ed.path_id, args.name, args.description, args.content_hash, args.ord
    FROM args
    JOIN existing_details ed ON ed.name = args.name
    UNION ALL
    SELECT nnp.id, nna.name, nna.description, nna.content_hash, nna.ord
    FROM numbered_new_paths nnp
    JOIN numbered_new_name_args nna ON nna.rn = nnp.rn
),
-- Create versioned_objects for ALL names
new_versions AS (
    INSERT INTO rev.versioned_objects (path_id, content_hash)
    SELECT ap.path_id, ap.content_hash
    FROM all_paths ap
    ORDER BY ap.ord
    RETURNING id, path_id, content_hash, created_at
),
-- Correlate versions back to all_paths for metadata
numbered_versions AS (
    SELECT nv.*, row_number() OVER (ORDER BY nv.id) AS rn
    FROM new_versions nv
),
numbered_all_paths AS (
    SELECT ap.*, row_number() OVER (ORDER BY ap.ord) AS rn
    FROM all_paths ap
),
-- Create fluent_messages for ALL names
new_messages AS (
    INSERT INTO rev.fluent_messages (versioned_object_id)
    SELECT nv.id FROM numbered_versions nv ORDER BY nv.rn
    RETURNING id, versioned_object_id, created_at
),
numbered_messages AS (
    SELECT nm.*, row_number() OVER (ORDER BY nm.id) AS rn
    FROM new_messages nm
)
SELECT
    nmsg.id,
    nmsg.versioned_object_id,
    nap.path_id AS "path_id!",
    nap.name AS "name!",
    nap.description,
    nver.content_hash,
    nmsg.created_at
FROM numbered_messages nmsg
JOIN numbered_versions nver ON nver.rn = nmsg.rn
JOIN numbered_all_paths nap ON nap.rn = nmsg.rn
ORDER BY nmsg.id;
