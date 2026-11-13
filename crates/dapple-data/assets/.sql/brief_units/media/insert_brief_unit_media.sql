INSERT INTO rev.brief_unit_media (brief_unit_id)
VALUES ($1::UUID)
RETURNING brief_unit_id;
