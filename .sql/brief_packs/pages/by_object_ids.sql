args AS (
    SELECT *
    FROM UNNEST($1::UUID[]) WITH ORDINALITY AS o(id, ord)
),
brief_pack_page AS (
    SELECT bp.id
    FROM args
    JOIN rev.brief_packs bp ON bp.id = args.id
)
