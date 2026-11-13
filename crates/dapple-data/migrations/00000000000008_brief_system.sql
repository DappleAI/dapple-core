-- Migration 8: Brief system
-- briefs, brief_units (subtypes), brief_groups, brief_packs, journey_brief_packs

-- ============================================================================
-- rev.briefs — Versioned interview/data-gathering definitions
-- ============================================================================
CREATE TABLE rev.briefs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    title TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX briefs_versioned_object_idx ON rev.briefs (versioned_object_id);

-- ============================================================================
-- rev.brief_slots — Typed reference to value_slots
-- ============================================================================
CREATE TABLE rev.brief_slots (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_id UUID NOT NULL REFERENCES rev.briefs (id),
    slot_id TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (brief_id, slot_id)
);

CREATE INDEX brief_slots_brief_idx ON rev.brief_slots (brief_id);

-- ============================================================================
-- rev.brief_units — Discriminated by subtype tables
-- ============================================================================
CREATE TABLE rev.brief_units (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_id UUID NOT NULL REFERENCES rev.briefs (id),
    unit_key TEXT NOT NULL,
    layout JSONB NOT NULL DEFAULT '{}'::jsonb,
    requires JSONB NOT NULL DEFAULT '{}'::jsonb,
    extensions JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (brief_id, unit_key)
);

CREATE INDEX brief_units_brief_idx ON rev.brief_units (brief_id);

-- ============================================================================
-- rev.brief_outputs — Brief-level output mappings
-- ============================================================================
CREATE TABLE rev.brief_outputs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_id UUID NOT NULL REFERENCES rev.briefs (id),
    value_slot_id UUID NOT NULL REFERENCES rev.value_slots (id),
    unit_key TEXT NOT NULL,
    source_kind_id SMALLINT NOT NULL REFERENCES rev.brief_output_source_kinds (id),
    source_path TEXT NOT NULL,
    transform JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (brief_id, value_slot_id)
);

CREATE INDEX brief_outputs_brief_idx ON rev.brief_outputs (brief_id);
CREATE INDEX brief_outputs_value_slot_idx ON rev.brief_outputs (value_slot_id);
CREATE INDEX brief_outputs_source_kind_idx ON rev.brief_outputs (source_kind_id);

-- ============================================================================
-- Brief unit subtype tables
-- ============================================================================

-- Markdown subtype
CREATE TABLE rev.brief_unit_markdown (
    brief_unit_id UUID PRIMARY KEY REFERENCES rev.brief_units (id),
    markdown_text TEXT,
    fluent_message_id UUID,
    format TEXT,
    deliverable_spec JSONB NOT NULL DEFAULT '{}'::jsonb
);

-- Notice subtype
CREATE TABLE rev.brief_unit_notice (
    brief_unit_id UUID PRIMARY KEY REFERENCES rev.brief_units (id),
    level TEXT NOT NULL,
    title_text TEXT,
    title_fluent_message_id UUID,
    detail_text TEXT,
    detail_fluent_message_id UUID
);

