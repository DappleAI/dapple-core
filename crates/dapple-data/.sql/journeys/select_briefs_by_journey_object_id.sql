-- Select all briefs for a journey with pack/group context
-- Traverses: journey → journey_brief_packs → brief_packs → brief_pack_groups
--          → brief_groups → brief_group_briefs → briefs
-- Parameters:
--   $1: journey_id (UUID)

SELECT
    -- Brief fields
    b.id,
    b.versioned_object_id AS brief_versioned_object_id,
    bvo.path_id AS brief_path_id,
    bop.id AS path_id,
    bpd.name,
    bvo.content_hash,
    b.title,
    b.description,
    b.created_at,
    -- Context: BriefPack
    bpack.id AS brief_pack_id,
    -- Context: BriefGroup
    bg.id AS brief_group_id,
    bgpd.name AS brief_group_name,
    bg.title AS brief_group_title
FROM rev.journeys j
JOIN rev.journey_brief_packs jbp ON jbp.journey_id = j.id
JOIN rev.brief_packs bpack ON bpack.id = jbp.brief_pack_id
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
WHERE j.id = $1
ORDER BY bg.id, b.id;
