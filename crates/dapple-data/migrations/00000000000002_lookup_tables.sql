-- Migration 2: All lookup/enum tables with seed data
-- Lookup tables use SMALLINT GENERATED ALWAYS AS IDENTITY PKs

-- ============================================================================
-- rev.entity_types
-- ============================================================================
CREATE TABLE rev.entity_types (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.entity_types (key, description) VALUES
    ('task', 'Task definitions with function/execution config'),
    ('step', 'Workflow steps'),
    ('stage', 'Execution stages'),
    ('playbook', 'Workflow definitions'),
    ('prompt', 'LLM prompts with fluent message associations'),
    ('fluent_message', 'Localized/engine-specific text content'),
    ('category', 'Playbook categorization'),
    ('hat', 'Role/responsibility definitions'),
    ('brief', 'Interview/data-gathering definitions'),
    ('brief_group', 'Collections of briefs'),
    ('brief_pack', 'UI and specification bundles'),
    ('value_slot', 'Typed value definitions'),
    ('asset', 'Binary/media files'),
    ('directive', 'Conditional execution logic for stages and steps'),
    ('guide', 'Structured narrative documents that derive runtime objects'),
    ('inference_profile', 'Inference profile for journey-level model configuration'),
    ('shared_content', 'Shared content library path (common or internal scope)');

-- ============================================================================
-- rev.deprecation_levels
-- ============================================================================
CREATE TABLE rev.deprecation_levels (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.deprecation_levels (key, description) VALUES
    ('active', 'Object is fully supported and available for use'),
    ('deprecated', 'Object is scheduled for retirement; should not be used in new workflows'),
    ('retired', 'Object is no longer available for new references but existing references still work'),
    ('quarantined', 'Object is hidden from all queries except admin/audit');

-- ============================================================================
-- rev.dispatch_phases
-- ============================================================================
CREATE TABLE rev.dispatch_phases (
    code TEXT NOT NULL PRIMARY KEY,
    terminal BOOLEAN DEFAULT false NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.dispatch_phases (code, terminal) VALUES
    ('queued', false),
    ('running', false),
    ('failed', true),
    ('ok', true),
    ('canceled', true);

-- ============================================================================
-- rev.value_kinds
-- ============================================================================
CREATE TABLE rev.value_kinds (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.value_kinds (key) VALUES
    ('text'), ('int'), ('number'), ('bool'), ('json'), ('asset');

-- ============================================================================
-- rev.asset_kinds
-- ============================================================================
CREATE TABLE rev.asset_kinds (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.asset_kinds (key) VALUES
    ('image'), ('document'), ('audio'), ('video'), ('binary');

-- ============================================================================
-- rev.brief_unit_types
-- ============================================================================
CREATE TABLE rev.brief_unit_types (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.brief_unit_types (key) VALUES
    ('output'), ('markdown'), ('notice'), ('input'), ('media');

-- ============================================================================
-- rev.brief_input_control_types
-- ============================================================================
CREATE TABLE rev.brief_input_control_types (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.brief_input_control_types (key) VALUES
    ('text'), ('textarea'), ('select'), ('radio'),
    ('checkbox'), ('number'), ('date'), ('file');

-- ============================================================================
-- rev.brief_output_source_kinds
-- ============================================================================
CREATE TABLE rev.brief_output_source_kinds (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.brief_output_source_kinds (key) VALUES
    ('input'), ('files'), ('computed');

-- ============================================================================
-- rev.brief_media_kinds
-- ============================================================================
CREATE TABLE rev.brief_media_kinds (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.brief_media_kinds (key) VALUES
    ('image'), ('video'), ('audio'), ('document'), ('file');

-- ============================================================================
-- rev.brief_media_source_types
-- ============================================================================
CREATE TABLE rev.brief_media_source_types (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.brief_media_source_types (key) VALUES
    ('asset'), ('url'), ('value_slot');

-- ============================================================================
-- rev.fluent_message_locales
-- ============================================================================
CREATE TABLE rev.fluent_message_locales (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tag TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

-- ============================================================================
-- rev.llm_engine_makers (legacy, still referenced)
-- ============================================================================
CREATE TABLE rev.llm_engine_makers (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.llm_engine_makers (key) VALUES
    ('tts'), ('summarization'), ('mlm'), ('stub'), ('anthropic');

-- ============================================================================
-- rev.providers (model taxonomy)
-- ============================================================================
CREATE TABLE rev.providers (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    display_name TEXT,
    api_config JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO rev.providers (key, display_name) VALUES
    ('anthropic', 'Anthropic API'),
    ('openai', 'OpenAI API'),
    ('mistral', 'Mistral AI'),
    ('self-hosted', 'Self-Hosted'),
    ('stub', 'Stub/Mock');

-- ============================================================================
-- rev.lineages (model taxonomy)
-- ============================================================================
CREATE TABLE rev.lineages (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO rev.lineages (key, description) VALUES
    ('vits', 'VITS text-to-speech models'),
    ('flan-t5', 'FLAN-T5 summarization models'),
    ('modernbert', 'ModernBERT masked language models'),
    ('claude-3-5', 'Claude 3.5 family (pseudo-XML prompting)'),
    ('tts', 'Text-to-speech models'),
    ('summarization', 'Summarization models'),
    ('mlm', 'Masked language models'),
    ('stub', 'Stub/mock models for testing'),
    ('claude-4-6', 'Claude 4.6 family'),
    ('mistral', 'Mistral instruct models (local Candle inference)');

-- ============================================================================
-- rev.guide_tags
-- ============================================================================
CREATE TABLE rev.guide_tags (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    brief_unit_key TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.guide_tags (key, description, brief_unit_key) VALUES
    ('agent', 'Content visible to AI agents during execution', 'agent_context'),
    ('product', 'Content visible to product consumers and end users', 'product_context'),
    ('coder', 'Content visible to code-generation agents', 'coder_context'),
    ('auditor', 'Content visible to auditing/compliance consumers', 'auditor_context'),
    ('planner', 'Content visible to planning/orchestration agents', 'planner_context'),
    ('universal', 'Content visible to all consumers (default for untagged blocks)', NULL);

-- ============================================================================
-- rev.guide_block_directive_types
-- ============================================================================
CREATE TABLE rev.guide_block_directive_types (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

INSERT INTO rev.guide_block_directive_types (key, description) VALUES
    ('prose', 'Untagged or tagged prose content (:::block or plain markdown)'),
    ('stage', 'Playbook stage boundary (:::stage)'),
    ('step', 'Playbook step boundary (:::step)'),
    ('nushell', 'Nushell task script (:::nushell)'),
    ('brief_pack', 'Brief pack definition (:::brief-pack)'),
    ('inference_profile', 'Inference profile definition (:::inference-profile)'),
    ('ref', 'Reference to an existing object (:::ref)');

-- ============================================================================
-- rev.shared_scopes
-- ============================================================================
CREATE TABLE rev.shared_scopes (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO rev.shared_scopes (key, description) VALUES
    ('common', 'Shared content visible to all guides (public, partner, internal)'),
    ('internal', 'Shared content visible only to internal guides');

-- ============================================================================
-- rev.render_profiles
-- ============================================================================
CREATE TABLE rev.render_profiles (
    id SMALLINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    description TEXT,
    tag_filter JSONB NOT NULL DEFAULT '[]'::jsonb,
    style_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO rev.render_profiles (key, description, tag_filter, style_config) VALUES
    ('consumer', 'Polished consumer-facing PDF', '["universal", "product"]',
     '{"cover_page": true, "branding": "consumer", "watermark": null}'),
    ('agent', 'Technical agent-facing PDF', '["universal", "agent"]',
     '{"cover_page": true, "branding": "technical", "watermark": null}'),
    ('internal', 'Internal PDF with all content', '["universal", "agent", "product", "coder", "auditor", "planner"]',
     '{"cover_page": true, "branding": "internal", "watermark": "INTERNAL"}');

-- ============================================================================
-- web.role_types
-- ============================================================================
CREATE TABLE web.role_types (
    role TEXT PRIMARY KEY CHECK (LENGTH(role) <= 64)
);

INSERT INTO web.role_types (role) VALUES ('user'), ('admin');
