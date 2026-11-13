journey_packs AS (
    SELECT DISTINCT ON (bpvo.path_id) bpack.id AS brief_pack_id
    FROM journey_scope js
    JOIN rev.journey_brief_packs jbp ON jbp.journey_id = js.journey_id
    JOIN rev.brief_packs bpack ON bpack.id = jbp.brief_pack_id
    JOIN rev.versioned_objects bpvo ON bpvo.id = bpack.versioned_object_id
    ORDER BY bpvo.path_id, bpvo.created_at DESC, bpvo.id DESC
)
