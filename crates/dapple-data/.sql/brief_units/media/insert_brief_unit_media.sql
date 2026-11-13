-- Insert brief unit media subtype (just the parent record)
-- Parameters: $1: brief_unit_id (UUID)
INSERT INTO rev.brief_unit_media (brief_unit_id)
VALUES ($1::UUID)
RETURNING brief_unit_id;
