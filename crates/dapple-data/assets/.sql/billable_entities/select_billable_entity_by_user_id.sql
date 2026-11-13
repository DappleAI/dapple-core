SELECT
    ube.id,
    ube.billable_entity_id,
    ube.user_id,
    be.lago_customer_external_id AS "lago_customer_external_id?",
    be.lago_subscription_external_id AS "lago_subscription_external_id?",
    ube.created_at
FROM rev.user_billable_entities ube
JOIN rev.billable_entities be ON be.id = ube.billable_entity_id
WHERE ube.user_id = $1::BIGINT;
