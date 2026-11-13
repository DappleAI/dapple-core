-- Insert a user's billable entity (creates both billable_entity and user_billable_entity)
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],
        $2::TEXT[],
        $3::BIGINT[]
    ) WITH ORDINALITY AS o(lago_customer_external_id, lago_subscription_external_id, user_id, ord)
),
new_billable_entities AS (
    INSERT INTO rev.billable_entities (lago_customer_external_id, lago_subscription_external_id)
    SELECT args.lago_customer_external_id, args.lago_subscription_external_id
    FROM args
    ORDER BY args.ord
    RETURNING id, lago_customer_external_id, lago_subscription_external_id, created_at
),
numbered_be AS (
    SELECT nbe.*, row_number() OVER (ORDER BY nbe.id) AS ord
    FROM new_billable_entities nbe
),
new_user_billable_entities AS (
    INSERT INTO rev.user_billable_entities (billable_entity_id, user_id)
    SELECT nbe.id, args.user_id
    FROM args
    JOIN numbered_be nbe ON nbe.ord = args.ord
    ORDER BY args.ord
    RETURNING id, billable_entity_id, user_id, created_at
)
SELECT
    nube.id,
    nube.billable_entity_id,
    nube.user_id,
    nbe.lago_customer_external_id AS "lago_customer_external_id?",
    nbe.lago_subscription_external_id AS "lago_subscription_external_id?",
    nube.created_at
FROM new_user_billable_entities nube
JOIN numbered_be nbe ON nbe.id = nube.billable_entity_id
ORDER BY nube.id;
