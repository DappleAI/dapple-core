-- Insert prompt response after LLM streaming completes
-- Supports batch insert via UNNEST WITH ORDINALITY
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],      -- attempt_ids
        $2::UUID[],      -- prompt_ids
        $3::UUID[],      -- step_task_ids
        $4::TEXT[],       -- maker_keys
        $5::TEXT[],       -- engine_keys
        $6::TEXT[],       -- engine_versions
        $7::JSONB[],      -- request_messages
        $8::TEXT[],       -- response_contents
        $9::INTEGER[],    -- input_tokens
        $10::INTEGER[],   -- output_tokens
        $11::INTEGER[],   -- latency_ms
        $12::TEXT[],      -- finish_reasons
        $13::TIMESTAMPTZ[], -- started_ats
        $14::TIMESTAMPTZ[]  -- completed_ats
    ) WITH ORDINALITY AS o(
        attempt_id, prompt_id, step_task_id, maker_key, engine_key, engine_version,
        request_messages, response_content, input_tokens, output_tokens, latency_ms,
        finish_reason, started_at, completed_at, ord
    )
),
objs AS (
    INSERT INTO rev.prompt_responses (
        attempt_id, prompt_id, step_task_id,
        maker_key, engine_key, engine_version,
        request_messages, response_content,
        input_tokens, output_tokens, latency_ms,
        finish_reason, started_at, completed_at
    )
    SELECT
        args.attempt_id,
        args.prompt_id,
        args.step_task_id,
        args.maker_key,
        args.engine_key,
        args.engine_version,
        args.request_messages,
        args.response_content,
        args.input_tokens,
        args.output_tokens,
        args.latency_ms,
        args.finish_reason,
        args.started_at,
        args.completed_at
    FROM args
    RETURNING id, attempt_id, prompt_id, step_task_id,
              maker_key, engine_key, engine_version, request_messages,
              response_content, input_tokens, output_tokens, latency_ms,
              finish_reason, started_at, completed_at
)
SELECT * FROM objs ORDER BY id;
