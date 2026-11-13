WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],   -- journey_id[]
        $2::JSONB[]   -- intent[]
    ) WITH ORDINALITY AS o(journey_id, intent, ord)
),
-- Resolve journey IDs with ordinality preserved
resolved_args AS (
    SELECT
        args.ord,
        args.intent,
        j.id AS journey_id,
        j.user_id
    FROM args
    JOIN rev.journeys j ON j.id = args.journey_id
),
dispatch AS (
    INSERT INTO rev.dispatches (journey_id, user_id, intent)
    SELECT ra.journey_id, ra.user_id, ra.intent
    FROM resolved_args ra
    ORDER BY ra.ord
    RETURNING id, journey_id, intent, created_at
),
-- Number dispatches to preserve ordinality
numbered_dispatches AS (
    SELECT d.*, row_number() OVER (ORDER BY d.id) AS ord
    FROM dispatch d
)
SELECT
    nd.id,
    nd.journey_id,
    nd.intent,
    nd.created_at
FROM numbered_dispatches nd
JOIN resolved_args ra ON ra.ord = nd.ord
ORDER BY nd.id;
