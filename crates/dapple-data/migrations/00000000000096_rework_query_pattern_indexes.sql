-- Migration 96: rework indexes around the settled query patterns.
--
-- Derived from a predicate audit of every generated query in
-- crates/dapple-data/assets/.sql (519 files), cross-checked against live
-- pg_stat_user_indexes usage. Three moves, applied table by table:
--   1. CREATE compound / covering / expression indexes that match the real
--      WHERE + JOIN + ORDER BY shapes (index-only where the projection is small).
--   2. DROP single-column indexes superseded by (1) via the leftmost-prefix rule.
--   3. DROP indexes that duplicate a UNIQUE constraint's index outright, or that
--      index a column no query ever filters or joins on (lookup-table FKs,
--      display-only fluent references). These were the "before the queries
--      settled" guesses.
--
-- A compound index (a, b) serves every access that the old (a) index served,
-- including FK cascade checks, so no coverage is lost by the prefix drops.

------------------------------------------------------------------------------
-- A. brief_unit_input: the one true seq-scan hotspot (14.6k seq scans).
-- Every binding-status query joins value_slots through a JSONB extraction:
--   ON bui.control_spec ? 'value_slot_id'
--  AND vs.id = (bui.control_spec ->> 'value_slot_id')::UUID
-- No column index can serve that; this expression index can, and its partial
-- predicate is exactly the `?` guard the queries use.
------------------------------------------------------------------------------

CREATE INDEX brief_unit_input_value_slot_expr_idx
    ON rev.brief_unit_input (((control_spec ->> 'value_slot_id')::UUID))
    WHERE control_spec ? 'value_slot_id';

-- control_type_id / *_fluent_message_id are only ever the fact side of a
-- lookup join, never a predicate.
DROP INDEX rev.brief_unit_input_control_type_idx;
DROP INDEX rev.brief_unit_input_label_fluent_idx;
DROP INDEX rev.brief_unit_input_help_fluent_idx;
DROP INDEX rev.brief_unit_input_placeholder_fluent_idx;

------------------------------------------------------------------------------
-- B. Dispatch runtime (dispatches: 1.3M idx scans; the binding rollups and
-- autodispatch sweeps fan out through these tables on every poll).
------------------------------------------------------------------------------

-- journey fan-out + ORDER BY d.id [DESC] pagination, index-only for the
-- rollup CTEs that only read d.id.
CREATE INDEX dispatches_journey_id_idx ON rev.dispatches (journey_id, id);
DROP INDEX rev.dispatches_journey_idx;
-- "by user" paths filter journeys.user_id and join down; nothing filters
-- dispatches.user_id directly.
DROP INDEX rev.dispatches_user_idx;

CREATE INDEX dispatch_steps_stage_lookup_idx
    ON rev.dispatch_steps (dispatch_stage_id, id) INCLUDE (stage_step_id);
DROP INDEX rev.dispatch_steps_dispatch_stage_idx;

CREATE INDEX dispatch_tasks_step_lookup_idx
    ON rev.dispatch_tasks (dispatch_step_id, id) INCLUDE (step_task_id, created_at);
DROP INDEX rev.dispatch_tasks_dispatch_step_idx;

-- The latest-attempt LATERAL (6 listing queries) projects only a.id, which the
-- old index did not carry: every LIMIT 1 probe paid a heap fetch.
DROP INDEX rev.dispatch_task_attempts_step_task_created_idx;
CREATE INDEX dispatch_task_attempts_step_task_created_idx
    ON rev.dispatch_task_attempts (dispatch_step_id, task_id, created_at DESC)
    INCLUDE (id);
DROP INDEX rev.dispatch_task_attempts_dispatch_step_idx;
-- task_id only ever appears alongside dispatch_step_id (2nd key above).
DROP INDEX rev.dispatch_task_attempts_task_idx;

-- attempt_latest / orphan sweeps sort by (attempt_id, id DESC) — a sort the
-- created_at-keyed index cannot supply.
CREATE INDEX dispatch_task_attempt_events_attempt_id_idx
    ON rev.dispatch_task_attempt_events (attempt_id, id DESC) INCLUDE (phase_code);
