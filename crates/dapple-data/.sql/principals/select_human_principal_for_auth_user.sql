-- Translate a NextAuth user id (auth.users.id, BIGINT) to the corresponding
-- human_principal.principal_id (UUID). Used by internal-API handlers when
-- they accept a delegated JWT and need to stamp principal_id onto a new
-- journey / dispatch.
--
-- Returns NULL if no human_principal exists for the given user — which
-- shouldn't happen after Mig 37 (every user_details row got a human_principal
-- in the backfill), but the handler treats it as an error.
SELECT
    hp.principal_id AS "principal_id!"
FROM rev.human_principals hp
WHERE hp.user_id = $1::BIGINT;
