WITH args AS (
    SELECT * FROM UNNEST($1::TEXT[], $2::TEXT[])
    WITH ORDINALITY AS o(slug, content_hash, ord)
)
SELECT
    p.id,
    ppd.slug,
    vo.content_hash
FROM args
JOIN rev.playbook_path_details ppd
    ON ppd.slug = args.slug
    AND ppd.namespace_company_id = COALESCE(
        NULLIF(current_setting('dapple.namespace_company_id', true), ''),
        '019e132e-2610-7a4a-b64a-77a49e286ac0'
    )::UUID
JOIN rev.object_paths op ON op.id = ppd.path_id
JOIN rev.versioned_objects vo ON vo.path_id = op.id
    AND vo.content_hash = args.content_hash
JOIN rev.playbooks p ON p.versioned_object_id = vo.id
ORDER BY args.ord;
