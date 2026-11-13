-- Migration 4: All versioned entity tables
-- Every non-lookup table uses id UUID DEFAULT uuidv7() PRIMARY KEY

-- ============================================================================
-- rev.tasks — Versioned function execution config
-- ============================================================================
CREATE TABLE rev.tasks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT NOT NULL,
    function TEXT NOT NULL,
    function_input JSON NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX tasks_versioned_object_idx ON rev.tasks (versioned_object_id);

-- ============================================================================
-- rev.steps — Versioned workflow steps
-- ============================================================================
CREATE TABLE rev.steps (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX steps_versioned_object_idx ON rev.steps (versioned_object_id);

-- ============================================================================
-- rev.step_tasks — Associative (Step <-> Task)
-- ============================================================================
CREATE TABLE rev.step_tasks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    step_id UUID NOT NULL REFERENCES rev.steps (id),
    task_id UUID NOT NULL REFERENCES rev.tasks (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX step_tasks_step_idx ON rev.step_tasks (step_id);
CREATE INDEX step_tasks_task_idx ON rev.step_tasks (task_id);

-- ============================================================================
-- rev.stages — Versioned execution stages
-- ============================================================================
CREATE TABLE rev.stages (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX stages_versioned_object_idx ON rev.stages (versioned_object_id);

-- ============================================================================
-- rev.stage_steps — Associative (Stage <-> Step)
-- ============================================================================
CREATE TABLE rev.stage_steps (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    stage_id UUID NOT NULL REFERENCES rev.stages (id),
    step_id UUID NOT NULL REFERENCES rev.steps (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX stage_steps_stage_idx ON rev.stage_steps (stage_id);
CREATE INDEX stage_steps_step_idx ON rev.stage_steps (step_id);

-- ============================================================================
-- rev.categories — Versioned playbook categorization
-- ============================================================================
CREATE TABLE rev.categories (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX categories_versioned_object_idx ON rev.categories (versioned_object_id);

-- ============================================================================
-- rev.hats — Versioned role/responsibility definitions
-- ============================================================================
CREATE TABLE rev.hats (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX hats_versioned_object_idx ON rev.hats (versioned_object_id);

-- ============================================================================
-- rev.hat_categories — Associative (Hat <-> Category)
-- ============================================================================
CREATE TABLE rev.hat_categories (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    hat_id UUID NOT NULL REFERENCES rev.hats (id),
    category_id UUID NOT NULL REFERENCES rev.categories (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (hat_id, category_id)
);

CREATE INDEX hat_categories_hat_idx ON rev.hat_categories (hat_id);
CREATE INDEX hat_categories_category_idx ON rev.hat_categories (category_id);

-- ============================================================================
-- rev.playbooks — Versioned workflow definitions
-- ============================================================================
CREATE TABLE rev.playbooks (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX playbooks_versioned_object_idx ON rev.playbooks (versioned_object_id);

-- ============================================================================
-- rev.playbook_categories — Associative (Playbook <-> Category)
-- ============================================================================
CREATE TABLE rev.playbook_categories (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    playbook_id UUID NOT NULL REFERENCES rev.playbooks (id),
    category_id UUID NOT NULL REFERENCES rev.categories (id),
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (playbook_id, category_id)
);

CREATE INDEX playbook_categories_playbook_idx ON rev.playbook_categories (playbook_id);
CREATE INDEX playbook_categories_category_idx ON rev.playbook_categories (category_id);

-- ============================================================================
-- rev.value_slots — Versioned typed value definitions
-- ============================================================================
CREATE TABLE rev.value_slots (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL UNIQUE REFERENCES rev.versioned_objects (id),
    title TEXT NOT NULL,
    description TEXT,
    constraints JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX value_slots_versioned_object_idx ON rev.value_slots (versioned_object_id);

-- ============================================================================
-- rev.deprecation_events — Audit log for deprecation state changes
-- ============================================================================
CREATE TABLE rev.deprecation_events (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    versioned_object_id UUID NOT NULL REFERENCES rev.versioned_objects (id),
    from_level_id SMALLINT REFERENCES rev.deprecation_levels (id),
    to_level_id SMALLINT NOT NULL REFERENCES rev.deprecation_levels (id),
    reason TEXT,
    deprecated_by_user_id UUID,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE INDEX deprecation_events_versioned_object_idx ON rev.deprecation_events (versioned_object_id);
CREATE INDEX deprecation_events_from_level_idx ON rev.deprecation_events (from_level_id)
    WHERE from_level_id IS NOT NULL;
CREATE INDEX deprecation_events_to_level_idx ON rev.deprecation_events (to_level_id);
CREATE INDEX deprecation_events_created_at_idx ON rev.deprecation_events (created_at);

-- ============================================================================
-- rev.llm_engines — Legacy engine records (referenced by fluent_message_engine_bodies)
-- ============================================================================
CREATE TABLE rev.llm_engines (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    maker_id SMALLINT NOT NULL REFERENCES rev.llm_engine_makers (id),
    key TEXT NOT NULL,
    version TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (maker_id, key, version)
);

CREATE INDEX llm_engines_maker_idx ON rev.llm_engines (maker_id);

-- Seed llm_engines
INSERT INTO rev.llm_engines (maker_id, key, version)
SELECT m.id, 'vits-ljspeech', NULL FROM rev.llm_engine_makers m WHERE m.key = 'tts'
ON CONFLICT (maker_id, key, version) DO NOTHING;

INSERT INTO rev.llm_engines (maker_id, key, version)
SELECT m.id, 'vits-vctk', NULL FROM rev.llm_engine_makers m WHERE m.key = 'tts'
ON CONFLICT (maker_id, key, version) DO NOTHING;

INSERT INTO rev.llm_engines (maker_id, key, version)
SELECT m.id, 'vits-melo-zh-en', NULL FROM rev.llm_engine_makers m WHERE m.key = 'tts'
ON CONFLICT (maker_id, key, version) DO NOTHING;

INSERT INTO rev.llm_engines (maker_id, key, version)
SELECT m.id, 'flan-t5-base-samsum', NULL FROM rev.llm_engine_makers m WHERE m.key = 'summarization'
ON CONFLICT (maker_id, key, version) DO NOTHING;

INSERT INTO rev.llm_engines (maker_id, key, version)
SELECT m.id, 'modernbert-base', NULL FROM rev.llm_engine_makers m WHERE m.key = 'mlm'
ON CONFLICT (maker_id, key, version) DO NOTHING;

-- ============================================================================
-- rev.model_releases — Specific models at providers (new taxonomy)
-- ============================================================================
CREATE TABLE rev.model_releases (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    lineage_id SMALLINT NOT NULL REFERENCES rev.lineages (id),
    provider_id SMALLINT NOT NULL REFERENCES rev.providers (id),
    tier TEXT NOT NULL,
    release TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (lineage_id, provider_id, tier, release)
);

CREATE INDEX model_releases_lineage_idx ON rev.model_releases (lineage_id);
CREATE INDEX model_releases_provider_idx ON rev.model_releases (provider_id);

-- Seed model releases: TTS (vits / self-hosted)
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'ljspeech', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'vits' AND p.key = 'self-hosted'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'vctk', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'vits' AND p.key = 'self-hosted'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'melo-zh-en', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'vits' AND p.key = 'self-hosted'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

-- Seed model releases: Summarization (flan-t5 / self-hosted)
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'base-samsum', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'flan-t5' AND p.key = 'self-hosted'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

-- Seed model releases: MLM (modernbert / self-hosted)
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'base', NULL
FROM rev.lineages l, rev.providers p
WHERE l.key = 'modernbert' AND p.key = 'self-hosted'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

-- Seed model releases: Claude 3.5 (anthropic)
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, tier, release
FROM rev.lineages l, rev.providers p,
    (VALUES ('sonnet', '20241022'), ('opus', '20241022'), ('haiku', '20241022')) AS v(tier, release)
WHERE l.key = 'claude-3-5' AND p.key = 'anthropic'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

-- Seed model releases: Claude 4.6 (anthropic)
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, 'sonnet', '20250929'
FROM rev.lineages l, rev.providers p
WHERE l.key = 'claude-4-6' AND p.key = 'anthropic'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

-- Seed model releases: Mistral (self-hosted)
INSERT INTO rev.model_releases (lineage_id, provider_id, tier, release)
SELECT l.id, p.id, '7b-instruct', 'v0.3'
FROM rev.lineages l, rev.providers p
WHERE l.key = 'mistral' AND p.key = 'self-hosted'
ON CONFLICT (lineage_id, provider_id, tier, release) DO NOTHING;

-- ============================================================================
-- rev.model_variants — Fine-tuned branches
-- ============================================================================
CREATE TABLE rev.model_variants (
    id UUID DEFAULT uuidv7() PRIMARY KEY,
    base_release_id UUID NOT NULL REFERENCES rev.model_releases (id),
    key TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (base_release_id, key)
);

CREATE INDEX model_variants_base_release_idx ON rev.model_variants (base_release_id);

-- ============================================================================
-- rev.build_release_chain — Helper function for model resolution
-- ============================================================================
CREATE OR REPLACE FUNCTION rev.build_release_chain(
    p_lineage_key TEXT,
    p_tier TEXT,
    p_release TEXT,
    p_provider_key TEXT
) RETURNS TEXT[] AS $$
DECLARE
    chain TEXT[] := '{}';
BEGIN
    IF p_release IS NOT NULL THEN
        chain := chain || format('@%s:%s/%s:%s', p_provider_key, p_lineage_key, p_tier, p_release);
    END IF;
    chain := chain || format('@%s:%s/%s', p_provider_key, p_lineage_key, p_tier);

    IF p_release IS NOT NULL THEN
        chain := chain || format('%s/%s:%s', p_lineage_key, p_tier, p_release);
    END IF;
    chain := chain || format('%s/%s', p_lineage_key, p_tier);

    chain := chain || p_lineage_key;

    RETURN chain;
END;
$$ LANGUAGE plpgsql IMMUTABLE;
