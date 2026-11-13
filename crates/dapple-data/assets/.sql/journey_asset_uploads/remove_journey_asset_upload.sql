WITH active AS (
    SELECT u.id
    FROM rev.journey_asset_uploads u
    LEFT JOIN rev.journey_asset_uploads_removed r
        ON r.journey_asset_upload_id = u.id
    WHERE u.id = $2::UUID
      AND u.journey_id = $1::UUID
      AND r.journey_asset_upload_id IS NULL
)
INSERT INTO rev.journey_asset_uploads_removed (journey_asset_upload_id)
SELECT id
FROM active
RETURNING journey_asset_upload_id, removed_at;
