-- Insert prompt_fluent_messages (associative table entries)
-- Note: The prompt_fluent_messages table does not have an ordinal column,
-- so ordering is implicit based on insertion order (id).
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
)
INSERT INTO rev.prompt_fluent_messages (prompt_id, fluent_message_id, role, created_at)
SELECT args.prompt_id, args.fluent_message_id, args.role, now()
FROM args
ORDER BY ord
RETURNING id, prompt_id, fluent_message_id, role, created_at;