DROP INDEX rev.dispatch_task_attempt_events_attempt_created_id_idx;
CREATE INDEX dispatch_task_attempt_events_attempt_created_id_idx
    ON rev.dispatch_task_attempt_events (attempt_id, created_at DESC, id DESC)
    INCLUDE (phase_code);
-- completed-task-context rollup: only 'ok' events with payloads qualify.
CREATE INDEX dispatch_task_attempt_events_ok_context_idx
    ON rev.dispatch_task_attempt_events (attempt_id, created_at DESC, id DESC)
    WHERE phase_code = 'ok' AND context_data IS NOT NULL;
DROP INDEX rev.dispatch_task_attempt_events_attempt_idx;
DROP INDEX rev.dispatch_task_attempt_events_phase_idx;

-- Binding-status rollup probe: slot -> dispatches holding it, with the
-- ORDER BY dib.id DESC LIMIT 1 `blocked` gate reading produced_empty.
CREATE INDEX dispatch_intent_bindings_slot_dispatch_idx
    ON rev.dispatch_intent_bindings (value_slot_id, dispatch_id, id DESC)
    INCLUDE (produced_empty);
DROP INDEX rev.dispatch_intent_bindings_value_slot_idx;
-- prefix of dispatch_intent_bindings_dispatch_id_value_slot_id_key
DROP INDEX rev.dispatch_intent_bindings_dispatch_idx;
-- prefix of dispatch_stages_dispatch_id_playbook_stage_id_key (index-only for
-- the fork_cutoff_filter fragment already)
DROP INDEX rev.dispatch_stages_dispatch_idx;

-- Typed binding item tables: reads are `binding_id = X ORDER BY id`; give the
-- ordering in the key. asset/dataset variants already have a composite UNIQUE.
CREATE INDEX dispatch_intent_binding_text_items_binding_id_idx
    ON rev.dispatch_intent_binding_text_items (dispatch_intent_binding_id, id);
DROP INDEX rev.dispatch_intent_binding_text_items_binding_idx;
CREATE INDEX dispatch_intent_binding_json_items_binding_id_idx
    ON rev.dispatch_intent_binding_json_items (dispatch_intent_binding_id, id);
DROP INDEX rev.dispatch_intent_binding_json_items_binding_idx;
CREATE INDEX dispatch_intent_binding_int_items_binding_id_idx
    ON rev.dispatch_intent_binding_int_items (dispatch_intent_binding_id, id);
DROP INDEX rev.dispatch_intent_binding_int_items_binding_idx;
CREATE INDEX dispatch_intent_binding_number_items_binding_id_idx
    ON rev.dispatch_intent_binding_number_items (dispatch_intent_binding_id, id);
DROP INDEX rev.dispatch_intent_binding_number_items_binding_idx;
CREATE INDEX dispatch_intent_binding_bool_items_binding_id_idx
    ON rev.dispatch_intent_binding_bool_items (dispatch_intent_binding_id, id);
DROP INDEX rev.dispatch_intent_binding_bool_items_binding_idx;
DROP INDEX rev.dispatch_intent_binding_asset_items_binding_idx;
DROP INDEX rev.dispatch_intent_binding_dataset_items_binding_idx;

------------------------------------------------------------------------------
-- C. Journeys, preferences, prompt responses, job/queue tables.
------------------------------------------------------------------------------

-- chain-root listing: predecessor IS NULL + principal + guide, newest first.
CREATE INDEX journeys_chain_roots_idx
    ON rev.journeys (principal_id, guide_id, created_at DESC)
    WHERE predecessor_journey_id IS NULL;
-- descendant walk + leaf NOT EXISTS become index-only with id in the key.
DROP INDEX rev.journeys_predecessor_journey_idx;
CREATE INDEX journeys_predecessor_journey_idx
    ON rev.journeys (predecessor_journey_id, id)
    WHERE predecessor_journey_id IS NOT NULL;
-- paginated list endpoints all ORDER BY id DESC under a single-column filter.
DROP INDEX rev.journeys_owner_company_idx;
CREATE INDEX journeys_owner_company_idx ON rev.journeys (owner_company_id, id DESC);
DROP INDEX rev.journeys_user_idx;
CREATE INDEX journeys_user_idx ON rev.journeys (user_id, id DESC);
DROP INDEX rev.journeys_playbook_idx;
CREATE INDEX journeys_playbook_idx ON rev.journeys (playbook_id, id DESC);

