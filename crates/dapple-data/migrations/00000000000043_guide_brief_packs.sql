-- Migration 43: rev.guide_brief_packs — persist the playbook→brief_pack
-- association at ingest time.
--
-- Background: a journey's brief-form is assembled by walking
--   journey → journey_brief_packs → brief_packs → brief_groups → briefs
--     → brief_units → control_spec.value_slot_id
-- All five steps were healthy as of migrations 42/41 — except
-- `journey_brief_packs` is populated only when the *caller* of POST /journeys
-- passes `brief_pack_ids` in the request body. dapple-ui-demo, acme,
-- and any other client that only knows `playbook_id` has no way to discover
-- the brief_packs the playbook's source guide declares, so the journey is
-- created with zero `journey_brief_packs` rows and downstream
-- `/api/journeys/{id}/brief-form` returns an empty input list.
--
-- The forward fix (auto-derive on journey creation) needs a clean FK chain
-- back from `playbook_id` to its source guide's brief_packs. The only
-- existing connection is name-matching through `guide_blocks` — the
-- anti-pattern that already produced one cross-guide leak. This migration
-- adds a real association table so the journey-creation handler walks
--   playbook_id → guide_playbooks → guide_brief_packs → brief_pack_ids
-- with no name lookups.
--
-- Natural primary key on (guide_id, brief_pack_id); no synthetic `id` column.
-- The ingest path inserts with `ON CONFLICT (guide_id, brief_pack_id) DO NOTHING`
-- so re-ingest and refresh paths are idempotent.

CREATE TABLE rev.guide_brief_packs (
    guide_id      UUID NOT NULL REFERENCES rev.guides (id),
    brief_pack_id UUID NOT NULL REFERENCES rev.brief_packs (id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guide_id, brief_pack_id)
);

CREATE INDEX guide_brief_packs_pack_idx ON rev.guide_brief_packs (brief_pack_id);

-- ---------------------------------------------------------------------------
-- Backfill from existing guide_blocks
-- ---------------------------------------------------------------------------
-- For every brief_pack, find the guide(s) whose guide_blocks contain a
-- `brief_pack` directive with the matching name **in the same namespace**.
-- One-shot reconstruction using the (now-deleted) name-walking pattern;
-- safe here because it runs once at migration time, not on every query.
--
-- The namespace gate (`bpop.namespace_company_id = gop.namespace_company_id`)
-- is load-bearing: without it the backfill reintroduces the cross-guide
-- leak that earlier rounds removed (two guides in different namespaces
-- shipping a brief_pack of the same name would otherwise cross-associate).
INSERT INTO rev.guide_brief_packs (guide_id, brief_pack_id)
SELECT DISTINCT g.id AS guide_id, bp.id AS brief_pack_id
FROM rev.guides g
JOIN rev.versioned_objects gvo ON gvo.id = g.versioned_object_id
JOIN rev.object_paths gop ON gop.id = gvo.path_id
JOIN rev.guide_chapters gc ON gc.guide_id = g.id
JOIN rev.guide_blocks gb ON gb.chapter_id = gc.id
JOIN rev.guide_block_directive_types dt
    ON dt.id = gb.directive_type_id
   AND dt.key = 'brief_pack'
JOIN rev.brief_pack_path_details bpd ON bpd.name = gb.name
JOIN rev.object_paths bpop
    ON bpop.id = bpd.path_id
   AND bpop.namespace_company_id = gop.namespace_company_id
JOIN rev.versioned_objects bpvo ON bpvo.path_id = bpop.id
JOIN rev.brief_packs bp ON bp.versioned_object_id = bpvo.id
ON CONFLICT (guide_id, brief_pack_id) DO NOTHING;
