SELECT bo.id, b.id AS brief_id,
       bo.unit_key, vs.id AS value_slot_id, vspd.name AS value_slot_name,
       sk.key AS source_kind, bo.source_path, bo.transform, bo.created_at
FROM rev.brief_outputs bo
JOIN rev.briefs b ON b.id = bo.brief_id
JOIN rev.value_slots vs ON vs.id = bo.value_slot_id
JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id
JOIN rev.object_paths op ON op.id = vo.path_id
JOIN rev.value_slot_path_details vspd ON vspd.path_id = op.id
JOIN rev.brief_output_source_kinds sk ON sk.id = bo.source_kind_id
WHERE b.id = ANY($1::UUID[])
ORDER BY b.id, bo.id;
