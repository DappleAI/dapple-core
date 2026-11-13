SELECT
    j.id,
    j.owner_company_id,
    j.principal_id,
    j.guide_id AS "guide_id?",
    j.playbook_id
FROM rev.journeys j
WHERE j.id = $1::UUID;
