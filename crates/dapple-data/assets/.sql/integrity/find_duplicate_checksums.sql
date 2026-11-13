SELECT
    cc.checksum,
    COUNT(DISTINCT cc.chunk_id)::BIGINT as chunk_count,
    array_agg(cc.chunk_id ORDER BY cc.chunk_id) as chunk_ids
FROM rev.chunk_checksums cc
JOIN rev.chunks c ON c.id = cc.chunk_id
GROUP BY cc.checksum
HAVING COUNT(DISTINCT cc.chunk_id) > 1
ORDER BY chunk_count DESC;
