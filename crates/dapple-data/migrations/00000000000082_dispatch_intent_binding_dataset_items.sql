-- Migration 82: dataset binding typed-item table.
--
-- The dedicated typed-item table for `dataset` value_kind bindings. It PINS the
-- rev.asset_dataset_extractions row current at bind time by hard FK, and carries
-- NO envelope copy and NO direct asset_id — both are reachable through the pinned
-- extraction row. Two bindings of the same dataset (e.g. an operator input and a
-- downstream computed output pointing at one upload) pin the SAME extraction row:
-- one envelope, two lightweight FK items ("reference exact versions; don't bake
-- copies").
--
-- This is the FIRST binding typed-item table with hard FK integrity on its
-- content reference — the existing rev.dispatch_intent_binding_asset_items.asset_id
-- is a bare UUID with no REFERENCES (Migration 9); the dataset item tightens that
-- by pointing at a real FK, so a binding cannot reference a non-existent extraction.
--
-- DDL-only (no seed). See docs/plans/spreadsheet-dataset-ingest.md (Section 4.6 / 6.1).
CREATE TABLE rev.dispatch_intent_binding_dataset_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL
        REFERENCES rev.dispatch_intent_bindings (id),
    asset_dataset_extraction_id UUID NOT NULL
        REFERENCES rev.asset_dataset_extractions (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (dispatch_intent_binding_id, asset_dataset_extraction_id)
);

-- Both FK columns get an explicit index (Postgres does not auto-index FKs).
CREATE INDEX dispatch_intent_binding_dataset_items_binding_idx
    ON rev.dispatch_intent_binding_dataset_items (dispatch_intent_binding_id);
CREATE INDEX dispatch_intent_binding_dataset_items_extraction_idx
    ON rev.dispatch_intent_binding_dataset_items (asset_dataset_extraction_id);
