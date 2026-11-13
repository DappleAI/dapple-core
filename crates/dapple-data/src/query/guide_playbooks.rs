use crate::DbPool;
use crate::error::DataError;
use crate::model::guide_playbooks::{GuidePlaybookEnrichedModel, GuidePlaybookModel};
use sqlx::types::Uuid;
use sqlx::{PgPool, Postgres, Row, Transaction};

/// Insert a guide-playbook association.
///
/// Uses ON CONFLICT to make re-ingestion idempotent.
pub async fn insert_guide_playbook_in_tx(
    tx: &mut Transaction<'static, Postgres>,
    guide_id: Uuid,
    playbook_id: Uuid,
    chapter_index: i16,
) -> Result<(), DataError> {
    sqlx::query(
        "INSERT INTO rev.guide_playbooks (guide_id, playbook_id, chapter_index)
         VALUES ($1, $2, $3)
         ON CONFLICT (guide_id, playbook_id) DO NOTHING",
    )
    .bind(guide_id)
    .bind(playbook_id)
    .bind(chapter_index)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Select all playbooks linked to a guide, ordered by chapter index.
pub async fn select_guide_playbooks(
    pool: &PgPool,
    guide_id: Uuid,
) -> Result<Vec<GuidePlaybookModel>, DataError> {
    let rows = sqlx::query_as::<_, GuidePlaybookModel>(
        "SELECT id, guide_id, playbook_id, chapter_index, created_at
         FROM rev.guide_playbooks
         WHERE guide_id = $1
         ORDER BY chapter_index",
    )
    .bind(guide_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Batch fetch enriched playbook data for multiple guides.
///
/// Joins through versioned_objects → object_paths → playbook_path_details
/// to return slug and name alongside the junction row data.
///
/// Collapses to exactly ONE playbook per `(guide_id, playbook_path)` — the
/// latest non-deprecated version. A re-ingest / `?backfill=true` that PATCHes a
/// playbook to a new version stacks multiple `rev.guide_playbooks` rows on the
/// SAME guide (the junction is append-only and keyed by *version* id). Returning
/// all of them lets the journey-create caller pin a stale (broken) version even
/// though a corrected one exists — the "re-ingest doesn't fix the demo" bug.
/// Collapse to the newest non-deprecated version per path via the correlated
/// latest-deprecation probe (shared/select/latest_deprecation_probe.sqlc) — the
/// same idiom `select_brief_pack_ids_for_guide` uses for the brief-pack side.
pub async fn select_guide_playbooks_by_guide_ids(
    pool: &DbPool,
    guide_ids: &[Uuid],
) -> Result<Vec<GuidePlaybookEnrichedModel>, DataError> {
    let rows = sqlx::query_as::<_, GuidePlaybookEnrichedModel>(
        "WITH candidates AS (
            SELECT
                gp.guide_id,
                gp.playbook_id,
                op.id AS playbook_path_id,
                ppd.slug AS playbook_slug,
                ppd.name AS playbook_name,
                gp.chapter_index,
                vo.created_at AS vo_created_at
            FROM rev.guide_playbooks gp
            JOIN rev.playbooks p ON p.id = gp.playbook_id
            JOIN rev.versioned_objects vo ON vo.id = p.versioned_object_id
            JOIN rev.object_paths op ON op.id = vo.path_id
            JOIN rev.playbook_path_details ppd ON ppd.path_id = op.id
            LEFT JOIN LATERAL (
                SELECT de.to_level_id
                FROM rev.deprecation_events de
                WHERE de.versioned_object_id = vo.id
                ORDER BY de.created_at DESC
                LIMIT 1
            ) ld ON TRUE
            WHERE gp.guide_id = ANY($1)
              AND ld.to_level_id IS NULL
        ),
        latest AS (
            SELECT DISTINCT ON (guide_id, playbook_path_id)
                guide_id, playbook_id, playbook_path_id,
                playbook_slug, playbook_name, chapter_index
            FROM candidates
            -- playbook_id (UUIDv7) breaks created_at ties for a deterministic total order.
            ORDER BY guide_id, playbook_path_id, vo_created_at DESC, playbook_id DESC
        )
        SELECT guide_id, playbook_id, playbook_path_id,
               playbook_slug, playbook_name, chapter_index
        FROM latest
        ORDER BY guide_id, chapter_index",
    )
    .bind(guide_ids)
    .fetch_all(pool.get_pool())
    .await?;

    Ok(rows)
}

/// Look up guide IDs for a journey via the guide_playbooks join table.
///
/// Follows the chain: journey → playbook → guide_playbooks → guide_id.
/// Used by dispatch_runner to auto-populate RAG context from the source guide.
pub async fn select_guide_ids_by_journey_id(
    pool: &PgPool,
    journey_id: Uuid,
) -> Result<Vec<Uuid>, DataError> {
    let rows = sqlx::query(
        "SELECT DISTINCT gp.guide_id
         FROM rev.journeys j
         JOIN rev.guide_playbooks gp ON gp.playbook_id = j.playbook_id
         WHERE j.id = $1",
    )
    .bind(journey_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(|r| r.get("guide_id")).collect())
}
