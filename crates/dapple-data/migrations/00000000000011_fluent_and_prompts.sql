-- Migration 11: Fluent messages, prompts, inference profiles

-- ============================================================================
-- rev.fluent_messages — Versioned localization/engine-specific text
-- ============================================================================
CREATE TABLE rev.fluent_messages (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX fluent_messages_versioned_object_idx ON rev.fluent_messages (versioned_object_id);

-- ============================================================================
-- rev.fluent_message_bodies — Per-locale message body
-- ============================================================================
CREATE TABLE rev.fluent_message_bodies (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    fluent_message_id UUID NOT NULL REFERENCES rev.fluent_messages (id),
    locale_id SMALLINT NOT NULL REFERENCES rev.fluent_message_locales (id),
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (fluent_message_id, locale_id)
);

CREATE INDEX fluent_message_bodies_message_idx ON rev.fluent_message_bodies (fluent_message_id);
CREATE INDEX fluent_message_bodies_locale_idx ON rev.fluent_message_bodies (locale_id);

-- ============================================================================
-- rev.fluent_message_engine_bodies — Per-engine variant
-- ============================================================================
CREATE TABLE rev.fluent_message_engine_bodies (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    fluent_message_id UUID NOT NULL REFERENCES rev.fluent_messages (id),
    model_release_id UUID REFERENCES rev.model_releases (id),
    model_variant_id UUID REFERENCES rev.model_variants (id),
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT chk_one_model CHECK (
        (model_release_id IS NOT NULL)::int +
        (model_variant_id IS NOT NULL)::int = 1
    )
);

CREATE INDEX fluent_message_engine_bodies_message_release_idx
    ON rev.fluent_message_engine_bodies (fluent_message_id, model_release_id);
CREATE INDEX fluent_message_engine_bodies_release_idx ON rev.fluent_message_engine_bodies (model_release_id)
    WHERE model_release_id IS NOT NULL;
CREATE INDEX fluent_message_engine_bodies_variant_idx ON rev.fluent_message_engine_bodies (model_variant_id)
    WHERE model_variant_id IS NOT NULL;

-- ============================================================================
-- rev.prompts — Versioned LLM prompts
-- ============================================================================
CREATE TABLE rev.prompts (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX prompts_versioned_object_idx ON rev.prompts (versioned_object_id);

-- ============================================================================
-- rev.prompt_fluent_messages — Associative (Prompt <-> FluentMessage)
-- ============================================================================
CREATE TABLE rev.prompt_fluent_messages (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    prompt_id UUID NOT NULL REFERENCES rev.prompts (id),
    fluent_message_id UUID NOT NULL REFERENCES rev.fluent_messages (id),
    role TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (prompt_id, fluent_message_id)
);

CREATE INDEX prompt_fluent_messages_prompt_idx ON rev.prompt_fluent_messages (prompt_id);
CREATE INDEX prompt_fluent_messages_message_idx ON rev.prompt_fluent_messages (fluent_message_id);

-- ============================================================================
-- rev.task_prompts — Associative (Task <-> Prompt)
-- ============================================================================
CREATE TABLE rev.task_prompts (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES rev.tasks (id),
    prompt_id UUID NOT NULL REFERENCES rev.prompts (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (task_id, prompt_id)
);

CREATE INDEX task_prompts_task_idx ON rev.task_prompts (task_id);
CREATE INDEX task_prompts_prompt_idx ON rev.task_prompts (prompt_id);

-- ============================================================================
-- rev.prompt_responses — Audit trail for LLM responses
-- ============================================================================
CREATE TABLE rev.prompt_responses (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    attempt_id UUID NOT NULL REFERENCES rev.dispatch_task_attempts (id),
    prompt_id UUID NOT NULL REFERENCES rev.prompts (id),
    playbook_task_id UUID NOT NULL REFERENCES rev.playbook_tasks (id),
    maker_key TEXT NOT NULL,
    engine_key TEXT NOT NULL,
    engine_version TEXT,
    request_messages JSONB NOT NULL,
    response_content TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    latency_ms INTEGER NOT NULL DEFAULT 0,
    finish_reason TEXT NOT NULL DEFAULT 'stop',
    started_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX prompt_responses_attempt_idx ON rev.prompt_responses (attempt_id);
CREATE INDEX prompt_responses_prompt_idx ON rev.prompt_responses (prompt_id);
CREATE INDEX prompt_responses_playbook_task_idx ON rev.prompt_responses (playbook_task_id);
CREATE INDEX prompt_responses_started_at_idx ON rev.prompt_responses (started_at);

-- ============================================================================
-- rev.inference_profiles — Versioned model configuration
-- ============================================================================
CREATE TABLE rev.inference_profiles (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX inference_profiles_versioned_object_idx ON rev.inference_profiles (versioned_object_id);

-- ============================================================================
-- rev.inference_profile_configs — Prompt → Model mapping
-- ============================================================================
CREATE TABLE rev.inference_profile_configs (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    inference_profile_id UUID NOT NULL REFERENCES rev.inference_profiles (id),
    prompt_path_object_id UUID NOT NULL,
    model_release_id UUID REFERENCES rev.model_releases (id),
    model_variant_id UUID REFERENCES rev.model_variants (id),
    priority INT NOT NULL DEFAULT 0,
    CONSTRAINT chk_one_model CHECK (
        (model_release_id IS NOT NULL)::int +
        (model_variant_id IS NOT NULL)::int = 1
    )
);

CREATE INDEX inference_profile_configs_profile_idx ON rev.inference_profile_configs (inference_profile_id);
CREATE INDEX inference_profile_configs_prompt_idx ON rev.inference_profile_configs (prompt_path_object_id);
CREATE INDEX inference_profile_configs_release_idx ON rev.inference_profile_configs (model_release_id)
    WHERE model_release_id IS NOT NULL;
CREATE INDEX inference_profile_configs_variant_idx ON rev.inference_profile_configs (model_variant_id)
    WHERE model_variant_id IS NOT NULL;

-- ============================================================================
-- rev.journey_inference_profiles — Associative (Journey <-> InferenceProfile)
-- ============================================================================
CREATE TABLE rev.journey_inference_profiles (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    journey_id UUID NOT NULL REFERENCES rev.journeys (id),
    inference_profile_id UUID NOT NULL REFERENCES rev.inference_profiles (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (journey_id, inference_profile_id)
);

CREATE INDEX journey_inference_profiles_journey_idx ON rev.journey_inference_profiles (journey_id);
CREATE INDEX journey_inference_profiles_profile_idx ON rev.journey_inference_profiles (inference_profile_id);

-- ============================================================================
-- Now add FK constraints for fluent_message_id references deferred from migration 8
-- ============================================================================
ALTER TABLE rev.brief_unit_markdown
    ADD CONSTRAINT brief_unit_markdown_fluent_fk
    FOREIGN KEY (fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_notice
    ADD CONSTRAINT brief_unit_notice_title_fluent_fk
    FOREIGN KEY (title_fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_notice
    ADD CONSTRAINT brief_unit_notice_detail_fluent_fk
    FOREIGN KEY (detail_fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_input
    ADD CONSTRAINT brief_unit_input_label_fluent_fk
    FOREIGN KEY (label_fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_input
    ADD CONSTRAINT brief_unit_input_help_fluent_fk
    FOREIGN KEY (help_fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_input
    ADD CONSTRAINT brief_unit_input_placeholder_fluent_fk
    FOREIGN KEY (placeholder_fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_input_choices
    ADD CONSTRAINT brief_unit_input_choices_label_fluent_fk
    FOREIGN KEY (label_fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_media_item_alt_texts
    ADD CONSTRAINT brief_unit_media_item_alt_texts_fluent_fk
    FOREIGN KEY (fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_media_item_captions
    ADD CONSTRAINT brief_unit_media_item_captions_fluent_fk
    FOREIGN KEY (fluent_message_id) REFERENCES rev.fluent_messages (id);

ALTER TABLE rev.brief_unit_media_item_titles
    ADD CONSTRAINT brief_unit_media_item_titles_fluent_fk
    FOREIGN KEY (fluent_message_id) REFERENCES rev.fluent_messages (id);

-- Add indexes for deferred fluent_message_id FKs (columns from migration 8, FKs added above)
CREATE INDEX brief_unit_markdown_fluent_idx ON rev.brief_unit_markdown (fluent_message_id)
    WHERE fluent_message_id IS NOT NULL;
CREATE INDEX brief_unit_notice_title_fluent_idx ON rev.brief_unit_notice (title_fluent_message_id)
    WHERE title_fluent_message_id IS NOT NULL;
CREATE INDEX brief_unit_notice_detail_fluent_idx ON rev.brief_unit_notice (detail_fluent_message_id)
    WHERE detail_fluent_message_id IS NOT NULL;
CREATE INDEX brief_unit_input_label_fluent_idx ON rev.brief_unit_input (label_fluent_message_id)
    WHERE label_fluent_message_id IS NOT NULL;
CREATE INDEX brief_unit_input_help_fluent_idx ON rev.brief_unit_input (help_fluent_message_id)
    WHERE help_fluent_message_id IS NOT NULL;
CREATE INDEX brief_unit_input_placeholder_fluent_idx ON rev.brief_unit_input (placeholder_fluent_message_id)
    WHERE placeholder_fluent_message_id IS NOT NULL;
CREATE INDEX brief_unit_input_choices_label_fluent_idx ON rev.brief_unit_input_choices (label_fluent_message_id)
    WHERE label_fluent_message_id IS NOT NULL;

-- Add asset FK constraints deferred from migration 8 (assets exist now via migration 10)
ALTER TABLE rev.brief_unit_media_item_asset_sources
    ADD CONSTRAINT brief_unit_media_item_asset_sources_asset_fk
    FOREIGN KEY (asset_id) REFERENCES rev.assets (id);

ALTER TABLE rev.brief_unit_media_item_poster_asset_sources
    ADD CONSTRAINT brief_unit_media_item_poster_asset_sources_asset_fk
    FOREIGN KEY (asset_id) REFERENCES rev.assets (id);

-- Add asset FK for dispatch_intent_binding_asset_items (deferred from migration 9)
ALTER TABLE rev.dispatch_intent_binding_asset_items
    ADD CONSTRAINT dispatch_intent_binding_asset_items_asset_fk
    FOREIGN KEY (asset_id) REFERENCES rev.assets (id);

CREATE INDEX dispatch_intent_binding_asset_items_asset_idx ON rev.dispatch_intent_binding_asset_items (asset_id);
