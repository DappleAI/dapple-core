WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],            $2::UUID[],            $3::UUID[],            $4::TEXT[],             $5::TEXT[],             $6::TEXT[]          ) WITH ORDINALITY AS o(
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
