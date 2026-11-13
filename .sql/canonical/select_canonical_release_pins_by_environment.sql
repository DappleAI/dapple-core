-- Select current (non-superseded) canonical release pins filtered by scope and environment.
SELECT
    crp.id,
    crp.scope,
    crp.environment,
    crp.object_type,
    crp.ref_key,
    crp.content_hash,
    crp.versioned_object_id,
    crp.resolved_at,
    crp.supersedes_id,
    crp.created_at
FROM rev.canonical_release_pins crp
LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
WHERE newer.id IS NULL
  AND crp.scope = $1
  AND crp.environment = $2
ORDER BY crp.object_type, crp.ref_key
LIMIT $3 OFFSET $4;
