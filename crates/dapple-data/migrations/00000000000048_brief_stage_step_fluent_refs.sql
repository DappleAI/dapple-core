-- ============================================================================
-- Migration 48 — Reference exact Fluent versions instead of baking title/desc
-- ============================================================================
-- The data model is a content-addressed DAG. A versioned object that references
-- another must PIN the exact version id, never bake a text copy of the other
-- object's content (which drifts: ingest's PATCH path re-baked the *slug* over
-- the good title on every re-version).
--
-- briefs / brief_groups / brief_packs carried baked `title`/`description` TEXT
-- copies of a Fluent message body; stages / steps carried a baked
-- `description` slug even though the real description "lives in the
-- FluentMessage body" (per dapple-parser's StageDef/StepDef).
--
-- This migration replaces those baked columns with `*_fluent_message_id`
-- references to rev.fluent_messages(id) — each id is 1:1 with a versioned
-- object, so the FK is an exact-version pin. Display text is resolved live at
-- read time from the pinned Fluent version (locale-aware, name fallback).
--
-- The baked columns are DROPPED. A full guide re-ingest repopulates every
-- reference (the blessed "re-version the world"); no row-level backfill.
--
-- Mirrors the existing brief-unit pattern (rev.brief_unit_input.label_fluent_message_id,
-- rev.brief_unit_notice.title_fluent_message_id) which already references
-- Fluent versions instead of baking text.
-- ============================================================================

-- ----------------------------------------------------------------------------
-- rev.briefs
-- ----------------------------------------------------------------------------
ALTER TABLE rev.briefs
    ADD COLUMN title_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    ADD COLUMN description_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    DROP COLUMN title,
    DROP COLUMN description;

CREATE INDEX briefs_title_fluent_idx ON rev.briefs (title_fluent_message_id)
    WHERE title_fluent_message_id IS NOT NULL;
CREATE INDEX briefs_description_fluent_idx ON rev.briefs (description_fluent_message_id)
    WHERE description_fluent_message_id IS NOT NULL;

-- ----------------------------------------------------------------------------
-- rev.brief_groups
-- ----------------------------------------------------------------------------
ALTER TABLE rev.brief_groups
    ADD COLUMN title_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    ADD COLUMN description_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    DROP COLUMN title,
    DROP COLUMN description;

CREATE INDEX brief_groups_title_fluent_idx ON rev.brief_groups (title_fluent_message_id)
    WHERE title_fluent_message_id IS NOT NULL;
CREATE INDEX brief_groups_description_fluent_idx ON rev.brief_groups (description_fluent_message_id)
    WHERE description_fluent_message_id IS NOT NULL;

-- ----------------------------------------------------------------------------
-- rev.brief_packs (pack_spec retained)
-- ----------------------------------------------------------------------------
ALTER TABLE rev.brief_packs
    ADD COLUMN title_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    ADD COLUMN description_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    DROP COLUMN title,
    DROP COLUMN description;

CREATE INDEX brief_packs_title_fluent_idx ON rev.brief_packs (title_fluent_message_id)
    WHERE title_fluent_message_id IS NOT NULL;
CREATE INDEX brief_packs_description_fluent_idx ON rev.brief_packs (description_fluent_message_id)
    WHERE description_fluent_message_id IS NOT NULL;

-- ----------------------------------------------------------------------------
-- rev.stages (description only — stages have no separate title)
-- ----------------------------------------------------------------------------
ALTER TABLE rev.stages
    ADD COLUMN description_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    DROP COLUMN description;

CREATE INDEX stages_description_fluent_idx ON rev.stages (description_fluent_message_id)
    WHERE description_fluent_message_id IS NOT NULL;

-- ----------------------------------------------------------------------------
-- rev.steps (description only)
-- ----------------------------------------------------------------------------
ALTER TABLE rev.steps
    ADD COLUMN description_fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    DROP COLUMN description;

CREATE INDEX steps_description_fluent_idx ON rev.steps (description_fluent_message_id)
    WHERE description_fluent_message_id IS NOT NULL;
