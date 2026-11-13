-- =============================================================================
-- Path details: denormalize namespace_company_id + scope UNIQUE constraints
-- =============================================================================
-- Today every `rev.*_path_details.name`/`.slug`/`.title` has a GLOBAL UNIQUE
-- constraint (see migration 00000000000003). Pre-multi-tenant rollout this is
-- safe; the moment a second customer ships a guide whose author picks a name
-- that collides with dapple's or another customer's (e.g., a playbook named
-- "Onboarding"), the second customer's ingest 500s with a
-- `*_path_details_*_key` UNIQUE violation. The materializer can't dodge this
-- because the constraint is at the DB level.
--
-- Fix: denormalize `namespace_company_id` onto each path-details table and
-- replace the global UNIQUE with a composite `(name, namespace_company_id)`
-- and `(slug, namespace_company_id)` UNIQUE. The composite shape:
--   - lets every namespace independently pick the same name (so customers
--     don't collide on common terms like "Onboarding"),
--   - lets the materializer-side namespace filter via the GUC produce a
--     correct per-namespace REUSE/PATCH/INSERT decision instead of silently
--     reusing a sibling-namespace row.
--
-- Pre-rollout, acme is the only non-dapple data. Backfill is fast and
-- locks are brief. A future operator running this against a populated
-- multi-tenant prod database should NOT assume the same — schema-alter
-- holds an exclusive lock per table for the duration of the ALTER+UPDATE.
--
-- Reversible: drop the composite UNIQUE, re-add the global, drop the column.
-- Reverse migration is feasible TODAY only because no two rows in different
-- namespaces share a name yet. Once multi-tenant data lands, rollback would
-- fail on the re-added global UNIQUE — call that out at rollout planning.
--
-- Companion changes in this PR:
--   - Materializer SELECTs (`select_latest_*_by_*_in_tx`) gain a namespace
--     filter using the same `current_setting('dapple.namespace_company_id',
--     true)` GUC the INSERT side already uses.
--   - 14 INSERT .sqlc files propagate namespace_company_id from the shared
--     numbered_paths CTE into the path_details INSERT column list.
--   - Multi-tenant lifecycle tests assert two customers can ship guides with
--     colliding names and stay isolated.
-- =============================================================================

-- ---------------------------------------------------------------------------
-- task_path_details
-- ---------------------------------------------------------------------------
ALTER TABLE rev.task_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.task_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.task_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS task_path_details_name_key,
    DROP CONSTRAINT IF EXISTS task_path_details_slug_key,
    ADD CONSTRAINT task_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id),
    ADD CONSTRAINT task_path_details_slug_namespace_key
        UNIQUE (slug, namespace_company_id);
CREATE INDEX task_path_details_namespace_idx
    ON rev.task_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- step_path_details
-- ---------------------------------------------------------------------------
ALTER TABLE rev.step_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.step_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.step_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS step_path_details_name_key,
    DROP CONSTRAINT IF EXISTS step_path_details_slug_key,
    ADD CONSTRAINT step_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id),
    ADD CONSTRAINT step_path_details_slug_namespace_key
        UNIQUE (slug, namespace_company_id);
CREATE INDEX step_path_details_namespace_idx
    ON rev.step_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- stage_path_details
-- ---------------------------------------------------------------------------
ALTER TABLE rev.stage_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.stage_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.stage_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS stage_path_details_name_key,
    DROP CONSTRAINT IF EXISTS stage_path_details_slug_key,
    ADD CONSTRAINT stage_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id),
    ADD CONSTRAINT stage_path_details_slug_namespace_key
        UNIQUE (slug, namespace_company_id);
CREATE INDEX stage_path_details_namespace_idx
    ON rev.stage_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- playbook_path_details
-- ---------------------------------------------------------------------------
ALTER TABLE rev.playbook_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.playbook_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.playbook_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS playbook_path_details_name_key,
    DROP CONSTRAINT IF EXISTS playbook_path_details_slug_key,
    ADD CONSTRAINT playbook_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id),
    ADD CONSTRAINT playbook_path_details_slug_namespace_key
        UNIQUE (slug, namespace_company_id);
CREATE INDEX playbook_path_details_namespace_idx
    ON rev.playbook_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- prompt_path_details (only `name` is UNIQUE)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.prompt_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.prompt_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.prompt_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS prompt_path_details_name_key,
    ADD CONSTRAINT prompt_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id);
