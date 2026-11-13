-- Add the `unsafe` terminal dispatch phase.
--
-- A step is an ordered, fail-together task-set. A task may emit a poisoning veto
-- (`signals.unsafe = true`): the task ran, but its result must not be trusted.
-- That task attempt ends in this terminal phase, distinct from a transient
-- `failed` (freely retryable via re-dispatch) and a deliberately `canceled`
-- dispatch, so a consumer reads `unsafe` as "do not re-dispatch -- this is
-- poisoned." (Existing terminals: failed, ok, canceled; non-terminal: queued,
-- running.)
INSERT INTO rev.dispatch_phases (code, terminal) VALUES
    ('unsafe', true)
ON CONFLICT (code) DO NOTHING;
