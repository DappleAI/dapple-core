-- ⚠ TODO(-- outside dapple-data, so the bugs are latent — but anyone wiring this up
-- to a handler/SDK route needs to fix both first.
--
-- 1. Fold-ordering trap: outer `ORDER BY d.uoid_ord` orders parents by the
--    caller's input array position, but FlatDispatchModel::fold_models /
--    MergeOrdered require ascending `d.id` UUID order. Any caller passing
--    UUIDs in non-id-sorted order will trip a `child key X appears before
--    its parent key Y` error. See PR --    sibling by_journey/by_user/by_playbook queries (switch outer ORDER BY
--    to `d.id`, optionally re-sort in Rust after folding).
--
-- 2. Filter-semantics gap: `JOIN uoid ON ud.object_id = uoid.universal_user_id`
--    keys on `uoid.universal_user_id` (which is `$1` for every uoid row),
--    NOT on `uoid.id`. Despite the function name `*_by_universal_ids_by_user`,
--    the supplied `$2` UUID array is never used as a filter — the query
--    returns all dispatches for the user, duplicated `len($2)` times via
--    cartesian join. Intended semantics need a product call before the
--    fix is obvious; see issue for details.
--
-- See https://github.com/soulstompp/dapple-prompt/issues/450
WITH uoid AS (
    SELECT
        $1::UUID AS universal_user_id,
        t.id,
        t.ord
    FROM UNNEST($2::UUID[]) WITH ORDINALITY
        AS t (id, ord)
),
limited AS (
    SELECT d.*, uoid.ord AS uoid_ord
    FROM rev.dispatches d
    JOIN rev.journeys j ON d.journey_id = j.id
    JOIN rev.user_details ud ON j.user_id = ud.user_id
    JOIN uoid ON ud.object_id = uoid.universal_user_id
    ORDER BY uoid.ord
    LIMIT $3 OFFSET $4
)
SELECT
    d.id,
    d.journey_id,
    d.intent,
    d.created_at,
    dst.id AS "dispatch_stage_id?",
    dst.playbook_stage_id AS "dispatch_stage_playbook_stage_id?",
    dst_pst.playbook_id AS "dispatch_stage_playbook_id?",
    dst_pst.stage_id AS "dispatch_stage_stage_id?",
    dst.params AS "dispatch_stage_params?",
    dst.created_at AS "dispatch_stage_created_at?",
    ds.id AS "dispatch_step_id?",
    ds.stage_step_id AS "dispatch_step_stage_step_id?",
    dst.playbook_stage_id AS "dispatch_step_playbook_stage_id?",
    s.id AS "dispatch_step_step_id?",
    dst_pst.playbook_id AS "dispatch_step_playbook_id?",
    ds.params AS "dispatch_step_params?",
    ds.created_at AS "dispatch_step_created_at?",
    dt.id AS "dispatch_task_id?",
    dt.step_task_id AS "dispatch_task_step_task_id?",
    t.id AS "dispatch_task_task_id?",
    dt.created_at AS "dispatch_task_created_at?",
    dta.status AS "dispatch_task_status?"
FROM limited d
JOIN rev.journeys j ON d.journey_id = j.id
LEFT JOIN rev.dispatch_stages dst ON d.id = dst.dispatch_id
LEFT JOIN rev.playbook_stages dst_pst ON dst.playbook_stage_id = dst_pst.id
LEFT JOIN rev.playbooks dp ON dst_pst.playbook_id = dp.id
LEFT JOIN rev.stages ds2 ON dst_pst.stage_id = ds2.id
LEFT JOIN rev.dispatch_steps ds ON dst.id = ds.dispatch_stage_id
LEFT JOIN rev.stage_steps ss ON ds.stage_step_id = ss.id
LEFT JOIN rev.steps s ON ss.step_id = s.id
LEFT JOIN rev.dispatch_tasks dt ON dt.dispatch_step_id = ds.id
LEFT JOIN rev.step_tasks stk ON dt.step_task_id = stk.id
LEFT JOIN rev.tasks t ON stk.task_id = t.id
LEFT JOIN LATERAL (
    SELECT COALESCE(ev.phase_code, a.status) as status
    FROM rev.dispatch_task_attempts a
    LEFT JOIN LATERAL (
        SELECT phase_code
        FROM rev.dispatch_task_attempt_events ev2
        WHERE ev2.attempt_id = a.id
        ORDER BY ev2.created_at DESC, ev2.id DESC
        LIMIT 1
    ) ev ON true
    WHERE a.dispatch_step_id = ds.id
      AND a.task_id = t.id
    ORDER BY a.created_at DESC
    LIMIT 1
) dta ON true
ORDER BY d.uoid_ord, dst.id NULLS FIRST, ds.id NULLS FIRST, dt.id NULLS FIRST;
