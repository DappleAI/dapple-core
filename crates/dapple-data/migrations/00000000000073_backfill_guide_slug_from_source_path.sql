-- Backfill rev.guide_path_details.slug to equal the source_path directory
-- basename -- the "directory = slug" rule now enforced at ingest by
-- insert_guides.sql (rev.slugify(basename(source_path)), title fallback).
--
-- Historically the slug was slugify(title), so a guide whose title did not
-- reduce to its folder ended up at the wrong URL (e.g. the folder
-- 'learn-from-gaming-vod' got slug 'dapple-learns-from-a-gaming-vod' from the
-- title 'Dapple Learns From a Gaming VOD'). This one-time backfill realigns
-- every existing guide so the invariant holds for past rows too; new ingests
-- already satisfy it.
--
-- Collision-safe by construction. slug is UNIQUE per namespace
-- (guide_path_details_slug_namespace_key). Where several paths in one namespace
-- share a source_path basename (e.g. two paths created for the same folder
-- under different titles), only the NEWEST path takes the clean slug, and only
-- when that slug is not already occupied by another path in the namespace.
-- Paths that don't win keep their existing slug. Guides with no source_path are
-- untouched (their slug stays title-derived, matching the ingest fallback).
-- Idempotent: once slugs match, re-running is a no-op.

WITH latest_src AS (
    -- The latest guide version's source_path per path, with the target slug.
    SELECT DISTINCT ON (gpd.path_id)
        gpd.path_id,
        gpd.slug                 AS current_slug,
        gpd.namespace_company_id AS ns,
        rev.slugify(regexp_replace(NULLIF(btrim(g.source_path), ''), '^.*/', '')) AS derived_slug,
        g.created_at             AS latest_at
    FROM rev.guide_path_details gpd
    JOIN rev.object_paths op      ON op.id = gpd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.guides g             ON g.versioned_object_id = vo.id
    WHERE COALESCE(btrim(g.source_path), '') <> ''
    ORDER BY gpd.path_id, g.created_at DESC
),
candidates AS (
    SELECT *
    FROM latest_src
    WHERE derived_slug IS NOT NULL
      AND derived_slug <> ''
      AND derived_slug <> current_slug
),
ranked AS (
    -- Newest path wins each (namespace, derived_slug) contest.
    SELECT c.*,
           row_number() OVER (
               PARTITION BY c.ns, c.derived_slug
               ORDER BY c.latest_at DESC, c.path_id DESC
           ) AS rn
    FROM candidates c
),
winners AS (
    SELECT r.path_id, r.derived_slug
    FROM ranked r
    WHERE r.rn = 1
      -- Skip when the target slug is already held by any path in the namespace,
      -- so the UPDATE can never violate the per-namespace slug UNIQUE.
      AND NOT EXISTS (
          SELECT 1
          FROM rev.guide_path_details x
          WHERE x.namespace_company_id = r.ns
            AND x.slug = r.derived_slug
      )
)
UPDATE rev.guide_path_details gpd
SET slug = w.derived_slug
FROM winners w
WHERE gpd.path_id = w.path_id;
