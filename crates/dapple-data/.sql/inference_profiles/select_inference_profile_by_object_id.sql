-- Select an inference profile by id
-- Parameters:
--   $1: id (UUID)

SELECT
    ip.id,
    ip.versioned_object_id,
    ip.name,
    ip.description,
    ip.created_at
FROM rev.inference_profiles ip
WHERE ip.id = $1;
