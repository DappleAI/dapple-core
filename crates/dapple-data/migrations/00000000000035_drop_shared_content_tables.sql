-- =============================================================================
-- Drop shared-content tables (deferred to V2+)
-- =============================================================================
-- Both rev.guide_shared_refs and rev.shared_content_objects were originally
-- defined in migration 12 to support the _shared/ content reference system.
-- The verifier at plan time confirmed both are empty (0 rows in either) and
-- all the code paths that populate them have been removed:
--   - dapple-prompt-api/src/handler/shared_ingest.rs (removed)
--   - dapple-prompt-api/src/route/shared_content.rs (removed)
--   - dapple-prompt-api/src/schema/shared_ingest.rs (removed)
--   - dapple-data/src/query/shared.rs (removed)
--   - dapple-data/assets/sqlc/shared/* (removed)
--   - dapple-data/assets/sqlc/guides/insert_guide_shared_refs.sqlc (removed)
--   - dapple-parser/src/shared.rs (removed)
--   - dapple-parser/src/transform: SharedContentPlan, SharedRefDef,
--     plan_shared_content (removed)
--   - dapple-types/src/model/guides.rs::GuideSharedRef (removed)
--   - dapple-sdk/src/resource/shared_content.rs (removed)
--
-- The shared-content identifier system will be redesigned when shared content
-- becomes real V2+ functionality. At that point a proper
-- rev.shared_content_path_details table can be designed alongside the rest of
-- the path-detail family.
--
-- See docs/plans/external-api/00-architecture.md §3 Decision 3 and §7.
-- =============================================================================

DROP TABLE IF EXISTS rev.guide_shared_refs;
DROP TABLE IF EXISTS rev.shared_content_objects;
