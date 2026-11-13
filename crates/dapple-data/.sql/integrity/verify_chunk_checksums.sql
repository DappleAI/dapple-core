-- Verify that stored checksums match actual chunk data
-- Returns chunks where computed SHA-256 doesn't match stored checksum
SELECT
    c.id,
    cc.checksum as stored_checksum,
    sha256(c.data) as computed_checksum,
    octet_length(c.data) as byte_length,
    c.created_at
FROM rev.chunks c
JOIN rev.chunk_checksums cc ON cc.chunk_id = c.id
WHERE cc.checksum != sha256(c.data)
ORDER BY c.id;
