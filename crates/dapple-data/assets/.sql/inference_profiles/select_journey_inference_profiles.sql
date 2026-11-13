
WITH profiles AS (
    SELECT DISTINCT ON (ipvo.path_id)
        jip.id,
        jip.journey_id,
        jip.inference_profile_id,
        ip.name AS inference_profile_name
    FROM rev.journey_inference_profiles jip
    JOIN rev.inference_profiles ip ON ip.id = jip.inference_profile_id
    JOIN rev.versioned_objects ipvo ON ipvo.id = ip.versioned_object_id
            LEFT JOIN LATERAL (
        SELECT de.to_level_id
        FROM rev.deprecation_events de
        WHERE de.versioned_object_id = ipvo.id
        ORDER BY de.created_at DESC
        LIMIT 1
    ) ld ON TRUE
    WHERE jip.journey_id = $1::UUID
      AND ld.to_level_id IS NULL
        ORDER BY ipvo.path_id, ipvo.created_at DESC, ipvo.id DESC
)
SELECT
    id,
    journey_id,
    inference_profile_id,
    inference_profile_name
FROM profiles
ORDER BY inference_profile_id;