-- viewer_prefs CTE: key already matched the DISTINCT ON; the projection
-- (name, hidden_at — the latter also a filter) forced a heap fetch per row.
DROP INDEX rev.principal_journey_preferences_latest_idx;
CREATE INDEX principal_journey_preferences_latest_idx
    ON rev.principal_journey_preferences (principal_id, journey_id, id DESC)
    INCLUDE (name, hidden_at);

-- prefixes of their composite UNIQUE constraints (which are index-only for
-- the journey_pinned_packs walk).
DROP INDEX rev.journey_brief_packs_journey_idx;
DROP INDEX rev.journey_inference_profiles_journey_idx;

-- per-attempt read sorts (started_at, id); id is uuidv7 so the global
-- started_at index answered nothing the pkey didn't.
CREATE INDEX prompt_responses_attempt_started_idx
    ON rev.prompt_responses (attempt_id, started_at, id);
DROP INDEX rev.prompt_responses_attempt_idx;
DROP INDEX rev.prompt_responses_started_at_idx;
DROP INDEX rev.prompt_responses_prompt_idx;
DROP INDEX rev.prompt_responses_step_task_id_idx;

DROP INDEX rev.llm_usages_event_idx;
CREATE INDEX llm_usages_event_idx
    ON rev.dispatch_task_attempt_event_llm_usages (event_id, id);

-- pending-sync sweep: evaluate the next_retry_at backoff filter and read the
-- whole projection inside the index instead of heap-fetching rows only to
-- discard the not-yet-due ones.
DROP INDEX rev.lago_event_sync_pending_idx;
CREATE INDEX lago_event_sync_pending_idx
    ON rev.lago_event_sync (created_at)
    INCLUDE (next_retry_at, resource_type_id, resource_id, lago_transaction_id, retry_count)
    WHERE sent_at IS NULL;

-- favorites: keys already matched; created_at is the only projected column.
DROP INDEX web.user_favorite_guides_lookup;
CREATE INDEX user_favorite_guides_lookup
    ON web.user_favorite_guides (user_id, guide_path_id, id DESC)
    INCLUDE (created_at);
DROP INDEX web.user_favorite_journey_chains_lookup;
CREATE INDEX user_favorite_journey_chains_lookup
    ON web.user_favorite_journey_chains (user_id, chain_root_journey_id, id DESC)
    INCLUDE (created_at);

-- claim_next_background_job's job_kind disjunction is non-sargable, so the
-- (job_kind, status) index cannot be used by the only query that mentions
-- job_kind; nothing filters target_id. background_jobs_claim_idx (the partial
-- scheduled_at index) is the one the claim actually uses — kept.
DROP INDEX rev.background_jobs_kind_status_idx;
DROP INDEX rev.background_jobs_target_idx;

------------------------------------------------------------------------------
-- D. Playbook structure: the (parent, ord) UNIQUE constraints are the access
-- path for every stage/step/task fan-out (50k-156k scans each) plus the
-- ubiquitous ORDER BY ord. Rebuild them with the two projected columns as
-- INCLUDE payload so the fan-outs go index-only. ADD CONSTRAINT ... USING
-- INDEX renames the new index to the constraint name, so names stay stable.
------------------------------------------------------------------------------

CREATE UNIQUE INDEX stage_steps_stage_ord_new_idx
    ON rev.stage_steps (stage_id, ord) INCLUDE (id, step_id);
ALTER TABLE rev.stage_steps
    DROP CONSTRAINT stage_steps_stage_id_ord_unique;
ALTER TABLE rev.stage_steps
    ADD CONSTRAINT stage_steps_stage_id_ord_unique
    UNIQUE USING INDEX stage_steps_stage_ord_new_idx;
DROP INDEX rev.stage_steps_stage_idx;

CREATE UNIQUE INDEX step_tasks_step_ord_new_idx
    ON rev.step_tasks (step_id, ord) INCLUDE (id, task_id);
ALTER TABLE rev.step_tasks
    DROP CONSTRAINT step_tasks_step_id_ord_unique;
