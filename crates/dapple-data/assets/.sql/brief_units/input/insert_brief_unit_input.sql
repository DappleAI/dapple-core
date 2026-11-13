
WITH control_type_ref AS (
    SELECT id FROM rev.brief_input_control_types WHERE key = $2::TEXT
)
INSERT INTO rev.brief_unit_input (
    brief_unit_id,
    control_type_id,
    label_text,
    label_fluent_message_id,
    help_text,
    help_fluent_message_id,
    placeholder_text,
    placeholder_fluent_message_id,
    control_spec
)
SELECT
    $1::UUID,
    control_type_ref.id,
    $3::TEXT,
    $4::UUID,
    $5::TEXT,
    $6::UUID,
    $7::TEXT,
    $8::UUID,
    COALESCE($9::JSONB, '{}'::jsonb)
FROM control_type_ref
RETURNING brief_unit_id;
