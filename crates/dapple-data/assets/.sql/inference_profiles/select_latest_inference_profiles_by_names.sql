WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[])
    WITH ORDINALITY AS o(name, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (ippd.name)
        ip.id,
        op.id AS path_id,
        ippd.name,
        vo.content_hash
    FROM rev.inference_profile_path_details ippd
    JOIN rev.object_paths op ON op.id = ippd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.inference_profiles ip ON ip.versioned_object_id = vo.id
    WHERE ippd.name = ANY($1::TEXT[])
      AND ippd.namespace_company_id = COALESCE(
          NULLIF(current_setting('dapple.namespace_company_id', true), ''),
          '019e132e-2610-7a4a-b64a-77a49e286ac0'
      )::UUID
    ORDER BY ippd.name, vo.created_at DESC
)
SELECT
    lv.id,
    lv.path_id,
    lv.name,
    lv.content_hash
FROM latest_versions lv
JOIN args ON args.name = lv.name
ORDER BY args.ord;