ALTER TABLE rev.step_tasks
    ADD CONSTRAINT step_tasks_step_id_ord_unique
    UNIQUE USING INDEX step_tasks_step_ord_new_idx;
DROP INDEX rev.step_tasks_step_idx;

CREATE UNIQUE INDEX playbook_stages_playbook_ord_new_idx
    ON rev.playbook_stages (playbook_id, ord) INCLUDE (id, stage_id);
ALTER TABLE rev.playbook_stages
    DROP CONSTRAINT playbook_stages_playbook_id_ord_unique;
ALTER TABLE rev.playbook_stages
    ADD CONSTRAINT playbook_stages_playbook_id_ord_unique
    UNIQUE USING INDEX playbook_stages_playbook_ord_new_idx;
DROP INDEX rev.playbook_stages_playbook_idx;

-- Overlay cascade (12 queries read exactly parent_id -> value_slot_path_id,
-- visibility): carry visibility so the whole cascade is index-only. NOTE:
-- deliberately NOT partial on visibility = 'expose' — the ranked CTEs must see
-- 'mask' rows for priority override semantics.
CREATE UNIQUE INDEX playbook_stage_overlays_stage_path_new_idx
    ON rev.playbook_stage_overlays (playbook_stage_id, value_slot_path_id)
    INCLUDE (visibility);
ALTER TABLE rev.playbook_stage_overlays
    DROP CONSTRAINT playbook_stage_overlays_playbook_stage_id_value_slot_path_i_key;
ALTER TABLE rev.playbook_stage_overlays
    ADD CONSTRAINT playbook_stage_overlays_playbook_stage_id_value_slot_path_i_key
    UNIQUE USING INDEX playbook_stage_overlays_stage_path_new_idx;
DROP INDEX rev.playbook_stage_overlays_stage_idx;

CREATE UNIQUE INDEX stage_step_overlays_step_path_new_idx
    ON rev.stage_step_overlays (stage_step_id, value_slot_path_id)
    INCLUDE (visibility);
ALTER TABLE rev.stage_step_overlays
    DROP CONSTRAINT stage_step_overlays_stage_step_id_value_slot_path_id_key;
ALTER TABLE rev.stage_step_overlays
    ADD CONSTRAINT stage_step_overlays_stage_step_id_value_slot_path_id_key
    UNIQUE USING INDEX stage_step_overlays_step_path_new_idx;

CREATE UNIQUE INDEX step_task_overlays_task_path_new_idx
    ON rev.step_task_overlays (step_task_id, value_slot_path_id)
    INCLUDE (visibility);
ALTER TABLE rev.step_task_overlays
    DROP CONSTRAINT step_task_overlays_step_task_id_value_slot_path_id_key;
ALTER TABLE rev.step_task_overlays
    ADD CONSTRAINT step_task_overlays_step_task_id_value_slot_path_id_key
    UNIQUE USING INDEX step_task_overlays_task_path_new_idx;

-- task -> prompts fan-out with ORDER BY (t.id, tp.id) in six listing queries.
CREATE INDEX task_prompts_task_id_idx
    ON rev.task_prompts (task_id, id) INCLUDE (prompt_id);
DROP INDEX rev.task_prompts_task_idx;

-- prompt -> messages: (prompt_id, id) is the window PARTITION BY/ORDER BY.
CREATE INDEX prompt_fluent_messages_prompt_id_idx
    ON rev.prompt_fluent_messages (prompt_id, id) INCLUDE (fluent_message_id, role);
DROP INDEX rev.prompt_fluent_messages_prompt_idx;

-- profile config resolution + the insert anti-join dedupe. NOT unique: the
-- live table holds duplicate (profile, prompt_path) pairs.
CREATE INDEX inference_profile_configs_profile_prompt_idx
    ON rev.inference_profile_configs (inference_profile_id, prompt_path_object_id)
    INCLUDE (priority, model_release_id, model_variant_id);
DROP INDEX rev.inference_profile_configs_profile_idx;

-- prefix of model_releases_lineage_id_provider_id_tier_release_key
DROP INDEX rev.model_releases_lineage_idx;
-- prefix of playbook_stage_directives_playbook_stage_id_key (UNIQUE)
DROP INDEX rev.playbook_stage_directives_stage_idx;

