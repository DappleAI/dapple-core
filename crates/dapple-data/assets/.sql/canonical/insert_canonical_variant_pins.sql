WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],               $2::TEXT[],               $3::TEXT[],               $4::TEXT[],               $5::TEXT[],               $6::TEXT[],               $7::UUID[],               $8::TIMESTAMPTZ[]     ) WITH ORDINALITY AS o(
        scope, environment, bucket, object_type, ref_key,
        content_hash,
        versioned_object_id, resolved_at, ord
    )
),
current_pins AS (
    SELECT crp.id, crp.scope, crp.environment, crp.bucket, crp.object_type, crp.ref_key
    FROM rev.canonical_variant_pins crp
    LEFT JOIN rev.canonical_variant_pins newer ON newer.supersedes_id = crp.id
    WHERE newer.id IS NULL
),
matched AS (
    SELECT args.*, cp.id AS old_pin_id
    FROM args
    LEFT JOIN current_pins cp
        ON cp.scope = args.scope AND cp.environment = args.environment
       AND cp.bucket = args.bucket
       AND cp.object_type = args.object_type AND cp.ref_key = args.ref_key
),
inserted AS (
    INSERT INTO rev.canonical_variant_pins (
        scope, environment, bucket, object_type, ref_key,
        content_hash,
        versioned_object_id, resolved_at, supersedes_id
    )
    SELECT
        m.scope, m.environment, m.bucket, m.object_type, m.ref_key,
        m.content_hash,
        m.versioned_object_id, m.resolved_at, m.old_pin_id
    FROM matched m
    ORDER BY m.ord
    RETURNING id, scope, environment, bucket, object_type, ref_key,
              content_hash,
              versioned_object_id, resolved_at, supersedes_id, created_at
)
SELECT * FROM inserted ORDER BY id;
