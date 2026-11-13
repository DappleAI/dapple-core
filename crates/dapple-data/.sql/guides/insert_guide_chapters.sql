-- Insert guide chapters as owned dependents of a guide.
-- Chapters are inserted in topological order (parents before children).
-- The parent_chapter_id is NULL for top-level chapters.
-- Caller pre-generates UUIDs so parent_chapter_id can reference sibling IDs.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],    -- id[] (pre-generated UUIDv7)
        $2::UUID[],    -- guide_id[]
        $3::UUID[],    -- parent_chapter_id[] (NULL for top-level)
        $4::TEXT[],     -- title[]
        $5::TEXT[],     -- source_file[]
        $6::TEXT[]      -- subtitle[]
    ) WITH ORDINALITY AS o(
        id,
        guide_id,
        parent_chapter_id,
        title,
        source_file,
        subtitle,
        ord
    )
),
new_chapters AS (
    INSERT INTO rev.guide_chapters (id, guide_id, parent_chapter_id, title, source_file, subtitle)
    SELECT
        args.id,
        args.guide_id,
        args.parent_chapter_id,
        args.title,
        args.source_file,
        args.subtitle
    FROM args
    ORDER BY args.ord
    RETURNING id, guide_id, parent_chapter_id, title, subtitle, source_file, created_at
)
SELECT * FROM new_chapters ORDER BY id;
