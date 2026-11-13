
SELECT DISTINCT l.key AS lineage_key
FROM rev.prompts p
JOIN rev.prompt_fluent_messages pfm ON pfm.prompt_id = p.id
JOIN rev.fluent_message_engine_bodies fmeb ON fmeb.fluent_message_id = pfm.fluent_message_id
JOIN rev.model_releases mr ON mr.id = fmeb.model_release_id
JOIN rev.lineages l ON l.id = mr.lineage_id
WHERE p.id = $1
ORDER BY l.key;
