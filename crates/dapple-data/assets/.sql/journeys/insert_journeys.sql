WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::UUID[],
        $4::TEXT[],
        $5::UUID[],
        $6::UUID[],
        $7::UUID[],
        $8::UUID[],
        $9::UUID[]
    ) WITH ORDINALITY AS o(
        billable_entity_id,
        fork_stage_id,
        guide_id,
        name,
        owner_company_id,
        playbook_id,
        predecessor_journey_id,
        principal_id,
        subject_principal_id,
        ord
    )
),
resolved_args AS (
    SELECT
        args.ord,
        args.name,
        args.owner_company_id,
        args.billable_entity_id,
        args.guide_id,
        args.predecessor_journey_id,
        args.fork_stage_id,
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
        principal_id, subject_principal_id, billable_entity_id,
        predecessor_journey_id, guide_id, fork_stage_id
    )
    SELECT
        ra.playbook_id, ra.user_id, ra.name, ra.owner_company_id,
        ra.principal_id, ra.subject_principal_id, ra.billable_entity_id,
        ra.predecessor_journey_id, ra.guide_id, ra.fork_stage_id
    FROM resolved_args ra
    ORDER BY ra.ord
    RETURNING id, playbook_id, name, created_at, principal_id, owner_company_id,
              predecessor_journey_id, guide_id, fork_stage_id
),
numbered_objs AS (
    SELECT o.*, row_number() OVER (ORDER BY o.id) AS ord
    FROM objs o
)
SELECT
    no.id,
    no.playbook_id,
    no.principal_id,
                ud.object_id AS "user_id?",
    no.name,
    no.created_at,
    no.predecessor_journey_id AS "predecessor_journey_id?",
    no.guide_id AS "guide_id?",
    no.fork_stage_id AS "fork_stage_id?"
FROM numbered_objs no
JOIN resolved_args ra ON ra.ord = no.ord
LEFT JOIN rev.user_details ud ON ud.user_id = ra.user_id
ORDER BY no.id;
