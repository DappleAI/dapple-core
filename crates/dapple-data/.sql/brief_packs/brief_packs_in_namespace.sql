-- True iff EVERY brief_pack_id in $1 is in the caller's
-- namespace. Used by external POST /api/journeys to validate that any
-- brief_pack_ids attached at journey-create time belong to the customer.
SELECT (
    SELECT count(DISTINCT bp.id)
    FROM UNNEST($1::UUID[]) AS requested(id)
    JOIN rev.brief_packs bp ON bp.id = requested.id
    JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE op.namespace_company_id = $2::UUID
) = COALESCE(array_length($1::UUID[], 1), 0)
AS all_in_namespace;
