WITH args AS (
    SELECT *
    FROM UNNEST(
        $1::UUID[],           $2::BYTEA[]       ) WITH ORDINALITY AS o(
        draft_id,
        data,
        ord
    )
),
input_checksums AS (
    SELECT
        args.draft_id,
        args.data,
        args.ord,
        sha256(args.data) as checksum
    FROM args
),
existing_chunks AS (
    SELECT DISTINCT ON (ic.ord)
        ic.draft_id,
        ic.data,
        ic.ord,
        ic.checksum,
        cc.chunk_id,
        c.id as chunk_uuid
    FROM input_checksums ic
    JOIN rev.chunk_checksums cc ON cc.checksum = ic.checksum
    JOIN rev.chunks c ON c.id = cc.chunk_id
    ORDER BY ic.ord, cc.id  ),
inputs_needing_chunks AS (
    SELECT
        ic.draft_id,
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
        inc.draft_id,
        inc.data,
        inc.ord,
        inc.checksum,
        nnc.id as chunk_id,
        nnc.id as chunk_uuid
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
    SELECT
        ec.draft_id,
        ec.ord,
        ec.chunk_id,
        octet_length(ec.data)::BIGINT as byte_length
    FROM existing_chunks ec
    UNION ALL
    SELECT
        ncm.draft_id,
        ncm.ord,
        ncm.chunk_id,
        octet_length(ncm.data)::BIGINT as byte_length
    FROM new_chunk_mapping ncm
),
draft_ordinals AS (
    SELECT
        acm.draft_id,
        acm.ord,
        acm.chunk_id,
        acm.byte_length,
        (COALESCE(
            (SELECT MAX(dc.ordinal) FROM rev.draft_chunks dc WHERE dc.draft_id = acm.draft_id),
            0
         ) + row_number() OVER (PARTITION BY acm.draft_id ORDER BY acm.ord))::INTEGER as ordinal
    FROM all_chunk_mappings acm
),
new_draft_chunks AS (
    INSERT INTO rev.draft_chunks (draft_id, chunk_id, ordinal)
    SELECT
        dord.draft_id,
        dord.chunk_id,
        dord.ordinal
    FROM draft_ordinals dord
    ORDER BY dord.ord
    RETURNING id, draft_id, chunk_id, ordinal, created_at
)
SELECT
    ndc.id,
    ndc.draft_id,
    ndc.chunk_id,
    dord.byte_length,
    ndc.ordinal,
    ndc.created_at
FROM new_draft_chunks ndc
JOIN draft_ordinals dord ON dord.draft_id = ndc.draft_id AND dord.ordinal = ndc.ordinal
ORDER BY ndc.chunk_id;
