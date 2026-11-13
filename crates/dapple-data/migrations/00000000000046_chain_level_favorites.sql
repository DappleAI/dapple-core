-- =============================================================================
-- Chain-level favorites: replace web.user_favorite_journeys
-- =============================================================================
-- Migration 7 introduced `web.user_favorite_journeys` keyed on a single
-- `journey_id`. That model breaks every time a user advances ("next chapter")
-- or forks ("start over from here"): the journey id the favorite is anchored
-- to becomes an internal chain node or an abandoned sibling, and the user's
-- "this essay is a favorite" intent visibly snaps. The fix is to treat the
-- chain — not the journey — as the favorited entity. The chain root's UUIDv7
-- IS the chain identifier (we resolve it on demand via a recursive walk of
-- `predecessor_journey_id`; we do NOT denormalize a `chain_root_id` column
-- onto rev.journeys in this migration — that's a future team decision).
--
-- This migration:
--   1. Creates web.user_favorite_journey_chains + _removed (event-sourced,
--      mirrors the old shape).
--   2. Backfills active favorites (those with no row in the old _removed
--      table) by resolving each `journey_id` to its chain root via a
--      recursive CTE, dedup'd per (user_id, chain_root_journey_id).
--   3. Drops the old tables. No parallel-run window.
-- =============================================================================

CREATE TABLE web.user_favorite_journey_chains (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES rev.user_details (user_id),
    chain_root_journey_id UUID NOT NULL REFERENCES rev.journeys (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX user_favorite_journey_chains_lookup
    ON web.user_favorite_journey_chains (user_id, chain_root_journey_id, id DESC);

CREATE TABLE web.user_favorite_journey_chains_removed (
    user_favorite_journey_chains_id UUID PRIMARY KEY
        REFERENCES web.user_favorite_journey_chains (id),
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- -----------------------------------------------------------------------------
-- Backfill from web.user_favorite_journeys
-- -----------------------------------------------------------------------------
-- Resolve each active (not-removed) favorite's journey to its chain root by
-- walking `predecessor_journey_id` until NULL. Dedup per
-- (user_id, chain_root_journey_id) keeping the earliest `created_at` (the
-- user's original intent point).

WITH RECURSIVE active_old AS (
    SELECT f.id AS favorite_id, f.user_id, f.journey_id, f.created_at
    FROM web.user_favorite_journeys f
    LEFT JOIN web.user_favorite_journeys_removed r
        ON r.user_favorite_journeys_id = f.id
    WHERE r.user_favorite_journeys_id IS NULL
),
chain_walk AS (
    SELECT
        a.favorite_id,
        a.user_id,
        a.created_at,
        j.id,
        j.predecessor_journey_id
    FROM active_old a
    JOIN rev.journeys j ON j.id = a.journey_id
    UNION ALL
    SELECT
        cw.favorite_id,
        cw.user_id,
        cw.created_at,
        j.id,
        j.predecessor_journey_id
    FROM chain_walk cw
    JOIN rev.journeys j ON j.id = cw.predecessor_journey_id
    WHERE cw.predecessor_journey_id IS NOT NULL
),
resolved AS (
    SELECT
        cw.user_id,
        cw.id AS chain_root_journey_id,
        cw.created_at
    FROM chain_walk cw
    WHERE cw.predecessor_journey_id IS NULL
),
deduped AS (
    SELECT DISTINCT ON (user_id, chain_root_journey_id)
        user_id,
        chain_root_journey_id,
        created_at
    FROM resolved
    ORDER BY user_id, chain_root_journey_id, created_at ASC
)
INSERT INTO web.user_favorite_journey_chains (user_id, chain_root_journey_id, created_at)
SELECT user_id, chain_root_journey_id, created_at
FROM deduped;

-- -----------------------------------------------------------------------------
-- Drop the old tables. Clean cut — no parallel-run window. The historical
-- _removed event log is intentionally not migrated; the new table starts
-- with a fresh event history. Current state is preserved by the backfill.
-- -----------------------------------------------------------------------------

DROP TABLE web.user_favorite_journeys_removed;
DROP TABLE web.user_favorite_journeys;
