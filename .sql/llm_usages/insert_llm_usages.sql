-- Batch insert LLM usage records
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::UUID[],
        $3::INTEGER[],
        $4::UUID[],
        $5::INTEGER[],
        $6::SMALLINT[],
        $7::INTEGER[],
        $8::INTEGER[]
    ) WITH ORDINALITY AS o(claim_id, event_id, input_tokens, model_release_id, output_tokens, provider_id, provider_input_tokens, provider_output_tokens, ord)
),
new_usages AS (
    INSERT INTO rev.dispatch_task_attempt_event_llm_usages (
        event_id, claim_id, input_tokens, output_tokens,
        provider_input_tokens, provider_output_tokens,
        provider_id, model_release_id
    )
    SELECT
        args.event_id, args.claim_id, args.input_tokens, args.output_tokens,
        args.provider_input_tokens, args.provider_output_tokens,
        args.provider_id, args.model_release_id
    FROM args
    ORDER BY args.ord
    RETURNING id, event_id, created_at
)
SELECT nu.id, nu.event_id, nu.created_at
FROM new_usages nu
ORDER BY nu.id;
