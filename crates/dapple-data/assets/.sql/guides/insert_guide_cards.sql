WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],             $2::TEXT[],              $3::UUID[],             $4::TEXT[],              $5::TEXT[],              $6::TEXT[]           ) WITH ORDINALITY AS o(guide_id, subtitle, card_image_asset_id, category, color, accent, ord)
),
objs AS (
    INSERT INTO rev.guide_cards (guide_id, subtitle, card_image_asset_id, category, color, accent)
    SELECT args.guide_id, args.subtitle, args.card_image_asset_id, args.category, args.color, args.accent
    FROM args
    ORDER BY args.ord
                        ON CONFLICT (guide_id) DO UPDATE SET
        subtitle = COALESCE(EXCLUDED.subtitle, rev.guide_cards.subtitle),
        category = COALESCE(EXCLUDED.category, rev.guide_cards.category),
        color = EXCLUDED.color,
        accent = EXCLUDED.accent
    RETURNING id, guide_id, subtitle, card_image_asset_id, category, color, accent, created_at
)
SELECT * FROM objs ORDER BY objs.id;
