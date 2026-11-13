-- Insert checksums for chunks that are missing them
-- Used by integrity service to repair missing checksum records
WITH missing AS (
    SELECT c.id as chunk_id, sha256(c.data) as checksum
    FROM rev.chunks c
    LEFT JOIN rev.chunk_checksums cc ON cc.chunk_id = c.id
    WHERE cc.id IS NULL
)
INSERT INTO rev.chunk_checksums (checksum, chunk_id)
SELECT checksum, chunk_id FROM missing
RETURNING id, checksum, chunk_id, created_at;
