-- Copy all brief units (and their entire subtree) from old brief to new brief
-- Parameters:
--   $1: old_brief_id (UUID)
--   $2: new_brief_id (UUID)

WITH old_units AS (
    SELECT id, unit_key, layout, requires, extensions
    FROM rev.brief_units
    WHERE brief_id = $1
),
new_units AS (
    INSERT INTO rev.brief_units (brief_id, unit_key, layout, requires, extensions)
    SELECT $2, ou.unit_key, ou.layout, ou.requires, ou.extensions
    FROM old_units ou
    ORDER BY ou.id
    RETURNING id, unit_key
),
unit_map AS (
    SELECT ou.id AS old_id, nu.id AS new_id
    FROM old_units ou
    JOIN new_units nu ON nu.unit_key = ou.unit_key
),
copy_markdown AS (
    INSERT INTO rev.brief_unit_markdown (brief_unit_id, markdown_text, fluent_message_id, format, deliverable_spec)
    SELECT um.new_id, bum.markdown_text, bum.fluent_message_id, bum.format, bum.deliverable_spec
    FROM rev.brief_unit_markdown bum
    JOIN unit_map um ON um.old_id = bum.brief_unit_id
    RETURNING brief_unit_id
),
copy_notice AS (
    INSERT INTO rev.brief_unit_notice (brief_unit_id, level, title_text, title_fluent_message_id, detail_text, detail_fluent_message_id)
    SELECT um.new_id, bun.level, bun.title_text, bun.title_fluent_message_id, bun.detail_text, bun.detail_fluent_message_id
    FROM rev.brief_unit_notice bun
    JOIN unit_map um ON um.old_id = bun.brief_unit_id
    RETURNING brief_unit_id
),
copy_input AS (
    INSERT INTO rev.brief_unit_input (brief_unit_id, control_type_id, label_text, label_fluent_message_id, help_text, help_fluent_message_id, placeholder_text, placeholder_fluent_message_id, control_spec)
    SELECT um.new_id, bui.control_type_id, bui.label_text, bui.label_fluent_message_id, bui.help_text, bui.help_fluent_message_id, bui.placeholder_text, bui.placeholder_fluent_message_id, bui.control_spec
    FROM rev.brief_unit_input bui
    JOIN unit_map um ON um.old_id = bui.brief_unit_id
    RETURNING brief_unit_id
),
copy_input_choices AS (
    INSERT INTO rev.brief_unit_input_choices (brief_unit_id, label_text, label_fluent_message_id, value_text)
    SELECT um.new_id, buic.label_text, buic.label_fluent_message_id, buic.value_text
    FROM rev.brief_unit_input_choices buic
    JOIN unit_map um ON um.old_id = buic.brief_unit_id
    ORDER BY buic.id
    RETURNING id, brief_unit_id
),
copy_media AS (
    INSERT INTO rev.brief_unit_media (brief_unit_id)
    SELECT um.new_id
    FROM rev.brief_unit_media bum
    JOIN unit_map um ON um.old_id = bum.brief_unit_id
    RETURNING brief_unit_id
),
-- For media items we need to track old->new mapping for sources/posters
old_media_items AS (
    SELECT bumi.id, bumi.brief_unit_id, bumi.media_kind_id, bumi.source_type_id,
           row_number() OVER (PARTITION BY bumi.brief_unit_id ORDER BY bumi.id) AS rn
    FROM rev.brief_unit_media_items bumi
    WHERE bumi.brief_unit_id IN (SELECT old_id FROM unit_map)
),
new_media_items AS (
    INSERT INTO rev.brief_unit_media_items (brief_unit_id, media_kind_id, source_type_id)
    SELECT um.new_id, omi.media_kind_id, omi.source_type_id
    FROM old_media_items omi
    JOIN unit_map um ON um.old_id = omi.brief_unit_id
    ORDER BY um.new_id, omi.rn
    RETURNING id, brief_unit_id
),
-- Number new items to correlate with old items
numbered_new_items AS (
    SELECT nmi.id, nmi.brief_unit_id,
           row_number() OVER (PARTITION BY nmi.brief_unit_id ORDER BY nmi.id) AS rn
    FROM new_media_items nmi
),
media_item_map AS (
    SELECT omi.id AS old_id, nni.id AS new_id
    FROM old_media_items omi
    JOIN unit_map um ON um.old_id = omi.brief_unit_id
    JOIN numbered_new_items nni ON nni.brief_unit_id = um.new_id AND nni.rn = omi.rn
),
copy_item_asset_sources AS (
    INSERT INTO rev.brief_unit_media_item_asset_sources (media_item_id, asset_id)
    SELECT mim.new_id, bumias.asset_id
    FROM rev.brief_unit_media_item_asset_sources bumias
    JOIN media_item_map mim ON mim.old_id = bumias.media_item_id
    RETURNING media_item_id
),
copy_item_url_sources AS (
    INSERT INTO rev.brief_unit_media_item_url_sources (media_item_id, url)
    SELECT mim.new_id, bumius.url
    FROM rev.brief_unit_media_item_url_sources bumius
    JOIN media_item_map mim ON mim.old_id = bumius.media_item_id
    RETURNING media_item_id
),
copy_item_value_slot_sources AS (
    INSERT INTO rev.brief_unit_media_item_value_slot_sources (media_item_id, value_slot_id)
    SELECT mim.new_id, bumivss.value_slot_id
    FROM rev.brief_unit_media_item_value_slot_sources bumivss
    JOIN media_item_map mim ON mim.old_id = bumivss.media_item_id
    RETURNING media_item_id
),
-- Posters
old_posters AS (
    SELECT bumip.id, bumip.media_item_id, bumip.source_type_id
    FROM rev.brief_unit_media_item_posters bumip
    WHERE bumip.media_item_id IN (SELECT old_id FROM media_item_map)
),
new_posters AS (
    INSERT INTO rev.brief_unit_media_item_posters (media_item_id, source_type_id)
    SELECT mim.new_id, op.source_type_id
    FROM old_posters op
    JOIN media_item_map mim ON mim.old_id = op.media_item_id
    RETURNING id, media_item_id
),
poster_map AS (
    SELECT op.id AS old_id, np.id AS new_id
    FROM old_posters op
    JOIN media_item_map mim ON mim.old_id = op.media_item_id
    JOIN new_posters np ON np.media_item_id = mim.new_id
),
copy_poster_asset_sources AS (
    INSERT INTO rev.brief_unit_media_item_poster_asset_sources (poster_id, asset_id)
    SELECT pm.new_id, bumipas.asset_id
    FROM rev.brief_unit_media_item_poster_asset_sources bumipas
    JOIN poster_map pm ON pm.old_id = bumipas.poster_id
    RETURNING poster_id
),
copy_poster_url_sources AS (
    INSERT INTO rev.brief_unit_media_item_poster_url_sources (poster_id, url)
    SELECT pm.new_id, bumipus.url
    FROM rev.brief_unit_media_item_poster_url_sources bumipus
    JOIN poster_map pm ON pm.old_id = bumipus.poster_id
    RETURNING poster_id
),
copy_poster_value_slot_sources AS (
    INSERT INTO rev.brief_unit_media_item_poster_value_slot_sources (poster_id, value_slot_id)
    SELECT pm.new_id, bumipvss.value_slot_id
    FROM rev.brief_unit_media_item_poster_value_slot_sources bumipvss
    JOIN poster_map pm ON pm.old_id = bumipvss.poster_id
    RETURNING poster_id
),
-- Text references
copy_alt_texts AS (
    INSERT INTO rev.brief_unit_media_item_alt_texts (media_item_id, fluent_message_id)
    SELECT mim.new_id, bat.fluent_message_id
    FROM rev.brief_unit_media_item_alt_texts bat
    JOIN media_item_map mim ON mim.old_id = bat.media_item_id
    RETURNING media_item_id
),
copy_captions AS (
    INSERT INTO rev.brief_unit_media_item_captions (media_item_id, fluent_message_id)
    SELECT mim.new_id, bc.fluent_message_id
    FROM rev.brief_unit_media_item_captions bc
    JOIN media_item_map mim ON mim.old_id = bc.media_item_id
    RETURNING media_item_id
),
copy_titles AS (
    INSERT INTO rev.brief_unit_media_item_titles (media_item_id, fluent_message_id)
    SELECT mim.new_id, bt.fluent_message_id
    FROM rev.brief_unit_media_item_titles bt
    JOIN media_item_map mim ON mim.old_id = bt.media_item_id
    RETURNING media_item_id
)
SELECT count(*) AS copied_units FROM new_units;
