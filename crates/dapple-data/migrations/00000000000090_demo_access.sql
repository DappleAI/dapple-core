-- ============================================================================
-- auth.demo_access — Access gate + request queue for the limited demo
--
-- The hosted demo is scoped to the team plus a hand-picked set of invited
-- guests and alpha testers. This table is the dynamic half of that gate:
-- staff are admitted by email domain (ALLOWED_LOGIN_DOMAINS in dapple-ui, which
-- changes approximately never), while everyone else is admitted by a row here,
-- editable live from the demo's own admin page with no deploy.
--
-- One table rather than two, because the allowlist and the request queue are the
-- same rows at different statuses: someone who completes Google auth but isn't
-- approved is recorded as 'requested', and an admin promotes them in place. Two
-- tables would make approval a cross-table copy and let the two lists disagree
-- about one address.
--
-- Written and read exclusively by dapple-ui (NextAuth's signIn callback and the
-- /admin/access BFF routes) over the pg pool. Unlike the journey tables, there
-- is no dapple-prompt-api path to this data and therefore no delegated-JWT
-- scoping — dapple-ui is the sole authorization boundary for it.
--
-- Lives in auth.* alongside the other NextAuth tables and follows their BIGINT
-- identity PK convention.
--
-- Temporary: this is demo-window scaffolding, expected to be dropped once the
-- demo opens up. Nothing else should take a dependency on it.
-- ============================================================================
CREATE TABLE auth.demo_access (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- Always stored lowercased; dapple-ui normalizes on write and on lookup.
    email TEXT NOT NULL UNIQUE,
    -- From the Google profile, so admins can recognize who is asking.
    name TEXT,
    status TEXT NOT NULL DEFAULT 'requested'
        CHECK (status IN ('requested', 'allowed', 'revoked')),
    -- Admin's own note: "advisor", "alpha tester".
    note TEXT,
    -- How many times this address has hit the gate. >1 means they came back,
    -- which is the useful signal when triaging the queue.
    attempt_count INTEGER NOT NULL DEFAULT 1,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at TIMESTAMPTZ,
    -- Email of the admin who allowed or revoked.
    decided_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE auth.demo_access OWNER TO dapple;

-- The admin queue reads pending requests newest-first; the allowed/revoked
-- lists are small enough to fall out of the same index.
CREATE INDEX demo_access_status_idx
    ON auth.demo_access (status, last_seen_at DESC);
