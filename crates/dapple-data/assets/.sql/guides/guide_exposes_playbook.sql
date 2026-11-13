SELECT EXISTS (
    SELECT 1
    FROM rev.guide_playbooks gp
    WHERE gp.guide_id = $1::UUID
      AND gp.playbook_id = $2::UUID
) AS "exposes!";
