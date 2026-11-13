-- Resolve the billable_entity_id to stamp onto a new journey or dispatch.
-- The chain (matches the auditor book's "who pays" rule):
--
--   1. The subject principal's exclusive PBE (if subject is set and has one)
--   2. The subject principal's owner-company CBE (if subject is set)
--   3. The acting principal's exclusive PBE
--   4. The acting principal's owner-company CBE
--
-- Returns NULL if no link is found — V1 allows this (many users don't have
-- a billable_entity yet, since Lago integration isn't fully wired). Once
-- onboarding always creates one, the handler will hard-fail on NULL.
--
-- subject_principal_id is NULLABLE; coalesce-style handling at the SQL
-- layer lets the caller pass either NULL or a real UUID without branching.
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
