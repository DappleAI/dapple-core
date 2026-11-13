-- Insert guide cards (promotional card data per guide version).
WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],     -- guide_id[]
        $2::TEXT[],      -- subtitle[]
        $3::UUID[],     -- card_image_asset_id[]
        $4::TEXT[],      -- category[]
        $5::TEXT[],      -- color[]
        $6::TEXT[]       -- accent[]
    ) WITH ORDINALITY AS o(guide_id, subtitle, card_image_asset_id, category, color, accent, ord)
),
objs AS (
    INSERT INTO rev.guide_cards (guide_id, subtitle, card_image_asset_id, category, color, accent)
    SELECT args.guide_id, args.subtitle, args.card_image_asset_id, args.category, args.color, args.accent
    FROM args
    ORDER BY args.ord
    RETURNING id, guide_id, subtitle, card_image_asset_id, category, color, accent, created_at
)
SELECT * FROM objs ORDER BY objs.id;
