WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],           $2::JSONB[]       ) WITH ORDINALITY AS o(journey_id, intent, ord)
),
resolved_args AS (
    SELECT
        args.ord,
        args.intent,
        j.id AS journey_id,
        j.user_id,
        j.owner_company_id,
        j.principal_id,
        j.subject_principal_id,
        j.billable_entity_id
    FROM args
    JOIN rev.journeys j ON j.id = args.journey_id
),
dispatch AS (
    INSERT INTO rev.dispatches (
        journey_id, user_id, intent, owner_company_id,
        principal_id, subject_principal_id, billable_entity_id
    )
    SELECT
        ra.journey_id, ra.user_id, ra.intent, ra.owner_company_id,
        ra.principal_id, ra.subject_principal_id, ra.billable_entity_id
    FROM resolved_args ra
    ORDER BY ra.ord
    RETURNING id, journey_id, intent, created_at
),
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
