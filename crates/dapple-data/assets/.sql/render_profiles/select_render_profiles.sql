SELECT
    rp.id,
    rp.key,
    rp.description,
    rp.tag_filter,
    rp.style_config,
    rp.created_at
FROM rev.render_profiles rp
ORDER BY rp.id;
