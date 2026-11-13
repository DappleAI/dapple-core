-- Migration 42: Unicode-aware slugify + backfill existing path-details rows.
--
-- The pre-migration `rev.slugify()` stripped every character outside ASCII
-- `[a-z0-9\s_-]`, silently deleting accented Latin characters: "Como Lida
-- com Alergênios" → "como-lida-com-alergnios". That's lossy for no
-- structural reason — URLs accept Unicode (percent-encoded in transport,
-- displayed natively in browsers) and POSIX filenames accept anything
-- except `/` and NUL. Swap the strip class to `[^[:alnum:]\s_-]` so the
-- function preserves Unicode letters/digits.
--
-- The Rust mirrors (`dapple_data::slugify`, `dapple_sdk::resource::upsert::
-- slugify`) move to `char::is_alphanumeric()` in the same change. SQL and
-- Rust must agree byte-for-byte or slug-based lookups divide; the unit-test
-- fixtures pin parity.
--
-- Backfill: changing the function without rewriting stored slugs would
-- reproduce the acme outage in reverse — the next re-ingest of any
-- accented guide computes the new slug, misses the existing row's old
-- (stripped) slug, falls through to INSERT, and trips the
-- `(name, namespace_company_id)` UNIQUE constraint. Backfill is safe to
-- run inside the function-replacement transaction: the new function
-- preserves *more* information than the old one, so any pair of names
-- that produced distinct old-slugs still produces distinct new-slugs.

CREATE OR REPLACE FUNCTION rev.slugify(input TEXT) RETURNS TEXT AS $$
BEGIN
    RETURN trim(BOTH '-' FROM
        regexp_replace(
            regexp_replace(
                lower(input),
                '[^[:alnum:]\s_-]', '', 'g'
            ),
            '\s+', '-', 'g'
        )
    );
END;
$$ LANGUAGE plpgsql IMMUTABLE;

-- Backfill. Only seven `*_path_details` tables carry a `slug` column; the
-- remaining nine path-details tables have only `name` (no derived slug) and
-- need no backfill. `guide_path_details` derives from `title`; the rest
-- derive from `name`.

UPDATE rev.guide_path_details
SET    slug = rev.slugify(title)
WHERE  slug IS DISTINCT FROM rev.slugify(title);

UPDATE rev.category_path_details
SET    slug = rev.slugify(name)
WHERE  slug IS DISTINCT FROM rev.slugify(name);

UPDATE rev.hat_path_details
SET    slug = rev.slugify(name)
WHERE  slug IS DISTINCT FROM rev.slugify(name);

UPDATE rev.playbook_path_details
SET    slug = rev.slugify(name)
WHERE  slug IS DISTINCT FROM rev.slugify(name);

UPDATE rev.stage_path_details
SET    slug = rev.slugify(name)
WHERE  slug IS DISTINCT FROM rev.slugify(name);

UPDATE rev.step_path_details
SET    slug = rev.slugify(name)
WHERE  slug IS DISTINCT FROM rev.slugify(name);

UPDATE rev.task_path_details
SET    slug = rev.slugify(name)
WHERE  slug IS DISTINCT FROM rev.slugify(name);
