-- Select chunks (with binary data) for a given extraction ordered by ordinal.
-- Parameters:
--   $1: extraction_id (UUID)
SELECT
    aec.extraction_id,
    aec.chunk_id,
    aec.ordinal,
    c.data AS "data!"
FROM rev.asset_extraction_chunks aec
JOIN rev.chunks c ON c.id = aec.chunk_id
WHERE aec.extraction_id = $1
ORDER BY aec.ordinal;