------------------------------------------------------------------------------
-- E. Brief content: the authored-order reads added by migration 95 sort by
-- (brief_id, ord, id); give them that key directly.
------------------------------------------------------------------------------

CREATE INDEX brief_units_brief_ord_idx
    ON rev.brief_units (brief_id, ord, id) INCLUDE (unit_key);
DROP INDEX rev.brief_units_brief_idx;

CREATE INDEX brief_outputs_brief_ord_idx
    ON rev.brief_outputs (brief_id, ord, id)
    INCLUDE (value_slot_id, source_kind_id, display_hint_id);
DROP INDEX rev.brief_outputs_brief_idx;
DROP INDEX rev.brief_outputs_source_kind_idx;
DROP INDEX rev.brief_outputs_display_hint_idx;

CREATE INDEX brief_unit_input_choices_unit_id_idx
    ON rev.brief_unit_input_choices (brief_unit_id, id);
DROP INDEX rev.brief_unit_input_choices_unit_idx;
DROP INDEX rev.brief_unit_input_choices_label_fluent_idx;

-- prefixes of composite UNIQUEs / reverse directions no query walks
DROP INDEX rev.brief_group_briefs_group_idx;
DROP INDEX rev.brief_group_briefs_brief_idx;
DROP INDEX rev.brief_pack_groups_pack_idx;
DROP INDEX rev.brief_pack_groups_group_idx;
DROP INDEX rev.brief_slots_brief_idx;

------------------------------------------------------------------------------
-- F. Versioned-object core: the "latest version per path" walk (~40 queries)
-- projects exactly vo.id + vo.content_hash; carry them so the hottest join
-- target in the schema (594k scans) goes index-only.
------------------------------------------------------------------------------

DROP INDEX rev.versioned_objects_path_created_desc_idx;
CREATE INDEX versioned_objects_path_created_desc_idx
    ON rev.versioned_objects (path_id, created_at DESC) INCLUDE (id, content_hash);
-- every content_hash predicate is paired with path_id (served by the UNIQUE)
DROP INDEX rev.versioned_objects_content_hash_idx;

DROP INDEX rev.object_paths_namespace_scope_idx;
CREATE INDEX object_paths_namespace_scope_idx
    ON rev.object_paths (namespace_company_id, scope) INCLUDE (id);
-- entity types are reached pkey-first; scope is never filtered alone
DROP INDEX rev.object_paths_entity_type_idx;
DROP INDEX rev.object_paths_scope_idx;

-- latest_deprecation is fully served by deprecation_events_latest_idx
-- (versioned_object_id, created_at DESC) INCLUDE (to_level_id); the rest of
-- the family answers no query.
DROP INDEX rev.deprecation_events_versioned_object_idx;
DROP INDEX rev.deprecation_events_created_at_idx;
DROP INDEX rev.deprecation_events_to_level_idx;
DROP INDEX rev.deprecation_events_from_level_idx;

-- Every entity table declares versioned_object_id UNIQUE and then a plain
-- index on the same column — pure duplicates, write cost only.
DROP INDEX rev.assets_versioned_object_idx;
DROP INDEX rev.briefs_versioned_object_idx;
DROP INDEX rev.brief_groups_versioned_object_idx;
DROP INDEX rev.brief_packs_versioned_object_idx;
DROP INDEX rev.categories_versioned_object_idx;
DROP INDEX rev.directives_versioned_object_idx;
DROP INDEX rev.fluent_messages_versioned_object_idx;
DROP INDEX rev.guides_versioned_object_idx;
DROP INDEX rev.hats_versioned_object_idx;
DROP INDEX rev.inference_profiles_versioned_object_idx;
DROP INDEX rev.playbooks_versioned_object_idx;
DROP INDEX rev.prompts_versioned_object_idx;
DROP INDEX rev.stages_versioned_object_idx;
DROP INDEX rev.steps_versioned_object_idx;
DROP INDEX rev.tasks_versioned_object_idx;
DROP INDEX rev.value_slots_versioned_object_idx;

