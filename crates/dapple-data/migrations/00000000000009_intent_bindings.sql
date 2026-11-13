-- Migration 9: Dispatch intent bindings + typed items

-- ============================================================================
-- rev.dispatch_intent_bindings — Typed canonical intent
-- ============================================================================
CREATE TABLE rev.dispatch_intent_bindings (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_id UUID NOT NULL REFERENCES rev.dispatches (id),
    value_slot_id UUID NOT NULL REFERENCES rev.value_slots (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (dispatch_id, value_slot_id)
);

CREATE INDEX dispatch_intent_bindings_dispatch_idx ON rev.dispatch_intent_bindings (dispatch_id);
CREATE INDEX dispatch_intent_bindings_value_slot_idx ON rev.dispatch_intent_bindings (value_slot_id);

-- ============================================================================
-- Typed item tables — one per value_kind
-- ============================================================================

CREATE TABLE rev.dispatch_intent_binding_text_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL REFERENCES rev.dispatch_intent_bindings (id),
    value TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX dispatch_intent_binding_text_items_binding_idx ON rev.dispatch_intent_binding_text_items (dispatch_intent_binding_id);

CREATE TABLE rev.dispatch_intent_binding_int_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL REFERENCES rev.dispatch_intent_bindings (id),
    value BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX dispatch_intent_binding_int_items_binding_idx ON rev.dispatch_intent_binding_int_items (dispatch_intent_binding_id);

CREATE TABLE rev.dispatch_intent_binding_number_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL REFERENCES rev.dispatch_intent_bindings (id),
    value NUMERIC NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX dispatch_intent_binding_number_items_binding_idx ON rev.dispatch_intent_binding_number_items (dispatch_intent_binding_id);

CREATE TABLE rev.dispatch_intent_binding_bool_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL REFERENCES rev.dispatch_intent_bindings (id),
    value BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX dispatch_intent_binding_bool_items_binding_idx ON rev.dispatch_intent_binding_bool_items (dispatch_intent_binding_id);

CREATE TABLE rev.dispatch_intent_binding_json_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL REFERENCES rev.dispatch_intent_bindings (id),
    value JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX dispatch_intent_binding_json_items_binding_idx ON rev.dispatch_intent_binding_json_items (dispatch_intent_binding_id);

CREATE TABLE rev.dispatch_intent_binding_asset_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL REFERENCES rev.dispatch_intent_bindings (id),
    asset_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (dispatch_intent_binding_id, asset_id)
);

CREATE INDEX dispatch_intent_binding_asset_items_binding_idx ON rev.dispatch_intent_binding_asset_items (dispatch_intent_binding_id);

-- ============================================================================
-- rev.dispatch_intent_binding_value_meta — Per-item UI specifications
-- ============================================================================
CREATE TABLE rev.dispatch_intent_binding_value_meta (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    dispatch_intent_binding_id UUID NOT NULL REFERENCES rev.dispatch_intent_bindings (id),
    ui_spec JSONB NOT NULL,
    producer TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX dispatch_intent_binding_value_meta_binding_idx ON rev.dispatch_intent_binding_value_meta (dispatch_intent_binding_id);
