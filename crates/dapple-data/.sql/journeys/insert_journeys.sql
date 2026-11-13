-- Insert journeys keyed on principal_id (UUID) instead of user_object_id.
-- The caller is responsible for resolving auth_user_id / external_service_token
-- to a principal_id before calling. Billing attribution is also resolved
-- caller-side and stamped into billable_entity_id at create-time (locked,
-- never recomputed — matches the auditor book's "no modifications" rule).
--
-- user_id (BIGINT) is written for the deprecation window via a LEFT JOIN to
-- human_principals — non-human principals leave it NULL. A future migration
-- drops the column once all readers have switched to principal_id.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::TEXT[],
        $3::UUID[],
        $4::UUID[],
        $5::UUID[],
        $6::UUID[]
    ) WITH ORDINALITY AS o(
        billable_entity_id,
        name,
        owner_company_id,
        playbook_id,
        principal_id,
        subject_principal_id,
        ord
    )
),
-- Resolve playbook (existence check) and derive user_id for the deprecation
-- window via the human_principal link (NULL for non-human principals).
resolved_args AS (
    SELECT
        args.ord,
        args.name,
        args.owner_company_id,
        args.billable_entity_id,
        args.subject_principal_id,
        p.id AS playbook_id,
        args.principal_id,
        hp.user_id
    FROM args
    JOIN rev.playbooks p ON args.playbook_id = p.id
    LEFT JOIN rev.human_principals hp ON hp.principal_id = args.principal_id
),
objs AS (
    INSERT INTO rev.journeys (
        playbook_id, user_id, name, owner_company_id,
        principal_id, subject_principal_id, billable_entity_id
    )
    SELECT
        ra.playbook_id, ra.user_id, ra.name, ra.owner_company_id,
        ra.principal_id, ra.subject_principal_id, ra.billable_entity_id
    FROM resolved_args ra
    ORDER BY ra.ord
    RETURNING id, playbook_id, name, created_at, principal_id, owner_company_id
),
numbered_objs AS (
    SELECT o.*, row_number() OVER (ORDER BY o.id) AS ord
    FROM objs o
)
SELECT
    no.id,
    no.playbook_id,
    no.principal_id,
    -- Legacy alias: callers still expect a `user_id: Uuid` field in the model
    -- shape during the deprecation window. We return the user_details.object_id
    -- (NULL for non-human principals); a future release drops the column.
    ud.object_id AS "user_id?",
    no.name,
    no.created_at
FROM numbered_objs no
JOIN resolved_args ra ON ra.ord = no.ord
LEFT JOIN rev.user_details ud ON ud.user_id = ra.user_id
ORDER BY no.id;
