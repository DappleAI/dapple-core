-- =============================================================================
-- Path layer: namespace_company_id + flat scope; drop vestigial ref_key
-- =============================================================================
-- Adds the namespace attribution layer to rev.object_paths and
-- rev.canonical_release_pins so every path is owned by a rev.companies row
-- and every canonical pin can be filtered by namespace at the SQL layer.
--
-- Drops the long-vestigial rev.object_paths.ref_key column. The SQL inventory
-- at plan time confirmed:
--   - 16 of 17 entity types ignored ref_key (NULL across 13,116 rows)
--   - The 2 remaining shared_content rows held orphaned values
--     ('common/common', 'internal/internal') with zero versioned_objects and
--     an empty rev.guide_shared_refs
--   - The shared-content code paths that referenced ref_key are removed in
--     the same change set as this migration
-- The shared-content identifier system is deferred to V2+; when it ships it
-- will use a proper rev.shared_content_path_details table mirroring the
-- other 16 path-detail tables.
--
-- Real path identifiers (rev.guides.source_path,
-- rev.{entity}_path_details.slug) are NOT touched by this migration.
--
-- Hostile-collision protection across namespaces is provided by the auth
-- layer (Pattern B router separation: external tokens can only INSERT under
-- their bound namespace_company_id) — not by a unique index on object_paths.
--
-- See docs/plans/external-api/00-architecture.md §3 Decision 3 and §4.3.
-- =============================================================================

-- ---------------------------------------------------------------------------
-- 1. Drop the unique index that depended on ref_key
-- ---------------------------------------------------------------------------

DROP INDEX rev.object_paths_ref_key_unique_idx;

-- ---------------------------------------------------------------------------
-- 2. Delete orphan shared_content rows
-- ---------------------------------------------------------------------------
-- Verified at plan time: zero versioned_objects reference these rows and
-- rev.guide_shared_refs is empty, so they have no live dependents.

DELETE FROM rev.object_paths
WHERE entity_type_id = (SELECT id FROM rev.entity_types WHERE key = 'shared_content')
  AND ref_key IS NOT NULL;

-- ---------------------------------------------------------------------------
-- 3. Drop the vestigial ref_key column on object_paths
-- ---------------------------------------------------------------------------
-- Note: rev.canonical_release_pins.ref_key is NOT dropped — that column
-- carries source_path-style strings and is the key used to match
-- canonical pins against rev.guides.source_path.

ALTER TABLE rev.object_paths DROP COLUMN ref_key;

-- ---------------------------------------------------------------------------
-- 4. Add namespace_company_id (nullable for backfill, then tightened)
-- ---------------------------------------------------------------------------

ALTER TABLE rev.object_paths
    ADD COLUMN namespace_company_id UUID NULL REFERENCES rev.companies (id);

ALTER TABLE rev.canonical_release_pins
    ADD COLUMN namespace_company_id UUID NULL REFERENCES rev.companies (id);

-- ---------------------------------------------------------------------------
-- 5. Backfill: every existing row is dapple-owned
-- ---------------------------------------------------------------------------
-- The dapple UUID is hardcoded for clarity; it matches the seed in
-- migration 31. Using a literal here (rather than a subquery) makes the
-- migration's intent obvious in audit logs.

UPDATE rev.object_paths
SET namespace_company_id = '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID;

UPDATE rev.canonical_release_pins
SET namespace_company_id = '019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID;

-- ---------------------------------------------------------------------------
-- 6. Backfill scope on object_paths: NULL → 'public' (regular paths)
-- ---------------------------------------------------------------------------
-- Existing 'common'/'internal' values came from shared-content rows that
-- were deleted in step 2; this UPDATE handles the remaining NULL rows.

UPDATE rev.object_paths SET scope = 'public' WHERE scope IS NULL;

-- canonical_release_pins.scope is empty in current envs; nothing to backfill.

-- ---------------------------------------------------------------------------
-- 7. Tighten constraints
-- ---------------------------------------------------------------------------

ALTER TABLE rev.object_paths
    ALTER COLUMN namespace_company_id SET NOT NULL,
    ALTER COLUMN scope SET NOT NULL,
    ADD CONSTRAINT object_paths_scope_format
        CHECK (scope IN ('public', 'common', 'internal', 'external'));

ALTER TABLE rev.canonical_release_pins
    ALTER COLUMN namespace_company_id SET NOT NULL,
    ADD CONSTRAINT canonical_release_pins_scope_format
        CHECK (scope IN ('public', 'common', 'internal', 'external'));

-- ---------------------------------------------------------------------------
-- 8. Indexes
-- ---------------------------------------------------------------------------

CREATE INDEX object_paths_namespace_scope_idx
    ON rev.object_paths (namespace_company_id, scope);

DROP INDEX IF EXISTS rev.canonical_release_pins_scope_env_idx;
CREATE INDEX canonical_release_pins_namespace_scope_env_idx
    ON rev.canonical_release_pins (namespace_company_id, scope, environment, object_type, ref_key);

-- ---------------------------------------------------------------------------
-- 9. Reseed rev.shared_scopes with the four V1 tier values
-- ---------------------------------------------------------------------------
-- The legacy lookup held only ('common', 'internal'). Reseed with the
-- complete V1 tier set; future 'partner' lands when partner support ships.

DELETE FROM rev.shared_scopes;
INSERT INTO rev.shared_scopes (key, description) VALUES
    ('public',   'Dapple-only tier: broadly visible content within the dapple namespace'),
    ('common',   'Any-namespace tier: shared content available across guides in a namespace'),
    ('internal', 'Dapple-only tier: dapple-private content'),
    ('external', 'Customer-only tier: customer-private content hosted by dapple (mirror of internal on the customer side)');
