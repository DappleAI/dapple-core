-- Migration 12: Guide system
-- guides, chapters, blocks, tags, cards, renders, shared content

-- ============================================================================
-- rev.guides — Versioned narrative documents
-- ============================================================================
CREATE TABLE rev.guides (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT,
    author TEXT,
    classification TEXT,
    source_path TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guides_versioned_object_idx ON rev.guides (versioned_object_id);

-- ============================================================================
-- rev.guide_chapters — Owned dependent (maps to .md files)
-- ============================================================================
CREATE TABLE rev.guide_chapters (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    parent_chapter_id UUID REFERENCES rev.guide_chapters (id),
    title TEXT NOT NULL,
    source_file TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guide_chapters_guide_idx ON rev.guide_chapters (guide_id);
CREATE INDEX guide_chapters_parent_idx ON rev.guide_chapters (parent_chapter_id)
    WHERE parent_chapter_id IS NOT NULL;

-- ============================================================================
-- rev.guide_blocks — Atomic content unit
-- ============================================================================
CREATE TABLE rev.guide_blocks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    chapter_id UUID NOT NULL REFERENCES rev.guide_chapters (id),
    directive_type_id SMALLINT NOT NULL REFERENCES rev.guide_block_directive_types (id),
    fluent_message_id UUID REFERENCES rev.fluent_messages (id),
    name TEXT,
    content TEXT,
    attributes JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX guide_blocks_chapter_idx ON rev.guide_blocks (chapter_id);
CREATE INDEX guide_blocks_directive_type_idx ON rev.guide_blocks (directive_type_id);
CREATE INDEX guide_blocks_fluent_message_idx ON rev.guide_blocks (fluent_message_id)
    WHERE fluent_message_id IS NOT NULL;

-- ============================================================================
-- rev.guide_block_tags — Associative (Block <-> Tag)
-- ============================================================================
CREATE TABLE rev.guide_block_tags (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    block_id UUID NOT NULL REFERENCES rev.guide_blocks (id),
    tag_id SMALLINT NOT NULL REFERENCES rev.guide_tags (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (block_id, tag_id)
);

CREATE INDEX guide_block_tags_block_idx ON rev.guide_block_tags (block_id);
CREATE INDEX guide_block_tags_tag_idx ON rev.guide_block_tags (tag_id);

-- ============================================================================
-- rev.guide_assets — Associative (Guide <-> Asset)
-- ============================================================================
CREATE TABLE rev.guide_assets (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    role TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, asset_id)
);

CREATE INDEX guide_assets_guide_idx ON rev.guide_assets (guide_id);
CREATE INDEX guide_assets_asset_idx ON rev.guide_assets (asset_id);
CREATE INDEX guide_assets_role_idx ON rev.guide_assets (role);

-- ============================================================================
-- rev.guide_cards — Promotional card per guide version
-- ============================================================================
CREATE TABLE rev.guide_cards (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL UNIQUE REFERENCES rev.guides (id),
    subtitle TEXT,
    card_image_asset_id UUID REFERENCES rev.assets (id),
    category TEXT,
    color TEXT,
    accent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX guide_cards_guide_idx ON rev.guide_cards (guide_id);
CREATE INDEX guide_cards_category_idx ON rev.guide_cards (category);
CREATE INDEX guide_cards_card_image_asset_idx ON rev.guide_cards (card_image_asset_id)
    WHERE card_image_asset_id IS NOT NULL;

-- ============================================================================
-- rev.guide_renders — Render cache
-- ============================================================================
CREATE TABLE rev.guide_renders (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    render_profile_id SMALLINT NOT NULL REFERENCES rev.render_profiles (id),
    asset_id UUID NOT NULL REFERENCES rev.assets (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, render_profile_id)
);

CREATE INDEX guide_renders_guide_idx ON rev.guide_renders (guide_id);
CREATE INDEX guide_renders_render_profile_idx ON rev.guide_renders (render_profile_id);
CREATE INDEX guide_renders_asset_idx ON rev.guide_renders (asset_id);

-- ============================================================================
-- web.user_favorite_guides — Append-only with removal tracking
-- ============================================================================
CREATE TABLE web.user_favorite_guides (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES rev.user_details (user_id),
    guide_path_id UUID NOT NULL REFERENCES rev.guide_path_details (path_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX user_favorite_guides_lookup ON web.user_favorite_guides (user_id, guide_path_id, id DESC);

CREATE TABLE web.user_favorite_guides_removed (
    user_favorite_guides_id UUID PRIMARY KEY REFERENCES web.user_favorite_guides (id),
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================================
-- rev.guide_shared_refs — Associative (Guide <-> SharedContent)
-- ============================================================================
CREATE TABLE rev.guide_shared_refs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    guide_id UUID NOT NULL REFERENCES rev.guides (id),
    object_path_id UUID NOT NULL REFERENCES rev.object_paths (id),
    ref_key TEXT NOT NULL,
    directive_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guide_id, ref_key)
);

CREATE INDEX guide_shared_refs_guide_idx ON rev.guide_shared_refs (guide_id);
CREATE INDEX guide_shared_refs_object_path_idx ON rev.guide_shared_refs (object_path_id);

-- ============================================================================
-- rev.shared_content_objects — Links shared content to derived objects
-- ============================================================================
CREATE TABLE rev.shared_content_objects (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    shared_path_id UUID NOT NULL REFERENCES rev.object_paths (id),
    target_path_id UUID NOT NULL REFERENCES rev.object_paths (id),
    object_kind TEXT NOT NULL,
    object_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX shared_content_objects_shared_path_idx ON rev.shared_content_objects (shared_path_id);
CREATE INDEX shared_content_objects_target_path_idx ON rev.shared_content_objects (target_path_id);
