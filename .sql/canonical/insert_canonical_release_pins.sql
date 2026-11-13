-- Insert canonical release pins with supersession.
-- Finds current (non-superseded) pins per key via anti-join,
-- then inserts new rows with supersedes_id pointing to the old pin.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],       -- scope
        $2::TEXT[],       -- environment
        $3::TEXT[],       -- object_type
        $4::TEXT[],       -- ref_key
        $5::TEXT[],       -- content_hash
        $6::UUID[],       -- versioned_object_id (nullable)
        $7::TIMESTAMPTZ[] -- resolved_at (nullable)
    ) WITH ORDINALITY AS o(
        scope, environment, object_type, ref_key,
        content_hash,
        versioned_object_id, resolved_at, ord
    )
),
current_pins AS (
    SELECT crp.id, crp.scope, crp.environment, crp.object_type, crp.ref_key
    FROM rev.canonical_release_pins crp
    LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
    WHERE newer.id IS NULL
),
matched AS (
    SELECT args.*, cp.id AS old_pin_id
    FROM args
    LEFT JOIN current_pins cp
        ON cp.scope = args.scope AND cp.environment = args.environment
       AND cp.object_type = args.object_type AND cp.ref_key = args.ref_key
),
inserted AS (
    INSERT INTO rev.canonical_release_pins (
        scope, environment, object_type, ref_key,
        content_hash,
        versioned_object_id, resolved_at, supersedes_id
    )
    SELECT
        m.scope, m.environment, m.object_type, m.ref_key,
        m.content_hash,
        m.versioned_object_id, m.resolved_at, m.old_pin_id
    FROM matched m
    ORDER BY m.ord
    RETURNING id, scope, environment, object_type, ref_key,
              content_hash,
              versioned_object_id, resolved_at, supersedes_id, created_at
)
SELECT * FROM inserted ORDER BY id;
