WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[])
    WITH ORDINALITY AS o(name, ord)
),
latest_versions AS (
    SELECT DISTINCT ON (vspd.name)
        vs.id,
        op.id AS path_id,
        vspd.name,
        vo.content_hash
    FROM rev.value_slot_path_details vspd
    JOIN rev.object_paths op ON op.id = vspd.path_id
    JOIN rev.versioned_objects vo ON vo.path_id = op.id
    JOIN rev.value_slots vs ON vs.versioned_object_id = vo.id
    WHERE vspd.name = ANY($1::TEXT[])
      AND vspd.namespace_company_id = COALESCE(
          NULLIF(current_setting('dapple.namespace_company_id', true), ''),
          '019e132e-2610-7a4a-b64a-77a49e286ac0'
      )::UUID
                    ORDER BY vspd.name, vo.created_at DESC, vo.id DESC
)
SELECT
    lv.id,
    lv.path_id,
    lv.name,
    lv.content_hash
FROM latest_versions lv
JOIN args ON args.name = lv.name
ORDER BY args.ord;
