-- Migration 91: auto-create a human principal for every rev.user_details row
--
-- Migration 37 established the invariant "every rev.user_details row has a
-- matching rev.human_principals row", but only half-enforced it:
--
--   * a ONE-TIME backfill covered the user_details rows that existed when the
--     migration ran, and
--   * an AFTER INSERT trigger covers new rev.external_service_tokens rows.
--
-- Nothing covered a HUMAN who first signs in AFTER Mig 37. `rev.user_details`
-- rows are auto-created by the login upsert
-- (assets/sqlc/user_details/upsert_user_detail_by_jwt_ref.sqlc), which knows
-- nothing about principals — so every post-Mig-37 user has no principal, and
-- `select_human_principal_for_user_object_id` returns NULL for them.
--
-- Consequence: those users 500 on POST /api/journeys ("no human_principal for
-- user_id=..."), on every journey index/read that overlays viewer preferences,
-- and on the journey-chain / favorites readers. On a freshly provisioned
-- database the Mig 37 backfill inserts ZERO rows, so EVERY user is in this
-- state — which is exactly what happened on prod (2026-07-29): the first real
-- sign-ups could not start a journey at all. The test suites had already
-- papered over the gap by hand-inserting principals in their user fixtures.
--
-- Fix: mirror the external-service-token trigger for humans, then backfill any
-- user_details row the Mig 37 backfill missed.

-- ---------------------------------------------------------------------------
-- 1. Auto-create trigger: every new rev.user_details row gets a principal
-- ---------------------------------------------------------------------------
-- Direct counterpart of Mig 37's `create_principal_for_external_service_token`.
-- Humans always live in dapple's own namespace, so `owner_company_id` is the
-- dapple company UUID seeded by Migration 31 (same literal the Mig 37 human
-- backfill used; also exported as DAPPLE_COMPANY_UUID in the nushell env).

CREATE OR REPLACE FUNCTION rev.create_principal_for_user_detail()
    RETURNS TRIGGER AS $$
DECLARE
    new_principal_id UUID;
BEGIN
    INSERT INTO rev.principals (owner_company_id)
    VALUES ('019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID)
    RETURNING id INTO new_principal_id;

    INSERT INTO rev.human_principals (principal_id, user_id)
    VALUES (new_principal_id, NEW.user_id);

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER user_detail_creates_principal
    AFTER INSERT ON rev.user_details
    FOR EACH ROW EXECUTE FUNCTION rev.create_principal_for_user_detail();

-- ---------------------------------------------------------------------------
-- 2. Backfill: user_details rows created between Mig 37 and this migration
-- ---------------------------------------------------------------------------
-- Row-by-row rather than Mig 37's set-wise ordinal-correlation CTE. That CTE
-- pairs freshly inserted principals back to their users by `row_number() OVER
-- (ORDER BY id)` over the RETURNING set, which only holds while uuidv7()
-- generation order matches insert order; a mismatch here would silently
-- attribute a principal to the WRONG user (an identity/billing bug, not a
-- visible failure). The orphan set is small and one-time — every user who
-- signed in since Mig 37 — so the unambiguous loop is the right trade.
--
-- The trigger above is already live, so this handles pre-existing rows only.

DO $$
DECLARE
    orphan RECORD;
    new_principal_id UUID;
BEGIN
    FOR orphan IN
        SELECT ud.user_id
        FROM rev.user_details ud
        LEFT JOIN rev.human_principals hp ON hp.user_id = ud.user_id
        WHERE hp.principal_id IS NULL
        ORDER BY ud.user_id
    LOOP
        INSERT INTO rev.principals (owner_company_id)
        VALUES ('019e132e-2610-7a4a-b64a-77a49e286ac0'::UUID)
        RETURNING id INTO new_principal_id;

        INSERT INTO rev.human_principals (principal_id, user_id)
        VALUES (new_principal_id, orphan.user_id);
    END LOOP;
END $$;
