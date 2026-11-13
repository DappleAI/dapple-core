-- Insert journey_brief_packs (associative table entries)
-- Resolves ids via CTE - no separate lookup query needed
-- Parameters:
--   $1: journey_ids (UUID[]) - flattened, one per association
--   $2: brief_pack_ids (UUID[]) - flattened, one per association

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[]
    ) WITH ORDINALITY AS o(journey_id, brief_pack_id, ord)
),
resolved AS (
    SELECT
        j.id AS journey_id,
        bp.id AS brief_pack_id,
        args.ord
    FROM args
    JOIN rev.journeys j ON j.id = args.journey_id
    JOIN rev.brief_packs bp ON bp.id = args.brief_pack_id
)
INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id, created_at)
SELECT journey_id, brief_pack_id, now()
FROM resolved
ORDER BY ord
RETURNING id, journey_id, brief_pack_id, created_at;
