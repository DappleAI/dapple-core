-- canonical_cleanup: Eliminate all UPDATEs and DELETEs from runtime code.
--
-- Category 1: Remove deprecation_level_id from rev.versioned_objects.
-- Deprecation level is now derived at query time from the latest
-- rev.deprecation_events row via a correlated subquery.
--
-- Category 4: Drop the UNIQUE constraint on rev.guide_renders that
-- enables ON CONFLICT DO UPDATE (the insert function was dead code
-- and has been removed).
--
-- Category 5: Add generation-based embedding versioning to replace
-- DELETE-then-reinsert pattern for guide content embeddings.

-- ============================================================================
-- Category 1: Deprecation system — remove denormalized column
-- ============================================================================

-- 1a. Add covering index for fast derivation of current deprecation level.
-- The subquery (SELECT de.to_level_id FROM rev.deprecation_events de
-- WHERE de.versioned_object_id = vo.id ORDER BY de.created_at DESC LIMIT 1)
-- becomes an index-only scan with this covering index.
CREATE INDEX IF NOT EXISTS deprecation_events_latest_idx
    ON rev.deprecation_events (versioned_object_id, created_at DESC)
    INCLUDE (to_level_id);

-- 1b. Drop the index on the column we're about to remove.
DROP INDEX IF EXISTS rev.versioned_objects_deprecation_idx;

-- 1c. Drop the denormalized column.
ALTER TABLE rev.versioned_objects DROP COLUMN IF EXISTS deprecation_level_id;

-- ============================================================================
-- Category 4: Guide renders — remove upsert constraint (dead code)
-- ============================================================================

-- The insert_guide_render.sql ON CONFLICT clause targeted this constraint.
-- That SQL file and its Rust function have been deleted. Drop the constraint
-- so no future code can accidentally re-introduce the upsert.
ALTER TABLE rev.guide_renders
    DROP CONSTRAINT IF EXISTS guide_renders_guide_id_render_profile_id_key;

-- ============================================================================
-- Category 5: Embedding generations — append-only embedding versioning
-- ============================================================================

-- 5a. Create the generation tracking table.
CREATE TABLE rev.guide_embedding_generations (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX guide_embedding_generations_latest_idx
    ON rev.guide_embedding_generations (guide_id, created_at DESC);

-- 5b. Backfill: create one generation per existing guide_id.
INSERT INTO rev.guide_embedding_generations (guide_id)
SELECT DISTINCT guide_id FROM rev.guide_content_embeddings;

-- 5c. Add generation_id to embeddings table and backfill.
ALTER TABLE rev.guide_content_embeddings
    ADD COLUMN generation_id UUID;

-- Backfill existing embeddings with the generation from step 5b.
UPDATE rev.guide_content_embeddings gce
SET generation_id = (
    SELECT g.id FROM rev.guide_embedding_generations g
    WHERE g.guide_id = gce.guide_id
    LIMIT 1
);

-- Make generation_id NOT NULL and add FK.
-- Only add NOT NULL if there are actually rows to constrain
-- (if table is empty, just set NOT NULL directly).
ALTER TABLE rev.guide_content_embeddings
    ALTER COLUMN generation_id SET NOT NULL;

ALTER TABLE rev.guide_content_embeddings
    ADD CONSTRAINT guide_content_embeddings_generation_id_fkey
        FOREIGN KEY (generation_id) REFERENCES rev.guide_embedding_generations(id);

CREATE INDEX guide_content_embeddings_generation_idx
    ON rev.guide_content_embeddings (generation_id);
