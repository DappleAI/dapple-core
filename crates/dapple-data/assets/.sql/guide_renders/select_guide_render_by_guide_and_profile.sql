SELECT
    gr.id,
    gr.guide_id,
    gr.render_profile_id,
    gr.asset_id,
    gr.created_at
FROM rev.guide_renders gr
WHERE gr.guide_id = $1
  AND gr.render_profile_id = $2;
