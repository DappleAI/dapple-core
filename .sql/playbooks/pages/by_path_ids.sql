args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(path_id, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (op.id)
        op.id AS path_id,
        p.id AS playbook_id
    FROM args
    JOIN rev.object_paths op ON op.id = args.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.playbooks p ON p.versioned_object_id = vo.id
    ORDER BY op.id, vo.created_at DESC
),
playbook_page AS (
    SELECT lv.playbook_id AS id
    FROM latest_versions lv
)
