//! Regression test for the repeated-chunk asset bug.
//!
//! `insert_draft_chunks` is content-addressable: identical bytes reuse the
//! existing `rev.chunks` row. The asset-upload path streams ONE 256 KB chunk per
//! call (`handler::assets` loops over `content.chunks(MAX_CHUNK_SIZE)`), so a
//! draft whose content repeats a byte-identical block holds the SAME `chunk_id`
//! at two ordinals. `rev.asset_chunks` was keyed `PRIMARY KEY (asset_id,
//! chunk_id)`, so finalize's copy of those refs raised "duplicate key value
//! violates unique constraint asset_chunks_pkey" and the whole asset create
//! 409'd.
//!
//! That is not a corner case: a WAV stem is digital silence whenever its
//! instrument rests, so every multi-timelined-video stem upload failed this way
//! while the mastered mix (no two identical blocks) succeeded.
//!
//! An asset_chunks row is a POSITION in a byte-range assembly, not a claim that
//! the chunk is unique to the asset — dedup guarantees it is not. The key is
//! `(asset_id, ordinal)`.

use dapple_data::DbPool;
use dapple_data::hash::hash_asset;
use dapple_data::model::drafts::{InsertDraftArgs, InsertDraftChunkArgs};
use dapple_data::query::chunks::read_asset_content;
use dapple_data::query::drafts::{finalize_draft, insert_draft_chunks, insert_drafts};

#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn asset_with_repeated_identical_chunks_finalizes_and_round_trips(pool: sqlx::PgPool) {
    let db = DbPool::from_pool(pool);

    let drafts = insert_drafts(
        &db,
        InsertDraftArgs {
            asset_kind_keys: vec!["audio".to_string()],
            mime_types: vec!["audio/wav".to_string()],
        },
    )
    .await
    .expect("insert draft");
    let draft_id = drafts[0].id;

    // A stem in miniature: sound, rest, rest, sound. The two rests are
    // byte-identical, and they are NOT adjacent-merged anywhere — each is its
    // own streamed call, exactly how the upload handler loops.
    let sound_a = vec![0xAAu8; 16];
    let silence = vec![0x00u8; 16];
    let sound_b = vec![0xBBu8; 16];
    let written = vec![
        sound_a.clone(),
        silence.clone(),
        silence.clone(),
        sound_b.clone(),
    ];

    let mut chunk_ids = Vec::new();
    let mut ordinals = Vec::new();
    for chunk in &written {
        let rows = insert_draft_chunks(
            &db,
            InsertDraftChunkArgs {
                draft_ids: vec![draft_id],
                data: vec![chunk.clone()],
            },
        )
        .await
        .expect("streamed chunk insert");
        chunk_ids.push(rows[0].chunk_id);
        ordinals.push(rows[0].ordinal);
    }

    // Pin the mechanism the bug rides on: the second rest reuses the first's
    // chunk row, so one chunk_id legitimately occupies two ordinals.
    assert_eq!(
        chunk_ids[1], chunk_ids[2],
        "byte-identical chunks must dedup to a single rev.chunks row"
    );
    assert_eq!(
        ordinals,
        vec![1, 2, 3, 4],
        "each streamed call appends the next assembly position"
    );

    // Pre-fix this is where it blew up: two asset_chunks rows share
    // (asset_id, chunk_id) for the repeated rest.
    let expected: Vec<u8> = written.concat();
    let content_hash = hash_asset(&expected);
    let finalized = finalize_draft(&db, draft_id, content_hash.as_str())
        .await
        .expect("finalize must tolerate a chunk repeated across ordinals");

    assert_eq!(
        finalized.chunk_count, 4,
        "every assembly position links, including the repeat"
    );
    assert_eq!(finalized.content_length, expected.len() as i64);

    // The payoff: the repeated chunk must come back at BOTH positions, in order.
    let read_back = read_asset_content(&db, finalized.asset_id)
        .await
        .expect("read asset content");
    assert_eq!(
        read_back, expected,
        "a repeated chunk must reassemble at every position it occupies"
    );
}