-- Display-only fluent references: projected, never filtered.
DROP INDEX rev.briefs_title_fluent_idx;
DROP INDEX rev.briefs_description_fluent_idx;
DROP INDEX rev.brief_groups_title_fluent_idx;
DROP INDEX rev.brief_groups_description_fluent_idx;
DROP INDEX rev.brief_packs_title_fluent_idx;
DROP INDEX rev.brief_packs_description_fluent_idx;
DROP INDEX rev.stages_description_fluent_idx;
DROP INDEX rev.steps_description_fluent_idx;

-- *_path_details namespace filters are always paired with name/slug and thus
-- served by the composite UNIQUE constraints from migration 41.
DROP INDEX rev.brief_path_details_namespace_idx;
DROP INDEX rev.brief_group_path_details_namespace_idx;
DROP INDEX rev.brief_pack_path_details_namespace_idx;
DROP INDEX rev.category_path_details_namespace_idx;
DROP INDEX rev.directive_path_details_namespace_idx;
DROP INDEX rev.fluent_message_path_details_namespace_idx;
DROP INDEX rev.guide_path_details_namespace_idx;
DROP INDEX rev.hat_path_details_namespace_idx;
DROP INDEX rev.inference_profile_path_details_namespace_idx;
DROP INDEX rev.playbook_path_details_namespace_idx;
DROP INDEX rev.prompt_path_details_namespace_idx;
DROP INDEX rev.stage_path_details_namespace_idx;
DROP INDEX rev.step_path_details_namespace_idx;
DROP INDEX rev.task_path_details_namespace_idx;
DROP INDEX rev.value_slot_path_details_namespace_idx;

-- value_kind_id / asset_kind_id are lookup-table FKs, never predicates.
DROP INDEX rev.value_slots_value_kind_idx;
DROP INDEX rev.value_slot_path_details_value_kind_idx;
DROP INDEX rev.asset_path_details_asset_kind_idx;
DROP INDEX rev.drafts_asset_kind_idx;

------------------------------------------------------------------------------
-- G. Guide pipeline.
------------------------------------------------------------------------------

-- canonical-pin resolution joins and version listing filter source_path;
-- classification browse filters classification. Neither had any index (the
-- source of rev.guides' 322 seq scans that weren't deliberate full pages).
CREATE INDEX guides_source_path_idx
    ON rev.guides (source_path) INCLUDE (id, versioned_object_id)
    WHERE source_path IS NOT NULL;
CREATE INDEX guides_classification_idx
    ON rev.guides (classification) INCLUDE (id, versioned_object_id, source_path)
    WHERE classification IS NOT NULL;

-- chapter/block ladders finish ORDER BY (g.id, gc.id, gb.id); put the tiebreak
-- in the key so children come back pre-sorted.
CREATE INDEX guide_chapters_guide_id_idx ON rev.guide_chapters (guide_id, id);
DROP INDEX rev.guide_chapters_guide_idx;
CREATE INDEX guide_blocks_chapter_id_idx ON rev.guide_blocks (chapter_id, id);
DROP INDEX rev.guide_blocks_chapter_idx;
-- directive types are reached pkey-first from blocks, never the reverse
DROP INDEX rev.guide_blocks_directive_type_idx;

CREATE INDEX guide_cards_category_guide_idx
    ON rev.guide_cards (category, guide_id)
    INCLUDE (id, color, accent, card_image_asset_id)
    WHERE category IS NOT NULL;
DROP INDEX rev.guide_cards_category_idx;
-- duplicate of the UNIQUE (guide_id) constraint index
DROP INDEX rev.guide_cards_guide_idx;

-- restores the composite uniqueness migration 19 dropped as collateral
-- (verified: no duplicate pairs in live data) and serves the only render read.
CREATE UNIQUE INDEX guide_renders_guide_profile_idx
    ON rev.guide_renders (guide_id, render_profile_id) INCLUDE (asset_id, id);
DROP INDEX rev.guide_renders_guide_idx;
DROP INDEX rev.guide_renders_render_profile_idx;

-- embedding count check + guide-scoped ANN searches
CREATE INDEX guide_content_embeddings_guide_generation_idx
    ON rev.guide_content_embeddings (guide_id, generation_id);
