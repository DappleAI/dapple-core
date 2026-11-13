WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
fluent_message_page AS (
    SELECT fm.id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.fluent_messages fm ON fm.versioned_object_id = vo.id
)

SELECT
    fm.id,
    fm.versioned_object_id,
    op.id AS path_id,
    fmpd.name,
    fmpd.description,
    vo.content_hash,
    dl.key AS "deprecation_level_key?",
    fm.created_at,

        fmb.id as "body_id?",
    fmb.locale_id as "body_locale_id?",
    l.tag as "body_locale_tag?",
    fmb.body as "body_text?",
    fmb.created_at as "body_created_at?",

        fmeb.id as "engine_body_id?",
    fmeb.model_release_id as "engine_body_model_release_id?",
    mr.id as "engine_body_model_release_object_id?",
    lin.key as "engine_body_lineage_key?",
    prov.key as "engine_body_provider_key?",
    mr.tier as "engine_body_tier?",
    mr.release as "engine_body_release?",
    fmeb.body as "engine_body_text?",
    fmeb.created_at as "engine_body_created_at?"
FROM fluent_message_page fmp
JOIN rev.fluent_messages fm ON fm.id = fmp.id
JOIN rev.versioned_objects vo ON vo.id = fm.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.fluent_message_path_details fmpd ON fmpd.path_id = op.id
LEFT JOIN LATERAL (
    SELECT de.to_level_id
FROM rev.deprecation_events de
WHERE de.versioned_object_id = vo.id
ORDER BY de.created_at DESC
LIMIT 1

) ld ON TRUE
LEFT JOIN rev.deprecation_levels dl ON dl.id = ld.to_level_id
LEFT JOIN rev.fluent_message_bodies fmb ON fmb.fluent_message_id = fm.id
LEFT JOIN rev.fluent_message_locales l ON l.id = fmb.locale_id
LEFT JOIN rev.fluent_message_engine_bodies fmeb ON fmeb.fluent_message_id = fm.id
LEFT JOIN rev.model_releases mr ON mr.id = fmeb.model_release_id
LEFT JOIN rev.lineages lin ON lin.id = mr.lineage_id
LEFT JOIN rev.providers prov ON prov.id = mr.provider_id
ORDER BY fm.id, fmb.id, fmeb.id;

