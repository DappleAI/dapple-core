-- Migration 1: Schemas, extensions, and utility functions
-- PG18 native uuidv7() — no custom function needed
-- pgcrypto not needed — PG18 gen_random_uuid() is built-in

CREATE EXTENSION IF NOT EXISTS hstore;

CREATE SCHEMA IF NOT EXISTS rev;
CREATE SCHEMA IF NOT EXISTS auth;
CREATE SCHEMA IF NOT EXISTS web;

-- Slugify function for generating URL-safe slugs
CREATE OR REPLACE FUNCTION rev.slugify(input TEXT) RETURNS TEXT AS $$
BEGIN
    RETURN trim(BOTH '-' FROM
        regexp_replace(
            regexp_replace(
                lower(input),
                '[^a-z0-9\s_-]', '', 'g'
            ),
            '\s+', '-', 'g'
        )
    );
END;
$$ LANGUAGE plpgsql IMMUTABLE;