DROP INDEX rev.guide_content_embeddings_guide_idx;
DROP INDEX rev.guide_content_embeddings_directive_type_idx;

-- tag/locale junctions are INNER-JOINed before the vector ORDER BY; the
-- selective direction is tag/locale -> embedding.
CREATE INDEX guide_embedding_tags_tag_embedding_idx
    ON rev.guide_embedding_tags (tag_id, embedding_id);
DROP INDEX rev.guide_embedding_tags_tag_idx;
DROP INDEX rev.guide_embedding_tags_embedding_idx;
CREATE INDEX guide_embedding_locales_locale_embedding_idx
    ON rev.guide_embedding_locales (locale_id, embedding_id);
DROP INDEX rev.guide_embedding_locales_locale_idx;
DROP INDEX rev.guide_embedding_locales_embedding_idx;

-- prefixes/duplicates of composite UNIQUEs; role is written, never read
DROP INDEX rev.guide_block_tags_block_idx;
DROP INDEX rev.guide_block_tags_tag_idx;
DROP INDEX rev.guide_assets_guide_idx;
DROP INDEX rev.guide_assets_role_idx;
DROP INDEX rev.guide_resources_guide_idx;
DROP INDEX rev.idx_guide_playbooks_guide_id;

-- Restore the access path migration 33 removed: every pin read filters
-- (scope, environment, object_type[, ref_key]) and never namespace_company_id.
-- The key order also supplies both paging sorts. The INCLUDE list is the
-- whole current_pins projection.
CREATE INDEX canonical_release_pins_scope_env_type_ref_idx
    ON rev.canonical_release_pins (scope, environment, object_type, ref_key)
    INCLUDE (id, versioned_object_id, resolved_at, content_hash, supersedes_id);
DROP INDEX rev.canonical_release_pins_namespace_scope_env_idx;

-- hats' input CTE: array_agg(category_id ORDER BY hc.id) GROUP BY hat_id
CREATE INDEX hat_categories_hat_ord_idx
    ON rev.hat_categories (hat_id, id) INCLUDE (category_id);
DROP INDEX rev.hat_categories_hat_idx;

-- prefix of fluent_message_bodies_fluent_message_id_locale_id_key; locale is
-- never a standalone filter on bodies
DROP INDEX rev.fluent_message_bodies_message_idx;
DROP INDEX rev.fluent_message_bodies_locale_idx;

------------------------------------------------------------------------------
-- H. Chunked assets, extractions, EDGAR caches, auth surfaces.
------------------------------------------------------------------------------

-- holders-by-issuer read: filter (issuer_cik, trans_date range), then sort by
-- exactly (trans_date, owner_name, accession) — extend the key to kill the sort.
CREATE INDEX insider_transactions_issuer_window_idx
    ON rev.insider_transactions (issuer_cik, trans_date, owner_name, accession);
DROP INDEX rev.insider_transactions_issuer_date_idx;
-- wholesale filing replace deletes by (issuer_cik, accession); nothing looks
-- up bare accession.
CREATE INDEX insider_transactions_issuer_accession_idx
    ON rev.insider_transactions (issuer_cik, accession);
DROP INDEX rev.insider_transactions_accession_idx;

-- holders-by-cusip: (cusip, quarter_end range) then value_usd DESC per quarter.
CREATE INDEX institutional_holdings_cusip_quarter_value_idx
    ON rev.institutional_holdings (cusip, quarter_end, value_usd DESC);
DROP INDEX rev.institutional_holdings_cusip_quarter_idx;

-- latest current extraction: filter + ORDER BY extractor_version DESC LIMIT 1
-- becomes a single tuple fetch; partial predicate matches the query.
CREATE INDEX asset_extractions_current_latest_idx
    ON rev.asset_extractions (asset_id, extractor_kind, extractor_version DESC)
    WHERE is_current;
DROP INDEX rev.asset_extractions_current_idx;
-- prefix of the (asset_id, extractor_kind, extractor_version) UNIQUE
DROP INDEX rev.asset_extractions_asset_idx;
-- no query filters extractor_kind without asset_id
DROP INDEX rev.asset_extractions_kind_version_idx;

CREATE INDEX asset_dataset_extractions_asset_latest_idx
    ON rev.asset_dataset_extractions (asset_id, id DESC);
