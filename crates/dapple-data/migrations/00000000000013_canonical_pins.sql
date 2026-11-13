-- Migration 13: Canonical release pins + ledger

-- ============================================================================
-- rev.canonical_release_pins — Append-only version pins
-- ============================================================================
CREATE TABLE rev.canonical_release_pins (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    scope TEXT NOT NULL,
    environment TEXT NOT NULL,
    object_type TEXT NOT NULL,
    ref_key TEXT NOT NULL,
    version_major INTEGER NOT NULL,
    version_minor INTEGER NOT NULL,
    version_patch INTEGER NOT NULL,
    versioned_object_id UUID REFERENCES rev.versioned_objects (id),
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    supersedes_id UUID REFERENCES rev.canonical_release_pins (id)
);

CREATE INDEX canonical_release_pins_scope_env_idx ON rev.canonical_release_pins (scope, environment, object_type, ref_key);
CREATE INDEX canonical_release_pins_versioned_object_idx ON rev.canonical_release_pins (versioned_object_id)
    WHERE versioned_object_id IS NOT NULL;
CREATE INDEX canonical_release_pins_supersedes_idx ON rev.canonical_release_pins (supersedes_id)
    WHERE supersedes_id IS NOT NULL;

-- ============================================================================
-- rev.canonical_variant_pins — Append-only variant pins
-- ============================================================================
CREATE TABLE rev.canonical_variant_pins (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    scope TEXT NOT NULL,
    environment TEXT NOT NULL,
    bucket TEXT NOT NULL,
    object_type TEXT NOT NULL,
    ref_key TEXT NOT NULL,
    version_major INTEGER NOT NULL,
    version_minor INTEGER NOT NULL,
    version_patch INTEGER NOT NULL,
    versioned_object_id UUID REFERENCES rev.versioned_objects (id),
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    supersedes_id UUID REFERENCES rev.canonical_variant_pins (id)
);

CREATE INDEX canonical_variant_pins_scope_env_idx ON rev.canonical_variant_pins (scope, environment, bucket, object_type, ref_key);
CREATE INDEX canonical_variant_pins_versioned_object_idx ON rev.canonical_variant_pins (versioned_object_id)
    WHERE versioned_object_id IS NOT NULL;
CREATE INDEX canonical_variant_pins_supersedes_idx ON rev.canonical_variant_pins (supersedes_id)
    WHERE supersedes_id IS NOT NULL;
