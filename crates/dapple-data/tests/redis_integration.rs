use serde_json::json;
use sqlx::types::Uuid;

// This integration test exercises a real Redis instance reachable via the
// `DAPPLE_REDIS_HOST` env var (or the default cluster URL). It is no longer
// `#[ignore]`d: it probes connectivity at runtime, runs the assertions when
// Redis answers, and prints `SKIP: ...` (passing) when it does not — so a bare
// `cargo test` stays green off-cluster. Set `DAPPLE_REDIS_STRICT=1` to turn an
// unreachable Redis into a hard failure (for CI lanes that must have it).

/// True if `name` is set to a truthy value (`1`/`true`/`yes`).
fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// Skip because Redis is unavailable — unless `DAPPLE_REDIS_STRICT` is set, in
/// which case fail loudly.
fn skip_or_strict(test: &str, reason: &str) {
    if env_flag("DAPPLE_REDIS_STRICT") {
        panic!("{test}: {reason} (DAPPLE_REDIS_STRICT is set — failing instead of skipping)");
    }
    eprintln!("SKIP {test}: {reason}");
}

#[tokio::test]
async fn redis_cache_and_publisher_roundtrip() {
    let name = "redis_cache_and_publisher_roundtrip";

    // Prefer explicit env var; fall back to the default used by the crate.
    let url = std::env::var("DAPPLE_REDIS_HOST")
        .unwrap_or_else(|_| "redis://dapple-redis.kube.dappleai.dev:6379".to_string());

    // Connectivity probe: PINGs Redis and installs the integrations on success,
    // leaving them unchanged on failure.
    if let Err(e) = dapple_data::integrations::try_init_redis_with_url_async(&url).await {
        skip_or_strict(name, &format!("Redis unreachable: {e}"));
        return;
    }

    let cache = dapple_data::integrations::get_cache()
        .expect("cache initialized")
        .clone();
    let publisher = dapple_data::integrations::get_publisher()
        .expect("publisher initialized")
        .clone();

    let id = Uuid::now_v7();
    let val = json!({ "hello": "world" });

    // cache.set should succeed against a reachable Redis
    let set_res = cache.set("test_integration", &id, val).await;
    assert!(set_res.is_ok(), "cache.set failed: {:?}", set_res);

    // publisher should be able to append to streams
    let pub_res = publisher
        .principal_changed("test_integration", &id, "insert")
        .await;
    assert!(
        pub_res.is_ok(),
        "publisher.principal_changed failed: {:?}",
        pub_res
    );
}
