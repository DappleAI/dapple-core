WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],
        $2::INTEGER[]
    ) WITH ORDINALITY AS o(asset_id, extractor_version, ord)
),
objs AS (
    INSERT INTO rev.asset_dataset_extractions (asset_id, extractor_version, status)
    SELECT args.asset_id, args.extractor_version, 'pending'
    FROM args
    ORDER BY args.ord
    RETURNING
        id,
        asset_id,
        extractor_version,
        status,
        envelope,
        table_count,
        row_count,
        error_message,
        created_at,
        completed_at
)
SELECT
    id,
    asset_id,
    extractor_version,
    status,
    envelope,
    table_count,
    row_count,
    error_message,
    created_at,
    completed_at
FROM objs
ORDER BY id;
