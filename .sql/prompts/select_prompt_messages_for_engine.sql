-- Select prompt fluent messages with engine-specific body fallback
-- For rendering prompts, we prefer engine-specific bodies but fall back to localized bodies
-- Parameters:
--   $1: prompt_id (UUID)
--   $2: lineage_key (TEXT) - e.g., 'claude-3-5', 'vits'
--   $3: tier (TEXT) - e.g., 'sonnet', 'ljspeech'
--   $4: release (TEXT, nullable) - optional specific version like '20241022'

WITH target_prompt AS (
    SELECT p.id, ppd.name AS prompt_name, p.description AS prompt_description
    FROM rev.prompts p
    JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    JOIN rev.prompt_path_details ppd ON ppd.path_id = op.id
    WHERE p.id = $1
),
target_release AS (
    SELECT mr.id AS release_id
    FROM rev.model_releases mr
    JOIN rev.lineages l ON l.id = mr.lineage_id
    WHERE l.key = $2
      AND mr.tier = $3
      AND (mr.release = $4 OR ($4 IS NULL AND mr.release IS NULL) OR $4 IS NULL)
    LIMIT 1
)
SELECT
    tp.id AS prompt_id,
    tp.prompt_name,
    tp.prompt_description,
    pfm.role,
    -- Prefer engine-specific body, fall back to any localized body (prefer en-US)
    COALESCE(
        fmeb.body,
        (SELECT fmlb.body
         FROM rev.fluent_message_bodies fmlb
         LEFT JOIN rev.fluent_message_locales l ON l.id = fmlb.locale_id
         WHERE fmlb.fluent_message_id = fm.id
         ORDER BY CASE WHEN l.tag = 'en-US' THEN 0 ELSE 1 END, fmlb.id
         LIMIT 1)
    ) AS body,
    ROW_NUMBER() OVER (ORDER BY pfm.id) AS ordinal
FROM target_prompt tp
JOIN rev.prompt_fluent_messages pfm ON pfm.prompt_id = tp.id
JOIN rev.fluent_messages fm ON fm.id = pfm.fluent_message_id
LEFT JOIN target_release tr ON true
LEFT JOIN rev.fluent_message_engine_bodies fmeb
    ON fmeb.fluent_message_id = fm.id
    AND fmeb.model_release_id = tr.release_id
ORDER BY ordinal;
