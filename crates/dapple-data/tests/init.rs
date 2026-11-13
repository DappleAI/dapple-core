/// Integration tests for redis init behavior (idempotency).
#[test]
fn init_idempotent_and_conflict() {
    // Instead of manipulating process env (which can be target-dependent),
    // directly call `init_redis_with_url` with a URL that includes a password
    // to simulate the injected form.
    let canonical = "redis://:testpw@localhost:6379";

    // First init should succeed and record canonical URL containing the password.
    let r = dapple_data::integrations::init_redis_with_url(canonical);
    assert!(r.is_ok(), "init_redis_with_url failed: {:?}", r.err());

    // Calling again with same canonical URL is idempotent.
    let r2 = dapple_data::integrations::init_redis_with_url(canonical);
    assert!(r2.is_ok(), "second init with same url should be ok");

    // Now attempt to init with a different URL — this should fail with a conflict error.
    let other = "redis://localhost:6380";
    let r4 = dapple_data::integrations::init_redis_with_url(other);
    assert!(r4.is_err(), "conflicting init should return Err");
    let msg = r4.err().unwrap();
    // Error message should include masked representations
    assert!(
        msg.contains("attempted") || msg.contains("existing"),
        "unexpected error: {}",
        msg
    );
    assert!(
        msg.contains("****"),
        "masked password not present in error: {}",
        msg
    );
}
