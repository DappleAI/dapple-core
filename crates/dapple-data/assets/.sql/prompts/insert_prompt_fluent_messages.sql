
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::TEXT[]
    ) WITH ORDINALITY AS o(prompt_id, fluent_message_id, role, ord)
),
_inserted AS (
    INSERT INTO rev.prompt_fluent_messages (prompt_id, fluent_message_id, role, created_at)
    SELECT args.prompt_id, args.fluent_message_id, args.role, now()
    FROM args
    ORDER BY ord
    ON CONFLICT (prompt_id, fluent_message_id) DO NOTHING
    RETURNING id, prompt_id, fluent_message_id, role, created_at
),
all_links AS (
    SELECT id, prompt_id, fluent_message_id, role, created_at FROM _inserted
    UNION ALL
    SELECT pfm.id, pfm.prompt_id, pfm.fluent_message_id, pfm.role, pfm.created_at
    FROM args
    JOIN rev.prompt_fluent_messages pfm
        ON pfm.prompt_id = args.prompt_id
        AND pfm.fluent_message_id = args.fluent_message_id
    WHERE NOT EXISTS (
        SELECT 1 FROM _inserted i
        WHERE i.prompt_id = args.prompt_id AND i.fluent_message_id = args.fluent_message_id
    )
)
SELECT
    al.id AS "id!",
    al.prompt_id AS "prompt_id!",
    al.fluent_message_id AS "fluent_message_id!",
    al.role,
    al.created_at AS "created_at!"
FROM all_links al
ORDER BY al.prompt_id, al.id;
