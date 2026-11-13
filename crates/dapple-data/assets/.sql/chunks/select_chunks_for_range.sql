WITH asset_info AS (
    SELECT a.id, a.content_length
    FROM rev.assets a
    WHERE a.id = $1
),
chunk_positions AS (
    SELECT
        c.id,
        c.data,
        c.created_at,
        ac.ordinal,
                COALESCE(
            SUM(octet_length(c.data)) OVER (
                PARTITION BY ac.asset_id
                ORDER BY ac.ordinal
                ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
            ),
            0
        )::BIGINT as start_byte,
                (COALESCE(
            SUM(octet_length(c.data)) OVER (
                PARTITION BY ac.asset_id
                ORDER BY ac.ordinal
                ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
            ),
            0
        ) + octet_length(c.data) - 1)::BIGINT as end_byte
    FROM asset_info ai
    JOIN rev.asset_chunks ac ON ac.asset_id = ai.id
    JOIN rev.chunks c ON c.id = ac.chunk_id
)
SELECT
    id,
    data,
    start_byte,
    end_byte,
    created_at
FROM chunk_positions
WHERE end_byte >= $2      AND start_byte <= $3  ORDER BY ordinal;
