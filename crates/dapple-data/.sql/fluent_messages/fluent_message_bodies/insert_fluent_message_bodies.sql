-- Insert localized bodies for fluent messages
-- Parameters:
--   $1: message_ids (UUID[]) - flattened, one per body
--   $2: locale_tags (TEXT[]) - flattened, one per body
--   $3: bodies (TEXT[]) - flattened, one per body

WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::TEXT[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(message_id, locale_tag, body, ord)
),
-- Get or insert locales
locale_refs AS (
    SELECT DISTINCT locale_tag FROM args
),
inserted_locales AS (
    INSERT INTO rev.fluent_message_locales (tag)
    SELECT locale_tag FROM locale_refs
    ON CONFLICT (tag) DO NOTHING
    RETURNING *
),
all_locales AS (
    SELECT id, tag FROM inserted_locales
    UNION
    SELECT id, tag FROM rev.fluent_message_locales
    WHERE tag IN (SELECT locale_tag FROM locale_refs)
),
new_bodies AS (
    INSERT INTO rev.fluent_message_bodies (
        fluent_message_id,
        locale_id,
        body,
        created_at
    )
    SELECT
        args.message_id,
        l.id,
        args.body,
        now()
    FROM args
    JOIN all_locales l ON l.tag = args.locale_tag
    WHERE NOT EXISTS (
        SELECT 1 FROM rev.fluent_message_bodies fb
        WHERE fb.fluent_message_id = args.message_id AND fb.locale_id = l.id
    )
    RETURNING *
),
objs AS (
    SELECT * FROM new_bodies
    UNION ALL
    SELECT fb.*
    FROM rev.fluent_message_bodies fb
    JOIN args ON fb.fluent_message_id = args.message_id
    JOIN all_locales l ON l.tag = args.locale_tag AND fb.locale_id = l.id
    WHERE NOT EXISTS (SELECT 1 FROM new_bodies nb WHERE nb.fluent_message_id = fb.fluent_message_id AND nb.locale_id = fb.locale_id)
)
SELECT
    objs.id AS "id!",
    objs.fluent_message_id AS "fluent_message_id!",
    l.id as "locale_id!",
    l.tag as "locale_tag!",
    objs.body AS "body!",
    objs.created_at AS "created_at!"
FROM objs
JOIN all_locales l ON l.id = objs.locale_id
ORDER BY objs.id;
