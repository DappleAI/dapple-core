WITH unresolved AS (
    SELECT crp.id, crp.scope, crp.environment, crp.object_type, crp.ref_key,
           crp.content_hash
    FROM rev.canonical_release_pins crp
    LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
    WHERE newer.id IS NULL
      AND crp.object_type = 'guides'
      AND crp.versioned_object_id IS NULL
),
resolved AS (
    SELECT u.id AS old_pin_id, u.scope, u.environment, u.object_type, u.ref_key,
           vo.content_hash AS resolved_content_hash, vo.id AS versioned_object_id
    FROM unresolved u
    JOIN rev.guides g ON g.source_path = u.ref_key
    JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
       AND vo.content_hash LIKE (u.content_hash || '%')
),
inserted AS (
    INSERT INTO rev.canonical_release_pins (
        scope, environment, object_type, ref_key,
        content_hash,
        versioned_object_id, resolved_at, supersedes_id
    )
    SELECT
        r.scope, r.environment, r.object_type, r.ref_key,
        r.resolved_content_hash,
        r.versioned_object_id, NOW(), r.old_pin_id
    FROM resolved r
    ORDER BY r.old_pin_id
    RETURNING id, scope, environment, object_type, ref_key,
              content_hash,
              versioned_object_id AS "versioned_object_id!",
              resolved_at AS "resolved_at!",
              supersedes_id, created_at
)
SELECT * FROM inserted ORDER BY id;
