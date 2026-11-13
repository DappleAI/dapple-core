
INSERT INTO rev.brief_unit_notice (
    brief_unit_id,
    level,
    title_text,
    title_fluent_message_id,
    detail_text,
    detail_fluent_message_id
)
VALUES (
    $1::UUID,
    $2::TEXT,
    $3::TEXT,
    $4::UUID,
    $5::TEXT,
    $6::UUID
)
RETURNING brief_unit_id;
