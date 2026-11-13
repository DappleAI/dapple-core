-- Migration 78: key rev.asset_chunks by assembly position, not chunk identity
--
-- rev.asset_chunks is an ORDERED list of byte-range positions (migration 10's
-- DDL says so: "ordinal is semantic here (byte-range assembly position)"), but
-- it was keyed PRIMARY KEY (asset_id, chunk_id). That key asserts a chunk
-- appears at most once per asset, which directly contradicts the
-- content-addressable dedup in drafts/insert_draft_chunks.sqlc: byte-identical
-- chunks reuse a single rev.chunks row, so an asset whose content repeats a
-- 256KB block legitimately carries one chunk_id at several ordinals.
--
-- Consequence: finalize_draft copies the draft's chunk refs into asset_chunks,
-- hit 23505 on the repeat, and the whole asset create 409'd. Any content with a
-- repeated block was unstorable. That is not a corner case: a WAV stem is
-- digital silence whenever its instrument rests, so every stem upload failed
-- while the mastered mix (never two identical blocks) stored fine.
--
-- The identity of an asset_chunks row is the position it fills. Two positions
-- may quote the same chunk; one position cannot be filled twice.

-- Step 1 -- repair legacy rows so the position key can be applied.
--
-- An earlier bug (since fixed in insert_draft_chunks.sqlc) restarted ordinals at
-- 1 on every streamed call, leaving each chunk of a multi-chunk asset at
-- ordinal = 1. Those assets are already corrupt: read_asset_content ORDER BY
-- ordinal breaks the tie arbitrarily. They also collide under the new key.
--
-- The true write order is still recoverable: draft_chunks.id is a uuidv7 and so
-- sorts by insertion time. These assets finalized under the OLD key, which
-- means their chunk_ids are distinct within the asset, so mapping each row back
-- to its position is unambiguous. Rows already numbered correctly are left
-- alone. Updating ordinal cannot collide with the outgoing (asset_id, chunk_id)
-- key, which is why the repair runs before the swap.
WITH draft_order AS (
    SELECT
        dc.draft_id,
        dc.chunk_id,
        row_number() OVER (PARTITION BY dc.draft_id ORDER BY dc.id) AS pos
    FROM rev.draft_chunks dc
),
collided AS (
    SELECT asset_id
    FROM rev.asset_chunks
    GROUP BY asset_id, ordinal
    HAVING count(*) > 1
)
UPDATE rev.asset_chunks ac
SET ordinal = d.pos
FROM rev.assets a
JOIN draft_order d ON d.draft_id = a.draft_id
WHERE ac.asset_id = a.id
  AND d.chunk_id = ac.chunk_id
  AND ac.asset_id IN (SELECT asset_id FROM collided);

-- Step 2 -- swap the key onto the assembly position.
ALTER TABLE rev.asset_chunks DROP CONSTRAINT asset_chunks_pkey;

ALTER TABLE rev.asset_chunks
    ADD CONSTRAINT asset_chunks_pkey PRIMARY KEY (asset_id, ordinal);
