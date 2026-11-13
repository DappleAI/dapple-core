
WITH copy_outputs AS (
    INSERT INTO rev.brief_outputs (
        brief_id, value_slot_id, unit_key, source_kind_id, source_path, transform,
        display_hint_id, reveal, title_text, title_fluent_message_id,
        display_group, display_format, ord
    )
    SELECT
        $2, bo.value_slot_id, bo.unit_key, bo.source_kind_id, bo.source_path, bo.transform,
        bo.display_hint_id, bo.reveal, bo.title_text, bo.title_fluent_message_id,
        bo.display_group, bo.display_format, bo.ord
    FROM rev.brief_outputs bo
    WHERE bo.brief_id = $1
    ORDER BY bo.ord, bo.id
    RETURNING id
)
SELECT count(*) AS copied_outputs FROM copy_outputs;
