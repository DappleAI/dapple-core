-- Insert guide embedding locale associations.
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],       -- embedding_id[]
        $2::SMALLINT[]    -- locale_id[]
    ) WITH ORDINALITY AS o(embedding_id, locale_id, ord)
),
new_locales AS (
    INSERT INTO rev.guide_embedding_locales (id, embedding_id, locale_id)
    SELECT uuidv7(), args.embedding_id, args.locale_id
    FROM args
    ORDER BY args.ord
    ON CONFLICT (embedding_id, locale_id) DO NOTHING
    RETURNING id, embedding_id, locale_id, created_at
)
SELECT
    nl.id,
    nl.embedding_id,
    nl.locale_id,
    nl.created_at
FROM new_locales nl
ORDER BY nl.id;
