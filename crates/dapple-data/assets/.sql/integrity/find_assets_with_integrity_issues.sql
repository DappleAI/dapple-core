WITH chunks_missing_checksums AS (
    SELECT c.id as chunk_id
    FROM rev.chunks c
    LEFT JOIN rev.chunk_checksums cc ON cc.chunk_id = c.id
    WHERE cc.id IS NULL
),
chunks_with_mismatches AS (
    SELECT c.id as chunk_id
    FROM rev.chunks c
    JOIN rev.chunk_checksums cc ON cc.chunk_id = c.id
    WHERE cc.checksum != sha256(c.data)
),
problem_chunks AS (
    SELECT chunk_id FROM chunks_missing_checksums
    UNION
    SELECT chunk_id FROM chunks_with_mismatches
)
SELECT DISTINCT
    a.id as asset_id,
    vo.path_id,
    a.content_length,
    a.mime_type,
    a.created_at,
    CASE
        WHEN EXISTS (SELECT 1 FROM chunks_missing_checksums cmc
                     JOIN rev.asset_chunks ac ON ac.chunk_id = cmc.chunk_id
                     WHERE ac.asset_id = a.id) THEN 'missing_checksum'
        WHEN EXISTS (SELECT 1 FROM chunks_with_mismatches cwm
                     JOIN rev.asset_chunks ac ON ac.chunk_id = cwm.chunk_id
                     WHERE ac.asset_id = a.id) THEN 'checksum_mismatch'
    END as issue_type
FROM rev.assets a
JOIN rev.versioned_objects vo ON vo.id = a.versioned_object_id
JOIN rev.asset_chunks ac ON ac.asset_id = a.id
JOIN problem_chunks pc ON pc.chunk_id = ac.chunk_id
ORDER BY a.id;
