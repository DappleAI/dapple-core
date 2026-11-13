SELECT
    pr.id,
    pr.attempt_id,
    pr.prompt_id,
    pr.step_task_id,
    pr.maker_key,
    pr.engine_key,
    pr.engine_version,
    pr.request_messages,
    pr.response_content,
    pr.input_tokens,
    pr.output_tokens,
    pr.latency_ms,
    pr.finish_reason,
    pr.started_at,
    pr.completed_at
FROM rev.prompt_responses pr
WHERE pr.attempt_id = $1::UUID
ORDER BY pr.started_at ASC, pr.id ASC;
