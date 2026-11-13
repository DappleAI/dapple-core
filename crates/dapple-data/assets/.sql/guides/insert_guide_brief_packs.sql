INSERT INTO rev.guide_brief_packs (guide_id, brief_pack_id)
SELECT $2::UUID, unnest($1::UUID[])
ON CONFLICT (guide_id, brief_pack_id) DO NOTHING;
