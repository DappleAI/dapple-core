-- Migration 55: Typed display hint for brief outputs
--
-- A brief-output is a displayed unit; until now it carried no machine-readable
-- instruction for HOW to render its (json/asset) value, so UI clients guessed
-- per-slot. This adds a typed, authored display vocabulary, threaded exactly like
-- the input control_type / output source_kind lookup precedents: a lookup table
-- with a FK column on rev.brief_outputs. Plus two modifiers: `reveal` (collapse
-- behind a spoiler toggle) and a Fluent-referenced `title` (heading label),
-- mirroring the rev.brief_unit_notice title pair.

-- ============================================================================
-- rev.brief_output_display_hints — closed display vocabulary
--   image/text/markdown/list/checklist/table/fields render the value;
--   hidden marks a computed slot that exists only for data flow (it must ride a
--   :::brief-output to be produced/bound) and is never an operator surface.
-- ============================================================================
CREATE TABLE rev.brief_output_display_hints (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.brief_output_display_hints (key) VALUES
    ('image'), ('text'), ('markdown'), ('list'),
    ('checklist'), ('table'), ('fields'), ('hidden');

-- ============================================================================
-- rev.brief_outputs — add the display hint + modifiers
--   display_hint_id nullable (absent ⇒ 'text' semantics at read).
--   title_text / title_fluent_message_id: hybrid display label (raw fallback +
--   Fluent reference), same shape as rev.brief_unit_notice.
-- ============================================================================
ALTER TABLE rev.brief_outputs
    ADD COLUMN display_hint_id SMALLINT REFERENCES rev.brief_output_display_hints (id),
    ADD COLUMN reveal BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN title_text TEXT,
    ADD COLUMN title_fluent_message_id UUID;

CREATE INDEX brief_outputs_display_hint_idx ON rev.brief_outputs (display_hint_id);
