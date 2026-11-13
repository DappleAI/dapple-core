WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],              $2::UUID[],              $3::UUID[],              $4::TEXT[],               $5::TEXT[],               $6::TEXT[],               $7::JSONB[],              $8::TEXT[],               $9::INTEGER[],            $10::INTEGER[],           $11::INTEGER[],           $12::TEXT[],              $13::TIMESTAMPTZ[],         $14::TIMESTAMPTZ[]      ) WITH ORDINALITY AS o(
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
