SELECT
    id,
    extraction_id,
    byte_offset,
    byte_length,
    source_position,
    created_at
FROM rev.asset_extraction_positions
WHERE extraction_id = $1
ORDER BY byte_offset;
