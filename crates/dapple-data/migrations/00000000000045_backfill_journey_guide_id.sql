-- Migration 45 — Backfill journey.guide_id on existing journeys and prune
-- stale rev.journey_brief_packs attachments left over from the playbook-
-- based auto-derive path.
--
-- Why
-- ----
-- Guides are versioned objects. Before this slice, `rev.journeys.guide_id`
-- was added by Migration 44 but only enforced for chained journeys; non-
-- chained journeys silently left it NULL. The journey-create auto-derive
-- for `brief_pack_ids` walked `playbook → guide_playbooks → guide_brief_packs`
-- across every guide version that had ever exposed the playbook, so a
-- journey's brief_pack attachments could "shift underfoot" whenever a new
-- guide version was ingested. Symptom: a journey created against an
-- essay-outline-builder guide ended up with two brief_pack rows (one per
-- ingested guide version) and `/brief-form` returned 6 entries instead of 3.
--
-- The forward fix (handler change in this slice) auto-pins every new
-- journey to a specific guide_id at create time and resolves brief_packs
-- via that pin. This migration handles pre-existing data:
--
--   1. Set `journey.guide_id` on every legacy journey to the guide
--      version that was latest at THAT journey's `created_at` moment.
--      Not today's latest — we want the snapshot the user actually
--      started against, so soft deprecation works (a journey pinned to a
--      now-deprecated version keeps resolving against it; only NEW
--      starts pick the latest non-deprecated version).
--
--   2. Delete `rev.journey_brief_packs` rows that don't belong to the
--      journey's (now-pinned) guide_id — these are leftovers from the
--      playbook-walk era when the journey was attached to brief_packs
--      from multiple guide versions.
--
-- Both statements are idempotent — re-running the migration on a healthy
-- DB is a no-op (no NULL guide_ids to fill, no stale attachments to drop).

-- ---------------------------------------------------------------------
-- Step 1: Pin legacy journeys to the guide version that was latest at
-- their created_at moment.
-- ---------------------------------------------------------------------
UPDATE rev.journeys j
SET guide_id = sub.guide_id
FROM (
    SELECT DISTINCT ON (j2.id, gvo.path_id)
        j2.id AS journey_id,
        g.id AS guide_id
    FROM rev.journeys j2
    JOIN rev.guide_playbooks gp ON gp.playbook_id = j2.playbook_id
    JOIN rev.guides g ON g.id = gp.guide_id
    JOIN rev.versioned_objects gvo ON gvo.id = g.versioned_object_id
    WHERE j2.guide_id IS NULL
      AND g.created_at <= j2.created_at
    ORDER BY j2.id, gvo.path_id, g.id DESC
) sub
WHERE j.id = sub.journey_id
  AND j.guide_id IS NULL;

-- ---------------------------------------------------------------------
-- Step 2: Drop stale journey_brief_packs attachments that don't match
-- the journey's pinned guide_id. Any leftover from the playbook-walk
-- era (a journey attached to brief_packs from a guide version other
-- than the one it's pinned to) is a hygiene artifact and must go.
-- ---------------------------------------------------------------------
DELETE FROM rev.journey_brief_packs jbp
WHERE NOT EXISTS (
    SELECT 1
    FROM rev.journeys j
    JOIN rev.guide_brief_packs gbp
        ON gbp.guide_id = j.guide_id
       AND gbp.brief_pack_id = jbp.brief_pack_id
    WHERE j.id = jbp.journey_id
      AND j.guide_id IS NOT NULL
);
