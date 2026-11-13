SELECT
    bumi.id, bumi.brief_unit_id,
    bmk.key AS media_kind_key,
    bmst.key AS source_type_key,
        bumias.asset_id AS "asset_id?",
    bumius.url AS "url?",
    bumivss.value_slot_id AS "value_slot_id?",
        bumip.id AS "poster_id?",
    bmst_p.key AS "poster_source_type_key?",
        bumipas.asset_id AS "poster_asset_id?",
    bumipus.url AS "poster_url?",
    bumipvss.value_slot_id AS "poster_value_slot_id?",
        bumi_alt.fluent_message_id AS "alt_text_fluent_message_id?",
    bumi_cap.fluent_message_id AS "caption_fluent_message_id?",
    bumi_title.fluent_message_id AS "title_fluent_message_id?",
    bumi.created_at
FROM rev.brief_unit_media_items bumi
JOIN rev.brief_media_kinds bmk ON bmk.id = bumi.media_kind_id
JOIN rev.brief_media_source_types bmst ON bmst.id = bumi.source_type_id
LEFT JOIN rev.brief_unit_media_item_asset_sources bumias ON bumias.media_item_id = bumi.id
LEFT JOIN rev.brief_unit_media_item_url_sources bumius ON bumius.media_item_id = bumi.id
LEFT JOIN rev.brief_unit_media_item_value_slot_sources bumivss ON bumivss.media_item_id = bumi.id
LEFT JOIN rev.brief_unit_media_item_posters bumip ON bumip.media_item_id = bumi.id
LEFT JOIN rev.brief_media_source_types bmst_p ON bmst_p.id = bumip.source_type_id
LEFT JOIN rev.brief_unit_media_item_poster_asset_sources bumipas ON bumipas.poster_id = bumip.id
LEFT JOIN rev.brief_unit_media_item_poster_url_sources bumipus ON bumipus.poster_id = bumip.id
LEFT JOIN rev.brief_unit_media_item_poster_value_slot_sources bumipvss ON bumipvss.poster_id = bumip.id
LEFT JOIN rev.brief_unit_media_item_alt_texts bumi_alt ON bumi_alt.media_item_id = bumi.id
LEFT JOIN rev.brief_unit_media_item_captions bumi_cap ON bumi_cap.media_item_id = bumi.id
LEFT JOIN rev.brief_unit_media_item_titles bumi_title ON bumi_title.media_item_id = bumi.id
WHERE bumi.brief_unit_id = ANY($1::UUID[])
ORDER BY bumi.brief_unit_id, bumi.id;
