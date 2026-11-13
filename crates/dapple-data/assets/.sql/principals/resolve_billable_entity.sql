WITH
subject_pbe AS (
    SELECT pbe.billable_entity_id
    FROM rev.principal_billable_entities pbe
    WHERE pbe.principal_id = $2::UUID
),
subject_cbe AS (
    SELECT cbe.billable_entity_id
    FROM rev.principals p
    JOIN rev.company_billable_entities cbe ON cbe.company_id = p.owner_company_id
    WHERE p.id = $2::UUID
),
principal_pbe AS (
    SELECT pbe.billable_entity_id
    FROM rev.principal_billable_entities pbe
    WHERE pbe.principal_id = $1::UUID
),
principal_cbe AS (
    SELECT cbe.billable_entity_id
    FROM rev.principals p
    JOIN rev.company_billable_entities cbe ON cbe.company_id = p.owner_company_id
    WHERE p.id = $1::UUID
)
SELECT COALESCE(
    (SELECT billable_entity_id FROM subject_pbe),
    (SELECT billable_entity_id FROM subject_cbe),
    (SELECT billable_entity_id FROM principal_pbe),
    (SELECT billable_entity_id FROM principal_cbe)
) AS billable_entity_id;
