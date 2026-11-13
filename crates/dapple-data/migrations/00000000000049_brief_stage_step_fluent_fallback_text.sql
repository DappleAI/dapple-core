-- ============================================================================
-- Migration 49 — Hybrid text+fluent shape for container display fields
-- ============================================================================
-- Migration 48 added the `*_fluent_message_id` references (the exact-version
-- pins) to briefs / brief_groups / brief_packs / stages / steps and removed the
-- baked title/description TEXT columns.
--
-- This migration restores `title`/`description` as **NULLABLE** fallback text,
-- so the container tables match the shape their own children (the brief unit
-- subtypes) already use:
--
--     rev.brief_unit_input(label_text, label_fluent_message_id, ...)
--     rev.brief_unit_notice(title_text, title_fluent_message_id, ...)
--
-- Division of labor (identical to the units):
--   * guide-ingest writes the `*_fluent_message_id` reference (text left NULL)
--     -> never drifts; the title resolves live from the pinned Fluent version.
--   * the direct POST config endpoints write their own authored `title`/
--     `description` TEXT (reference left NULL) -> a brief's own title, not a
--     copy of another versioned object, so it is NOT the baked-copy anti-pattern.
--
-- Reads resolve display text via rev.fluent_body() (defined below):
--   COALESCE(rev.fluent_body(ref, 'en'), <text column>, <path name>)
-- ============================================================================

-- ----------------------------------------------------------------------------
-- Restore nullable fallback text columns
-- ----------------------------------------------------------------------------
ALTER TABLE rev.briefs
    ADD COLUMN title TEXT,
    ADD COLUMN description TEXT;

ALTER TABLE rev.brief_groups
    ADD COLUMN title TEXT,
    ADD COLUMN description TEXT;

ALTER TABLE rev.brief_packs
    ADD COLUMN title TEXT,
    ADD COLUMN description TEXT;

ALTER TABLE rev.stages
    ADD COLUMN description TEXT;

ALTER TABLE rev.steps
    ADD COLUMN description TEXT;

-- ----------------------------------------------------------------------------
-- rev.fluent_body — resolve a Fluent message id to its body for a locale.
-- Prefers `preferred_locale`, else any available locale (lowest body id).
-- Returns NULL for a NULL/unknown message id, so callers COALESCE to the
-- fallback text column and then the path name. STABLE so the planner can
-- inline it in SELECT/RETURNING.
-- ----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION rev.fluent_body(msg_id UUID, preferred_locale TEXT DEFAULT 'en')
RETURNS TEXT
LANGUAGE sql
STABLE
AS $$
    SELECT fmb.body
    FROM rev.fluent_message_bodies fmb
    JOIN rev.fluent_message_locales fml ON fml.id = fmb.locale_id
    WHERE fmb.fluent_message_id = msg_id
    ORDER BY (fml.tag = preferred_locale) DESC, fmb.id
    LIMIT 1
$$;
