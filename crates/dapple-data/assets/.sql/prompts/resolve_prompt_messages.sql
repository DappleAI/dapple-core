
WITH target_prompt AS (
    SELECT p.id, ppd.name AS prompt_name, p.description AS prompt_description
    FROM rev.prompts p
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
    WHERE p.id = $1
),
release_candidates AS (
    SELECT
        mr.id AS release_id,
                '@' || p.key || ':' || l.key || '/' || mr.tier ||
            COALESCE(':' || mr.release, '') AS provider_key,
                l.key || '/' || mr.tier ||
            COALESCE(':' || mr.release, '') AS release_key,
                l.key AS lineage_key,
                array_position($3::text[], '@' || p.key || ':' || l.key || '/' || mr.tier ||
            COALESCE(':' || mr.release, '')) AS provider_priority,
        array_position($3::text[], l.key || '/' || mr.tier ||
            COALESCE(':' || mr.release, '')) AS release_priority,
        array_position($3::text[], l.key) AS lineage_priority
    FROM rev.model_releases mr
    JOIN rev.lineages l ON l.id = mr.lineage_id
    JOIN rev.providers p ON p.id = mr.provider_id
    WHERE
                '@' || p.key || ':' || l.key || '/' || mr.tier || COALESCE(':' || mr.release, '') = ANY($3::text[])
        OR l.key || '/' || mr.tier || COALESCE(':' || mr.release, '') = ANY($3::text[])
        OR l.key = ANY($3::text[])
),
prompt_messages AS (
    SELECT
        pfm.id AS pfm_id,
        pfm.fluent_message_id,
        pfm.role,
        ROW_NUMBER() OVER (ORDER BY pfm.id) AS ordinal
    FROM target_prompt tp
    JOIN rev.prompt_fluent_messages pfm ON pfm.prompt_id = tp.id
),
locale_candidates AS (
    SELECT
        pm.ordinal,
        pm.role,
        pm.fluent_message_id,
        fmb.id AS body_id,
        fmb.body,
        l.tag AS locale_tag,
        array_position($2::text[], l.tag) AS locale_priority,
        NULL::UUID AS release_id,
        NULL::integer AS provider_priority,
        NULL::integer AS release_priority,
        NULL::integer AS lineage_priority
    FROM prompt_messages pm
    JOIN rev.fluent_message_bodies fmb ON fmb.fluent_message_id = pm.fluent_message_id
    JOIN rev.fluent_message_locales l ON l.id = fmb.locale_id
    WHERE l.tag = ANY($2::text[])
),
engine_body_candidates AS (
    SELECT
        pm.ordinal,
        pm.role,
        pm.fluent_message_id,
        fmeb.id AS body_id,
        fmeb.body,
        NULL::text AS locale_tag,
        NULL::integer AS locale_priority,
        rc.release_id,
        rc.provider_priority,
        rc.release_priority,
        rc.lineage_priority
    FROM prompt_messages pm
    JOIN rev.fluent_message_engine_bodies fmeb ON fmeb.fluent_message_id = pm.fluent_message_id
    JOIN release_candidates rc ON rc.release_id = fmeb.model_release_id
),
all_candidates AS (
    SELECT * FROM locale_candidates
    UNION ALL
    SELECT * FROM engine_body_candidates
),
ranked_candidates AS (
    SELECT
        ordinal,
        role,
        body,
                CASE
            WHEN release_id IS NOT NULL THEN 0
            ELSE 1
        END AS source_priority,
        COALESCE(provider_priority, 999999) AS provider_rank,
        COALESCE(release_priority, 999999) AS release_rank,
        COALESCE(lineage_priority, 999999) AS lineage_rank,
        COALESCE(locale_priority, 999999) AS locale_rank
    FROM all_candidates
)
SELECT DISTINCT ON (ordinal)
    (SELECT id FROM target_prompt) AS prompt_id,
    (SELECT prompt_name FROM target_prompt) AS prompt_name,
    (SELECT prompt_description FROM target_prompt) AS prompt_description,
    ordinal,
    role,
    body
FROM ranked_candidates
ORDER BY
    ordinal,
    source_priority,          provider_rank,            release_rank,             lineage_rank,             locale_rank;          