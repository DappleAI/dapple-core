//! Regression test for the multi-chunk asset ordinal bug.
//!
//! `persist_image_asset` (and the TTS / asset-upload / guide-ingest paths) stream
//! an asset to `insert_draft_chunks` ONE 256 KB chunk per call. The query used to
//! number ordinals per-call — every call restarted at 1 — so a multi-chunk asset
//! ended up with every chunk at `ordinal = 1`, and `read_asset_content`
//! (`ORDER BY ordinal`) reassembled them in arbitrary tie-order. That produced
//! corrupt bytes and the `composite: decode: The decoder to parse Unknown ...`
//! 500 when a non-PNG-header chunk happened to sort first.
//!
//! The fix offsets each call's ordinals by the draft's existing max, honoring the
//! "streaming writes / append-only during creation" contract. These assertions
//! pin it: successive single-chunk calls APPEND with increasing ordinals, and a
//! finalized multi-chunk asset reads back in write order.

use dapple_data::DbPool;
use dapple_data::hash::hash_asset;
use dapple_data::model::drafts::{InsertDraftArgs, InsertDraftChunkArgs};
use dapple_data::query::chunks::read_asset_content;
use dapple_data::query::drafts::{finalize_draft, insert_draft_chunks, insert_drafts};

#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn streamed_chunks_append_with_increasing_ordinals_and_round_trip(pool: sqlx::PgPool) {
    let db = DbPool::from_pool(pool);

    // One image draft.
    let drafts = insert_drafts(
        &db,
        InsertDraftArgs {
            asset_kind_keys: vec!["image".to_string()],
            mime_types: vec!["image/png".to_string()],
        },
    )
    .await
    .expect("insert draft");
    let draft_id = drafts[0].id;

    // Two SEPARATE single-chunk calls — exactly how persist_image_asset loops over
    // `bytes.chunks(MAX_CHUNK_SIZE)`. Distinct content so a mis-order is detectable.
    let chunk_a = vec![0xAAu8; 16];
    let chunk_b = vec![0xBBu8; 16];

    let first = insert_draft_chunks(
        &db,
        InsertDraftChunkArgs {
            draft_ids: vec![draft_id],
            data: vec![chunk_a.clone()],
        },
    )
    .await
    .expect("first chunk insert");
    let second = insert_draft_chunks(
        &db,
        InsertDraftChunkArgs {
            draft_ids: vec![draft_id],
            data: vec![chunk_b.clone()],
        },
    )
    .await
    .expect("second chunk insert");

    // The crux of the fix: the second streamed call APPENDS at ordinal 2 rather
    // than restarting at ordinal 1 (the bug left both at 1).
    assert_eq!(first[0].ordinal, 1, "first streamed chunk is ordinal 1");
    assert_eq!(
        second[0].ordinal, 2,
        "second streamed chunk must append at ordinal 2 (pre-fix it restarted at 1)"
    );

    // End-to-end: finalize, then read back. Bytes must be in write order (A then B).
    let expected: Vec<u8> = [chunk_a, chunk_b].concat();
    let content_hash = hash_asset(&expected);
    let finalized = finalize_draft(&db, draft_id, content_hash.as_str())
        .await
        .expect("finalize draft");
    assert_eq!(finalized.content_length, expected.len() as i64);
    assert_eq!(finalized.chunk_count, 2, "both chunks linked to the asset");

    let read_back = read_asset_content(&db, finalized.asset_id)
        .await
        .expect("read asset content");
    assert_eq!(
        read_back, expected,
        "a multi-chunk asset must reassemble in write order (A then B)"
    );
}
