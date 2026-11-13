-- Insert brief unit markdown subtype
-- Parameters:
--   $1: brief_unit_id (UUID)
--   $2: markdown_text (TEXT) - nullable
--   $3: fluent_message_id (UUID) - nullable
--   $4: format (TEXT) - nullable
--   $5: deliverable_spec (JSONB)

INSERT INTO rev.brief_unit_markdown (
    brief_unit_id,
    markdown_text,
    fluent_message_id,
    format,
    deliverable_spec
)
VALUES (
    $1::UUID,
    $2::TEXT,
    $3::UUID,
    $4::TEXT,
    COALESCE($5::JSONB, '{}'::jsonb)
)
RETURNING brief_unit_id;