CREATE INDEX prompt_path_details_namespace_idx
    ON rev.prompt_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- fluent_message_path_details (only `name` is UNIQUE)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.fluent_message_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.fluent_message_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.fluent_message_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS fluent_message_path_details_name_key,
    ADD CONSTRAINT fluent_message_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id);
CREATE INDEX fluent_message_path_details_namespace_idx
    ON rev.fluent_message_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- category_path_details
-- ---------------------------------------------------------------------------
ALTER TABLE rev.category_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.category_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.category_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS category_path_details_name_key,
    DROP CONSTRAINT IF EXISTS category_path_details_slug_key,
    ADD CONSTRAINT category_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id),
    ADD CONSTRAINT category_path_details_slug_namespace_key
        UNIQUE (slug, namespace_company_id);
CREATE INDEX category_path_details_namespace_idx
    ON rev.category_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- hat_path_details
-- ---------------------------------------------------------------------------
ALTER TABLE rev.hat_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.hat_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.hat_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS hat_path_details_name_key,
    DROP CONSTRAINT IF EXISTS hat_path_details_slug_key,
    ADD CONSTRAINT hat_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id),
    ADD CONSTRAINT hat_path_details_slug_namespace_key
        UNIQUE (slug, namespace_company_id);
CREATE INDEX hat_path_details_namespace_idx
    ON rev.hat_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- brief_path_details (only `name` is UNIQUE)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.brief_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.brief_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.brief_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS brief_path_details_name_key,
    ADD CONSTRAINT brief_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id);
CREATE INDEX brief_path_details_namespace_idx
    ON rev.brief_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- brief_group_path_details (only `name` is UNIQUE)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.brief_group_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.brief_group_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.brief_group_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS brief_group_path_details_name_key,
    ADD CONSTRAINT brief_group_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id);
CREATE INDEX brief_group_path_details_namespace_idx
    ON rev.brief_group_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- brief_pack_path_details (only `name` is UNIQUE)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.brief_pack_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.brief_pack_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.brief_pack_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS brief_pack_path_details_name_key,
    ADD CONSTRAINT brief_pack_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id);
CREATE INDEX brief_pack_path_details_namespace_idx
    ON rev.brief_pack_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- value_slot_path_details (only `name` is UNIQUE; value_kind_id stays)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.value_slot_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.value_slot_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.value_slot_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS value_slot_path_details_name_key,
    ADD CONSTRAINT value_slot_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id);
CREATE INDEX value_slot_path_details_namespace_idx
    ON rev.value_slot_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- directive_path_details (only `name` is UNIQUE)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.directive_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.directive_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.directive_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS directive_path_details_name_key,
    ADD CONSTRAINT directive_path_details_name_namespace_key
        UNIQUE (name, namespace_company_id);
CREATE INDEX directive_path_details_namespace_idx
    ON rev.directive_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- inference_profile_path_details (no UNIQUE today; add namespace for
-- SELECT-side filtering parity with sibling tables)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.inference_profile_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.inference_profile_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.inference_profile_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL;
CREATE INDEX inference_profile_path_details_namespace_idx
    ON rev.inference_profile_path_details (namespace_company_id);

-- ---------------------------------------------------------------------------
-- guide_path_details (note: uses `title` not `name`)
-- ---------------------------------------------------------------------------
ALTER TABLE rev.guide_path_details
    ADD COLUMN namespace_company_id UUID REFERENCES rev.companies (id);
UPDATE rev.guide_path_details tpd
    SET namespace_company_id = op.namespace_company_id
    FROM rev.object_paths op
    WHERE op.id = tpd.path_id;
ALTER TABLE rev.guide_path_details
    ALTER COLUMN namespace_company_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS guide_path_details_title_key,
    DROP CONSTRAINT IF EXISTS guide_path_details_slug_key,
    ADD CONSTRAINT guide_path_details_title_namespace_key
        UNIQUE (title, namespace_company_id),
    ADD CONSTRAINT guide_path_details_slug_namespace_key
        UNIQUE (slug, namespace_company_id);
CREATE INDEX guide_path_details_namespace_idx
    ON rev.guide_path_details (namespace_company_id);
