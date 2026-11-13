WITH args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(guide_id, ord)
)
SELECT
    gc.id,
    gc.guide_id,
    gc.subtitle,
    gc.card_image_asset_id,
    gc.category,
    gc.color,
    gc.accent,
    gc.created_at
FROM args
JOIN rev.guide_cards gc ON gc.guide_id = args.guide_id
ORDER BY gc.guide_id, gc.id;
