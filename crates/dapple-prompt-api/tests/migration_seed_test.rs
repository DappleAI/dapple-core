//! Fresh-database verification of lookup-table seed migrations.
//!
//! Every `#[sqlx::test]` here runs against a brand-new database auto-migrated
//! from zero, so these tests fail if a seed migration's INSERT silently
//! matched nothing (wrong lookup key, missing prerequisite seed, reordered
//! migration) even though the migration itself "applied" successfully. A seed
//! migration that inserts zero rows is indistinguishable from a working one in
//! `_sqlx_migrations` — only asserting on the seeded rows catches it.

use dapple_data::model::briefs::outputs::BriefOutputDisplay;
use dapple_data::model::stages::StageKind;
use sqlx::PgPool;
use std::collections::BTreeSet;

/// `rev.value_kinds` must carry exactly the seven expected keys on a fresh DB.
/// Migration 80 seeds the 7th (`dataset`); because `insert_value_slots`
/// INNER JOINs `rev.value_kinds` on `key`, a missing / mis-ordered seed would
/// silently drop every `kind: "dataset"` value slot at insert. Asserting on the
/// seeded rows (not the migration's apply status) catches that.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn value_kinds_seed_includes_dataset(pool: PgPool) {
    let seeded: Vec<String> = sqlx::query_scalar("SELECT key FROM rev.value_kinds")
        .fetch_all(&pool)
        .await
        .expect("select value_kinds");

    let seeded: BTreeSet<&str> = seeded.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = ["text", "int", "number", "bool", "json", "asset", "dataset"]
        .into_iter()
        .collect();

    assert_eq!(
        seeded, expected,
        "rev.value_kinds seed diverged from the 7 expected kinds; add the missing \
         seed migration (Migration 80 seeds 'dataset')"
    );
}

/// The `rev.brief_output_display_hints` seed (migrations 20250116... base set +
/// 069 audio + 072 video + 076 file) must agree exactly with
/// `BriefOutputDisplay::ALL_KEYS`. A key present in only one of the two is how
/// a guide's `display:` hint silently downgrades to text (the exact bug the
/// `display: "video"` regression documented in dapple-types).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn display_hint_seed_matches_enum(pool: PgPool) {
    let seeded: Vec<String> = sqlx::query_scalar("SELECT key FROM rev.brief_output_display_hints")
        .fetch_all(&pool)
        .await
        .expect("select display hints");

    let seeded: BTreeSet<&str> = seeded.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = BriefOutputDisplay::ALL_KEYS.into_iter().collect();

    assert_eq!(
        seeded, expected,
        "rev.brief_output_display_hints seed and BriefOutputDisplay::ALL_KEYS diverged; \
         add the missing seed migration or enum variant"
    );
}

/// `rev.brief_input_control_types` must carry exactly the 17 expected keys on
/// a fresh DB (base 8 from migration 002 + list_builder 059, repeater 060,
/// timeline 061, list-ref/timeline-ref 062, timer 063, duration 065,
/// conversation 071, table 083). The ingest catalog check and the
/// `insert_brief_unit_input` CTE both resolve the control by key, so a missing
/// seed row rejects (or silently drops) every guide using that control — the
/// migration-071 "conversation lookup-FK landmine".
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn brief_input_control_types_seed_complete(pool: PgPool) {
    let seeded: Vec<String> = sqlx::query_scalar("SELECT key FROM rev.brief_input_control_types")
        .fetch_all(&pool)
        .await
        .expect("select brief_input_control_types");

    let seeded: BTreeSet<&str> = seeded.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = [
        "text",
        "textarea",
        "select",
        "radio",
        "checkbox",
        "number",
        "date",
        "file",
        "list_builder",
        "repeater",
        "timeline",
        "list-ref",
        "timeline-ref",
        "timer",
        "duration",
        "conversation",
        "table",
    ]
    .into_iter()
    .collect();

    assert_eq!(
        seeded, expected,
        "rev.brief_input_control_types seed diverged from the 17 expected keys; \
         every new control needs its own seed migration (083 seeds 'table')"
    );
}

/// The `rev.stage_kinds` seed (migration 097) must agree exactly with
/// `StageKind::ALL_KEYS`. The vocabulary is enforced in three places that all
/// key off `ALL_KEYS` — the ingest warning collector, `stage_kind_for_hash`
/// (which folds only valid keys into the stage content hash), and the insert's
/// `LEFT JOIN rev.stage_kinds` (which stores NULL for keys the table lacks) —
/// so a seed↔enum skew makes a valid authored kind silently store NULL while
/// hashing as present, de-syncing hash from row.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn stage_kinds_seed_matches_enum(pool: PgPool) {
    let seeded: Vec<String> = sqlx::query_scalar("SELECT key FROM rev.stage_kinds")
        .fetch_all(&pool)
        .await
        .expect("select stage kinds");

    let seeded: BTreeSet<&str> = seeded.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = StageKind::ALL_KEYS.into_iter().collect();

    assert_eq!(
        seeded, expected,
        "rev.stage_kinds seed and StageKind::ALL_KEYS diverged; \
         add the missing seed migration or enum variant"
    );
}

/// Migration 075 must leave resolvable `(video, openai, sora-2[-pro])` rows on
/// a fresh database — this is what guide materialization looks up when an
/// `:::inference-profile` declares `.lineage = video` with the OpenAI engine
/// body. Without these rows the materializer inserts an
/// `inference_profile_configs` row with both model ids NULL and dies on
/// `chk_one_model` (the recurring new-backend landmine).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn sora_video_releases_seeded(pool: PgPool) {
    let tiers: Vec<String> = sqlx::query_scalar(
        "SELECT mr.tier
         FROM rev.model_releases mr
         JOIN rev.lineages l ON l.id = mr.lineage_id
         JOIN rev.providers p ON p.id = mr.provider_id
         WHERE l.key = 'video' AND p.key = 'openai' AND mr.release IS NULL
         ORDER BY mr.tier",
    )
    .fetch_all(&pool)
    .await
    .expect("select openai video releases");

    assert_eq!(
        tiers,
        vec!["sora-2".to_string(), "sora-2-pro".to_string()],
        "migration 075 did not seed the Sora releases (lineage 'video' / provider 'openai')"
    );
}
