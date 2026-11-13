args AS (
    SELECT *
    FROM UNNEST($2::UUID[]) WITH ORDINALITY AS o(id, ord)
),
value_slot_page AS (
    SELECT vs.id
    FROM args
    JOIN rev.value_slots vs ON vs.id = args.id
    JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE op.namespace_company_id = $1::UUID
)
