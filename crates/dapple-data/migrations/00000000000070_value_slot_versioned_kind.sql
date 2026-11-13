-- ============================================================================
-- Migration 70 — Versioned value_kind
-- ============================================================================
-- value_kind was path-level identity on rev.value_slot_path_details (immutable).
-- A guide changing a slot's kind (e.g. text -> json) minted a new value_slots
-- version but kept the FROZEN path kind, so json picks were String()'d into
-- text_items ("[object Object]"). Kind is a VERSIONED property, like constraints
-- and cardinality (migration 50): move it onto rev.value_slots so a new version
-- carries the new kind (append-only Merkle).
--
-- rev.value_slot_path_details.value_kind_id is KEPT as the creation-time / path
-- birth kind and the display default for the two genuinely version-less path
-- enumerators (playbook overlays, the path-metadata endpoint) that have no
-- version/journey context. It is written on INSERT only; PATCH no longer touches
-- it, so it may lag the latest version's kind by design. Authoritative,
-- routing-driving kind = rev.value_slots.value_kind_id.
-- ============================================================================

ALTER TABLE rev.value_slots
    ADD COLUMN value_kind_id SMALLINT REFERENCES rev.value_kinds (id);

-- Backfill every existing version from its path's kind (each value_slots row has
-- exactly one versioned_object -> one path -> one value_slot_path_details row).
UPDATE rev.value_slots vs
SET value_kind_id = vspd.value_kind_id
FROM rev.versioned_objects vo
JOIN rev.value_slot_path_details vspd ON vspd.path_id = vo.path_id
WHERE vo.id = vs.versioned_object_id;

ALTER TABLE rev.value_slots
    ALTER COLUMN value_kind_id SET NOT NULL;

CREATE INDEX value_slots_value_kind_idx ON rev.value_slots (value_kind_id);
