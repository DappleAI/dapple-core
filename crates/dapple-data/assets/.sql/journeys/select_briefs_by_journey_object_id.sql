
WITH journey_scope AS (
    SELECT $1::UUID AS journey_id
),
journey_packs AS (
    SELECT DISTINCT ON (bpvo.path_id) bpack.id AS brief_pack_id
    FROM journey_scope js
    JOIN rev.journey_brief_packs jbp ON jbp.journey_id = js.journey_id
    JOIN rev.brief_packs bpack ON bpack.id = jbp.brief_pack_id
    JOIN rev.versioned_objects bpvo ON bpvo.id = bpack.versioned_object_id
    ORDER BY bpvo.path_id, bpvo.created_at DESC, bpvo.id DESC
)

SELECT
        b.id,
    b.versioned_object_id AS brief_versioned_object_id,
    bvo.path_id AS brief_path_id,
    bop.id AS path_id,
    bpd.name,
    bvo.content_hash,
    COALESCE(rev.fluent_body(b.title_fluent_message_id, 'en'), b.title, bpd.name) AS "title!",
    COALESCE(rev.fluent_body(b.description_fluent_message_id, 'en'), b.description) AS description,
    b.created_at,
        bpack.id AS brief_pack_id,
        bg.id AS brief_group_id,
    bgpd.name AS brief_group_name,
    COALESCE(rev.fluent_body(bg.title_fluent_message_id, 'en'), bg.title, bgpd.name) AS "brief_group_title!"
FROM journey_packs jp
JOIN rev.brief_packs bpack ON bpack.id = jp.brief_pack_id
JOIN rev.brief_pack_groups bpg ON bpg.brief_pack_id = bpack.id
JOIN rev.brief_groups bg ON bg.id = bpg.brief_group_id
JOIN rev.versioned_objects bgvo ON bgvo.id = bg.versioned_object_id
JOIN rev.object_paths bgop ON bgop.id = bgvo.path_id
JOIN rev.brief_group_path_details bgpd ON bgpd.path_id = bgop.id
JOIN rev.brief_group_briefs bgb ON bgb.brief_group_id = bg.id
JOIN rev.briefs b ON b.id = bgb.brief_id
JOIN rev.versioned_objects bvo ON bvo.id = b.versioned_object_id
JOIN rev.object_paths bop ON bop.id = bvo.path_id
JOIN rev.brief_path_details bpd ON bpd.path_id = bop.id
ORDER BY bg.id, b.id;