DROP INDEX rev.asset_dataset_extractions_asset_idx;

-- ingest dedup probe (checksum -> chunk, DISTINCT ON tie-broken by id) and the
-- integrity sweeps (chunk -> checksum), both index-only.
CREATE INDEX chunk_checksums_checksum_id_idx
    ON rev.chunk_checksums (checksum, id) INCLUDE (chunk_id);
DROP INDEX rev.chunk_checksums_checksum_idx;
CREATE INDEX chunk_checksums_chunk_checksum_idx
    ON rev.chunk_checksums (chunk_id) INCLUDE (checksum, id);
DROP INDEX rev.chunk_checksums_chunk_idx;

-- integrity EXISTS probes correlate on both columns
CREATE INDEX asset_chunks_chunk_asset_idx ON rev.asset_chunks (chunk_id, asset_id);
DROP INDEX rev.asset_chunks_chunk_idx;
-- prefix of asset_chunks_pkey (asset_id, ordinal) since migration 78
DROP INDEX rev.asset_chunks_asset_idx;

-- finalize_draft walks (draft_id, ordinal) reading chunk_id; MAX(ordinal)
-- becomes a backward one-tuple scan.
CREATE INDEX draft_chunks_draft_ordinal_chunk_idx
    ON rev.draft_chunks (draft_id, ordinal) INCLUDE (chunk_id, id, created_at);
DROP INDEX rev.draft_chunks_draft_idx;
DROP INDEX rev.draft_chunks_draft_ordinal_idx;

-- triple/double-covered prefixes of their pkey or ordinal composites
DROP INDEX rev.asset_extraction_chunks_extraction_idx;
DROP INDEX rev.asset_extraction_positions_extraction_idx;
DROP INDEX rev.asset_content_embeddings_extraction_idx;
-- duplicate of assets_versioned_object_id_key handled above

-- token listing per user: filter + ORDER BY created_at DESC with the full
-- projection covered; id DESC makes page boundaries deterministic.
CREATE INDEX api_tokens_user_created_idx
    ON web.api_tokens (user_id, created_at DESC, id DESC)
    INCLUDE (name, token_prefix, expires_at, token_hash);
DROP INDEX web.api_tokens_user_idx;
-- exact duplicate of api_tokens_token_hash_key (UNIQUE) on the hottest auth column
DROP INDEX web.api_tokens_token_hash_idx;

-- MIN(revoked_at) per token, executed per listed row: one-tuple index fetch.
CREATE INDEX api_token_revocations_token_revoked_idx
    ON web.api_token_revocations (api_token_id, revoked_at);
DROP INDEX web.api_token_revocations_api_token_idx;

-- service-token listing had no supporting index at all (full scan + sort).
CREATE INDEX service_tokens_created_idx
    ON web.service_tokens (created_at DESC, id DESC);
-- exact duplicate of service_tokens_service_id_key (UNIQUE)
DROP INDEX web.service_tokens_service_id_idx;
CREATE INDEX service_token_revocations_token_revoked_idx
    ON web.service_token_revocations (service_token_id, revoked_at);
DROP INDEX web.service_token_revocations_token_idx;

CREATE INDEX external_service_tokens_company_id_desc_idx
    ON rev.external_service_tokens (company_id, id DESC);
DROP INDEX rev.rev_external_service_tokens_company_idx;
-- external-API auth path checks revocation on every request
CREATE INDEX external_service_token_revocations_token_revoked_idx
    ON rev.external_service_token_revocations (external_service_token_id, revoked_at);
DROP INDEX rev.external_service_token_revocations_token_idx;

-- exact duplicates of UNIQUE constraint indexes
DROP INDEX rev.external_service_principals_token_idx;
DROP INDEX web.user_roles_user_idx;
DROP INDEX rev.human_principals_user_idx;
DROP INDEX rev.principal_billable_entities_principal_idx;
DROP INDEX rev.cbe_company_idx;
DROP INDEX rev.user_billable_entities_user_idx;

-- select_companies' `$1::BOOL OR revoked_at IS NULL` disjunction can never
-- prove this partial index's predicate, so it has no possible user.
DROP INDEX rev.companies_active_idx;
