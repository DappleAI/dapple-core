-- ============================================================================
-- rev.journey_asset_uploads — Staged operator uploads (append-only + removal)
--
-- Durable record of "asset X was uploaded for input I of journey J before the
-- form was submitted". The asset↔slot *binding* is still written at submit
-- time via dispatch_intent_bindings; this table only covers the pre-submit
-- window (client-side upload caches are volatile) so the UI can rehydrate
-- file fields after a reload and recover the asset id at submit.
--
-- input_key is the raw client input identifier: the journey-pinned
-- rev.value_slots.id for top-level brief fields, or a synthetic composite
-- (e.g. "<fieldId>-<index>-<childId>") for repeater-row fields. value_slot_id
-- is resolved at insert when input_key is a bare value-slot UUID, else NULL.
-- ============================================================================
CREATE TABLE rev.journey_asset_uploads (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    journey_id UUID NOT NULL REFERENCES rev.journeys (id),
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    input_key TEXT NOT NULL,
    value_slot_id UUID REFERENCES rev.value_slots (id),
    file_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX journey_asset_uploads_journey_idx
    ON rev.journey_asset_uploads (journey_id, id DESC);
CREATE INDEX journey_asset_uploads_asset_idx
    ON rev.journey_asset_uploads (asset_id);
CREATE INDEX journey_asset_uploads_slot_idx
    ON rev.journey_asset_uploads (value_slot_id)
    WHERE value_slot_id IS NOT NULL;

CREATE TABLE rev.journey_asset_uploads_removed (
    journey_asset_upload_id UUID PRIMARY KEY
        REFERENCES rev.journey_asset_uploads (id),
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
