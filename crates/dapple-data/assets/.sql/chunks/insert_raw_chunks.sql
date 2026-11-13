WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::BYTEA[]       ) WITH ORDINALITY AS o(
        data,
        ord
    )
),
input_checksums AS (
    SELECT
        args.data,
        args.ord,
        sha256(args.data) as checksum
    FROM args
),
existing_chunks AS (
    SELECT DISTINCT ON (ic.ord)
        ic.data,
        ic.ord,
        ic.checksum,
        cc.chunk_id,
        c.id as chunk_uuid
    FROM input_checksums ic
    JOIN rev.chunk_checksums cc ON cc.checksum = ic.checksum
    JOIN rev.chunks c ON c.id = cc.chunk_id
    ORDER BY ic.ord, cc.id
),
inputs_needing_chunks AS (
    SELECT
        ic.data,
        ic.ord,
        ic.checksum
    FROM input_checksums ic
    LEFT JOIN existing_chunks ec ON ec.ord = ic.ord
    WHERE ec.chunk_id IS NULL
),
new_chunks AS (
    INSERT INTO rev.chunks (data)
    SELECT inc.data
    FROM inputs_needing_chunks inc
    ORDER BY inc.ord
    RETURNING id, data
),
numbered_new_chunks AS (
    SELECT
        nc.id,
        nc.data,
        sha256(nc.data) as checksum,
        row_number() OVER (ORDER BY nc.id) as seq
    FROM new_chunks nc
),
new_chunk_mapping AS (
    SELECT
        inc.ord,
        nnc.id as chunk_id
    FROM inputs_needing_chunks inc
    JOIN numbered_new_chunks nnc ON nnc.checksum = inc.checksum
    WHERE nnc.seq = (
        SELECT COUNT(*) + 1
        FROM inputs_needing_chunks inc2
        WHERE inc2.checksum = inc.checksum AND inc2.ord < inc.ord
    )
),
new_checksums AS (
    INSERT INTO rev.chunk_checksums (checksum, chunk_id)
    SELECT nnc.checksum, nnc.id
    FROM numbered_new_chunks nnc
    RETURNING id, chunk_id
),
all_chunk_mappings AS (
    SELECT ec.ord, ec.chunk_id FROM existing_chunks ec
    UNION ALL
    SELECT ncm.ord, ncm.chunk_id FROM new_chunk_mapping ncm
)
SELECT
    acm.ord::INTEGER as "ord!",
    acm.chunk_id as "chunk_id!"
FROM all_chunk_mappings acm
ORDER BY acm.ord;
