WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::TEXT[],           $2::TEXT[]        ) WITH ORDINALITY AS o(
        asset_kind_key,
        mime_type,
        ord
    )
),
kinds AS (
    SELECT
        ak.id as kind_id,
        ak.key as kind_key,
        args.mime_type,
        args.ord
    FROM args
    JOIN rev.asset_kinds ak ON ak.key = args.asset_kind_key
),
new_drafts AS (
    INSERT INTO rev.drafts (asset_kind_id, mime_type)
    SELECT
        kinds.kind_id,
        kinds.mime_type
    FROM kinds
    ORDER BY kinds.ord
    RETURNING id, asset_kind_id, mime_type, created_at
),
numbered_drafts AS (
    SELECT nd.*, row_number() OVER (ORDER BY nd.id) as ord
    FROM new_drafts nd
)
SELECT
    nd.id,
    nd.asset_kind_id,
    ak.key as asset_kind_key,
    nd.mime_type,
    nd.created_at
FROM numbered_drafts nd
JOIN rev.asset_kinds ak ON ak.id = nd.asset_kind_id
ORDER BY nd.id;
