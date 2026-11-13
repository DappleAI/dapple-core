-- Select inference profiles attached to a journey
-- Parameters:
--   $1: journey_id (UUID)

SELECT
    jip.id,
    jip.journey_id,
    jip.inference_profile_id,
    ip.name AS inference_profile_name
FROM rev.journey_inference_profiles jip
JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
WHERE jip.journey_id = $1
ORDER BY ip.id;
