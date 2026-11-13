WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],          $2::UUID[]       ) WITH ORDINALITY AS o(playbook_id, stage_id, ord)
),
resolved AS (
    SELECT
        args.ord,
        ROW_NUMBER() OVER (PARTITION BY args.playbook_id ORDER BY args.ord) AS pos,
        args.playbook_id,
        s.id AS stage_id
    FROM args
    JOIN rev.playbooks p ON p.id = args.playbook_id
    JOIN rev.stages s ON s.id = args.stage_id
),
inserted AS (
    INSERT INTO rev.playbook_stages (playbook_id, stage_id, ord)
    SELECT r.playbook_id, r.stage_id, r.pos
    FROM resolved r
    ORDER BY r.ord
    ON CONFLICT (playbook_id, stage_id) DO NOTHING
    RETURNING id, playbook_id, stage_id, ord, created_at
),
all_links AS (
    SELECT id, playbook_id, stage_id, ord, created_at FROM inserted
    UNION ALL
    SELECT ps.id, ps.playbook_id, ps.stage_id, ps.ord, ps.created_at
    FROM resolved r
    JOIN rev.playbook_stages ps
        ON ps.playbook_id = r.playbook_id AND ps.stage_id = r.stage_id
    WHERE NOT EXISTS (
        SELECT 1 FROM inserted i
        WHERE i.playbook_id = r.playbook_id AND i.stage_id = r.stage_id
    )
)
SELECT
    al.id AS "id!",
    al.playbook_id AS "playbook_id!",
    al.stage_id AS "stage_id!",
    al.created_at AS "created_at!"
FROM all_links al
ORDER BY al.playbook_id, al.ord;
