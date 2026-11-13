SELECT
    c.id,
    octet_length(c.data) as byte_length,
    c.created_at
FROM rev.chunks c
LEFT JOIN rev.chunk_checksums cc ON cc.chunk_id = c.id
WHERE cc.id IS NULL
ORDER BY c.id;
