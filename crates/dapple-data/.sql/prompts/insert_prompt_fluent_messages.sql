-- Insert prompt_fluent_messages (associative table entries)
-- Note: The prompt_fluent_messages table does not have an ordinal column,
-- so ordering is implicit based on insertion order (id).
-- Uses ON CONFLICT DO NOTHING to skip duplicates when patched prompts
-- already have an existing link.
-- Parameters:
--   $1: prompt_ids (UUID[]) - prompt IDs
--   $2: fluent_message_ids (UUID[]) - fluent message IDs
--   $3: roles (TEXT[] - nullable)

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
    RETURNING id
)
SELECT
    pfm.id,
    pfm.prompt_id,
    pfm.fluent_message_id,
    pfm.role,
    pfm.created_at
FROM args
JOIN rev.prompt_fluent_messages pfm
    ON pfm.prompt_id = args.prompt_id
    AND pfm.fluent_message_id = args.fluent_message_id
ORDER BY pfm.id;
