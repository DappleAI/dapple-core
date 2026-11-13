-- Insert model-specific bodies for fluent messages using new taxonomy
-- Parameters:
--   $1: message_ids (UUID[]) - flattened, one per body
--   $2: lineage_keys (TEXT[]) - flattened, one per body
--   $3: provider_keys (TEXT[]) - flattened, one per body
--   $4: tiers (TEXT[]) - flattened, one per body
--   $5: releases (TEXT[]) - flattened, one per body (nullable)
--   $6: bodies (TEXT[]) - flattened, one per body
--
-- This replaces insert_fluent_message_engine_bodies.sql with the new taxonomy.

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::TEXT[],
        $3::TEXT[],
        $4::TEXT[],
        $5::TEXT[],
        $6::TEXT[]
    ) WITH ORDINALITY AS o(message_id, lineage_key, provider_key, tier, release, body, ord)
),
-- Get or insert lineages
lineage_refs AS (
    SELECT DISTINCT lineage_key FROM args
),
inserted_lineages AS (
    INSERT INTO rev.lineages (key)
    SELECT lineage_key FROM lineage_refs
    ON CONFLICT (key) DO NOTHING
    RETURNING *
),
all_lineages AS (
    SELECT id, key FROM inserted_lineages
    UNION
    SELECT id, key FROM rev.lineages
    WHERE key IN (SELECT lineage_key FROM lineage_refs)
),
-- Get or insert providers
provider_refs AS (
    SELECT DISTINCT provider_key FROM args
),
inserted_providers AS (
    INSERT INTO rev.providers (key)
    SELECT provider_key FROM provider_refs
    ON CONFLICT (key) DO NOTHING
    RETURNING *
),
all_providers AS (
    SELECT id, key FROM inserted_providers
    UNION
    SELECT id, key FROM rev.providers
    WHERE key IN (SELECT provider_key FROM provider_refs)
),
-- Get or insert model releases
release_refs AS (
    SELECT DISTINCT lineage_key, provider_key, tier, release FROM args
),
inserted_releases AS (
    INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
    SELECT
        l.id,
        p.id,
        rr.tier,
        rr.release
    FROM release_refs rr
    JOIN all_lineages l ON l.key = rr.lineage_key
    JOIN all_providers p ON p.key = rr.provider_key
    ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING
    RETURNING *
),
all_releases AS (
    SELECT mr.id, l.key as lineage_key, p.key as provider_key, mr.tier, mr.release
    FROM inserted_releases mr
    JOIN all_lineages l ON l.id = mr.lineage_id
    JOIN all_providers p ON p.id = mr.provider_id
    UNION
    SELECT mr.id, l.key as lineage_key, p.key as provider_key, mr.tier, mr.release
    FROM rev.model_releases mr
    JOIN rev.lineages l ON l.id = mr.lineage_id
    JOIN rev.providers p ON p.id = mr.provider_id
    WHERE (l.key, p.key, mr.tier, mr.release) IN (
        SELECT lineage_key, provider_key, tier, release FROM release_refs
    )
),
objs AS (
    INSERT INTO rev.fluent_message_engine_bodies (
        fluent_message_id,
        model_release_id,
        body,
        created_at
    )
    SELECT
        args.message_id,
        r.id,
        args.body,
        now()
    FROM args
    JOIN all_releases r ON r.lineage_key = args.lineage_key
        AND r.provider_key = args.provider_key
        AND r.tier = args.tier
        AND (r.release = args.release OR (r.release IS NULL AND args.release IS NULL))
    RETURNING *
)
SELECT
    objs.id,
    objs.fluent_message_id,
    objs.model_release_id,
    NULL::UUID AS model_variant_id,
    r.lineage_key,
    r.provider_key,
    r.tier,
    r.release,
    NULL::text AS variant_key,
    objs.body,
    objs.created_at
FROM objs
JOIN all_releases r ON r.id = objs.model_release_id
ORDER BY objs.id;
