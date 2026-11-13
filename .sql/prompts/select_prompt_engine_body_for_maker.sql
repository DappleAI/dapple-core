-- Select the first engine_body for a prompt given a lineage key
-- Used for fetching config (e.g., T5 prefix for summarization, TTS speaker_id)
-- Parameters:
--   $1: prompt_id (UUID)
--   $2: lineage_key (TEXT) - e.g., 'flan-t5', 'vits'

WITH target_prompt AS (
    SELECT p.id
    FROM rev.prompts p
    WHERE p.id = $1
),
target_releases AS (
    SELECT mr.id AS release_id
    FROM rev.model_releases mr
    JOIN rev.lineages l ON l.id = mr.lineage_id
    WHERE l.key = $2
)
SELECT
    fmeb.body AS engine_body
FROM target_prompt tp
JOIN rev.prompt_fluent_messages pfm ON pfm.prompt_id = tp.id
JOIN rev.fluent_messages fm ON fm.id = pfm.fluent_message_id
JOIN target_releases tr ON true
JOIN rev.fluent_message_engine_bodies fmeb
    ON fmeb.fluent_message_id = fm.id
    AND fmeb.model_release_id = tr.release_id
ORDER BY pfm.id
LIMIT 1;
