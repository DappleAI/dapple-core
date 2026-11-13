-- True iff EVERY inference_profile_id in $2 is in
-- the caller's namespace. Used by external POST /api/journeys to validate that
-- any inference_profile_ids attached at journey-create time belong to the
-- customer.
SELECT (
    SELECT count(DISTINCT ip.id)
    FROM UNNEST($2::UUID[]) AS requested(id)
    JOIN rev.inference_profiles ip ON ip.id = requested.id
    JOIN rev.versioned_objects vo ON vo.id = ip.versioned_object_id
    JOIN rev.object_paths op ON op.id = vo.path_id
    WHERE op.namespace_company_id = $1::UUID
) = COALESCE(array_length($2::UUID[], 1), 0)
AS all_in_namespace;
