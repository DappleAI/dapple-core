-- Select all chunks for an asset (full content read)
SELECT
    c.id,
    c.data,
    c.created_at,
    ac.ordinal
FROM rev.assets a
JOIN rev.asset_chunks ac ON ac.asset_id = a.id
JOIN rev.chunks c ON c.id = ac.chunk_id
WHERE a.id = $1
ORDER BY ac.ordinal;
