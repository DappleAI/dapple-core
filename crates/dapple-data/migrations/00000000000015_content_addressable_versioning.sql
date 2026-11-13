-- Migration 15: Content-addressable versioning
-- Replace semver (version_major/minor/patch) with content_hash (SHA-1 hex)

-- ============================================================================
-- rev.versioned_objects — Replace version columns with content_hash
-- ============================================================================

-- 1. Add content_hash column (nullable first so we can populate existing rows)
ALTER TABLE rev.versioned_objects ADD COLUMN content_hash TEXT;

-- 2. Populate existing rows with a hash derived from their version tuple
--    This creates a deterministic unique hash for each existing version
UPDATE rev.versioned_objects
SET content_hash = encode(
    sha256(
        convert_to(
            path_id::TEXT || ':' || version_major::TEXT || '.' || version_minor::TEXT || '.' || version_patch::TEXT,
            'UTF8'
        )
    ),
    'hex'
);

-- 3. Truncate to 40 chars (SHA-1 length) for consistency
UPDATE rev.versioned_objects
SET content_hash = LEFT(content_hash, 40);

-- 4. Add NOT NULL constraint and CHECK
ALTER TABLE rev.versioned_objects ALTER COLUMN content_hash SET NOT NULL;
ALTER TABLE rev.versioned_objects ADD CONSTRAINT versioned_objects_content_hash_format
    CHECK (content_hash ~ '^[0-9a-f]{40}$');

-- 5. Drop old unique constraint and indexes
ALTER TABLE rev.versioned_objects DROP CONSTRAINT versioned_objects_path_id_version_major_version_minor_versi_key;
DROP INDEX IF EXISTS rev.versioned_objects_path_version_desc_idx;

-- 6. Drop version columns
ALTER TABLE rev.versioned_objects DROP COLUMN version_major;
ALTER TABLE rev.versioned_objects DROP COLUMN version_minor;
ALTER TABLE rev.versioned_objects DROP COLUMN version_patch;

-- 7. Add new unique constraint and indexes
ALTER TABLE rev.versioned_objects ADD CONSTRAINT versioned_objects_path_content_hash_unique
    UNIQUE (path_id, content_hash);

CREATE INDEX versioned_objects_path_created_desc_idx
    ON rev.versioned_objects (path_id, created_at DESC);
CREATE INDEX versioned_objects_content_hash_idx
    ON rev.versioned_objects (content_hash);

-- ============================================================================
-- rev.canonical_release_pins — Replace version columns with content_hash
-- ============================================================================

ALTER TABLE rev.canonical_release_pins ADD COLUMN content_hash TEXT;

UPDATE rev.canonical_release_pins
SET content_hash = LEFT(
    encode(
        sha256(
            convert_to(
                COALESCE(versioned_object_id::TEXT, 'null') || ':' ||
                version_major::TEXT || '.' || version_minor::TEXT || '.' || version_patch::TEXT,
                'UTF8'
            )
        ),
        'hex'
    ),
    40
);

ALTER TABLE rev.canonical_release_pins ALTER COLUMN content_hash SET NOT NULL;
ALTER TABLE rev.canonical_release_pins ADD CONSTRAINT canonical_release_pins_content_hash_format
    CHECK (content_hash ~ '^[0-9a-f]{40}$');

ALTER TABLE rev.canonical_release_pins DROP COLUMN version_major;
ALTER TABLE rev.canonical_release_pins DROP COLUMN version_minor;
ALTER TABLE rev.canonical_release_pins DROP COLUMN version_patch;

-- ============================================================================
-- rev.canonical_variant_pins — Replace version columns with content_hash
-- ============================================================================

ALTER TABLE rev.canonical_variant_pins ADD COLUMN content_hash TEXT;

UPDATE rev.canonical_variant_pins
SET content_hash = LEFT(
    encode(
        sha256(
            convert_to(
                COALESCE(versioned_object_id::TEXT, 'null') || ':' ||
                version_major::TEXT || '.' || version_minor::TEXT || '.' || version_patch::TEXT,
                'UTF8'
            )
        ),
        'hex'
    ),
    40
);

ALTER TABLE rev.canonical_variant_pins ALTER COLUMN content_hash SET NOT NULL;
ALTER TABLE rev.canonical_variant_pins ADD CONSTRAINT canonical_variant_pins_content_hash_format
    CHECK (content_hash ~ '^[0-9a-f]{40}$');

ALTER TABLE rev.canonical_variant_pins DROP COLUMN version_major;
ALTER TABLE rev.canonical_variant_pins DROP COLUMN version_minor;
ALTER TABLE rev.canonical_variant_pins DROP COLUMN version_patch;
