WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
)
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
FROM args
JOIN rev.canonical_release_pins crp ON crp.id = args.id
LEFT JOIN rev.canonical_release_pins newer ON newer.supersedes_id = crp.id
WHERE newer.id IS NULL
ORDER BY crp.id;
