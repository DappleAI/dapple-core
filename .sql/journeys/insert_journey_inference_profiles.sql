-- Insert journey_inference_profiles (associative table entries)
-- Resolves ids via CTE - no separate lookup query needed
-- Parameters:
--   $1: journey_ids (UUID[]) - flattened, one per association
--   $2: inference_profile_ids (UUID[]) - flattened, one per association

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY AS o(journey_id, inference_profile_id, ord)
),
resolved AS (
    SELECT
        j.id AS journey_id,
        ip.id AS inference_profile_id,
        args.ord
    FROM args
    JOIN rev.journeys j ON j.id = args.journey_id
    JOIN rev.inference_profiles ip ON ip.id = args.inference_profile_id
)
INSERT INTO rev.journey_inference_profiles (journey_id, inference_profile_id)
SELECT journey_id, inference_profile_id
FROM resolved
ORDER BY ord
RETURNING id, journey_id, inference_profile_id;