-- Input subtype
CREATE TABLE rev.brief_unit_input (
    brief_unit_id UUID PRIMARY KEY REFERENCES rev.brief_units (id),
    control_type_id SMALLINT NOT NULL REFERENCES rev.brief_input_control_types (id),
    label_text TEXT,
    label_fluent_message_id UUID,
    help_text TEXT,
    help_fluent_message_id UUID,
    placeholder_text TEXT,
    placeholder_fluent_message_id UUID,
    control_spec JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX brief_unit_input_control_type_idx ON rev.brief_unit_input (control_type_id);

-- Input choices
CREATE TABLE rev.brief_unit_input_choices (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_unit_id UUID NOT NULL REFERENCES rev.brief_units (id),
    label_text TEXT,
    label_fluent_message_id UUID,
    value_text TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_input_choices_unit_idx ON rev.brief_unit_input_choices (brief_unit_id);

-- ============================================================================
-- Brief unit media subtype
-- ============================================================================

CREATE TABLE rev.brief_unit_media (
    brief_unit_id UUID PRIMARY KEY REFERENCES rev.brief_units (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE rev.brief_unit_media_items (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_unit_id UUID NOT NULL REFERENCES rev.brief_unit_media (brief_unit_id),
    media_kind_id SMALLINT NOT NULL REFERENCES rev.brief_media_kinds (id),
    source_type_id SMALLINT NOT NULL REFERENCES rev.brief_media_source_types (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_items_unit_idx ON rev.brief_unit_media_items (brief_unit_id);
CREATE INDEX brief_unit_media_items_kind_idx ON rev.brief_unit_media_items (media_kind_id);
CREATE INDEX brief_unit_media_items_source_type_idx ON rev.brief_unit_media_items (source_type_id);

-- Media item source tables (polymorphic)
CREATE TABLE rev.brief_unit_media_item_asset_sources (
    media_item_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_items (id),
    asset_id UUID NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_asset_sources_asset_idx ON rev.brief_unit_media_item_asset_sources (asset_id);

CREATE TABLE rev.brief_unit_media_item_url_sources (
    media_item_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_items (id),
    url TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE rev.brief_unit_media_item_value_slot_sources (
    media_item_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_items (id),
    value_slot_id UUID NOT NULL REFERENCES rev.value_slots (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_value_slot_sources_slot_idx ON rev.brief_unit_media_item_value_slot_sources (value_slot_id);

-- Media item poster tables
CREATE TABLE rev.brief_unit_media_item_posters (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    media_item_id UUID NOT NULL UNIQUE REFERENCES rev.brief_unit_media_items (id),
    source_type_id SMALLINT NOT NULL REFERENCES rev.brief_media_source_types (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_posters_item_idx ON rev.brief_unit_media_item_posters (media_item_id);
CREATE INDEX brief_unit_media_item_posters_source_type_idx ON rev.brief_unit_media_item_posters (source_type_id);

CREATE TABLE rev.brief_unit_media_item_poster_asset_sources (
    poster_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_item_posters (id),
    asset_id UUID NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_poster_asset_sources_asset_idx ON rev.brief_unit_media_item_poster_asset_sources (asset_id);

CREATE TABLE rev.brief_unit_media_item_poster_url_sources (
    poster_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_item_posters (id),
    url TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE rev.brief_unit_media_item_poster_value_slot_sources (
    poster_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_item_posters (id),
    value_slot_id UUID NOT NULL REFERENCES rev.value_slots (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_poster_value_slot_sources_slot_idx ON rev.brief_unit_media_item_poster_value_slot_sources (value_slot_id);

-- Media item text overlays (fluent-backed)
CREATE TABLE rev.brief_unit_media_item_alt_texts (
    media_item_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_items (id),
    fluent_message_id UUID NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_alt_texts_fluent_idx ON rev.brief_unit_media_item_alt_texts (fluent_message_id);

CREATE TABLE rev.brief_unit_media_item_captions (
    media_item_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_items (id),
    fluent_message_id UUID NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_captions_fluent_idx ON rev.brief_unit_media_item_captions (fluent_message_id);

CREATE TABLE rev.brief_unit_media_item_titles (
    media_item_id UUID PRIMARY KEY REFERENCES rev.brief_unit_media_items (id),
    fluent_message_id UUID NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_unit_media_item_titles_fluent_idx ON rev.brief_unit_media_item_titles (fluent_message_id);

-- ============================================================================
-- rev.brief_groups — Versioned collections of briefs
-- ============================================================================
CREATE TABLE rev.brief_groups (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    title TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_groups_versioned_object_idx ON rev.brief_groups (versioned_object_id);

-- ============================================================================
-- rev.brief_group_briefs — Ordered association (BriefGroup <-> Brief)
-- ============================================================================
CREATE TABLE rev.brief_group_briefs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_group_id UUID NOT NULL REFERENCES rev.brief_groups (id),
    brief_id UUID NOT NULL REFERENCES rev.briefs (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (brief_group_id, brief_id)
);

CREATE INDEX brief_group_briefs_group_idx ON rev.brief_group_briefs (brief_group_id);
CREATE INDEX brief_group_briefs_brief_idx ON rev.brief_group_briefs (brief_id);

-- ============================================================================
-- rev.brief_packs — Versioned UI/spec bundles
-- ============================================================================
CREATE TABLE rev.brief_packs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    title TEXT NOT NULL,
    description TEXT,
    pack_spec JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX brief_packs_versioned_object_idx ON rev.brief_packs (versioned_object_id);

-- ============================================================================
-- rev.brief_pack_groups — Ordered association (BriefPack <-> BriefGroup)
-- ============================================================================
CREATE TABLE rev.brief_pack_groups (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    brief_pack_id UUID NOT NULL REFERENCES rev.brief_packs (id),
    brief_group_id UUID NOT NULL REFERENCES rev.brief_groups (id),
    ui_overrides JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (brief_pack_id, brief_group_id)
);

CREATE INDEX brief_pack_groups_pack_idx ON rev.brief_pack_groups (brief_pack_id);
CREATE INDEX brief_pack_groups_group_idx ON rev.brief_pack_groups (brief_group_id);

-- ============================================================================
-- Add brief_pack_id to journeys (deferred from migration 7)
-- ============================================================================
ALTER TABLE rev.journeys ADD COLUMN brief_pack_id UUID REFERENCES rev.brief_packs (id);
CREATE INDEX journeys_brief_pack_idx ON rev.journeys (brief_pack_id) WHERE brief_pack_id IS NOT NULL;

-- ============================================================================
-- rev.journey_brief_packs — Associative (Journey <-> BriefPack)
-- ============================================================================
CREATE TABLE rev.journey_brief_packs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    journey_id UUID NOT NULL REFERENCES rev.journeys (id),
    brief_pack_id UUID NOT NULL REFERENCES rev.brief_packs (id),
    notes TEXT,
    extensions JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (journey_id, brief_pack_id)
);

CREATE INDEX journey_brief_packs_journey_idx ON rev.journey_brief_packs (journey_id);
CREATE INDEX journey_brief_packs_brief_pack_idx ON rev.journey_brief_packs (brief_pack_id);
