-- Migration 52: ON DELETE CASCADE for the owned brief_unit subtree.
--
-- The brief-owned dependent subtree (brief_units and everything beneath it:
-- input / notice / markdown / input_choices / media -> media_items -> posters
-- -> *_sources, alt_texts, captions, titles) was created with ON DELETE
-- NO ACTION on every FK. The repair-in-place backfill (Model B,
-- docs/plans/ingest-backfill-systemic-sweep.md) re-materializes a matched brief
-- version's owned dependents by delete-then-reinsert, so a single
-- `DELETE FROM rev.brief_units WHERE brief_id = ANY($1)` must wipe the whole
-- subtree in one statement. These rows are strictly OWNED by their parent (a
-- media item belongs to its unit; a poster belongs to its media item), so
-- ON DELETE CASCADE is the semantically-correct behavior regardless of Model B.
--
-- Intentionally NOT cascaded (left NO ACTION): FKs to LOOKUP / shared tables
-- (control_type_id, media_kind_id, source_type_id, source_kind_id, and the
-- value_slot_id FKs on the *_value_slot_sources tables). Those reference shared
-- rows, not owned children, and the owned source row is itself cascaded via its
-- media_item_id / poster_id FK below.

-- brief_units -> sub-rows
ALTER TABLE rev.brief_unit_markdown
  DROP CONSTRAINT brief_unit_markdown_brief_unit_id_fkey,
  ADD CONSTRAINT brief_unit_markdown_brief_unit_id_fkey
    FOREIGN KEY (brief_unit_id) REFERENCES rev.brief_units (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_notice
  DROP CONSTRAINT brief_unit_notice_brief_unit_id_fkey,
  ADD CONSTRAINT brief_unit_notice_brief_unit_id_fkey
    FOREIGN KEY (brief_unit_id) REFERENCES rev.brief_units (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_input
  DROP CONSTRAINT brief_unit_input_brief_unit_id_fkey,
  ADD CONSTRAINT brief_unit_input_brief_unit_id_fkey
    FOREIGN KEY (brief_unit_id) REFERENCES rev.brief_units (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_input_choices
  DROP CONSTRAINT brief_unit_input_choices_brief_unit_id_fkey,
  ADD CONSTRAINT brief_unit_input_choices_brief_unit_id_fkey
    FOREIGN KEY (brief_unit_id) REFERENCES rev.brief_units (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media
  DROP CONSTRAINT brief_unit_media_brief_unit_id_fkey,
  ADD CONSTRAINT brief_unit_media_brief_unit_id_fkey
    FOREIGN KEY (brief_unit_id) REFERENCES rev.brief_units (id) ON DELETE CASCADE;

-- brief_unit_media -> media_items
-- NB: brief_unit_media is a 1:1 extension of brief_unit, keyed by brief_unit_id
-- (not a surrogate `id`), so media_items reference brief_unit_media(brief_unit_id).
ALTER TABLE rev.brief_unit_media_items
  DROP CONSTRAINT brief_unit_media_items_brief_unit_id_fkey,
  ADD CONSTRAINT brief_unit_media_items_brief_unit_id_fkey
    FOREIGN KEY (brief_unit_id) REFERENCES rev.brief_unit_media (brief_unit_id) ON DELETE CASCADE;

-- media_items -> sources / posters / texts
ALTER TABLE rev.brief_unit_media_item_asset_sources
  DROP CONSTRAINT brief_unit_media_item_asset_sources_media_item_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_asset_sources_media_item_id_fkey
    FOREIGN KEY (media_item_id) REFERENCES rev.brief_unit_media_items (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_url_sources
  DROP CONSTRAINT brief_unit_media_item_url_sources_media_item_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_url_sources_media_item_id_fkey
    FOREIGN KEY (media_item_id) REFERENCES rev.brief_unit_media_items (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_value_slot_sources
  DROP CONSTRAINT brief_unit_media_item_value_slot_sources_media_item_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_value_slot_sources_media_item_id_fkey
    FOREIGN KEY (media_item_id) REFERENCES rev.brief_unit_media_items (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_posters
  DROP CONSTRAINT brief_unit_media_item_posters_media_item_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_posters_media_item_id_fkey
    FOREIGN KEY (media_item_id) REFERENCES rev.brief_unit_media_items (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_alt_texts
  DROP CONSTRAINT brief_unit_media_item_alt_texts_media_item_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_alt_texts_media_item_id_fkey
    FOREIGN KEY (media_item_id) REFERENCES rev.brief_unit_media_items (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_captions
  DROP CONSTRAINT brief_unit_media_item_captions_media_item_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_captions_media_item_id_fkey
    FOREIGN KEY (media_item_id) REFERENCES rev.brief_unit_media_items (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_titles
  DROP CONSTRAINT brief_unit_media_item_titles_media_item_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_titles_media_item_id_fkey
    FOREIGN KEY (media_item_id) REFERENCES rev.brief_unit_media_items (id) ON DELETE CASCADE;

-- posters -> poster sources
ALTER TABLE rev.brief_unit_media_item_poster_asset_sources
  DROP CONSTRAINT brief_unit_media_item_poster_asset_sources_poster_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_poster_asset_sources_poster_id_fkey
    FOREIGN KEY (poster_id) REFERENCES rev.brief_unit_media_item_posters (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_poster_url_sources
  DROP CONSTRAINT brief_unit_media_item_poster_url_sources_poster_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_poster_url_sources_poster_id_fkey
    FOREIGN KEY (poster_id) REFERENCES rev.brief_unit_media_item_posters (id) ON DELETE CASCADE;

ALTER TABLE rev.brief_unit_media_item_poster_value_slot_sources
  DROP CONSTRAINT brief_unit_media_item_poster_value_slot_sources_poster_id_fkey,
  ADD CONSTRAINT brief_unit_media_item_poster_value_slot_sources_poster_id_fkey
    FOREIGN KEY (poster_id) REFERENCES rev.brief_unit_media_item_posters (id) ON DELETE CASCADE;
