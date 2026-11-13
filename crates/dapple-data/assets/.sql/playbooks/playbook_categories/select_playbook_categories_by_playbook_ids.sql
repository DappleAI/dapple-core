WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY
    AS o(
      playbook_id, ord
    )
)
SELECT
    pc.id,
    pc.playbook_id,
    pc.category_id,
    pc.created_at
FROM rev.playbook_categories pc
JOIN rev.playbooks pb ON pc.playbook_id = pb.id
JOIN rev.categories c ON pc.category_id = c.id
JOIN args a ON pb.id = a.playbook_id
ORDER BY a.ord, pc.id;
