-- Migration 3: Versioning infrastructure
-- object_paths, versioned_objects, path_details, deprecation_events

-- ============================================================================
-- rev.object_paths — Central registry of all versioned object paths
-- ============================================================================
CREATE TABLE rev.object_paths (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    entity_type_id SMALLINT NOT NULL REFERENCES rev.entity_types (id),
    scope TEXT,
    ref_key TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX object_paths_entity_type_idx ON rev.object_paths (entity_type_id);
CREATE INDEX object_paths_scope_idx ON rev.object_paths (scope) WHERE scope IS NOT NULL;
CREATE UNIQUE INDEX object_paths_ref_key_unique_idx ON rev.object_paths (entity_type_id, scope, ref_key) WHERE ref_key IS NOT NULL;

-- ============================================================================
-- rev.versioned_objects — Central registry of all versioned object instances
-- ============================================================================
CREATE TABLE rev.versioned_objects (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    path_id UUID NOT NULL REFERENCES rev.object_paths (id),
    version_major INTEGER NOT NULL DEFAULT 0,
    version_minor INTEGER NOT NULL DEFAULT 0,
    version_patch INTEGER NOT NULL DEFAULT 1,
    deprecation_level_id SMALLINT REFERENCES rev.deprecation_levels (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (path_id, version_major, version_minor, version_patch)
);

CREATE INDEX versioned_objects_path_version_desc_idx
    ON rev.versioned_objects (path_id, version_major DESC, version_minor DESC, version_patch DESC);
CREATE INDEX versioned_objects_deprecation_idx ON rev.versioned_objects (deprecation_level_id)
    WHERE deprecation_level_id IS NOT NULL;

-- ============================================================================
-- Path detail tables — immutable metadata for each path
-- ============================================================================

CREATE TABLE rev.task_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.step_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.stage_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.playbook_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.prompt_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.fluent_message_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    description TEXT
);

CREATE TABLE rev.category_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.hat_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.brief_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.brief_group_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.brief_pack_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.value_slot_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE,
    value_kind_id SMALLINT NOT NULL REFERENCES rev.value_kinds (id)
);

CREATE INDEX value_slot_path_details_value_kind_idx ON rev.value_slot_path_details (value_kind_id);

CREATE TABLE rev.asset_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    asset_kind_id SMALLINT NOT NULL REFERENCES rev.asset_kinds (id)
);

CREATE INDEX asset_path_details_asset_kind_idx ON rev.asset_path_details (asset_kind_id);

CREATE TABLE rev.directive_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE rev.inference_profile_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    name TEXT NOT NULL
);

CREATE TABLE rev.guide_path_details (
    path_id UUID PRIMARY KEY REFERENCES rev.object_paths (id),
    title TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE,
    authoring_locale TEXT
);
