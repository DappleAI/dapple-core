-- Insert brief unit input subtype
-- Parameters:
--   $1: brief_unit_id (UUID)
--   $2: control_type_key (TEXT)
--   $3: label_text (TEXT) - nullable
--   $4: label_fluent_message_id (UUID) - nullable
--   $5: help_text (TEXT) - nullable
--   $6: help_fluent_message_id (UUID) - nullable
--   $7: placeholder_text (TEXT) - nullable
--   $8: placeholder_fluent_message_id (UUID) - nullable
--   $9: control_spec (JSONB)

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
