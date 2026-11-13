
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
    JOIN all_lineages l ON l.id = mr.lineage_id
    JOIN all_providers p ON p.id = mr.provider_id
    WHERE (l.key, p.key, mr.tier, mr.release) IN (
        SELECT lineage_key, provider_key, tier, release FROM release_refs
    )
),
new_engine_bodies AS (
    INSERT INTO rev.fluent_message_engine_bodies (
        fluent_message_id,
        model_release_id,
        body,
        created_at
    )
    SELECT
        args.message_id,
        mr.id,
        args.body,
        now()
    FROM args
    JOIN all_releases mr ON mr.lineage_key = args.lineage_key
        AND mr.provider_key = args.provider_key
        AND mr.tier = args.tier
        AND (mr.release = args.release OR (mr.release IS NULL AND args.release IS NULL))
    WHERE NOT EXISTS (
        SELECT 1 FROM rev.fluent_message_engine_bodies eb
        WHERE eb.fluent_message_id = args.message_id AND eb.model_release_id = mr.id
    )
    RETURNING *
),
objs AS (
    SELECT * FROM new_engine_bodies
    UNION ALL
    SELECT eb.*
    FROM rev.fluent_message_engine_bodies eb
    JOIN args ON eb.fluent_message_id = args.message_id
    JOIN all_releases mr ON mr.lineage_key = args.lineage_key
        AND mr.provider_key = args.provider_key
        AND mr.tier = args.tier
        AND (mr.release = args.release OR (mr.release IS NULL AND args.release IS NULL))
    WHERE eb.model_release_id = mr.id
        AND NOT EXISTS (SELECT 1 FROM new_engine_bodies neb WHERE neb.fluent_message_id = eb.fluent_message_id AND neb.model_release_id = eb.model_release_id)
)
SELECT
    objs.id AS "id!",
    objs.fluent_message_id AS "fluent_message_id!",
    objs.model_release_id AS "model_release_id!",
    NULL::UUID as model_variant_id,
    mr.id as "model_release_object_id!",
    mr.lineage_key AS "lineage_key!",
    mr.provider_key AS "provider_key!",
    mr.tier AS "tier!",
    mr.release,
    objs.body AS "body!",
    objs.created_at AS "created_at!"
FROM objs
JOIN all_releases mr ON mr.id = objs.model_release_id
ORDER BY objs.id;
