-- True iff `path_id` exists, belongs to the caller's company, and is at
-- scope='external'. Used by the external-API PATCH handler to gate every
-- guide-version ingest against cross-namespace targeting.
--
-- The scope filter is hardcoded to 'external' because customer-facing
-- callers can never legitimately PATCH a dapple-namespace path
-- (public/common/internal). Returning false (instead of a separate
-- "exists but wrong scope" signal) is intentional — a foreign or
-- mis-scoped path_id produces the same 404 RESOURCE_NOT_FOUND body on
-- the wire, no existence leak.
SELECT EXISTS (
    SELECT 1
    FROM rev.object_paths
    WHERE id = $2::UUID
      AND namespace_company_id = $1::UUID
      AND scope = 'external'
) AS in_namespace;
