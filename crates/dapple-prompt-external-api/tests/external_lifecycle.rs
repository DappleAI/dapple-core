//! End-to-end external customer lifecycle.
//!
//! This file is the integration spine for the external API surface. It
//! exercises every leg of the customer flow against a fresh test database
//! and the real `dapple-prompt-external-api` axum server bound to an
//! ephemeral port — no in-process `tower::oneshot` shortcuts. Every leg
//! goes over real HTTP via `reqwest` so the test exercises the same code
//! path a deployed customer would hit.
//!
//! 1. **Onboarding** — insert into `rev.companies` + `rev.external_service_tokens`
//! 2. **Token exchange** — sign an Ed25519 token, post to `/api/get-service-token`,
//!    receive a JWT, parse and validate the claim shape.
//! 3. **Authenticated GET** — list guides for the customer's namespace
//!    (initially empty).
//! 4. **Path-level isolation** — seed `(scope='external', namespace=A)` and
//!    `(scope='external', namespace=B)` guides; verify each customer sees
//!    ONLY their own.
//! 5. **Cross-surface JWT replay** — JWT signed with the internal-surface
//!    key cannot be used on the external surface.
//! 6. **Wrong issuer / wrong token_use** — tampered claims are rejected.
//! 7. **Expired JWT** — past-`exp` JWT is rejected at signature validation.
//! 8. **Token revocation** — once revoked, JWT exchange returns 401
//!    `SERVICE_TOKEN_REVOKED`.
//! 9. **Company revocation** — once the company is revoked, JWT exchange
//!    returns 401 `COMPANY_REVOKED`.
//!
//! Each `#[sqlx::test]` gets its own fresh database (auto-migrated) so
//! tests do not share state.
//!
//! `serial_test::serial` is applied to scenarios that mutate the
//! `DAPPLE_PROMPT_EXTERNAL_API_SIGNING_KEY` env var, since env var changes
//! are global. Other tests can run in parallel.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{Duration, Utc};
use dapple_data::DbPool;
use dapple_prompt_external_api::handler::service_token::ExternalServiceJwtClaims;
use dapple_prompt_external_api::{
    EXTERNAL_JWT_ISS, EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TOKEN_USE, ExternalAppState,
    build_router,
};
use ed25519_dalek::{Signer, SigningKey};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rand::rngs::OsRng;
use reqwest::StatusCode;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

const EXTERNAL_TEST_KEY: &str = "external-test-jwt-key-not-for-production-use-only";
const INTERNAL_TEST_KEY: &str = "internal-test-jwt-key-not-for-production-use-only";

// =============================================================================
// Test helpers
// =============================================================================

struct TestCustomer {
    company_id: Uuid,
    company_slug: String,
    service_id: String,
    signing_key: SigningKey,
}

/// Insert a `rev.companies` row and a `rev.external_service_tokens` row for
/// it, returning the keypair so the test can sign tokens on the customer's
/// behalf.
async fn setup_customer(pool: &PgPool, slug: &str, service_id: &str) -> TestCustomer {
    let company_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO rev.companies (id, slug, name) VALUES ($1, $2, $3)",
        company_id,
        slug,
        slug,
    )
    .execute(pool)
    .await
    .expect("insert company");

    let mut rng = OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    let public_key_pem = ed25519_pem(&signing_key.verifying_key());

    sqlx::query!(
        "INSERT INTO rev.external_service_tokens \
         (service_id, company_id, name, public_key_pem) \
         VALUES ($1, $2, $3, $4)",
        service_id,
        company_id,
        format!("{} test token", slug),
        public_key_pem,
    )
    .execute(pool)
    .await
    .expect("insert external_service_token");

    TestCustomer {
        company_id,
        company_slug: slug.to_string(),
        service_id: service_id.to_string(),
        signing_key,
    }
}

fn ed25519_pem(vk: &ed25519_dalek::VerifyingKey) -> String {
    use ed25519_dalek::pkcs8::EncodePublicKey;
    vk.to_public_key_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF)
        .expect("encode public key as PEM")
}

/// Build the 128-byte signed token in the format the external surface
/// expects (matches `dapple-prompt-api::verify_service_token` —
/// `version(1) + valid_from(8) + valid_until(8) + service_id_len(1) +
/// service_id(<=32) + padding`, total 64 bytes, prepended with a 64-byte
/// Ed25519 signature over that message).
fn sign_test_token(customer: &TestCustomer, valid_secs: u64, valid_from_offset: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_secs() as i64;
    let valid_from = (now + valid_from_offset).max(0) as u64;
    let valid_until = valid_from + valid_secs;

    let service_id_bytes = customer.service_id.as_bytes();
    assert!(
        service_id_bytes.len() <= 32,
        "test service_id must fit in 32 bytes (got {})",
        service_id_bytes.len()
    );

    let mut message = [0u8; 64];
    message[0] = 1; // version
    message[1..9].copy_from_slice(&valid_from.to_be_bytes());
    message[9..17].copy_from_slice(&valid_until.to_be_bytes());
    message[17] = service_id_bytes.len() as u8;
    message[18..18 + service_id_bytes.len()].copy_from_slice(service_id_bytes);
    // remaining bytes stay zero (padding)

    let signature = customer.signing_key.sign(&message);

    let mut token = [0u8; 128];
    token[..64].copy_from_slice(&signature.to_bytes());
    token[64..].copy_from_slice(&message);

    BASE64.encode(token)
}

/// Spawn the external-api server on an ephemeral 127.0.0.1 port and return
/// `(base_url, JoinHandle)`. Every test exercises the surface via real HTTP
/// (no `tower::oneshot` shortcut) so the binary's routing, body
/// serialization, and accept loop are part of the test surface — same
/// shape a deployed customer hits. The JoinHandle is not awaited; the
/// spawned task is torn down implicitly when the test ends.
async fn spawn_external_server(pool: PgPool) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let base_url = format!("http://{}", listener.local_addr().expect("local_addr"));

    let state = Arc::new(ExternalAppState::new(DbPool::from_pool(pool)));
    let app: axum::Router = build_router(state).into();
    let server = axum::serve(listener, app);

    let handle = tokio::spawn(async move {
        if let Err(e) = server.await {
            eprintln!("external-api test server error: {e}");
        }
    });

    (base_url, handle)
}

/// Drain a `reqwest::Response` into `(StatusCode, Value)`. Empty bodies
/// surface as `Value::Null`. Non-JSON bodies panic with the raw text so
/// failure diagnostics show what the server actually returned.
async fn read_json(resp: reqwest::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let bytes = resp.bytes().await.unwrap_or_default();
    if bytes.is_empty() {
        return (status, Value::Null);
    }
    let body: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "response body is not JSON: status={}, error={}, body={:?}",
            status,
            e,
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, body)
}

async fn exchange_token(
    http: &reqwest::Client,
    base_url: &str,
    token_b64: &str,
    service_id: &str,
) -> (StatusCode, Value) {
    let resp = http
        .post(format!("{}/api/get-service-token", base_url))
        .json(&serde_json::json!({ "token": token_b64, "service_id": service_id }))
        .send()
        .await
        .expect("token-exchange request");
    read_json(resp).await
}

async fn list_guides(http: &reqwest::Client, base_url: &str, jwt: &str) -> (StatusCode, Value) {
    let resp = http
        .get(format!("{}/api/guides", base_url))
        .bearer_auth(jwt)
        .send()
        .await
        .expect("list-guides request");
    read_json(resp).await
}

/// Mint a JWT directly (without going through the exchange handler) using
/// arbitrary claims and a chosen signing key. Used for negative tests:
/// cross-surface replay, wrong iss, wrong token_use, expired.
fn mint_jwt(
    secret: &str,
    sub: Uuid,
    company_slug: &str,
    service_id: &str,
    iss: &str,
    token_use: &str,
    exp_offset_secs: i64,
) -> String {
    let exp = (Utc::now() + Duration::seconds(exp_offset_secs)).timestamp() as usize;
    let claims = ExternalServiceJwtClaims {
        sub,
        company_slug: company_slug.to_string(),
        service_id: service_id.to_string(),
        principal_id: Uuid::new_v4(),
        iss: iss.to_string(),
        token_use: token_use.to_string(),
        exp,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("test JWT encoding")
}

/// Sets a process env var for the duration of a (serial) test and removes it
/// on drop — including during panic-unwind — so a short-TTL override can't
/// leak into a later serial test (e.g. the 20s dispatch poll). Pairs with
/// `#[serial_test::serial(external_signing_key)]`.
struct EnvVarGuard(&'static str);

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        unsafe { std::env::set_var(key, value) };
        EnvVarGuard(key)
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe { std::env::remove_var(self.0) };
    }
}

/// Seed an external-scoped guide for a given customer. Creates the
/// minimum row chain (object_paths + versioned_objects + guides +
/// guide_path_details) so the list endpoint can return it.
async fn seed_external_guide(pool: &PgPool, customer: &TestCustomer, slug: &str) -> Uuid {
    let entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'guide'")
            .fetch_one(pool)
            .await
            .unwrap();

    // 1. Path
    let path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // 2. Versioned object
    let vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        path_id,
        // 40-hex-char fake content hash that satisfies the CHECK regex
        format!(
            "{:020x}{:020x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // 3. Guide
    let guide_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.guides (versioned_object_id, source_path) \
         VALUES ($1, $2) RETURNING id",
        vo_id,
        format!("public/{}/{}", customer.company_slug, slug),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // 4. Path details (title + slug; required NOT NULL). Migration 41 made
    //    UNIQUE(title)/UNIQUE(slug) composite over namespace_company_id;
    //    the seed mirrors what the real INSERT pipeline writes.
    sqlx::query!(
        "INSERT INTO rev.guide_path_details (path_id, title, slug, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        format!("{} test guide", slug),
        slug,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();

    guide_id
}

/// Seed a minimal external-scoped playbook in `customer`'s namespace:
/// object_path + versioned_object + playbook + playbook_path_details. The
/// path-details row is required by the playbook SELECT body templates
/// (INNER JOIN on `rev.playbook_path_details`); without it the playbook
/// is structurally invisible to GET/list handlers.
async fn seed_external_playbook(pool: &PgPool, customer: &TestCustomer) -> Uuid {
    let entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'playbook'")
            .fetch_one(pool)
            .await
            .unwrap();

    let path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        path_id,
        format!(
            "{:020x}{:020x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // Globally-unique name/slug so multiple seeds per test don't collide on
    // `rev.playbook_path_details (name|slug) UNIQUE`.
    let unique = format!("pb-{}", Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.playbook_path_details (path_id, name, slug, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        unique,
        unique,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();

    let playbook_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.playbooks (versioned_object_id, description) \
         VALUES ($1, $2) RETURNING id",
        vo_id,
        format!("seed playbook for {}", customer.company_slug),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // Expose the playbook through a guide and link them in `rev.guide_playbooks`.
    // Journey-POST now REQUIRES a `guide_id` and validates the guide exposes the
    // playbook (400 GUIDE_PLAYBOOK_MISMATCH otherwise) — there is no auto-pin.
    // Tests resolve the linked guide via `guide_for_playbook` and pass it in the
    // request body; a bare playbook with no exposing guide can never be journeyed.
    let guide_id = seed_external_guide(pool, customer, &format!("{unique}-guide")).await;
    sqlx::query!(
        "INSERT INTO rev.guide_playbooks (guide_id, playbook_id, chapter_index) \
         VALUES ($1, $2, 0)",
        guide_id,
        playbook_id,
    )
    .execute(pool)
    .await
    .unwrap();

    playbook_id
}

/// Resolve the guide that exposes `playbook_id` (via `rev.guide_playbooks`).
/// `seed_external_playbook` / `onboard_with_playbook` already seed exactly one
/// such guide, so this returns the `guide_id` that the journey-POST handler
/// requires (it now validates `(guide_id, playbook_id)` is linked and 400s on
/// mismatch — there is no more auto-pin).
async fn guide_for_playbook(pool: &PgPool, playbook_id: Uuid) -> Uuid {
    sqlx::query_scalar!(
        "SELECT g.id FROM rev.guides g \
         JOIN rev.guide_playbooks gp ON gp.guide_id = g.id \
         WHERE gp.playbook_id = $1 ORDER BY g.id DESC LIMIT 1",
        playbook_id,
    )
    .fetch_one(pool)
    .await
    .expect("guide_for_playbook: no guide exposes this playbook — seed one first")
}

/// Link an already-seeded `guide_id` to `playbook_id` so the journey-POST
/// handler's `(guide_id, playbook_id)` validation passes. Idempotent.
async fn link_guide_playbook(pool: &PgPool, guide_id: Uuid, playbook_id: Uuid) {
    sqlx::query!(
        "INSERT INTO rev.guide_playbooks (guide_id, playbook_id, chapter_index) \
         VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
        guide_id,
        playbook_id,
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Resolve the external-service `principal_id` (UUID) auto-created by the
/// `external_service_token_creates_principal` trigger for this customer.
async fn service_principal_id(pool: &PgPool, customer: &TestCustomer) -> Uuid {
    sqlx::query_scalar!(
        "SELECT esp.principal_id \
         FROM rev.external_service_principals esp \
         JOIN rev.external_service_tokens est ON est.id = esp.external_service_token_id \
         WHERE est.service_id = $1",
        &customer.service_id
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Seed a minimal journey owned by `customer` (skips going through the
/// handler — direct INSERT, no playbook namespace check needed). user_id
/// stays NULL since the customer's principal is an external_service, not
/// a NextAuth user.
async fn seed_external_journey(pool: &PgPool, customer: &TestCustomer, playbook_id: Uuid) -> Uuid {
    let principal_id = service_principal_id(pool, customer).await;
    sqlx::query_scalar!(
        "INSERT INTO rev.journeys (playbook_id, principal_id, name, owner_company_id) \
         VALUES ($1, $2, $3, $4) RETURNING id",
        playbook_id,
        principal_id,
        "seed journey",
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Seed a minimal dispatch owned by `customer` (direct INSERT — sufficient
/// for namespace tests; no stages/steps/tasks attached).
async fn seed_external_dispatch(pool: &PgPool, customer: &TestCustomer, journey_id: Uuid) -> Uuid {
    let principal_id = service_principal_id(pool, customer).await;
    sqlx::query_scalar!(
        "INSERT INTO rev.dispatches (journey_id, principal_id, intent, owner_company_id) \
         VALUES ($1, $2, $3::JSONB, $4) RETURNING id",
        journey_id,
        principal_id,
        serde_json::json!({}),
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

// =============================================================================
// HTTP helpers for journeys / dispatches / bindings
// =============================================================================

async fn post_json(
    http: &reqwest::Client,
    base_url: &str,
    path: &str,
    jwt: &str,
    body: Value,
) -> (StatusCode, Value) {
    let resp = http
        .post(format!("{}{}", base_url, path))
        .bearer_auth(jwt)
        .json(&body)
        .send()
        .await
        .expect("POST request");
    read_json(resp).await
}

async fn get_authed(
    http: &reqwest::Client,
    base_url: &str,
    path: &str,
    jwt: &str,
) -> (StatusCode, Value) {
    let resp = http
        .get(format!("{}{}", base_url, path))
        .bearer_auth(jwt)
        .send()
        .await
        .expect("authenticated GET request");
    read_json(resp).await
}

/// Onboard a customer, seed an external-scope playbook in their namespace,
/// and exchange their token for a JWT. Returns the bits a test scenario
/// needs to issue authenticated calls.
async fn onboard_with_playbook(
    pool: &PgPool,
    http: &reqwest::Client,
    base_url: &str,
    slug: &str,
    service_id: &str,
) -> (TestCustomer, String, Uuid) {
    let customer = setup_customer(pool, slug, service_id).await;
    let playbook_id = seed_external_playbook(pool, &customer).await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(http, base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange failed: {}", body);
    let jwt = body["jwt"].as_str().expect("jwt in response").to_string();
    (customer, jwt, playbook_id)
}

// =============================================================================
// Happy path
// =============================================================================

/// The full healthy customer lifecycle: onboard → exchange → list (empty)
/// → seed → list (returns own).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn happy_path_full_lifecycle(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // 1. Onboard customer
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;

    // 2. Sign + exchange
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange failed: {}", body);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service_id"], "testcorp_prod");
    assert_eq!(body["company_slug"], "testcorp");
    let jwt = body["jwt"].as_str().expect("jwt in response").to_string();
    assert!(!jwt.is_empty());

    // 3. List guides — empty for a fresh customer
    let (status, body) = list_guides(&http, &base_url, &jwt).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["objs"].as_array().unwrap().len(), 0);

    // 4. Seed an external guide for this customer
    let _guide_id = seed_external_guide(&pool, &customer, "first-guide").await;

    // 5. List again — returns the seeded guide
    let (status, body) = list_guides(&http, &base_url, &jwt).await;
    assert_eq!(status, StatusCode::OK);
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(objs.len(), 1, "should see exactly one guide; got: {}", body);
    assert_eq!(objs[0]["source_path"], "public/testcorp/first-guide");
}

// =============================================================================
// Path-level isolation: customer A cannot see customer B's guides
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn cross_namespace_isolation(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;

    // Each customer owns one external guide.
    seed_external_guide(&pool, &alpha, "alpha-guide").await;
    seed_external_guide(&pool, &beta, "beta-guide").await;

    // Sign + exchange for alpha
    let alpha_signed = sign_test_token(&alpha, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &alpha_signed, &alpha.service_id).await;
    assert_eq!(status, StatusCode::OK);
    let alpha_jwt = body["jwt"].as_str().unwrap().to_string();

    // Alpha sees ONLY alpha's guide
    let (status, body) = list_guides(&http, &base_url, &alpha_jwt).await;
    assert_eq!(status, StatusCode::OK);
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0]["source_path"], "public/alpha/alpha-guide");

    // Same flow for beta
    let beta_signed = sign_test_token(&beta, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &beta_signed, &beta.service_id).await;
    let beta_jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = list_guides(&http, &base_url, &beta_jwt).await;
    assert_eq!(status, StatusCode::OK);
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0]["source_path"], "public/beta/beta-guide");
}

// =============================================================================
// Deprecation filtering on the external browse surface
// =============================================================================

/// A deprecated guide version must drop out of the customer's GET /api/guides
/// listing (the by_external_caller page filters on the latest-deprecation
/// probe), while an undeprecated sibling stays visible. Corrupt-then-verify:
/// list both, insert the deprecation event in SQL, list again.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn deprecated_guide_excluded_from_external_listing(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "depcorp", "depcorp_prod").await;
    let doomed_guide = seed_external_guide(&pool, &customer, "doomed-guide").await;
    seed_external_guide(&pool, &customer, "kept-guide").await;

    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange failed: {}", body);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // Baseline: both guides list.
    let (status, body) = list_guides(&http, &base_url, &jwt).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["objs"].as_array().unwrap().len(),
        2,
        "baseline must list both guides; got: {}",
        body
    );

    // CORRUPT: deprecate the doomed guide's version.
    sqlx::query!(
        "INSERT INTO rev.deprecation_events (versioned_object_id, to_level_id, reason) \
         SELECT g.versioned_object_id, \
                (SELECT id FROM rev.deprecation_levels WHERE key = 'deprecated'), \
                'test external browse exclusion' \
         FROM rev.guides g WHERE g.id = $1",
        doomed_guide,
    )
    .execute(&pool)
    .await
    .expect("deprecate doomed guide");

    // VERIFY: only the kept guide remains in the listing.
    let (status, body) = list_guides(&http, &base_url, &jwt).await;
    assert_eq!(status, StatusCode::OK);
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(
        objs.len(),
        1,
        "deprecated guide must drop out of the external listing; got: {}",
        body
    );
    assert_eq!(objs[0]["source_path"], "public/depcorp/kept-guide");
}

// =============================================================================
// Negative: cross-surface JWT replay
// =============================================================================

/// A JWT signed with the INTERNAL surface's key must fail signature
/// verification on the external surface. This is the headline
/// cryptographic-isolation guarantee.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn cross_surface_jwt_rejected(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;
    seed_external_guide(&pool, &customer, "secret-guide").await;

    // Mint a JWT with all-correct claims but signed with the INTERNAL key.
    let internal_jwt = mint_jwt(
        INTERNAL_TEST_KEY,
        customer.company_id,
        &customer.company_slug,
        &customer.service_id,
        EXTERNAL_JWT_ISS,
        EXTERNAL_TOKEN_USE,
        3600,
    );

    let (status, body) = list_guides(&http, &base_url, &internal_jwt).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "INVALID_JWT");
}

// =============================================================================
// Negative: wrong issuer / wrong token_use
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn wrong_issuer_rejected(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;

    // Same key, wrong iss
    let bad_iss_jwt = mint_jwt(
        EXTERNAL_TEST_KEY,
        customer.company_id,
        &customer.company_slug,
        &customer.service_id,
        "api.dapple.x", // internal surface's iss
        EXTERNAL_TOKEN_USE,
        3600,
    );
    let (status, body) = list_guides(&http, &base_url, &bad_iss_jwt).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "WRONG_ISSUER");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn wrong_token_use_rejected(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;

    let bad_use_jwt = mint_jwt(
        EXTERNAL_TEST_KEY,
        customer.company_id,
        &customer.company_slug,
        &customer.service_id,
        EXTERNAL_JWT_ISS,
        "service", // internal surface's token_use
        3600,
    );
    let (status, body) = list_guides(&http, &base_url, &bad_use_jwt).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "WRONG_TOKEN_USE");
}

// =============================================================================
// Negative: expired JWT
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn expired_jwt_rejected(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;

    let expired_jwt = mint_jwt(
        EXTERNAL_TEST_KEY,
        customer.company_id,
        &customer.company_slug,
        &customer.service_id,
        EXTERNAL_JWT_ISS,
        EXTERNAL_TOKEN_USE,
        -3600, // exp is one hour in the past
    );
    let (status, body) = list_guides(&http, &base_url, &expired_jwt).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "INVALID_JWT");
}

// =============================================================================
// Negative: missing / malformed Authorization
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn missing_auth_header_rejected(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    setup_customer(&pool, "testcorp", "testcorp_prod").await;

    // Hit GET /api/guides without an Authorization header.
    let resp = http
        .get(format!("{}/api/guides?environment=sandbox", base_url))
        .send()
        .await
        .expect("unauth list-guides request");
    let (status, body) = read_json(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "MISSING_AUTH_HEADER");
}

// =============================================================================
// Negative: token revocation
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn revoked_token_blocks_jwt_exchange(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;

    // Look up the freshly-inserted token id
    let token_id: Uuid = sqlx::query_scalar!(
        "SELECT id FROM rev.external_service_tokens WHERE service_id = $1",
        &customer.service_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Revoke it
    sqlx::query!(
        "INSERT INTO rev.external_service_token_revocations \
         (external_service_token_id, reason) VALUES ($1, 'lifecycle test')",
        token_id,
    )
    .execute(&pool)
    .await
    .unwrap();

    // Sign and exchange — must fail
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "SERVICE_TOKEN_REVOKED");
}

// =============================================================================
// Negative: company revocation
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn revoked_company_blocks_jwt_exchange(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;

    // Revoke the company itself — cuts off ALL of its tokens
    sqlx::query!(
        "UPDATE rev.companies SET revoked_at = now() WHERE id = $1",
        customer.company_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Company check fires before token-revocation check, so the error
    // surfaces COMPANY_REVOKED.
    assert_eq!(body["error"], "COMPANY_REVOKED");
}

// =============================================================================
// Negative: unknown service_id
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn unknown_service_id_rejected(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    // Don't onboard anyone

    // Build a throwaway customer just to get a signed token shape; we'll
    // post it under a service_id that doesn't exist in the DB.
    let mut rng = OsRng;
    let throwaway = TestCustomer {
        company_id: Uuid::now_v7(),
        company_slug: "ghost".to_string(),
        service_id: "ghost_svc".to_string(),
        signing_key: SigningKey::generate(&mut rng),
    };
    let signed = sign_test_token(&throwaway, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, "ghost_svc").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "SERVICE_NOT_FOUND");
}

// =============================================================================
// Negative: signing key not configured
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn missing_signing_key_returns_503(pool: PgPool) {
    unsafe {
        std::env::remove_var(EXTERNAL_JWT_SIGNING_KEY_ENV);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;

    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "SIGNING_KEY_NOT_CONFIGURED");
}

// =============================================================================
// Journey / dispatch lifecycle (V1 write surface)
// =============================================================================
//
// These cover the customer-consumption flow: POST /api/journeys against the
// caller's playbook, then per-resource namespace gating on briefs/dispatch/
// bindings. Each `_for_caller` namespace check should be exercised by both a
// "happy path" leg (own namespace, succeeds) and a "cross-namespace" leg
// (foreign namespace, 404 → no existence leak).
//
// Full end-to-end execution (POST dispatch → poll until ok) is deferred:
// it requires seeding a complete playbook tree (stages/steps/tasks/briefs/
// value_slots) plus a runnable task function. The cross-namespace and
// malformed-input scenarios below are the load-bearing coverage for the
// extractor + per-handler `*_in_namespace` checks.

// -----------------------------------------------------------------------------
// POST /api/journeys — happy path
// -----------------------------------------------------------------------------

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn post_journey_creates_in_own_namespace(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let (_customer, jwt, playbook_id) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;
    let guide_id = guide_for_playbook(&pool, playbook_id).await;

    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "my journey",
                "playbook_id": playbook_id,
                "guide_id": guide_id,
            }],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "body: {}", body);
    assert_eq!(body["status"], "ok");
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0]["name"], "my journey");
    assert_eq!(objs[0]["playbook_id"], serde_json::json!(playbook_id));
}

// -----------------------------------------------------------------------------
// Cross-namespace negatives — 5 scenarios
// -----------------------------------------------------------------------------

/// Customer B cannot POST a journey targeting customer A's playbook. The
/// playbook UUID exists, but `playbook_in_namespace` returns false. We
/// must return 404 (RESOURCE_NOT_FOUND) — never 403 — so the existence of a
/// foreign playbook is indistinguishable from a random UUID.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn post_journey_foreign_playbook_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;

    // Beta authenticates, tries to target alpha's playbook.
    let (_beta, beta_jwt, _beta_playbook) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;

    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &beta_jwt,
        serde_json::json!({
            "objs": [{
                "name": "hostile journey",
                "playbook_id": alpha_playbook,
                // guide_id is required by the schema now; the playbook is
                // foreign, so the playbook_in_namespace check 404s first
                // regardless of what guide_id we supply.
                "guide_id": Uuid::new_v4(),
            }],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_journey_briefs_foreign_journey_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;
    let alpha_journey = seed_external_journey(&pool, &alpha, alpha_playbook).await;

    let (_beta, beta_jwt, _beta_pb) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/briefs", alpha_journey),
        &beta_jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

// GET /api/journeys/{id}/stages — a foreign journey is 404 (namespace gating),
// identical to the briefs case.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_journey_stages_foreign_journey_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;
    let alpha_journey = seed_external_journey(&pool, &alpha, alpha_playbook).await;

    let (_beta, beta_jwt, _beta_pb) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/stages", alpha_journey),
        &beta_jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

// GET /api/journeys/{id}/stages — own journey returns 200 with a per-stage
// `objs` array. (A seeded playbook with no overlays yields done=true stages or
// an empty set; the done-flips-after-binding path is covered by the overlay
// ingest tests in dapple-prompt-api.)
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_journey_stages_own_journey_ok(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let (customer, jwt, playbook_id) =
        onboard_with_playbook(&pool, &http, &base_url, "alpha", "alpha_prod").await;
    let journey = seed_external_journey(&pool, &customer, playbook_id).await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/stages", journey),
        &jwt,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body: {}", body);
    assert_eq!(body["status"], "ok");
    assert!(body["objs"].is_array(), "objs should be an array: {}", body);
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn post_bindings_foreign_dispatch_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;
    let alpha_journey = seed_external_journey(&pool, &alpha, alpha_playbook).await;
    let alpha_dispatch = seed_external_dispatch(&pool, &alpha, alpha_journey).await;

    let (_beta, beta_jwt, _beta_pb) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;

    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/dispatch-intent-bindings",
        &beta_jwt,
        serde_json::json!({
            "objs": [{
                "dispatch_id": alpha_dispatch,
                "value_slot_id": Uuid::new_v4(),
            }],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_dispatch_foreign_dispatch_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;
    let alpha_journey = seed_external_journey(&pool, &alpha, alpha_playbook).await;
    let alpha_dispatch = seed_external_dispatch(&pool, &alpha, alpha_journey).await;

    let (_beta, beta_jwt, _beta_pb) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/dispatches/{}", alpha_dispatch),
        &beta_jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

// -----------------------------------------------------------------------------
// Malformed / nonexistent input — 5 scenarios
// -----------------------------------------------------------------------------

/// Nonexistent playbook UUID. Same 404 surface as cross-namespace —
/// "doesn't exist" and "exists but not in caller's namespace" are
/// deliberately indistinguishable.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn post_journey_nonexistent_playbook_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "ghost journey",
                "playbook_id": Uuid::new_v4(),
                // Required by the schema; the playbook UUID is bogus so the
                // playbook_in_namespace check 404s first.
                "guide_id": Uuid::new_v4(),
            }],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

/// Empty `objs` batch on `POST /api/journeys`. The handler should reject
/// with 400 EMPTY_BATCH before any namespace check fires.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn post_journey_empty_batch_400(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({ "objs": [] }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {}", body);
    assert_eq!(body["error"], "EMPTY_BATCH");
}

/// Empty `objs` batch on `POST /api/dispatch-intent-bindings`. Same 400
/// shape as journeys.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn post_bindings_empty_batch_400(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/dispatch-intent-bindings",
        &jwt,
        serde_json::json!({ "objs": [] }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {}", body);
    assert_eq!(body["error"], "EMPTY_BATCH");
}

/// Malformed UUID in path on `GET /api/dispatches/{id}`. Axum's path
/// extractor returns 400 before reaching the handler (plain-text response
/// body — not JSON, so we don't parse it).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_dispatch_malformed_uuid_400(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let resp = http
        .get(format!("{}/api/dispatches/not-a-uuid", base_url))
        .bearer_auth(&jwt)
        .send()
        .await
        .expect("malformed-uuid GET");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Nonexistent journey UUID on `GET /api/journeys/{id}/briefs`. 404 via
/// the same namespace check.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_journey_briefs_nonexistent_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/briefs", Uuid::new_v4()),
        &jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

// =============================================================================
// Happy-path full execution
// =============================================================================
//
// Ingests `assets/guides/stub-flow/` (a minimal nushell-only guide) via the
// internal materialize_guide pipeline (re-used through the `test-helpers`
// feature on dapple-prompt-api), relocates the resulting `object_paths` rows
// into the customer's namespace, then drives the end-to-end customer flow:
//
//   POST /api/journeys     → create a journey on the seeded playbook
//   POST /api/dispatches   → kick off execution (auto-spawn in-process)
//   GET  /api/dispatches/{id} (poll) → wait until the task status hits 'ok'
//
// The stub task returns `{ payload: { echo: "stub-flow", constant: 42 } }`,
// so a successful run is asserted on:
//   - dispatch_task.status == "ok"
//   - dispatch_task.context_data.data.<task-base>.json.constant == 42
//   - (with `data.<task-base>.json` matching the nu-pipeline data-namespace
//      convention from dapple-prompt-flow/AGENTS.md)

/// Resolve the stage-step + step-task associative IDs for a freshly ingested
/// playbook. These are the IDs the external dispatch handler binds (not the
/// `playbook_steps` / `playbook_tasks` table IDs).
async fn resolve_playbook_step_task(pool: &PgPool, playbook_id: Uuid) -> (Uuid, Uuid) {
    let row = sqlx::query!(
        r#"SELECT ss.id AS stage_step_id, st.id AS step_task_id
           FROM rev.playbooks p
           JOIN rev.playbook_stages ps ON ps.playbook_id = p.id
           JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
           JOIN rev.step_tasks st ON st.step_id = ss.step_id
           WHERE p.id = $1
           LIMIT 1"#,
        playbook_id,
    )
    .fetch_one(pool)
    .await
    .expect("resolve stage_step/step_task ids");
    (row.stage_step_id, row.step_task_id)
}

/// Move every `rev.object_paths` row currently owned by dapple into the
/// `customer`'s namespace AND re-scope it to 'external'. Safe on a fresh
/// `#[sqlx::test]` DB: the only paths in dapple's namespace are the ones the
/// immediately preceding internal ingest just created (at the default
/// 'public'/'common' scope), so this is effectively "donate the stub-flow tree
/// to the customer as external-scoped content." The 'external' re-scope is
/// required because the external surface's namespace gates (e.g.
/// `guide_in_namespace`, used by the journey-POST guide_id check) match only
/// `scope = 'external'`.
async fn relocate_dapple_paths_to(pool: &PgPool, customer_company_id: Uuid) {
    sqlx::query!(
        "UPDATE rev.object_paths SET namespace_company_id = $1, scope = 'external' \
         WHERE namespace_company_id = $2",
        customer_company_id,
        dapple_data::DAPPLE_COMPANY_UUID,
    )
    .execute(pool)
    .await
    .expect("relocate paths");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn happy_path_full_dispatch_execution(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }

    // Same real-HTTP pattern every test uses — see spawn_external_server.
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // 1. Onboard + exchange JWT (token exchange is part of the customer
    //    surface, so it goes over HTTP).
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange");
    let jwt = body["jwt"].as_str().expect("jwt").to_string();

    // 2. Ingest the stub-flow fixture via the internal pipeline (in-process).
    //    This is dapple-side setup, not a customer-visible operation — so it
    //    stays as a direct test-helper call rather than going over HTTP.
    let internal_state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });
    let fixture_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let ingest =
        dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &fixture_dir)
            .await
            .expect("ingest stub-flow");
    let playbook_id = ingest
        .derived_objects
        .playbook_ids
        .first()
        .copied()
        .expect("stub-flow should produce a derived playbook");

    // 3. Donate every dapple-namespace path produced by the ingest to the
    //    customer so the external surface can see them (in-process setup).
    relocate_dapple_paths_to(&pool, customer.company_id).await;

    // 4. POST /api/journeys (customer surface — real HTTP). The ingest linked
    //    the playbook to a guide via rev.guide_playbooks; resolve it for the pin.
    let guide_id = guide_for_playbook(&pool, playbook_id).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "stub journey",
                "playbook_id": playbook_id,
                "guide_id": guide_id,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "journey: {}", body);
    let journey_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // 5. Resolve the stage_step / step_task IDs the dispatch handler binds.
    let (stage_step_id, step_task_id) = resolve_playbook_step_task(&pool, playbook_id).await;

    // 6. POST /api/dispatches (customer surface — real HTTP). The handler
    //    auto-spawns execution in-process via tokio::spawn since stub-flow
    //    has no value-slots to bind.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/dispatches",
        &jwt,
        serde_json::json!({
            "objs": [{
                "journey_id": journey_id,
                "intent": {},
                "steps": [{
                    "stage_step_id": stage_step_id,
                    "params": {},
                    "tasks": [{ "step_task_id": step_task_id }],
                }],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "dispatch: {}", body);
    let dispatch_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // 7. Poll GET /api/dispatches/{id} until the task lands in `ok` (customer
    //    surface — real HTTP). Stub task is synchronous nu — should resolve
    //    within a couple of seconds in CI.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let (status, body) = get_authed(
            &http,
            &base_url,
            &format!("/api/dispatches/{}", dispatch_id),
            &jwt,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "poll: {}", body);

        let task_status = body["obj"]["stages"][0]["steps"][0]["tasks"][0]["status"]
            .as_str()
            .unwrap_or("");
        if task_status == "ok" {
            break;
        }
        if task_status == "failed" || task_status == "canceled" {
            panic!(
                "task transitioned to {} unexpectedly: {}",
                task_status, body
            );
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "task never reached ok within 20s; last status={:?}, last body={}",
                task_status, body
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // 8. Verify the persisted payload. The dispatch runner inserts an
    //    `ok` event on `rev.dispatch_task_attempt_events` carrying the
    //    flattened task output. The attempt row itself keeps its initial
    //    `{}` context_data; the payload lives on the event.
    let attempt_contexts: Vec<Option<Value>> = sqlx::query_scalar!(
        r#"SELECT e.context_data
           FROM rev.dispatch_task_attempt_events e
           JOIN rev.dispatch_task_attempts a ON a.id = e.attempt_id
           JOIN rev.dispatch_steps ds ON ds.id = a.dispatch_step_id
           JOIN rev.dispatch_stages dst ON dst.id = ds.dispatch_stage_id
           WHERE dst.dispatch_id = $1
             AND e.phase_code = 'ok'"#,
        dispatch_id,
    )
    .fetch_all(&pool)
    .await
    .expect("dispatch_task_attempt_events.context_data");

    assert!(
        !attempt_contexts.is_empty(),
        "expected at least one `ok` attempt-event row"
    );
    let merged = attempt_contexts
        .iter()
        .map(|v| v.as_ref().map(|j| j.to_string()).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        merged.contains("\"constant\":42") || merged.contains("\"constant\": 42"),
        "expected `constant: 42` in dispatch_task_attempts.context_data; got: {}",
        merged
    );
    assert!(
        merged.contains("stub-flow"),
        "expected `stub-flow` echo in dispatch_task_attempts.context_data; got: {}",
        merged
    );

    // 9. SDK round-trip — call ExternalDispatchesClient::task_attempts via
    //    the spawned server. Exercises the SDK shape end-to-end: the response
    //    must deserialize as Vec<ExternalDispatchTaskAttempt> — the projected
    //    operator DTO carrying the author `signals` channel in place of the
    //    internal `context_metadata` grab-bag.
    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );
    let attempts = sdk
        .dispatches()
        .task_attempts(dispatch_id)
        .await
        .expect("SDK task_attempts call");
    assert!(!attempts.is_empty(), "expected at least one task attempt");
    // The runner persists author signals under `context_metadata.signals`; the
    // endpoint projects that to the `signals` field. The ln task emits no custom
    // signal, so the projected value is an (empty) object — asserting the field
    // deserialized as the projected shape proves the wire contract end-to-end.
    assert!(
        attempts.iter().any(|a| a.signals.is_object()),
        "at least one attempt must expose the projected `signals` object"
    );

    // 10. SDK round-trip — ExternalDappleClient::playbooks(). The ingested
    //     `stub-flow` guide produced `playbook_id`; both list (paginated)
    //     and get-by-id should surface it through the customer-facing SDK.
    let playbooks = sdk
        .playbooks()
        .list(None)
        .await
        .expect("SDK playbooks().list call");
    assert!(
        playbooks.iter().any(|p| p.id == playbook_id),
        "SDK playbooks().list did not include the ingested playbook {}; got {:?}",
        playbook_id,
        playbooks.iter().map(|p| p.id).collect::<Vec<_>>(),
    );

    let fetched = sdk
        .playbooks()
        .get(playbook_id)
        .await
        .expect("SDK playbooks().get call");
    assert_eq!(fetched.id, playbook_id);
}

/// Computed-output completion on the EXTERNAL surface (acme's path): ingest
/// a guide whose single-step stage exposes a computed `:::brief-output`, run the
/// dispatch through the SHARED runner, and assert the journey's `/stages` and
/// `/steps` report `done == true`. Surface-neutral twin of the internal
/// `computed_output_reaches_done_after_dispatch` lifecycle trial — the producer
/// lives in the one shared `execute_dispatch`, so both surfaces must observe it.
/// The stage exposes ONLY the computed slot (slot-a is an unexposed input), so
/// the dispatch is start-ready and auto-spawns — no operator binding needed.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn computed_output_reaches_done_after_dispatch(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }

    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "donutco", "donutco_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange");
    let jwt = body["jwt"].as_str().expect("jwt").to_string();

    // Ingest the computed-output guide via the internal pipeline (dapple-side
    // setup), then donate its paths to the customer namespace.
    let internal_state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });
    let suffix = Uuid::now_v7().to_string();
    let tmp = make_computed_output_guide_dir(&suffix);
    let guide_dir = tmp.path().join(format!("computed-output-guide-{suffix}"));
    let ingest = dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &guide_dir)
        .await
        .expect("ingest computed-output guide");
    let playbook_id = ingest
        .derived_objects
        .playbook_ids
        .first()
        .copied()
        .expect("derived playbook");

    relocate_dapple_paths_to(&pool, customer.company_id).await;

    // POST journey — empty brief_pack_ids auto-derives the guide's pack.
    let guide_id = guide_for_playbook(&pool, playbook_id).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "computed journey",
                "playbook_id": playbook_id,
                "guide_id": guide_id,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "journey: {}", body);
    let journey_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    let (stage_step_id, step_task_id) = resolve_playbook_step_task(&pool, playbook_id).await;

    // POST dispatch — start-ready (only a computed slot is exposed), so it
    // auto-spawns the shared execute_dispatch.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/dispatches",
        &jwt,
        serde_json::json!({
            "objs": [{
                "journey_id": journey_id,
                "intent": {},
                "steps": [{
                    "stage_step_id": stage_step_id,
                    "params": {},
                    "tasks": [{ "step_task_id": step_task_id }],
                }],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "dispatch: {}", body);
    let dispatch_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // Poll until the task lands `ok`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let (status, body) = get_authed(
            &http,
            &base_url,
            &format!("/api/dispatches/{}", dispatch_id),
            &jwt,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "poll: {}", body);
        let task_status = body["obj"]["stages"][0]["steps"][0]["tasks"][0]["status"]
            .as_str()
            .unwrap_or("");
        if task_status == "ok" {
            break;
        }
        if task_status == "failed" || task_status == "canceled" {
            panic!(
                "task transitioned to {} unexpectedly: {}",
                task_status, body
            );
        }
        if std::time::Instant::now() > deadline {
            panic!("task never reached ok within 20s; last body={}", body);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // The shared runtime producer bound the computed output → the journey's
    // per-stage and per-step `done` rollups (what dapple-ui/dapple-suite and
    // acme all poll) must now be true.
    let (status, stages) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/stages", journey_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stages: {}", stages);
    let stage_objs = stages["objs"].as_array().expect("stages objs");
    assert!(!stage_objs.is_empty(), "must report at least one stage");
    assert!(
        stage_objs.iter().all(|o| o["done"].as_bool() == Some(true)),
        "every stage must be done after the computed output is produced: {}",
        stages
    );

    let (status, steps) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/steps", journey_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "steps: {}", steps);
    let step_objs = steps["objs"].as_array().expect("steps objs");
    assert!(
        step_objs.iter().any(|o| {
            o["stage_step_id"].as_str() == Some(stage_step_id.to_string().as_str())
                && o["done"].as_bool() == Some(true)
        }),
        "the step must be done after the computed output is produced: {}",
        steps
    );

    // Stage identity rides the EXTERNAL wire too (same DTOs as internal): every
    // step row names its owning stage; a customer renders the pane title from
    // this instead of inventing one from the stage ordinal.
    assert!(
        step_objs
            .iter()
            .all(|o| !o["stage_name"].as_str().unwrap_or_default().is_empty()),
        "every /steps row must carry its owning stage's authored name: {}",
        steps
    );
    assert!(
        stage_objs
            .iter()
            .all(|o| !o["name"].as_str().unwrap_or_default().is_empty()),
        "every /stages row must carry the authored stage name: {}",
        stages
    );

    // Self-describing slot contract (over the wire): the computed synthesis must
    // report an explicit `producer: "computed"` AND carry the journey-pinned
    // `value_slot_id` — so a consumer renders a "waiting on the server" state
    // instead of inferring server-produced-ness from a NULL id (the old,
    // implicit signal acme had to reverse-engineer).
    let synth_name = format!("synth-{suffix}");
    let synth_slot = step_objs
        .iter()
        .flat_map(|o| o["exposed_slots"].as_array().cloned().unwrap_or_default())
        .find(|sl| sl["name"].as_str() == Some(synth_name.as_str()))
        .unwrap_or_else(|| panic!("synth exposed slot must be present in /steps: {}", steps));
    assert_eq!(
        synth_slot["producer"].as_str(),
        Some("computed"),
        "the computed synthesis must report producer=computed: {}",
        synth_slot
    );
    assert!(
        synth_slot["value_slot_id"].as_str().is_some(),
        "computed slots now carry the journey-pinned value_slot_id (not NULL): {}",
        synth_slot
    );
    assert!(
        synth_slot.get("input").is_none() || synth_slot["input"].is_null(),
        "computed slots have no operator render spec: {}",
        synth_slot
    );

    // Typed display contract (Part C/D): the computed slot carries the authored
    // render hint over the EXTERNAL wire, so the UI on external-api.dapple.x
    // renders the produced value per `display.kind` instead of guessing. The
    // fixture authored `display: "markdown", reveal: true, title: "Synth Title"`
    // on the computed `:::brief-output`; assert it round-trips (a non-default
    // value proves the hint is threaded, not just the `text` fallback). The live
    // value itself stays on the bindings read (joined by value_slot_id) — never
    // on this cached structural slot.
    let display = &synth_slot["display"];
    assert_eq!(
        display["kind"].as_str(),
        Some("markdown"),
        "computed slot must carry the authored display kind over the external wire: {}",
        synth_slot
    );
    assert_eq!(
        display["reveal"].as_bool(),
        Some(true),
        "computed slot must carry the authored `reveal` modifier: {}",
        synth_slot
    );
    assert_eq!(
        display["title"].as_str(),
        Some("Synth Title"),
        "computed slot must carry the authored display title: {}",
        synth_slot
    );

    // The external task-attempt contract must surface the producer's per-output
    // outcome so a customer can see WHY a computed slot is (or is not) bound,
    // instead of reading dapple pod logs.
    let (status, attempts) = get_authed(
        &http,
        &base_url,
        &format!("/api/dispatches/{}/task_attempts", dispatch_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "task_attempts: {}", attempts);
    let attempt_objs = attempts["objs"].as_array().expect("task_attempts objs");
    assert!(
        attempt_objs.iter().any(|a| {
            a["computed_outputs"]
                .as_array()
                .map(|cos| {
                    cos.iter().any(|c| {
                        c["name"].as_str() == Some(synth_name.as_str())
                            && c["outcome"].as_str() == Some("produced")
                    })
                })
                .unwrap_or(false)
        }),
        "task_attempts must report a `produced` computed_outputs outcome for synth: {}",
        attempts
    );
}

/// Regression for the wimmelbook "render ran before the shotlist existed" race.
/// A guide with a PRODUCER stage (binds a computed slot) followed by a CONSUMER
/// stage (a later step that also exposes that computed slot) must:
///   1. GATE — a dispatch for the consumer step must NOT auto-execute while the
///      computed slot is unbound (verified by ZERO task attempts in the DB, not
///      by a response flag).
///   2. RE-TRIGGER — once the producer dispatch runs and binds the slot, the
///      previously-blocked consumer dispatch must execute on its own (the
///      `execute_dispatch_and_retrigger` drain), reaching `ok`.
/// Both halves run through the SHARED runner, so both API surfaces inherit them.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn consumed_computed_dep_gates_then_retriggers(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }

    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "renderco", "renderco_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange");
    let jwt = body["jwt"].as_str().expect("jwt").to_string();

    let internal_state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });
    let suffix = Uuid::now_v7().to_string();
    let tmp = make_consumed_computed_guide_dir(&suffix);
    let guide_dir = tmp.path().join(format!("consumed-computed-guide-{suffix}"));
    let ingest = dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &guide_dir)
        .await
        .expect("ingest consumed-computed guide");
    let playbook_id = ingest
        .derived_objects
        .playbook_ids
        .first()
        .copied()
        .expect("derived playbook");

    relocate_dapple_paths_to(&pool, customer.company_id).await;

    let guide_id = guide_for_playbook(&pool, playbook_id).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{ "name": "render-race journey", "playbook_id": playbook_id, "guide_id": guide_id }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "journey: {}", body);
    let journey_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // steps[0] = producer (stage ord 1), steps[1] = consumer (stage ord 2).
    let steps = resolve_playbook_steps_ordered(&pool, playbook_id).await;
    assert_eq!(steps.len(), 2, "fixture must derive exactly two steps");
    let (producer_step, producer_task) = steps[0];
    let (consumer_step, consumer_task) = steps[1];

    // 1. GATE: dispatch the CONSUMER first. Its step exposes the computed slot
    // the earlier producer stage binds, so it must NOT auto-execute yet.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/dispatches",
        &jwt,
        serde_json::json!({
            "objs": [{
                "journey_id": journey_id,
                "intent": {},
                "steps": [{ "stage_step_id": consumer_step, "params": {}, "tasks": [{ "step_task_id": consumer_task }] }],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "consumer dispatch: {}", body);
    let consumer_dispatch_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // Give any (incorrect) auto-spawn a chance to run, then assert via the DB
    // that the consumer dispatch executed NOTHING — the gate held it back.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(
        dispatch_attempt_count(&pool, consumer_dispatch_id).await,
        0,
        "the consumer dispatch must NOT execute while its computed dependency is unbound",
    );

    // 2. RE-TRIGGER: dispatch the PRODUCER. It is start-ready, runs, binds the
    // computed slot, and its drain must launch the blocked consumer.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/dispatches",
        &jwt,
        serde_json::json!({
            "objs": [{
                "journey_id": journey_id,
                "intent": {},
                "steps": [{ "stage_step_id": producer_step, "params": {}, "tasks": [{ "step_task_id": producer_task }] }],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "producer dispatch: {}", body);

    // Poll the CONSUMER dispatch (not the producer): it must now execute on its
    // own via the re-trigger drain and reach `ok`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    loop {
        let (status, body) = get_authed(
            &http,
            &base_url,
            &format!("/api/dispatches/{}", consumer_dispatch_id),
            &jwt,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "poll consumer: {}", body);
        let task_status = body["obj"]["stages"][0]["steps"][0]["tasks"][0]["status"]
            .as_str()
            .unwrap_or("");
        if task_status == "ok" {
            break;
        }
        if task_status == "failed" || task_status == "canceled" {
            panic!(
                "consumer task transitioned to {} unexpectedly: {}",
                task_status, body
            );
        }
        assert!(
            std::time::Instant::now() <= deadline,
            "consumer dispatch never re-triggered to ok within 25s; last body={}",
            body
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // Belt-and-suspenders: the DB now shows the consumer actually executed.
    assert!(
        dispatch_attempt_count(&pool, consumer_dispatch_id).await > 0,
        "consumer dispatch must have task attempts after the re-trigger ran it",
    );
}

/// Reconcile-on-poll recovery. The auto-dispatch cascade normally advances only as
/// a side-effect of a finishing dispatch's in-process retrigger drain; if that
/// detached future is lost (e.g. a pod restart during a long task) the journey
/// stalls with no recovery. The fix: `GET /journeys/{id}/stages` (and `/steps`)
/// best-effort spawn `reconcile_journey_dispatches`.
///
/// `make_consumed_computed_guide_dir` has TWO no-operator-input compute stages, so
/// journey-create kicks NO dispatch — the journey sits at zero dispatches, the exact
/// stalled shape (nothing has driven the cascade). Without reconcile-on-poll it
/// would stay stuck forever. This test drives it PURELY by polling `/stages`: each
/// poll spawns a reconcile that advances one cascade layer per drain (poll 1 runs
/// the producer stage; a later poll, once the producer is done, creates and runs the
/// consumer stage), until both stages reach `done`. Both API surfaces wire the same
/// shared `reconcile_journey_dispatches`, so the internal surface inherits this.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn reconcile_on_poll_drives_stalled_journey_cascade(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }

    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "reconcileco", "reconcileco_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange");
    let jwt = body["jwt"].as_str().expect("jwt").to_string();

    let internal_state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });
    let suffix = Uuid::now_v7().to_string();
    let tmp = make_two_stage_distinct_slots_guide_dir(&suffix);
    let guide_dir = tmp
        .path()
        .join(format!("two-stage-distinct-guide-{suffix}"));
    let ingest = dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &guide_dir)
        .await
        .expect("ingest two-stage distinct-slots guide");
    let playbook_id = ingest
        .derived_objects
        .playbook_ids
        .first()
        .copied()
        .expect("derived playbook");

    relocate_dapple_paths_to(&pool, customer.company_id).await;

    let guide_id = guide_for_playbook(&pool, playbook_id).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{ "name": "stalled cascade journey", "playbook_id": playbook_id, "guide_id": guide_id }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "journey: {}", body);
    let journey_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // Precondition (the stalled state): both stages are no-input compute steps, so
    // journey-create kicks NO dispatch. Nothing has driven the cascade yet.
    assert_eq!(
        journey_dispatch_count(&pool, journey_id).await,
        0,
        "a fresh all-no-input journey must start with zero dispatches (the stalled state reconcile recovers)",
    );

    // Drive purely by polling GET /stages. Each poll spawns reconcile_journey_dispatches
    // (after the namespace check passes) which advances one cascade layer per drain.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let (status, body) = get_authed(
            &http,
            &base_url,
            &format!("/api/journeys/{}/stages", journey_id),
            &jwt,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "poll stages: {}", body);
        let stages = body["objs"].as_array().expect("objs array");
        assert_eq!(stages.len(), 2, "fixture has two stages: {}", body);
        if stages.iter().all(|s| s["done"].as_bool().unwrap_or(false)) {
            break;
        }
        assert!(
            std::time::Instant::now() <= deadline,
            "reconcile-on-poll never drove both stages to done within 30s; last body={}",
            body
        );
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    // Belt-and-suspenders: the DB shows a dispatch was created (and ran) for EACH
    // no-input compute stage — reconcile drove the whole cascade from zero.
    assert_eq!(
        journey_dispatch_count(&pool, journey_id).await,
        2,
        "reconcile must have created a dispatch for each no-input compute stage",
    );
}

/// Change-B guard: a SINGLE awaited `reconcile_journey_dispatches` (= one
/// `drain_ready_dispatches`) must walk a linear multi-stage no-input compute
/// journey to completion, not advance only one stage. `ensure_pending_autodispatches`
/// now runs at the top of every drain-loop iteration, so as stage A completes and
/// binds its slot, stage B becomes creatable and runs in the SAME drain. Pre-fix
/// (ensure ran once, before the loop) one drain created+ran only stage A → the
/// count stalled at 1, and the whole journey depended on repeated external drives
/// (polling) to inch forward — which never happens in the flow-delegation topology.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn single_reconcile_walks_full_cascade(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }

    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "onedrainco", "onedrainco_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange");
    let jwt = body["jwt"].as_str().expect("jwt").to_string();

    let internal_state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });
    let suffix = Uuid::now_v7().to_string();
    let tmp = make_two_stage_distinct_slots_guide_dir(&suffix);
    let guide_dir = tmp
        .path()
        .join(format!("two-stage-distinct-guide-{suffix}"));
    let ingest = dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &guide_dir)
        .await
        .expect("ingest two-stage distinct-slots guide");
    let playbook_id = ingest
        .derived_objects
        .playbook_ids
        .first()
        .copied()
        .expect("derived playbook");

    relocate_dapple_paths_to(&pool, customer.company_id).await;

    let guide_id = guide_for_playbook(&pool, playbook_id).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{ "name": "one-drain cascade journey", "playbook_id": playbook_id, "guide_id": guide_id }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "journey: {}", body);
    let journey_id: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    assert_eq!(
        journey_dispatch_count(&pool, journey_id).await,
        0,
        "fresh all-no-input journey starts with zero dispatches",
    );

    // ONE awaited drain must walk BOTH stages: create+run A, then — because
    // ensure_pending_autodispatches re-runs each loop iteration — create+run B.
    dapple_prompt_flow::dispatch_runner::reconcile_journey_dispatches(
        DbPool::from_pool(pool.clone()),
        journey_id,
        4,
    )
    .await;

    assert_eq!(
        journey_dispatch_count(&pool, journey_id).await,
        2,
        "a single drain must walk the whole linear compute chain (change B); pre-fix this stalled at 1",
    );
}

// =============================================================================
// External ingest — POST /api/guides/ingest
// =============================================================================
//
// Customers push their own guide tarballs over the external surface. Every
// `rev.object_paths` row produced by this ingest lands under
// `(namespace_company_id = caller.company_id, scope = 'external')` via the
// per-transaction GUC pattern in `assets/sqlc/shared/insert_object_paths.sqlc`.
// Internal ingest stays untouched (no GUC set → defaults to dapple + 'public').

/// Tar+gzip a guide directory, mirroring what the TUI `/guide sync` command
/// produces.
fn build_tarball(guide_dir: &std::path::Path) -> Vec<u8> {
    let mut tar_bytes: Vec<u8> = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut tar_bytes, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        builder.follow_symlinks(true);
        builder
            .append_dir_all(guide_dir.file_name().unwrap(), guide_dir)
            .expect("tar build");
        builder
            .into_inner()
            .expect("encoder")
            .finish()
            .expect("flate finish");
    }
    tar_bytes
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_ingest_lands_in_customer_namespace(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // 1. Onboard customer + JWT exchange.
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 2. Build a tarball of the stub-flow fixture and POST it.
    let fixture_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let tar_bytes = build_tarball(&fixture_dir);

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tar_bytes)
            .file_name("stub-flow.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );

    let resp = http
        .post(format!("{}/api/guides/ingest", base_url))
        .bearer_auth(&jwt)
        .multipart(form)
        .send()
        .await
        .expect("ingest");
    assert_eq!(resp.status(), StatusCode::CREATED, "ingest failed");
    let ingest_body: Value = resp.json().await.unwrap();
    let guide_id: Uuid =
        serde_json::from_value(ingest_body["guide"]["id"].clone()).expect("guide id");

    // 3. Verify the guide's `object_paths` row landed in the customer's
    //    namespace at scope='external' (the load-bearing property: GUC
    //    override stamped the right values through the shared CTE).
    let guide_path_row = sqlx::query!(
        r#"SELECT op.namespace_company_id, op.scope
           FROM rev.guides g
           JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
           JOIN rev.object_paths op ON op.id = vo.path_id
           WHERE g.id = $1"#,
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .expect("guide path lookup");

    assert_eq!(
        guide_path_row.namespace_company_id, customer.company_id,
        "guide namespace must equal customer's company_id"
    );
    assert_eq!(
        guide_path_row.scope, "external",
        "guide scope must be 'external' for customer ingest"
    );

    // 3b. The conduit guide frame (CONDUITS.md) is stored on the externally
    //     ingested guide version row — external ingest shares the internal
    //     materializer, so the frame must land identically.
    let conduits_md: Option<String> = sqlx::query_scalar!(
        r#"SELECT conduits_md FROM rev.guides WHERE id = $1"#,
        guide_id
    )
    .fetch_one(&pool)
    .await
    .expect("conduits_md lookup");
    assert!(
        conduits_md
            .as_deref()
            .unwrap_or("")
            .contains("# Conduit Context"),
        "external ingest must store the guide's CONDUITS.md: {conduits_md:?}"
    );

    // 4. Verify ALL derived-object paths from this ingest also landed in
    //    the customer's namespace (the GUC propagates through every
    //    shared-CTE INSERT in the same transaction). We approximate "from
    //    this ingest" by `created_at >= guide_path.created_at` — fine for
    //    a fresh per-test DB with one ingest.
    let dapple_count_in_ingest = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.object_paths op
           WHERE op.namespace_company_id = $1::UUID
             AND op.created_at >= (
                 SELECT op2.created_at
                 FROM rev.object_paths op2
                 JOIN rev.versioned_objects vo ON vo.path_id = op2.id
                 JOIN rev.guides g ON g.versioned_object_id = vo.id
                 WHERE g.id = $2
             )"#,
        dapple_data::DAPPLE_COMPANY_UUID,
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .expect("dapple count");
    assert_eq!(
        dapple_count_in_ingest, 0,
        "ingest must not leak any paths into dapple's namespace"
    );

    // 5. List guides via the external surface — confirms the path is
    //    visible to the customer and only to them.
    let (status, body) = list_guides(&http, &base_url, &jwt).await;
    assert_eq!(status, StatusCode::OK);
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(
        objs.len(),
        1,
        "customer should see exactly the guide they just ingested"
    );
    assert_eq!(objs[0]["id"], serde_json::json!(guide_id));
}

/// Counterpart to the above: a second customer cannot see the first
/// customer's ingested guide. Real-HTTP version of the namespace-isolation
/// invariant for the ingest pipeline (the in-process test already covered
/// list-only; this covers full ingest + list).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_ingest_is_namespace_isolated(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // Onboard two customers.
    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;

    let exchange = |c: &TestCustomer| {
        let app_url = base_url.clone();
        let signed = sign_test_token(c, 60, 0);
        let sid = c.service_id.clone();
        let http = http.clone();
        async move {
            let (status, body) = exchange_token(&http, &app_url, &signed, &sid).await;
            assert_eq!(status, StatusCode::OK);
            body["jwt"].as_str().unwrap().to_string()
        }
    };

    let alpha_jwt = exchange(&alpha).await;
    let beta_jwt = exchange(&beta).await;

    // Alpha ingests the stub-flow fixture.
    let fixture_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let tar_bytes = build_tarball(&fixture_dir);
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tar_bytes)
            .file_name("stub-flow.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let resp = http
        .post(format!("{}/api/guides/ingest", base_url))
        .bearer_auth(&alpha_jwt)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Beta lists — must see zero guides.
    let (status, body) = list_guides(&http, &base_url, &beta_jwt).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["objs"].as_array().unwrap().len(),
        0,
        "beta must not see alpha's ingested guide"
    );
}

/// Regression: when a brief-input references a value-slot name that doesn't
/// exist in the ingest plan, the materializer used to silently insert a
/// brief_unit_input row with an empty `control_spec`. Downstream that surfaces
/// as input-less brief units on `GET /api/journeys/{id}/brief-form`, which is
/// what bug #3 from the dapple-ui UX team was — a guide ingested cleanly but
/// the form renderer got no inputs back.
///
/// The fix at `guide_ingest.rs::materialize_brief_system` converts the silent
/// `warn!()` into a hard `DataError::InvalidInput` with diagnostic context
/// (the offending slot name + the full list of known slot names). This test
/// locks that behavior in: a deliberately-broken guide tarball must fail
/// ingest with both substrings present in the error body.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_ingest_rejects_brief_input_missing_slot(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // 1. Onboard customer + JWT exchange.
    let customer = setup_customer(&pool, "broken-slot-corp", "broken_slot_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 2. Build a minimal broken guide on disk. The chapter wires a brief-pack
    //    → brief-group → brief → brief-input chain where the input references
    //    a value-slot name that is NOT defined anywhere in the guide.
    let tmp = tempfile::tempdir().expect("tempdir");
    let guide_dir = tmp.path().join("broken-slot-guide");
    std::fs::create_dir_all(guide_dir.join("locales")).expect("mkdir locales");

    std::fs::write(
        guide_dir.join("guide.toml"),
        r#"title = "Broken Slot Guide"
description = "Fixture that references a nonexistent value slot from a brief-input."
author = ""
classification = "workflow"
source_path = "broken-slot-guide"

[chapters."intro.md"]
subtitle = "Intentionally broken: brief-input points at intentionally-missing-slot."
"#,
    )
    .expect("write guide.toml");

    std::fs::write(
        guide_dir.join("SUMMARY.md"),
        "# Summary\n\n- [Introduction](intro.md)\n",
    )
    .expect("write SUMMARY.md");
    std::fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

    std::fs::write(
        guide_dir.join("intro.md"),
        r#"# Introduction

A minimal fixture that wires a brief-input to a value-slot name that no
`:::value-slot` directive declares. Used to lock in the loud-error path on the
brief-system materializer.

:::brief-input { name: "broken-slot-input", control: "text", slot: "intentionally-missing-slot" }
:::

:::brief { name: "broken-slot-brief", units: ["broken-slot-input"] }
:::

:::brief-group { name: "broken-slot-group", briefs: ["broken-slot-brief"] }
:::

:::brief-pack { name: "broken-slot-pack", groups: ["broken-slot-group"] }
:::
"#,
    )
    .expect("write intro.md");

    std::fs::write(
        guide_dir.join("locales/en.ftl"),
        r#"broken-slot-input = Broken slot input
broken-slot-brief = Broken slot brief
broken-slot-group = Broken slot group
broken-slot-pack = Broken slot pack
"#,
    )
    .expect("write en.ftl");

    let tar_bytes = build_tarball(&guide_dir);

    // 3. POST the broken tarball to the external ingest.
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tar_bytes)
            .file_name("broken-slot-guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let resp = http
        .post(format!("{}/api/guides/ingest", base_url))
        .bearer_auth(&jwt)
        .multipart(form)
        .send()
        .await
        .expect("ingest");

    // 4. Must return 422 UNPROCESSABLE_ENTITY. `materialize_brief_system`
    //    raises `DataError::InvalidInput`; the ingest handler maps
    //    `InvalidInput` to 422 via `data_error_to_status` so the customer
    //    can tell their guide is malformed (vs paging an oncall on 500).
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "ingest of guide with missing slot must return 422; got status {}",
        resp.status()
    );

    // 5. Body carries the loud error: names the offending slot, lists the
    //    known set, mentions "value slot" so future readers grepping the
    //    test for the failure mode find it.
    let body: Value = resp.json().await.expect("error body json");
    let message = body["message"].as_str().unwrap_or("");
    assert!(
        message.contains("intentionally-missing-slot"),
        "error message must name the missing slot; got: {}",
        message
    );
    assert!(
        message.contains("broken-slot-input"),
        "error message must name the offending brief-input; got: {}",
        message
    );
    assert!(
        message.contains("Known slot names"),
        "error message must include the diagnostic 'Known slot names' list; got: {}",
        message
    );

    // 6. The transaction must have rolled back: no `rev.brief_unit_input`
    //    row exists for the broken-slot-input. (If the silent-fail regressed,
    //    an empty-control_spec row would land here.)
    let leaked_inputs: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.brief_unit_input bui
           JOIN rev.brief_units bu ON bu.id = bui.brief_unit_id
           WHERE bu.unit_key = 'broken-slot-input'"#,
    )
    .fetch_one(&pool)
    .await
    .expect("brief_unit_input count");
    assert_eq!(
        leaked_inputs, 0,
        "transaction must roll back; no brief_unit_input rows for the broken input may persist"
    );
}

// =============================================================================
// New endpoints — GET /api/dispatches/{id}/task_attempts
// =============================================================================
//
// Plan: docs/plans/external-api/01-operator-runner-endpoints.md §6.1–§6.5

/// Build a minimal dispatch chain (dispatch → dispatch_stage → dispatch_step
/// → dispatch_task_attempt) so the task_attempts endpoint has data to return.
/// This bypasses the full materialize/ingest path by directly inserting the
/// chain; sufficient for namespace-isolation + pass-through-DTO tests.
async fn seed_task_attempt_for_dispatch(
    pool: &PgPool,
    customer: &TestCustomer,
    playbook_id: Uuid,
    dispatch_id: Uuid,
) -> Uuid {
    // 1. Need a `rev.stages` row + `rev.playbook_stages` row to satisfy the
    //    dispatch_stages FK. Borrow the same external-scope path machinery.
    let entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'stage'")
            .fetch_one(pool)
            .await
            .unwrap();
    let stage_path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let stage_vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        stage_path_id,
        format!(
            "{:020x}{:020x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let stage_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.stages (versioned_object_id, description) \
         VALUES ($1, $2) RETURNING id",
        stage_vo_id,
        "seed stage",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let playbook_stage_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.playbook_stages (playbook_id, stage_id, ord) \
         VALUES ($1, $2, 1) RETURNING id",
        playbook_id,
        stage_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // 2. Step + stage_step (for dispatch_steps.stage_step_id FK).
    let step_entity_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'step'")
            .fetch_one(pool)
            .await
            .unwrap();
    let step_path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        step_entity_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let step_vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        step_path_id,
        format!(
            "{:020x}{:020x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let step_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.steps (versioned_object_id, description) \
         VALUES ($1, $2) RETURNING id",
        step_vo_id,
        "seed step",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let stage_step_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.stage_steps (stage_id, step_id, ord) \
         VALUES ($1, $2, 1) RETURNING id",
        stage_id,
        step_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // 3. Task — referenced from dispatch_task_attempts.task_id.
    let task_entity_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'task'")
            .fetch_one(pool)
            .await
            .unwrap();
    let task_path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        task_entity_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let task_vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        task_path_id,
        format!(
            "{:020x}{:020x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let task_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.tasks (versioned_object_id, description, function, function_input) \
         VALUES ($1, $2, $3, $4::jsonb) RETURNING id",
        task_vo_id,
        "seed task",
        "noop",
        serde_json::json!({}),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // 4. dispatch_stage → dispatch_step linking to the dispatch.
    let dispatch_stage_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.dispatch_stages (dispatch_id, playbook_stage_id, params) \
         VALUES ($1, $2, $3::jsonb) RETURNING id",
        dispatch_id,
        playbook_stage_id,
        serde_json::json!({}),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let dispatch_step_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.dispatch_steps (dispatch_stage_id, stage_step_id, params) \
         VALUES ($1, $2, $3::jsonb) RETURNING id",
        dispatch_stage_id,
        stage_step_id,
        serde_json::json!({}),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // 5. dispatch_task_attempt — the row the endpoint actually returns.
    let attempt_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.dispatch_task_attempts \
         (dispatch_step_id, task_id, context_data, context_metadata) \
         VALUES ($1, $2, $3::jsonb, $4::jsonb) RETURNING id",
        dispatch_step_id,
        task_id,
        serde_json::json!({}),
        // `context_metadata.signals` is the operator-facing projection source.
        // Seed a real author signal so the endpoint test can assert it surfaces
        // on the projected `signals` field.
        serde_json::json!({"signals": {"menu_thin": true}}),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    attempt_id
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_dispatch_task_attempts_happy_path(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;
    let alpha_journey = seed_external_journey(&pool, &alpha, alpha_playbook).await;
    let alpha_dispatch = seed_external_dispatch(&pool, &alpha, alpha_journey).await;
    let attempt_id =
        seed_task_attempt_for_dispatch(&pool, &alpha, alpha_playbook, alpha_dispatch).await;

    let signed = sign_test_token(&alpha, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &alpha.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange: {}", body);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/dispatches/{}/task_attempts", alpha_dispatch),
        &jwt,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body: {}", body);
    assert_eq!(body["status"], "ok");
    let objs = body["objs"].as_array().expect("objs array");
    assert_eq!(
        objs.len(),
        1,
        "expected exactly one seeded attempt; body: {}",
        body
    );
    assert_eq!(objs[0]["id"], serde_json::json!(attempt_id));
    // Projected DTO: operator-facing fields present, the internal
    // `context_metadata` grab-bag absent, and the author `signals` channel
    // surfaced from `context_metadata.signals`.
    assert!(objs[0].get("context_data").is_some());
    assert!(objs[0].get("status").is_some());
    assert!(objs[0].get("task_id").is_some());
    assert!(
        objs[0].get("context_metadata").is_none(),
        "internal context_metadata grab-bag must not be on the external wire; body: {}",
        body
    );
    assert_eq!(
        objs[0]["signals"]["menu_thin"],
        serde_json::json!(true),
        "author signal must surface via the projected `signals` field; body: {}",
        body
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_dispatch_task_attempts_foreign_dispatch_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;
    let alpha_journey = seed_external_journey(&pool, &alpha, alpha_playbook).await;
    let alpha_dispatch = seed_external_dispatch(&pool, &alpha, alpha_journey).await;

    let (_beta, beta_jwt, _beta_pb) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/dispatches/{}/task_attempts", alpha_dispatch),
        &beta_jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("not found in caller's namespace"),
        "body: {}",
        body
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_dispatch_task_attempts_empty(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let playbook = seed_external_playbook(&pool, &alpha).await;
    let journey = seed_external_journey(&pool, &alpha, playbook).await;
    let dispatch = seed_external_dispatch(&pool, &alpha, journey).await;

    let signed = sign_test_token(&alpha, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &alpha.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange: {}", body);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/dispatches/{}/task_attempts", dispatch),
        &jwt,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body: {}", body);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["results"], 0);
    assert_eq!(body["objs"].as_array().unwrap().len(), 0);
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_dispatch_task_attempts_nonexistent_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/dispatches/{}/task_attempts", Uuid::new_v4()),
        &jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

/// Regression (fork over-dispatch): a stage-level fork at stage N must NOT
/// auto-dispatch the inherited pre-fork computed stages (ord < N) — only stage
/// >= N is (re)done. Builds a 2-stage playbook where BOTH stages are
/// pure-compute no-input steps (so both are auto-dispatch candidates), then
/// asserts `select_pending_autodispatch_steps_for_journey` returns BOTH stages
/// for a plain journey but ONLY the fork stage for a journey forked at stage 2.
/// Guards the `fork_cutoff` filter in the query against regression.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn fork_at_stage_excludes_pre_fork_pending_autodispatch(pool: PgPool) {
    let alpha = setup_customer(&pool, "forkcorp", "forkcorp_prod").await;
    let playbook_id = seed_external_playbook(&pool, &alpha).await;
    let guide_id = seed_external_guide(&pool, &alpha, "fork-guide").await;
    let principal_id = service_principal_id(&pool, &alpha).await;

    // Two stages, each a single pure-compute no-input step (one exposed
    // computed brief-output, no operator input) — both auto-dispatch candidates.
    let mut playbook_stage_ids: Vec<Uuid> = Vec::new();
    for i in 1..=2i32 {
        let stage_id = seed_external_stage(&pool, &alpha, &format!("fs{i}")).await;
        let step_id = seed_external_step(&pool, &alpha, &format!("fstep{i}")).await;
        let task_id = seed_external_task(&pool, &alpha, &format!("ftask{i}")).await;
        let ps_id: Uuid = sqlx::query_scalar!(
            "INSERT INTO rev.playbook_stages (playbook_id, stage_id, ord) VALUES ($1, $2, $3) RETURNING id",
            playbook_id, stage_id, i,
        ).fetch_one(&pool).await.unwrap();
        let ss_id: Uuid = sqlx::query_scalar!(
            "INSERT INTO rev.stage_steps (stage_id, step_id, ord) VALUES ($1, $2, 1) RETURNING id",
            stage_id,
            step_id,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query!(
            "INSERT INTO rev.step_tasks (step_id, task_id, ord) VALUES ($1, $2, 1)",
            step_id,
            task_id,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Computed output slot exposed at the step; NO operator input → the
        // step classifies as pure-compute auto-dispatch.
        let vs_id = seed_external_value_slot(&pool, &alpha, &format!("fout{i}")).await;
        let slot_path_id: Uuid = sqlx::query_scalar!(
            "SELECT vo.path_id FROM rev.value_slots vs \
             JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id WHERE vs.id = $1",
            vs_id,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query!(
            "INSERT INTO rev.stage_step_overlays (stage_step_id, value_slot_path_id, visibility) \
             VALUES ($1, $2, 'expose')",
            ss_id,
            slot_path_id,
        )
        .execute(&pool)
        .await
        .unwrap();
        let (_bpath, bvo) = seed_path_and_versioned(&pool, &alpha, "brief").await;
        let brief_id: Uuid = sqlx::query_scalar!(
            "INSERT INTO rev.briefs (versioned_object_id, title, description) \
             VALUES ($1, $2, $3) RETURNING id",
            bvo,
            format!("fork brief {i}"),
            format!("fork brief {i}"),
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query!(
            "INSERT INTO rev.brief_outputs (brief_id, value_slot_id, unit_key, source_kind_id, source_path) \
             VALUES ($1, $2, $3, (SELECT id FROM rev.brief_output_source_kinds WHERE key = 'computed'), '.')",
            brief_id, vs_id, format!("fu{i}"),
        ).execute(&pool).await.unwrap();

        playbook_stage_ids.push(ps_id);
    }
    let stage2_ps_id = playbook_stage_ids[1];

    // Plain journey A on the playbook (no fork).
    let journey_a: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.journeys (playbook_id, principal_id, name, owner_company_id, guide_id) \
         VALUES ($1, $2, 'fork A', $3, $4) RETURNING id",
        playbook_id,
        principal_id,
        alpha.company_id,
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Journey B forks A AT stage 2 → inherit stage 1, redo stage 2 onward.
    let journey_b: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.journeys \
             (playbook_id, principal_id, name, owner_company_id, guide_id, \
              predecessor_journey_id, fork_stage_id) \
         VALUES ($1, $2, 'fork B', $3, $4, $5, $6) RETURNING id",
        playbook_id,
        principal_id,
        alpha.company_id,
        guide_id,
        journey_a,
        stage2_ps_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let db = dapple_data::DbPool::from_pool(pool.clone());

    // Plain journey: BOTH stages are pending auto-dispatch.
    let pending_a = dapple_data::query::dispatches::select_pending_autodispatch_steps_for_journey(
        &db, journey_a,
    )
    .await
    .expect("pending for A");
    assert_eq!(
        pending_a.len(),
        2,
        "non-forked journey must auto-dispatch both compute stages; got {:?}",
        pending_a
            .iter()
            .map(|p| p.playbook_stage_id)
            .collect::<Vec<_>>()
    );

    // Fork at stage 2: ONLY the fork stage is pending; stage 1 is inherited.
    let pending_b = dapple_data::query::dispatches::select_pending_autodispatch_steps_for_journey(
        &db, journey_b,
    )
    .await
    .expect("pending for B");
    assert_eq!(
        pending_b.len(),
        1,
        "fork@stage-2 must NOT re-dispatch the inherited pre-fork stage 1; got {:?}",
        pending_b
            .iter()
            .map(|p| p.playbook_stage_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        pending_b[0].playbook_stage_id, stage2_ps_id,
        "the one pending step must be the fork stage (ord 2)",
    );
}

/// Regression (permanent dead-end): a PICKER-FEED generate step — one whose
/// computed output is some brief input's `candidate_source` — must be
/// auto-dispatched by the runtime like any other pure-compute step.
///
/// These used to be excluded from `select_pending_autodispatch_steps_for_journey`
/// on the rationale that the consumer dispatches them itself on pane open. That
/// made the runtime depend on every consumer advancing the journey, and a consumer
/// that can't stalls it with no runtime-side recovery: dapple-ui's only advancement
/// path was a poll loop bounded by a 20s deadline, so a slower step (a 238s EDGAR
/// pull) returned before advancing and nothing created the successor until the
/// operator happened to re-enter the submit path (live: 20 minutes, long after they
/// gave up). It also contradicted the published external contract, which tells
/// callers that a `producer: "computed"` slot is the server's to produce and they
/// should merely wait.
///
/// Builds one stage with two steps: a generate exposing a computed output that IS
/// a pick input's candidate_source, and the pick step exposing that operator input
/// (plus a computed output of its own, so the assertion pins `operator_steps` as
/// the thing excluding it rather than a missing computed output). Asserts exactly
/// the generate comes back.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn picker_feed_generate_is_pending_autodispatch(pool: PgPool) {
    let alpha = setup_customer(&pool, "feedcorp", "feedcorp_prod").await;
    let playbook_id = seed_external_playbook(&pool, &alpha).await;
    let guide_id = seed_external_guide(&pool, &alpha, "feed-guide").await;
    let principal_id = service_principal_id(&pool, &alpha).await;

    let stage_id = seed_external_stage(&pool, &alpha, "feed-stage").await;
    let playbook_stage_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.playbook_stages (playbook_id, stage_id, ord) VALUES ($1, $2, 1) RETURNING id",
        playbook_id,
        stage_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Two steps in authored order: generate (ord 1), then pick (ord 2). Each gets
    // a task, an exposed computed brief-output, and a stage_step_overlay exposing
    // that slot. Returns (stage_step_id, computed_slot_id) per step.
    let mut seeded: Vec<(Uuid, Uuid)> = Vec::new();
    for (ord, tag) in [(1i32, "gen"), (2i32, "pick")] {
        let step_id = seed_external_step(&pool, &alpha, &format!("feed-step-{tag}")).await;
        let task_id = seed_external_task(&pool, &alpha, &format!("feed-task-{tag}")).await;
        let stage_step_id: Uuid = sqlx::query_scalar!(
            "INSERT INTO rev.stage_steps (stage_id, step_id, ord) VALUES ($1, $2, $3) RETURNING id",
            stage_id,
            step_id,
            ord,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query!(
            "INSERT INTO rev.step_tasks (step_id, task_id, ord) VALUES ($1, $2, 1)",
            step_id,
            task_id,
        )
        .execute(&pool)
        .await
        .unwrap();

        let slot_id = seed_external_value_slot(&pool, &alpha, &format!("feed-out-{tag}")).await;
        let slot_path_id: Uuid = sqlx::query_scalar!(
            "SELECT vo.path_id FROM rev.value_slots vs \
             JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id WHERE vs.id = $1",
            slot_id,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query!(
            "INSERT INTO rev.stage_step_overlays (stage_step_id, value_slot_path_id, visibility) \
             VALUES ($1, $2, 'expose')",
            stage_step_id,
            slot_path_id,
        )
        .execute(&pool)
        .await
        .unwrap();

        let (_bpath, bvo) = seed_path_and_versioned(&pool, &alpha, "brief").await;
        let brief_id: Uuid = sqlx::query_scalar!(
            "INSERT INTO rev.briefs (versioned_object_id, title, description) \
             VALUES ($1, $2, $3) RETURNING id",
            bvo,
            format!("feed brief {tag}"),
            format!("feed brief {tag}"),
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query!(
            "INSERT INTO rev.brief_outputs (brief_id, value_slot_id, unit_key, source_kind_id, source_path) \
             VALUES ($1, $2, $3, (SELECT id FROM rev.brief_output_source_kinds WHERE key = 'computed'), '.')",
            brief_id, slot_id, format!("fu-{tag}"),
        ).execute(&pool).await.unwrap();

        seeded.push((stage_step_id, slot_id));
    }
    let (generate_ss_id, generate_slot_id) = seeded[0];
    let (pick_ss_id, _) = seeded[1];

    // The pick step's operator input: a `select` whose candidate_source is the
    // GENERATE step's computed slot. This is what used to make the generate a
    // "candidate feed" and get it excluded, and what keeps the pick step in
    // `operator_steps`.
    let pick_input_slot = seed_external_value_slot(&pool, &alpha, "feed-pick").await;
    let pick_input_path_id: Uuid = sqlx::query_scalar!(
        "SELECT vo.path_id FROM rev.value_slots vs \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id WHERE vs.id = $1",
        pick_input_slot,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.stage_step_overlays (stage_step_id, value_slot_path_id, visibility) \
         VALUES ($1, $2, 'expose')",
        pick_ss_id,
        pick_input_path_id,
    )
    .execute(&pool)
    .await
    .unwrap();

    let (_ipath, ivo) = seed_path_and_versioned(&pool, &alpha, "brief").await;
    let input_brief_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.briefs (versioned_object_id, title, description) \
         VALUES ($1, 'feed pick brief', 'feed pick brief') RETURNING id",
        ivo,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let brief_unit_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.brief_units (brief_id, unit_key) VALUES ($1, 'feed-pick-unit') RETURNING id",
        input_brief_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_unit_input (brief_unit_id, control_type_id, label_text, control_spec) \
         VALUES ($1, (SELECT id FROM rev.brief_input_control_types WHERE key = 'select'), 'Pick one', $2)",
        brief_unit_id,
        serde_json::json!({
            "value_slot_id": pick_input_slot,
            "candidate_source_value_slot_id": generate_slot_id,
        }),
    )
    .execute(&pool)
    .await
    .unwrap();

    let journey_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.journeys (playbook_id, principal_id, name, owner_company_id, guide_id) \
         VALUES ($1, $2, 'feed journey', $3, $4) RETURNING id",
        playbook_id,
        principal_id,
        alpha.company_id,
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let db = dapple_data::DbPool::from_pool(pool.clone());
    let pending = dapple_data::query::dispatches::select_pending_autodispatch_steps_for_journey(
        &db, journey_id,
    )
    .await
    .expect("pending autodispatch steps");

    let pending_ss: Vec<Uuid> = pending.iter().map(|p| p.stage_step_id).collect();
    assert!(
        pending_ss.contains(&generate_ss_id),
        "the picker-feed generate MUST be auto-dispatched by the runtime — a consumer \
         following the documented `producer: \"computed\"` contract only waits, so \
         excluding it dead-ends the journey. got {pending_ss:?}",
    );
    assert!(
        !pending_ss.contains(&pick_ss_id),
        "the pick step exposes an operator input and must stay excluded via \
         operator_steps — auto-dispatching it would pre-empt the operator. got {pending_ss:?}",
    );
    assert_eq!(
        pending_ss.len(),
        1,
        "exactly one step (the generate) should be pending; got {pending_ss:?}",
    );
    assert_eq!(
        pending[0].playbook_stage_id, playbook_stage_id,
        "the pending step must be attributed to the seeded playbook stage",
    );
}

/// A journey with ONE pure-compute step (computed brief-output exposed, no
/// operator input) and nothing else. Returns the ids the retry tests below drive:
/// `(journey_id, playbook_stage_id, stage_step_id, task_id, principal_id)`.
async fn seed_single_pure_compute_step(
    pool: &PgPool,
    customer: &TestCustomer,
    tag: &str,
) -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
    let playbook_id = seed_external_playbook(pool, customer).await;
    let guide_id = seed_external_guide(pool, customer, &format!("{tag}-guide")).await;
    let principal_id = service_principal_id(pool, customer).await;

    let stage_id = seed_external_stage(pool, customer, &format!("{tag}-stage")).await;
    let playbook_stage_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.playbook_stages (playbook_id, stage_id, ord) VALUES ($1, $2, 1) RETURNING id",
        playbook_id,
        stage_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let step_id = seed_external_step(pool, customer, &format!("{tag}-step")).await;
    let task_id = seed_external_task(pool, customer, &format!("{tag}-task")).await;
    let stage_step_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.stage_steps (stage_id, step_id, ord) VALUES ($1, $2, 1) RETURNING id",
        stage_id,
        step_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.step_tasks (step_id, task_id, ord) VALUES ($1, $2, 1)",
        step_id,
        task_id,
    )
    .execute(pool)
    .await
    .unwrap();

    // The computed output this step produces, exposed at the step level.
    let slot_id = seed_external_value_slot(pool, customer, &format!("{tag}-out")).await;
    let slot_path_id: Uuid = sqlx::query_scalar!(
        "SELECT vo.path_id FROM rev.value_slots vs \
         JOIN rev.versioned_objects vo ON vo.id = vs.versioned_object_id WHERE vs.id = $1",
        slot_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.stage_step_overlays (stage_step_id, value_slot_path_id, visibility) \
         VALUES ($1, $2, 'expose')",
        stage_step_id,
        slot_path_id,
    )
    .execute(pool)
    .await
    .unwrap();

    let (_bpath, bvo) = seed_path_and_versioned(pool, customer, "brief").await;
    let brief_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.briefs (versioned_object_id, title, description) \
         VALUES ($1, $2, $3) RETURNING id",
        bvo,
        format!("{tag} brief"),
        format!("{tag} brief"),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_outputs (brief_id, value_slot_id, unit_key, source_kind_id, source_path) \
         VALUES ($1, $2, $3, (SELECT id FROM rev.brief_output_source_kinds WHERE key = 'computed'), '.')",
        brief_id,
        slot_id,
        format!("{tag}-unit"),
    )
    .execute(pool)
    .await
    .unwrap();

    let journey_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.journeys (playbook_id, principal_id, name, owner_company_id, guide_id) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        playbook_id,
        principal_id,
        format!("{tag} journey"),
        customer.company_id,
        guide_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    (
        journey_id,
        playbook_stage_id,
        stage_step_id,
        task_id,
        principal_id,
    )
}

/// Insert a dispatch covering `stage_step_id` and return its `dispatch_step_id`.
async fn seed_dispatch_for_step(
    pool: &PgPool,
    customer: &TestCustomer,
    journey_id: Uuid,
    principal_id: Uuid,
    playbook_stage_id: Uuid,
    stage_step_id: Uuid,
) -> Uuid {
    let dispatch_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.dispatches (journey_id, principal_id, intent, owner_company_id) \
         VALUES ($1, $2, $3::JSONB, $4) RETURNING id",
        journey_id,
        principal_id,
        serde_json::json!({"source": "auto_dispatch"}),
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let dispatch_stage_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.dispatch_stages (dispatch_id, playbook_stage_id, params) \
         VALUES ($1, $2, $3::jsonb) RETURNING id",
        dispatch_id,
        playbook_stage_id,
        serde_json::json!({}),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.dispatch_steps (dispatch_stage_id, stage_step_id, params) \
         VALUES ($1, $2, $3::jsonb) RETURNING id",
        dispatch_stage_id,
        stage_step_id,
        serde_json::json!({}),
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Attach an attempt to `dispatch_step_id` and give it one event of `phase`.
async fn seed_attempt_in_phase(
    pool: &PgPool,
    dispatch_step_id: Uuid,
    task_id: Uuid,
    phase: &str,
) -> Uuid {
    let attempt_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.dispatch_task_attempts \
         (dispatch_step_id, task_id, context_data, context_metadata) \
         VALUES ($1, $2, $3::jsonb, $4::jsonb) RETURNING id",
        dispatch_step_id,
        task_id,
        serde_json::json!({}),
        serde_json::json!({}),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    append_attempt_event(pool, attempt_id, phase).await;
    attempt_id
}

/// Append an event, making `phase` the attempt's latest (current) phase.
async fn append_attempt_event(pool: &PgPool, attempt_id: Uuid, phase: &str) {
    append_attempt_event_with_metadata(pool, attempt_id, phase, serde_json::json!({})).await;
}

/// Append an event carrying explicit `context_metadata` — e.g. the
/// `error_class` key the dispatch runner stamps on nu-classified failures.
async fn append_attempt_event_with_metadata(
    pool: &PgPool,
    attempt_id: Uuid,
    phase: &str,
    metadata: serde_json::Value,
) {
    sqlx::query!(
        "INSERT INTO rev.dispatch_task_attempt_events \
         (attempt_id, phase_code, context_data, context_metadata) \
         VALUES ($1, $2, $3::jsonb, $4::jsonb)",
        attempt_id,
        phase,
        serde_json::json!({}),
        metadata,
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn pending_step_ids(pool: &PgPool, journey_id: Uuid) -> Vec<Uuid> {
    let db = dapple_data::DbPool::from_pool(pool.clone());
    dapple_data::query::dispatches::select_pending_autodispatch_steps_for_journey(&db, journey_id)
        .await
        .expect("pending autodispatch steps")
        .iter()
        .map(|p| p.stage_step_id)
        .collect()
}

/// Regression (permanent dead-end): a pure-compute step whose dispatches have ALL
/// ended `failed` must become eligible for a fresh auto-dispatch, bounded by a cap.
///
/// `dispatched_steps` used to be phase-blind ("has any dispatch at all"), which made
/// a restart-stranded step unrecoverable. The startup orphan sweep marks a
/// process-killed attempt `failed` precisely so it can be re-dispatched, but a
/// pure-compute step has no operator to do that (nothing to submit, and the
/// single-owner rule forbids consumers from dispatching it) and the other recovery
/// sweep skips any dispatch that already has an attempt. Seen live: a flow OOMKill
/// mid-generate left a run-the-10k journey permanently stalled.
///
/// Corrupt-then-verify against the DB at every step — never a response flag.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn failed_pure_compute_step_becomes_pending_again(pool: PgPool) {
    let alpha = setup_customer(&pool, "retrycorp", "retrycorp_prod").await;
    let (journey_id, ps_id, ss_id, task_id, principal_id) =
        seed_single_pure_compute_step(&pool, &alpha, "retry").await;

    assert!(
        pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "a pure-compute step with no dispatch at all must be pending",
    );

    // 1. A dispatch with NO attempts yet is about to run — it must still block, or
    //    the runtime would race a consumer-created dispatch (the double-owner
    //    condition that once made the candidate feed flap).
    let step_1 =
        seed_dispatch_for_step(&pool, &alpha, journey_id, principal_id, ps_id, ss_id).await;
    assert!(
        !pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "a dispatch with no attempts yet is live and MUST block a second one",
    );

    // 2. A non-terminal attempt is mid-flight — still blocking.
    let attempt_1 = seed_attempt_in_phase(&pool, step_1, task_id, "running").await;
    assert!(
        !pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "a `running` attempt is live and MUST block a second dispatch",
    );

    // 3. THE FIX: the orphan sweep terminates it `failed` -> eligible again.
    append_attempt_event(&pool, attempt_1, "failed").await;
    assert!(
        pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "once every dispatch ended `failed` the step MUST be re-dispatchable — \
         otherwise a restart-stranded pure-compute step dead-ends its journey forever",
    );

    // 4. The cap bounds a deterministically-failing step. Dispatches 2 and 3 also
    //    fail; at 3 the step stops coming back.
    for _ in 0..2 {
        let s = seed_dispatch_for_step(&pool, &alpha, journey_id, principal_id, ps_id, ss_id).await;
        seed_attempt_in_phase(&pool, s, task_id, "failed").await;
    }
    assert!(
        !pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "the retry cap (3 dispatches) MUST stop an endlessly-failing step from \
         re-dispatching forever",
    );
}

/// A failure the engine classified `deterministic` (a nushell script/shape error)
/// blocks auto-redispatch immediately — re-running the same script version fails
/// identically, and the 3x tight retry loop of exactly that shape re-seeded the
/// full journey context each time and OOMed the flow pod (2026-08-02). A failure
/// classified `transient` (conduit/network) keeps the full retry budget, as does
/// a class-less failure (legacy events, the orphan sweep — covered by
/// `failed_pure_compute_step_becomes_pending_again` above, whose metadata is `{}`).
///
/// Corrupt-then-verify against the DB at every step — never a response flag.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn deterministic_failure_stops_autodispatch_after_one_attempt(pool: PgPool) {
    let alpha = setup_customer(&pool, "detcorp", "detcorp_prod").await;
    let (journey_id, ps_id, ss_id, task_id, principal_id) =
        seed_single_pure_compute_step(&pool, &alpha, "det").await;

    // 1. A TRANSIENT-classed failure stays retryable: the step comes back.
    let step_1 =
        seed_dispatch_for_step(&pool, &alpha, journey_id, principal_id, ps_id, ss_id).await;
    let attempt_1 = seed_attempt_in_phase(&pool, step_1, task_id, "running").await;
    append_attempt_event_with_metadata(
        &pool,
        attempt_1,
        "failed",
        serde_json::json!({ "signals": {}, "error_class": "transient" }),
    )
    .await;
    assert!(
        pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "an explicitly transient failure MUST keep the step re-dispatchable",
    );

    // 2. A DETERMINISTIC-classed failure blocks — well below the 3-dispatch cap
    //    (this is dispatch 2 of 3), so the block is the CLASS, not the cap.
    let step_2 =
        seed_dispatch_for_step(&pool, &alpha, journey_id, principal_id, ps_id, ss_id).await;
    let attempt_2 = seed_attempt_in_phase(&pool, step_2, task_id, "running").await;
    append_attempt_event_with_metadata(
        &pool,
        attempt_2,
        "failed",
        serde_json::json!({ "signals": {}, "error_class": "deterministic" }),
    )
    .await;
    assert!(
        !pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "a deterministic script failure MUST block auto-redispatch immediately: \
         retrying an unchanged script burns a full context seed to fail identically",
    );
}

/// A step vetoed `unsafe` is poisoned and must NEVER be re-dispatched, however
/// many times the journey is polled — the phase model treats `unsafe` as terminal
/// and non-retryable, unlike `failed`.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
async fn unsafe_pure_compute_step_is_never_repending(pool: PgPool) {
    let alpha = setup_customer(&pool, "vetocorp", "vetocorp_prod").await;
    let (journey_id, ps_id, ss_id, task_id, principal_id) =
        seed_single_pure_compute_step(&pool, &alpha, "veto").await;

    let step_1 =
        seed_dispatch_for_step(&pool, &alpha, journey_id, principal_id, ps_id, ss_id).await;
    seed_attempt_in_phase(&pool, step_1, task_id, "unsafe").await;

    assert!(
        !pending_step_ids(&pool, journey_id).await.contains(&ss_id),
        "an `unsafe` veto is poisoned and MUST NOT be re-dispatched",
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_dispatch_task_attempts_malformed_uuid_400(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let resp = http
        .get(format!(
            "{}/api/dispatches/not-a-uuid/task_attempts",
            base_url
        ))
        .bearer_auth(&jwt)
        .send()
        .await
        .expect("malformed-uuid GET");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// =============================================================================
// New endpoints — GET /api/journeys/{id}
// =============================================================================
//
// Plan: §6.11–§6.13

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_journey_happy_path(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let playbook = seed_external_playbook(&pool, &alpha).await;
    let journey_id = seed_external_journey(&pool, &alpha, playbook).await;

    let signed = sign_test_token(&alpha, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &alpha.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange: {}", body);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}", journey_id),
        &jwt,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "body: {}", body);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["obj"]["id"], serde_json::json!(journey_id));
    assert_eq!(body["obj"]["playbook_id"], serde_json::json!(playbook));
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_journey_foreign_namespace_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_playbook = seed_external_playbook(&pool, &alpha).await;
    let alpha_journey = seed_external_journey(&pool, &alpha, alpha_playbook).await;

    let (_beta, beta_jwt, _beta_pb) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}", alpha_journey),
        &beta_jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("not found in caller's namespace"),
        "body: {}",
        body
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_journey_nonexistent_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}", Uuid::new_v4()),
        &jwt,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

// =============================================================================
// New endpoints — GET /api/journeys (list)
// =============================================================================
//
// Plan: §6.14–§6.16

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn list_journeys_empty(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();
    let (_c, jwt, _pb) =
        onboard_with_playbook(&pool, &http, &base_url, "testcorp", "testcorp_prod").await;

    let (status, body) = get_authed(&http, &base_url, "/api/journeys", &jwt).await;
    assert_eq!(status, StatusCode::OK, "body: {}", body);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["results"], 0);
    assert_eq!(body["objs"].as_array().unwrap().len(), 0);
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn list_journeys_non_empty(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let pb1 = seed_external_playbook(&pool, &alpha).await;
    let pb2 = seed_external_playbook(&pool, &alpha).await;
    let pb3 = seed_external_playbook(&pool, &alpha).await;
    let j1 = seed_external_journey(&pool, &alpha, pb1).await;
    let j2 = seed_external_journey(&pool, &alpha, pb2).await;
    let j3 = seed_external_journey(&pool, &alpha, pb3).await;

    let signed = sign_test_token(&alpha, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &alpha.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange: {}", body);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(&http, &base_url, "/api/journeys", &jwt).await;
    assert_eq!(status, StatusCode::OK, "body: {}", body);
    assert_eq!(body["results"], 3);
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(objs.len(), 3);

    // Every returned id must match one of the seeds; order is `id DESC` so
    // the newest UUIDv7 (j3) comes first.
    let seen: Vec<Uuid> = objs
        .iter()
        .map(|o| serde_json::from_value(o["id"].clone()).unwrap())
        .collect();
    assert!(seen.contains(&j1));
    assert!(seen.contains(&j2));
    assert!(seen.contains(&j3));
    // Verify DESC order: j3 (newest UUIDv7) is first.
    assert_eq!(
        seen[0], j3,
        "expected newest journey first; got: {:?}",
        seen
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn list_journeys_cross_namespace_isolated(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_pb1 = seed_external_playbook(&pool, &alpha).await;
    let alpha_pb2 = seed_external_playbook(&pool, &alpha).await;
    let alpha_j1 = seed_external_journey(&pool, &alpha, alpha_pb1).await;
    let alpha_j2 = seed_external_journey(&pool, &alpha, alpha_pb2).await;

    let beta = setup_customer(&pool, "beta", "beta_prod").await;
    let beta_pb = seed_external_playbook(&pool, &beta).await;
    let beta_j = seed_external_journey(&pool, &beta, beta_pb).await;

    let alpha_signed = sign_test_token(&alpha, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &alpha_signed, &alpha.service_id).await;
    let alpha_jwt = body["jwt"].as_str().unwrap().to_string();

    let beta_signed = sign_test_token(&beta, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &beta_signed, &beta.service_id).await;
    let beta_jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(&http, &base_url, "/api/journeys", &beta_jwt).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["results"], 1);
    let beta_seen: Vec<Uuid> = body["objs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| serde_json::from_value(o["id"].clone()).unwrap())
        .collect();
    assert_eq!(beta_seen, vec![beta_j]);

    let (status, body) = get_authed(&http, &base_url, "/api/journeys", &alpha_jwt).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["results"], 2);
    let alpha_seen: Vec<Uuid> = body["objs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| serde_json::from_value(o["id"].clone()).unwrap())
        .collect();
    assert!(alpha_seen.contains(&alpha_j1));
    assert!(alpha_seen.contains(&alpha_j2));
    assert!(!alpha_seen.contains(&beta_j));
}

// =============================================================================
// SDK round-trip — ExternalDappleClient.journeys() and dispatches()
// =============================================================================
//
// Plan: §6.7–§6.10, §6.17

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_create_journey(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let playbook = seed_external_playbook(&pool, &alpha).await;
    let guide_id = guide_for_playbook(&pool, playbook).await;

    let signing_key_bytes = alpha.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &alpha.service_id,
        &signing_key_bytes,
    );
    let created = sdk
        .journeys()
        .create(vec![dapple_sdk::CreateJourneySchema {
            name: "sdk journey".to_string(),
            playbook_id: playbook,
            brief_pack_ids: vec![],
            inference_profile_ids: vec![],
            predecessor_journey_id: None,
            fork_stage_id: None,
            guide_id,
        }])
        .await
        .expect("SDK create journey");

    assert_eq!(created.len(), 1);
    let journey_id = created[0].id;

    // Verify DB row carries the right namespace.
    let owner_company_id: Uuid = sqlx::query_scalar!(
        "SELECT owner_company_id FROM rev.journeys WHERE id = $1",
        journey_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner_company_id, alpha.company_id);
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_full_dispatch_round_trip(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // Use the internal ingest test-helper to materialize stub-flow, then
    // donate it to the customer's namespace — same pattern as
    // happy_path_full_dispatch_execution.
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;
    let internal_state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });
    let fixture_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let ingest =
        dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &fixture_dir)
            .await
            .expect("ingest stub-flow");
    let playbook_id = ingest
        .derived_objects
        .playbook_ids
        .first()
        .copied()
        .expect("stub-flow should produce a derived playbook");
    relocate_dapple_paths_to(&pool, customer.company_id).await;
    let guide_id = guide_for_playbook(&pool, playbook_id).await;

    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    // 1. SDK create journey.
    let journeys = sdk
        .journeys()
        .create(vec![dapple_sdk::CreateJourneySchema {
            name: "sdk-roundtrip".to_string(),
            playbook_id,
            brief_pack_ids: vec![],
            inference_profile_ids: vec![],
            predecessor_journey_id: None,
            fork_stage_id: None,
            guide_id,
        }])
        .await
        .expect("SDK create journey");
    assert_eq!(journeys.len(), 1);
    let journey_id = journeys[0].id;

    // 2. SDK create dispatch.
    let (stage_step_id, step_task_id) = resolve_playbook_step_task(&pool, playbook_id).await;
    let dispatches = sdk
        .dispatches()
        .create(vec![dapple_sdk::CreateDispatchSchema {
            journey_id,
            intent: serde_json::json!({}),
            steps: vec![dapple_sdk::CreateDispatchStepSchema {
                stage_step_id,
                params: serde_json::json!({}),
                tasks: vec![dapple_sdk::CreateDispatchTaskSchema { step_task_id }],
            }],
        }])
        .await
        .expect("SDK create dispatch");
    assert_eq!(dispatches.len(), 1);
    let dispatch_id = dispatches[0].id;

    // 3. SDK get dispatch.
    let dispatch = sdk
        .dispatches()
        .get(dispatch_id)
        .await
        .expect("SDK get dispatch");
    assert_eq!(dispatch.id, dispatch_id);

    // Quiet unused-import warning if reqwest isn't otherwise touched.
    let _ = &http;
}

/// Seed a value_slot in `customer`'s namespace. Sufficient for the
/// dispatch-intent-bindings insert path which JOINs through
/// `value_slots → versioned_objects → object_paths → value_slot_path_details`.
async fn seed_external_value_slot(pool: &PgPool, customer: &TestCustomer, slot_name: &str) -> Uuid {
    let entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'value_slot'")
            .fetch_one(pool)
            .await
            .unwrap();
    let path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        path_id,
        format!(
            "{:020x}{:020x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let value_kind_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.value_kinds WHERE key = 'text'")
            .fetch_one(pool)
            .await
            .unwrap();
    // value_slot_path_details UNIQUE(name) is namespace-scoped per migration
    // 41; the suffix is no longer strictly required but kept for legacy test
    // robustness across re-runs.
    let unique_name = format!("{}-{}", slot_name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.value_slot_path_details (path_id, name, value_kind_id, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        unique_name,
        value_kind_id,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    let value_slot_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.value_slots (versioned_object_id, title, value_kind_id) \
         VALUES ($1, $2, (SELECT id FROM rev.value_kinds WHERE key = 'text')) RETURNING id",
        vo_id,
        slot_name,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    value_slot_id
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_create_bindings(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    // Seed dispatch + a real value_slot. The bindings insert JOINs through
    // value_slots → path → path_details, so a bare UUID returns zero rows.
    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;
    let playbook = seed_external_playbook(&pool, &customer).await;
    let journey = seed_external_journey(&pool, &customer, playbook).await;
    let dispatch = seed_external_dispatch(&pool, &customer, journey).await;
    let value_slot = seed_external_value_slot(&pool, &customer, "operator_input_slot").await;

    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    let bindings = sdk
        .dispatch_intent_bindings()
        .create(vec![dapple_sdk::CreateDispatchIntentBindingSchema {
            dispatch_id: dispatch,
            value_slot_id: value_slot,
            text_items: vec![dapple_sdk::CreateBindingTextItemSchema {
                value: "operator-input".to_string(),
            }],
            int_items: vec![],
            number_items: vec![],
            bool_items: vec![],
            json_items: vec![],
            asset_items: vec![],
            value_meta: vec![],
        }])
        .await
        .expect("SDK create binding");
    assert_eq!(bindings.len(), 1);

    // Verify the binding was actually persisted.
    let row_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.dispatch_intent_bindings
           WHERE dispatch_id = $1 AND value_slot_id = $2"#,
        dispatch,
        value_slot,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row_count, 1);
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_cross_namespace_blocked(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_pb = seed_external_playbook(&pool, &alpha).await;
    let alpha_j = seed_external_journey(&pool, &alpha, alpha_pb).await;
    let alpha_d = seed_external_dispatch(&pool, &alpha, alpha_j).await;

    let beta = setup_customer(&pool, "beta", "beta_prod").await;
    let beta_signing_bytes = beta.signing_key.to_bytes();
    let beta_sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &beta.service_id,
        &beta_signing_bytes,
    );

    let err = beta_sdk
        .dispatches()
        .task_attempts(alpha_d)
        .await
        .expect_err("beta must not see alpha's task_attempts");
    match err {
        dapple_sdk::SdkError::Api { status, message } => {
            assert_eq!(
                status, 404,
                "expected 404, got status={} msg={}",
                status, message
            );
            assert!(
                message.contains("not found in caller's namespace")
                    || message.contains("RESOURCE_NOT_FOUND"),
                "unexpected message: {}",
                message
            );
        }
        other => panic!("expected SdkError::Api, got {:?}", other),
    }
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_get_and_list_journeys(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let customer = setup_customer(&pool, "testcorp", "testcorp_prod").await;
    let playbook = seed_external_playbook(&pool, &customer).await;
    let guide_id = guide_for_playbook(&pool, playbook).await;

    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    let created = sdk
        .journeys()
        .create(vec![
            dapple_sdk::CreateJourneySchema {
                name: "j1".to_string(),
                playbook_id: playbook,
                brief_pack_ids: vec![],
                inference_profile_ids: vec![],
                predecessor_journey_id: None,
                fork_stage_id: None,
                guide_id,
            },
            dapple_sdk::CreateJourneySchema {
                name: "j2".to_string(),
                playbook_id: playbook,
                brief_pack_ids: vec![],
                inference_profile_ids: vec![],
                predecessor_journey_id: None,
                fork_stage_id: None,
                guide_id,
            },
            dapple_sdk::CreateJourneySchema {
                name: "j3".to_string(),
                playbook_id: playbook,
                brief_pack_ids: vec![],
                inference_profile_ids: vec![],
                predecessor_journey_id: None,
                fork_stage_id: None,
                guide_id,
            },
        ])
        .await
        .expect("SDK batch create");
    assert_eq!(created.len(), 3);

    let fetched = sdk
        .journeys()
        .get(created[0].id)
        .await
        .expect("SDK get journey");
    assert_eq!(fetched.id, created[0].id);

    let listed = sdk.journeys().list(None).await.expect("SDK list journeys");
    assert_eq!(listed.len(), 3);
    let seen_ids: std::collections::HashSet<Uuid> = listed.iter().map(|j| j.id).collect();
    for c in &created {
        assert!(seen_ids.contains(&c.id), "missing created id {}", c.id);
    }
}

// =============================================================================
// Shared seed helpers for namespaced GET-by-id / list endpoints.
// =============================================================================
//
// Every entity below follows the same shape: object_path (with
// namespace_company_id + scope='external') + versioned_object + path_details
// (where required) + the principal row. Each helper takes the customer to
// stamp the namespace.

fn fake_content_hash() -> String {
    format!(
        "{:020x}{:020x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    )
}

async fn seed_path_and_versioned(
    pool: &PgPool,
    customer: &TestCustomer,
    entity_key: &str,
) -> (Uuid, Uuid) {
    let entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = $1", entity_key,)
            .fetch_one(pool)
            .await
            .unwrap();

    let path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        path_id,
        fake_content_hash(),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    (path_id, vo_id)
}

async fn seed_external_stage(pool: &PgPool, customer: &TestCustomer, name: &str) -> Uuid {
    let (path_id, vo_id) = seed_path_and_versioned(pool, customer, "stage").await;
    // UNIQUE(name)/UNIQUE(slug) on path-detail tables became namespace-scoped
    // in migration 41 — sibling namespaces no longer collide. The UUID
    // suffix is retained only to keep multiple seeds within the same test
    // namespace collision-free.
    let unique = format!("{}-{}", name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.stage_path_details (path_id, name, slug, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        unique,
        unique,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.stages (versioned_object_id, description) \
         VALUES ($1, $2) RETURNING id",
        vo_id,
        format!("seed stage {}", name),
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_external_step(pool: &PgPool, customer: &TestCustomer, name: &str) -> Uuid {
    let (path_id, vo_id) = seed_path_and_versioned(pool, customer, "step").await;
    let unique = format!("{}-{}", name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.step_path_details (path_id, name, slug, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        unique,
        unique,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.steps (versioned_object_id, description) \
         VALUES ($1, $2) RETURNING id",
        vo_id,
        format!("seed step {}", name),
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_external_task(pool: &PgPool, customer: &TestCustomer, name: &str) -> Uuid {
    let (path_id, vo_id) = seed_path_and_versioned(pool, customer, "task").await;
    let unique = format!("{}-{}", name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.task_path_details (path_id, name, slug, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        unique,
        unique,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.tasks (versioned_object_id, description, function, function_input) \
         VALUES ($1, $2, $3, $4::JSON) RETURNING id",
        vo_id,
        format!("seed task {}", name),
        "noop",
        serde_json::json!({}),
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_external_category(pool: &PgPool, customer: &TestCustomer, name: &str) -> Uuid {
    let (path_id, vo_id) = seed_path_and_versioned(pool, customer, "category").await;
    let unique = format!("{}-{}", name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.category_path_details (path_id, name, slug, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        unique,
        unique,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.categories (versioned_object_id, description) \
         VALUES ($1, $2) RETURNING id",
        vo_id,
        format!("seed category {}", name),
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_external_prompt(pool: &PgPool, customer: &TestCustomer, name: &str) -> Uuid {
    let (path_id, vo_id) = seed_path_and_versioned(pool, customer, "prompt").await;
    let unique = format!("{}-{}", name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.prompt_path_details (path_id, name, namespace_company_id) \
         VALUES ($1, $2, $3)",
        path_id,
        unique,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.prompts (versioned_object_id, description) \
         VALUES ($1, $2) RETURNING id",
        vo_id,
        format!("seed prompt {}", name),
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_external_fluent_message(pool: &PgPool, customer: &TestCustomer, name: &str) -> Uuid {
    let (path_id, vo_id) = seed_path_and_versioned(pool, customer, "fluent_message").await;
    let unique = format!("{}-{}", name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.fluent_message_path_details (path_id, name, description, namespace_company_id) \
         VALUES ($1, $2, $3, $4)",
        path_id,
        unique,
        Some(format!("seed fluent message {}", name)),
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.fluent_messages (versioned_object_id) VALUES ($1) RETURNING id",
        vo_id,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_external_directive(pool: &PgPool, customer: &TestCustomer, name: &str) -> Uuid {
    let (path_id, vo_id) = seed_path_and_versioned(pool, customer, "directive").await;
    let unique = format!("{}-{}", name, Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO rev.directive_path_details (path_id, name, namespace_company_id) \
         VALUES ($1, $2, $3)",
        path_id,
        unique,
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar!(
        "INSERT INTO rev.directives (versioned_object_id, logic, description) \
         VALUES ($1, $2::JSONB, $3) RETURNING id",
        vo_id,
        serde_json::json!({"if": [true, "proceed", "skip"]}),
        format!("seed directive {}", name),
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Seed a playbook with one nested stage → step → task chain, all in the
/// caller's namespace. Returns the playbook id; the nested objects can be
/// inspected via the regular GET endpoints if needed.
///
/// After migration 20 the playbook hierarchy collapsed: `playbook_steps`
/// and `playbook_tasks` were dropped, and the playbook SELECT joins through
/// `playbook_stages → stages → stage_steps → steps → step_tasks → tasks`.
async fn seed_nested_playbook(pool: &PgPool, customer: &TestCustomer, slug: &str) -> Uuid {
    let playbook_id = seed_external_playbook(pool, customer).await;
    let stage_id = seed_external_stage(pool, customer, &format!("{slug}-stage")).await;
    let step_id = seed_external_step(pool, customer, &format!("{slug}-step")).await;
    let task_id = seed_external_task(pool, customer, &format!("{slug}-task")).await;

    sqlx::query!(
        "INSERT INTO rev.playbook_stages (playbook_id, stage_id, ord) VALUES ($1, $2, 1)",
        playbook_id,
        stage_id,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO rev.stage_steps (stage_id, step_id, ord) VALUES ($1, $2, 1)",
        stage_id,
        step_id,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO rev.step_tasks (step_id, task_id, ord) VALUES ($1, $2, 1)",
        step_id,
        task_id,
    )
    .execute(pool)
    .await
    .unwrap();

    playbook_id
}

/// Spawn the external server and exchange a JWT for `customer` in one step.
async fn jwt_for(
    pool: &PgPool,
    http: &reqwest::Client,
    base_url: &str,
    customer: &TestCustomer,
) -> String {
    let _ = pool; // future-proofing: keep the pool in the signature even though
    let signed = sign_test_token(customer, 60, 0);
    let (status, body) = exchange_token(http, base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK, "exchange failed: {}", body);
    body["jwt"].as_str().expect("jwt in response").to_string()
}

// =============================================================================
// Playbook: full nested seed, happy GET-by-id + list, foreign 404, isolation
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_playbook_full_nested(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let playbook_id = seed_nested_playbook(&pool, &alpha, "p1").await;
    let jwt = jwt_for(&pool, &http, &base_url, &alpha).await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/playbooks/{}", playbook_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {}", body);
    let obj = &body["obj"];
    assert_eq!(
        obj["id"],
        serde_json::Value::String(playbook_id.to_string())
    );
    let stages = obj["stages"].as_array().expect("stages array");
    assert_eq!(stages.len(), 1, "should have one stage; got: {}", obj);
    let steps = stages[0]["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 1, "should have one step; got: {}", obj);
    let tasks = steps[0]["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks.len(), 1, "should have one task; got: {}", obj);
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn list_playbooks_happy(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let pb_a = seed_external_playbook(&pool, &alpha).await;
    let pb_b = seed_external_playbook(&pool, &alpha).await;
    let jwt = jwt_for(&pool, &http, &base_url, &alpha).await;

    let (status, body) = get_authed(&http, &base_url, "/api/playbooks", &jwt).await;
    assert_eq!(status, StatusCode::OK);
    let objs = body["objs"].as_array().unwrap();
    assert_eq!(objs.len(), 2, "should see two playbooks; got: {}", body);
    let ids: std::collections::HashSet<String> = objs
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&pb_a.to_string()));
    assert!(ids.contains(&pb_b.to_string()));
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn get_playbook_foreign_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_pb = seed_external_playbook(&pool, &alpha).await;

    let beta = setup_customer(&pool, "beta", "beta_prod").await;
    let beta_jwt = jwt_for(&pool, &http, &base_url, &beta).await;

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/playbooks/{}", alpha_pb),
        &beta_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn list_playbooks_cross_namespace_isolation(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;
    let alpha_pb = seed_external_playbook(&pool, &alpha).await;
    let beta_pb = seed_external_playbook(&pool, &beta).await;

    let alpha_jwt = jwt_for(&pool, &http, &base_url, &alpha).await;
    let beta_jwt = jwt_for(&pool, &http, &base_url, &beta).await;

    let (_, body) = get_authed(&http, &base_url, "/api/playbooks", &alpha_jwt).await;
    let alpha_objs = body["objs"].as_array().unwrap();
    assert_eq!(alpha_objs.len(), 1);
    assert_eq!(
        alpha_objs[0]["id"],
        serde_json::Value::String(alpha_pb.to_string())
    );

    let (_, body) = get_authed(&http, &base_url, "/api/playbooks", &beta_jwt).await;
    let beta_objs = body["objs"].as_array().unwrap();
    assert_eq!(beta_objs.len(), 1);
    assert_eq!(
        beta_objs[0]["id"],
        serde_json::Value::String(beta_pb.to_string())
    );
}

// =============================================================================
// SDK round-trip — ExternalDappleClient.value_slots()
// =============================================================================
//
// Two tests: roundtrip the new `get(id)` + `list_by_ids(ids)` methods on
// `ExternalValueSlotsClient`. The bare `list()` variant is covered by
// the `namespaced_entity_tests!(value_slots_endpoint, ...)` macro
// invocation below (smoke / cross-namespace isolation / foreign-404),
// so we don't re-test it here.

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_value_slots_get(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let slot_id = seed_external_value_slot(&pool, &alpha, "operator_input_slot").await;

    let alpha_sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &alpha.service_id,
        &alpha.signing_key.to_bytes(),
    );

    // Own-namespace fetch succeeds and returns the slot we seeded.
    let slot = alpha_sdk
        .value_slots()
        .get(slot_id)
        .await
        .expect("SDK get own-namespace slot");
    assert_eq!(slot.id, slot_id);

    // Foreign-namespace fetch returns 404 (RESOURCE_NOT_FOUND, no
    // existence leak — handler gates via `value_slot_in_namespace`).
    let beta = setup_customer(&pool, "beta", "beta_prod").await;
    let beta_sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &beta.service_id,
        &beta.signing_key.to_bytes(),
    );
    let err = beta_sdk
        .value_slots()
        .get(slot_id)
        .await
        .expect_err("foreign-namespace fetch must 404");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("404") || msg.contains("RESOURCE_NOT_FOUND"),
        "expected 404 / RESOURCE_NOT_FOUND, got: {msg}"
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_value_slots_list_by_ids_drops_foreign(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let alpha_slot_a = seed_external_value_slot(&pool, &alpha, "alpha_slot_a").await;
    let alpha_slot_b = seed_external_value_slot(&pool, &alpha, "alpha_slot_b").await;

    let beta = setup_customer(&pool, "beta", "beta_prod").await;
    let beta_slot = seed_external_value_slot(&pool, &beta, "beta_slot").await;

    let alpha_sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &alpha.service_id,
        &alpha.signing_key.to_bytes(),
    );

    // Caller asks for a mix of own-namespace + foreign-namespace ids.
    // Foreign rows are silently dropped (no error, just absent from the
    // result set), so a third party guessing UUIDs from another namespace
    // gets no existence signal back. Critical security-adjacent behaviour.
    let slots = alpha_sdk
        .value_slots()
        .list_by_ids(vec![alpha_slot_a, beta_slot, alpha_slot_b])
        .await
        .expect("SDK list_by_ids");

    let returned: std::collections::HashSet<Uuid> = slots.iter().map(|s| s.id).collect();
    assert!(returned.contains(&alpha_slot_a), "alpha_slot_a missing");
    assert!(returned.contains(&alpha_slot_b), "alpha_slot_b missing");
    assert!(
        !returned.contains(&beta_slot),
        "beta_slot must be silently dropped, got: {returned:?}"
    );
    assert_eq!(returned.len(), 2, "expected exactly 2 own-namespace slots");
}

// =============================================================================
// Helper macro: smoke + isolation + foreign-404 for each entity.
//
// Each entity has identical test shape; the macro keeps the test file
// readable and inserts the entity-specific path/seed function.
// =============================================================================
macro_rules! namespaced_entity_tests {
    (
        $mod_name:ident,
        endpoint = $endpoint:expr,
        seed_fn = $seed_fn:ident,
    ) => {
        mod $mod_name {
            use super::*;

            #[sqlx::test(migrations = "./../dapple-data/migrations")]
            #[serial_test::serial(external_signing_key)]
            async fn get_by_id_happy(pool: PgPool) {
                unsafe {
                    std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
                }
                let (base_url, _server) = spawn_external_server(pool.clone()).await;
                let http = reqwest::Client::new();

                let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
                let id = $seed_fn(&pool, &alpha, "smoke").await;
                let jwt = jwt_for(&pool, &http, &base_url, &alpha).await;

                let (status, body) =
                    get_authed(&http, &base_url, &format!("{}/{}", $endpoint, id), &jwt).await;
                assert_eq!(status, StatusCode::OK, "body: {}", body);
                assert_eq!(
                    body["obj"]["id"],
                    serde_json::Value::String(id.to_string()),
                    "body: {}",
                    body
                );
            }

            #[sqlx::test(migrations = "./../dapple-data/migrations")]
            #[serial_test::serial(external_signing_key)]
            async fn list_happy(pool: PgPool) {
                unsafe {
                    std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
                }
                let (base_url, _server) = spawn_external_server(pool.clone()).await;
                let http = reqwest::Client::new();

                let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
                let id_a = $seed_fn(&pool, &alpha, "alpha-1").await;
                let id_b = $seed_fn(&pool, &alpha, "alpha-2").await;
                let jwt = jwt_for(&pool, &http, &base_url, &alpha).await;

                let (status, body) = get_authed(&http, &base_url, $endpoint, &jwt).await;
                assert_eq!(status, StatusCode::OK, "body: {}", body);
                let objs = body["objs"].as_array().unwrap();
                assert_eq!(objs.len(), 2, "body: {}", body);
                let ids: std::collections::HashSet<String> = objs
                    .iter()
                    .map(|o| o["id"].as_str().unwrap().to_string())
                    .collect();
                assert!(ids.contains(&id_a.to_string()));
                assert!(ids.contains(&id_b.to_string()));
            }

            #[sqlx::test(migrations = "./../dapple-data/migrations")]
            #[serial_test::serial(external_signing_key)]
            async fn get_by_id_foreign_404(pool: PgPool) {
                unsafe {
                    std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
                }
                let (base_url, _server) = spawn_external_server(pool.clone()).await;
                let http = reqwest::Client::new();

                let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
                let alpha_id = $seed_fn(&pool, &alpha, "alpha-only").await;

                let beta = setup_customer(&pool, "beta", "beta_prod").await;
                let beta_jwt = jwt_for(&pool, &http, &base_url, &beta).await;

                let (status, body) = get_authed(
                    &http,
                    &base_url,
                    &format!("{}/{}", $endpoint, alpha_id),
                    &beta_jwt,
                )
                .await;
                assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
                assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
            }

            #[sqlx::test(migrations = "./../dapple-data/migrations")]
            #[serial_test::serial(external_signing_key)]
            async fn list_cross_namespace_isolation(pool: PgPool) {
                unsafe {
                    std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
                }
                let (base_url, _server) = spawn_external_server(pool.clone()).await;
                let http = reqwest::Client::new();

                let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
                let beta = setup_customer(&pool, "beta", "beta_prod").await;
                let alpha_id = $seed_fn(&pool, &alpha, "alpha-only").await;
                let beta_id = $seed_fn(&pool, &beta, "beta-only").await;

                let alpha_jwt = jwt_for(&pool, &http, &base_url, &alpha).await;
                let beta_jwt = jwt_for(&pool, &http, &base_url, &beta).await;

                let (_, body) = get_authed(&http, &base_url, $endpoint, &alpha_jwt).await;
                let objs = body["objs"].as_array().unwrap();
                assert_eq!(objs.len(), 1, "alpha sees only its own; body: {}", body);
                assert_eq!(
                    objs[0]["id"],
                    serde_json::Value::String(alpha_id.to_string())
                );

                let (_, body) = get_authed(&http, &base_url, $endpoint, &beta_jwt).await;
                let objs = body["objs"].as_array().unwrap();
                assert_eq!(objs.len(), 1, "beta sees only its own; body: {}", body);
                assert_eq!(
                    objs[0]["id"],
                    serde_json::Value::String(beta_id.to_string())
                );
            }
        }
    };
}

namespaced_entity_tests!(
    stages_endpoint,
    endpoint = "/api/stages",
    seed_fn = seed_external_stage,
);
namespaced_entity_tests!(
    steps_endpoint,
    endpoint = "/api/steps",
    seed_fn = seed_external_step,
);
namespaced_entity_tests!(
    tasks_endpoint,
    endpoint = "/api/tasks",
    seed_fn = seed_external_task,
);
namespaced_entity_tests!(
    categories_endpoint,
    endpoint = "/api/categories",
    seed_fn = seed_external_category,
);
namespaced_entity_tests!(
    prompts_endpoint,
    endpoint = "/api/prompts",
    seed_fn = seed_external_prompt,
);
namespaced_entity_tests!(
    directives_endpoint,
    endpoint = "/api/directives",
    seed_fn = seed_external_directive,
);
namespaced_entity_tests!(
    value_slots_endpoint,
    endpoint = "/api/value-slots",
    seed_fn = seed_external_value_slot,
);
namespaced_entity_tests!(
    fluent_messages_endpoint,
    endpoint = "/api/fluent-messages",
    seed_fn = seed_external_fluent_message,
);

// =============================================================================
// `source_path` + external PATCH ingest + upsert helper
// =============================================================================
//
// These scenarios cover the post-pagination cleanup pass:
//
// * Source-of-truth for `source_path` is `guide.toml`. The parser reads
//   it; `plan_guide()` propagates it; the materializers stamp it on
//   `rev.guides.source_path`. The stub-flow fixture carries
//   `source_path = "stub-flow"` so this is exercised on every external
//   ingest going forward.
// * `PATCH /api/guides/{path_id}/ingest` exposes the internal
//   refresh-or-version-bump flow to external callers, namespace-gated so
//   foreign or unknown path_ids return 404 with no existence leak. New
//   derived object_paths land in the customer's namespace via the per-tx
//   GUC override threaded through both the version-create and refresh
//   branches.
// * The `ExternalGuidesClient::upsert_from_dir` SDK helper looks up the
//   caller's guides by slug (derived from `guide.toml`'s title) and
//   either POSTs a new guide or PATCHes the matching one. The slug
//   derivation mirrors `rev.slugify`.

/// Copy a guide directory into a TempDir so a test can mutate it without
/// dirtying the on-disk fixture. Used by the PATCH/upsert tests that need
/// to change tarball contents between calls.
fn clone_guide_fixture(src: &std::path::Path) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let dest = tmp.path().join(src.file_name().unwrap());
    copy_dir_recursive(src, &dest).expect("copy fixture");
    tmp
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Helper: POST a guide via real HTTP, returning the parsed JSON body.
async fn post_guide_ingest(
    http: &reqwest::Client,
    base_url: &str,
    jwt: &str,
    guide_dir: &std::path::Path,
) -> Value {
    let (status, body) = post_guide_ingest_raw(http, base_url, jwt, guide_dir).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "POST ingest must 201; body={}",
        body
    );
    body
}

/// POST a guide via real HTTP without asserting status — for tests that
/// want to probe failure modes (UNIQUE collisions, namespace gates, etc.).
async fn post_guide_ingest_raw(
    http: &reqwest::Client,
    base_url: &str,
    jwt: &str,
    guide_dir: &std::path::Path,
) -> (StatusCode, Value) {
    let tar_bytes = build_tarball(guide_dir);
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tar_bytes)
            .file_name("guide.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let resp = http
        .post(format!("{}/api/guides/ingest", base_url))
        .bearer_auth(jwt)
        .multipart(form)
        .send()
        .await
        .expect("ingest POST");
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!(null));
    (status, body)
}

/// ASCII slugify a title into a hyphenated lowercase slug (e.g.
/// "Stub Flow A" -> "stub-flow-a"). Non-alphanumeric runs collapse to a single
/// hyphen; leading/trailing hyphens are trimmed.
fn slugify_title(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = true; // suppress a leading dash
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// Clone the stub-flow fixture into a TempDir, rewrite `guide.toml`'s `title`,
/// AND give the clone a per-title-unique guide slug — both the `source_path`
/// field and the on-disk directory name become `slugify_title(new_title)`.
///
/// Used by the "two guides with colliding derived-object names" tests: the
/// parser uses the guide title as the playbook name, so a distinct title gives
/// each guide a distinct playbook name while keeping the inner
/// stage/step/task/etc. directive names identical — reproducing the
/// cross-guide-within-namespace name-collision shape. The unique guide slug is
/// mandatory under Migration 73 (guide slug = basename(source_path)), where two
/// clones both keeping `source_path = "stub-flow"` collide on
/// `guide_path_details_slug_namespace_key`. Renaming the directory to the same
/// slug keeps the dir=slug convention clients rely on for POST-vs-PATCH lookup.
///
/// Returns `(TempDir, guide_dir)` — hold the TempDir for the test's lifetime and
/// pass `guide_dir` (already the slugged directory) to the ingest helpers.
fn clone_stub_flow_with_title(new_title: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let fixture_src =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let tmp = clone_guide_fixture(&fixture_src);

    // clone_guide_fixture copies into `<tmp>/stub-flow`; rename to the slug so the
    // directory basename matches the source_path (dir=slug convention).
    let slug = slugify_title(new_title);
    let original_dir = tmp.path().join("stub-flow");
    let guide_dir = tmp.path().join(&slug);
    if guide_dir != original_dir {
        std::fs::rename(&original_dir, &guide_dir).expect("rename cloned guide dir to slug");
    }

    let guide_toml = guide_dir.join("guide.toml");
    let original = std::fs::read_to_string(&guide_toml).expect("read guide.toml");
    let rewritten = original
        .replace(
            "title = \"Stub Flow\"",
            &format!("title = \"{}\"", new_title),
        )
        .replace(
            "source_path = \"stub-flow\"",
            &format!("source_path = \"{}\"", slug),
        );
    assert_ne!(
        rewritten, original,
        "title/source_path rewrite must change something — stub-flow's fixture changed?"
    );
    std::fs::write(&guide_toml, rewritten).expect("write guide.toml");
    (tmp, guide_dir)
}

/// Failing-test for the derived-object collision class. Today (pre-fix):
/// guide A is POSTed first, creating `stage_path_details.name='stub-stage'`
/// and `task_path_details.name='stub-task'`. Guide B has a distinct guide
/// title (so its playbook is `playbook_path_details.name='Stub Flow Twin'`)
/// but the same inner directive names, so its materializer hits A's
/// path_details rows by global slug lookup. Whether B succeeds (and how)
/// proves whether the materializers handle the "another guide already owns
/// these named derived objects" case correctly.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_two_guides_with_shared_stage_and_task_names(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let (_guide_a, guide_a_dir) = clone_stub_flow_with_title("Stub Flow A");
    let (_guide_b, guide_b_dir) = clone_stub_flow_with_title("Stub Flow B");

    // A goes first — should be a straightforward POST.
    let (status_a, body_a) = post_guide_ingest_raw(&http, &base_url, &jwt, &guide_a_dir).await;
    assert_eq!(
        status_a,
        StatusCode::CREATED,
        "guide A POST must succeed; body={}",
        body_a
    );

    // B has SAME stage/step/task names but different guide title (so a
    // distinct playbook name). The customer should be able to POST it.
    let (status_b, body_b) = post_guide_ingest_raw(&http, &base_url, &jwt, &guide_b_dir).await;
    assert_eq!(
        status_b,
        StatusCode::CREATED,
        "guide B POST must succeed (sibling guide reusing same stage/task names within same namespace); body={}",
        body_b
    );

    // Both guides should exist in the namespace.
    let guide_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.guides g
           JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
           JOIN rev.object_paths op ON op.id = vo.path_id
           WHERE op.namespace_company_id = $1"#,
        customer.company_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        guide_count, 2,
        "both guides should land in the customer's namespace"
    );
}

/// Reproduces the acme-shape failure verbatim: a guide whose title
/// carries non-ASCII characters (Portuguese accents in the acme case)
/// is POSTed, then re-ingested via the upsert helper. The second sync
/// SHOULD detect the existing guide and route through PATCH; the playbook
/// materializer SHOULD recognize the existing playbook by slug and PATCH it
/// (creating a new version) rather than try to INSERT a duplicate.
///
/// **Today (pre-fix) this fails** with:
///   `duplicate key value violates unique constraint "playbook_path_details_name_key"`
/// because `dapple_data::slugify` is Unicode-aware (`is_alphanumeric()`)
/// while SQL `rev.slugify` is ASCII-only (`[a-z0-9\s_-]`). For "Como Lida
/// com Alergênios" the Rust slug is "como-lida-com-alergênios" but the
/// stored SQL slug is "como-lida-com-alergnios". The materializer's
/// slug-based lookup misses the existing row, falls through to INSERT, and
/// hits the global UNIQUE on `playbook_path_details.name` (the raw, un-
/// slugified title which IS the same both times).
///
/// All 67 acme-side blocked guides matched this shape — Portuguese
/// titles, scripts/*.nu present (so a playbook is derived from the
/// stage/step structure).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_repatch_guide_with_non_ascii_title(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    // Mirror the acme shape: Portuguese title with `ê`. The body is
    // stub-flow's stage/step/task structure — that derives a playbook
    // named after the guide title (which is what hits the bug).
    let (_tmp, guide_dir) = clone_stub_flow_with_title("Como Lida com Alergênios");

    // 1. First sync: should POST cleanly.
    let first = sdk
        .guides()
        .upsert_from_dir(&guide_dir, false)
        .await
        .expect("first upsert (POST) must succeed");
    assert!(
        matches!(
            first,
            dapple_sdk::resource::external_guides::UpsertOutcome::Created(_)
        ),
        "first upsert must be Created"
    );

    // 2. Mutate body so the content hash changes → PATCH-new-version branch.
    let intro_path = guide_dir.join("introduction.md");
    let mut content = std::fs::read_to_string(&intro_path).expect("read intro");
    content.push_str("\n\n<!-- re-sync mutation -->\n");
    std::fs::write(&intro_path, content).expect("write intro");

    // 3. Second sync: lookup finds the existing guide by slug → upsert
    //    routes through PATCH → cascade should PATCH the existing playbook,
    //    NOT try to INSERT a fresh playbook_path_details row that collides
    //    on the name UNIQUE.
    let second = sdk
        .guides()
        .upsert_from_dir(&guide_dir, false)
        .await
        .expect("second upsert (PATCH) must succeed — proves the slugify/lookup fix");
    assert!(
        matches!(
            second,
            dapple_sdk::resource::external_guides::UpsertOutcome::Updated(_)
        ),
        "second upsert must be Updated"
    );
}

/// Same shape on the internal counterpart — proves the fix lands for the
/// dapple namespace too, not just external customers. Internal POSTs don't
/// flow through the GUC override (dapple is the default), but the materializer
/// lookups are still global so the within-namespace collision shape applies.
///
/// Implemented as a synthetic two-step using `dapple_prompt_api::test_utils::ingest_guide_directory`.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn internal_two_guides_with_shared_stage_and_task_names(pool: PgPool) {
    let internal_state = std::sync::Arc::new(dapple_prompt_api::AppState {
        db: std::sync::Arc::new(DbPool::from_pool(pool.clone())),
        conduit_url: None,
        flow_url: None,
    });

    let (_guide_a, guide_a_dir) = clone_stub_flow_with_title("Internal Stub Flow A");
    let (_guide_b, guide_b_dir) = clone_stub_flow_with_title("Internal Stub Flow B");

    dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &guide_a_dir)
        .await
        .expect("guide A internal ingest");
    dapple_prompt_api::test_utils::ingest_guide_directory(&internal_state, &guide_b_dir)
        .await
        .expect("guide B internal ingest (sibling reusing stage/task names) must not fail");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_ingest_populates_source_path_from_guide_toml(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let fixture_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let ingest_body = post_guide_ingest(&http, &base_url, &jwt, &fixture_dir).await;
    let guide_id: Uuid =
        serde_json::from_value(ingest_body["guide"]["id"].clone()).expect("guide id");

    let row = sqlx::query!("SELECT source_path FROM rev.guides WHERE id = $1", guide_id,)
        .fetch_one(&pool)
        .await
        .expect("source_path lookup");

    assert_eq!(
        row.source_path.as_deref(),
        Some("stub-flow"),
        "stub-flow's guide.toml declares source_path = \"stub-flow\" — ingest must populate the column",
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_patch_creates_new_version_in_namespace(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 1. POST a baseline guide.
    let fixture_src =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let tmp = clone_guide_fixture(&fixture_src);
    let guide_dir = tmp.path().join("stub-flow");

    let ingest_body = post_guide_ingest(&http, &base_url, &jwt, &guide_dir).await;
    let path_id = guide_path_id_from_ingest(&pool, &ingest_body).await;

    // 2. Mutate the fixture so the content hash differs.
    let intro_path = guide_dir.join("introduction.md");
    let mut content = std::fs::read_to_string(&intro_path).expect("read intro");
    content.push_str("\n\n<!-- patch-test mutation -->\n");
    std::fs::write(&intro_path, content).expect("write intro");

    // 3. PATCH via the SDK.
    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );
    let response = sdk
        .guides()
        .ingest_version_from_dir(path_id, &guide_dir, false)
        .await
        .expect("SDK ingest_version");
    assert_eq!(
        response.extra["outcome"], "new_version",
        "content hash changed; expect new_version branch",
    );

    // 4. Confirm a new versioned_object row exists under the same path_id.
    let count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!" FROM rev.versioned_objects WHERE path_id = $1"#,
        path_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        count >= 2,
        "expected at least two versioned_object rows under path_id after PATCH, got {}",
        count,
    );

    // 5. Confirm no path created during the PATCH leaked into dapple's
    //    namespace — mirrors the safety-check in
    //    external_ingest_lands_in_customer_namespace.
    let dapple_leak: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.object_paths op
           WHERE op.namespace_company_id = $1::UUID
             AND op.created_at >= (
                 SELECT MIN(op2.created_at) FROM rev.object_paths op2
                 JOIN rev.versioned_objects vo ON vo.path_id = op2.id
                 WHERE vo.path_id = $2
             )"#,
        dapple_data::DAPPLE_COMPANY_UUID,
        path_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dapple_leak, 0,
        "PATCH ingest must not leak any new object_paths into dapple's namespace",
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_patch_foreign_path_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // Alpha onboards and ingests; beta then tries to PATCH alpha's path_id.
    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;

    let alpha_signed = sign_test_token(&alpha, 60, 0);
    let (_, abody) = exchange_token(&http, &base_url, &alpha_signed, &alpha.service_id).await;
    let alpha_jwt = abody["jwt"].as_str().unwrap().to_string();

    let fixture_src =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let ingest_body = post_guide_ingest(&http, &base_url, &alpha_jwt, &fixture_src).await;
    let guide_id: Uuid =
        serde_json::from_value(ingest_body["guide"]["id"].clone()).expect("guide id");
    let alpha_path_id: Uuid = sqlx::query_scalar!(
        "SELECT vo.path_id FROM rev.guides g \
         JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id \
         WHERE g.id = $1",
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Beta crafts a PATCH against alpha's path_id via the SDK.
    let beta_signing_key_bytes = beta.signing_key.to_bytes();
    let beta_sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &beta.service_id,
        &beta_signing_key_bytes,
    );
    let result = beta_sdk
        .guides()
        .ingest_version_from_dir(alpha_path_id, &fixture_src, false)
        .await;
    let err = result.expect_err("foreign PATCH must fail");
    let status = match err {
        dapple_sdk::SdkError::Api { status, .. } => status,
        other => panic!("expected Api error, got {:?}", other),
    };
    assert_eq!(
        status, 404,
        "cross-namespace PATCH must return 404 (RESOURCE_NOT_FOUND) — no existence leak",
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_patch_refresh_branch_stays_in_namespace(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    // POST a baseline guide via the SDK (same tarball-creation routine as
    // the PATCH below — different routines produce different tarball entry
    // paths, which the guide-directory hash digests, which would defeat
    // the content-hash match this test relies on).
    let fixture_src =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let post_response = sdk
        .guides()
        .ingest_from_dir(&fixture_src)
        .await
        .expect("SDK ingest_from_dir baseline");
    let guide_id: Uuid =
        serde_json::from_value(post_response.extra["guide"]["id"].clone()).expect("guide id");
    let path_id: Uuid = sqlx::query_scalar!(
        "SELECT vo.path_id FROM rev.guides g \
         JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id \
         WHERE g.id = $1",
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Re-PATCH with the SAME content — content hash matches → refresh path.
    let response = sdk
        .guides()
        .ingest_version_from_dir(path_id, &fixture_src, false)
        .await
        .expect("SDK ingest_version (refresh)");
    assert_eq!(
        response.extra["outcome"], "refreshed",
        "same content hash should hit the refresh branch",
    );

    // Confirm no derived path created during the refresh leaked into
    // dapple's namespace. This is the load-bearing assertion for the GUC
    // fix in refresh_existing_guide.
    let dapple_leak: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.object_paths op
           WHERE op.namespace_company_id = $1::UUID
             AND op.created_at >= (
                 SELECT MIN(op2.created_at) FROM rev.object_paths op2
                 JOIN rev.versioned_objects vo ON vo.path_id = op2.id
                 WHERE vo.path_id = $2
             )"#,
        dapple_data::DAPPLE_COMPANY_UUID,
        path_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dapple_leak, 0,
        "refresh branch must not leak any new object_paths into dapple's namespace",
    );
}

/// `?backfill=true` from the SDK must reach the server's `ForceRepair` path
/// and do real repair work — not merely return 200. Full "Testing Repair
/// Operations" contract: corrupt DB state, then prove ONLY the backfill path
/// restores it.
///
/// Under Model B (repair-in-place) a backfill does NOT mint new versions, so
/// the old "count task versions" proxy no longer works (an unchanged-content
/// backfill is a no-op, same as a normal refresh). Instead we drop the
/// stub-flow playbook's (stub-stage → stub-step) stage_step and contrast:
///
///   1. POST baseline                      → stage_step present
///   2. corrupt: DELETE the stage_step     → absent
///   3. PATCH backfill=false (same bytes)  → "refreshed", still ABSENT
///      (Normal refresh reuses the playbook and does not repair the drop)
///   4. PATCH backfill=true  (same bytes)  → "refreshed", PRESENT again
///      (ForceRepair re-links the stage_step against the SAME version)
///
/// If the SDK failed to thread `backfill` into the PATCH query string, step 4
/// would behave like step 3 and the link would stay dropped — so this test
/// fails closed on a dropped flag, and also proves no version growth.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_backfill_force_repairs_derived_task(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    // Count derived-task versioned_objects in the customer's namespace.
    // (`task_path_details.path_id` is the object_path; one row per task
    // version once joined through versioned_objects.)
    async fn task_version_count(pool: &PgPool, company_id: Uuid) -> i64 {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*)::BIGINT AS "count!"
               FROM rev.versioned_objects vo
               JOIN rev.object_paths op ON op.id = vo.path_id
               JOIN rev.task_path_details tpd ON tpd.path_id = op.id
               WHERE op.namespace_company_id = $1"#,
            company_id,
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    // 1. POST a baseline guide via the SDK. Use the same tarball routine for
    //    POST and PATCH so the guide-directory content hash matches across
    //    calls (different routines produce different tarball entry paths,
    //    which the directory hash digests).
    let fixture_src =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let post_response = sdk
        .guides()
        .ingest_from_dir(&fixture_src)
        .await
        .expect("SDK ingest_from_dir baseline");
    let guide_id: Uuid =
        serde_json::from_value(post_response.extra["guide"]["id"].clone()).expect("guide id");
    let path_id: Uuid = sqlx::query_scalar!(
        "SELECT vo.path_id FROM rev.guides g \
         JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id \
         WHERE g.id = $1",
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        task_version_count(&pool, customer.company_id).await,
        1,
        "baseline POST should materialize exactly one task version",
    );

    // Model B: a backfill repairs IN PLACE (no new version), so an unchanged-
    // content backfill no longer differs from a normal refresh by minting a
    // version. To prove the SDK threads `backfill=true` AND that it reaches the
    // server's ForceRepair path, corrupt a structural link that ONLY ForceRepair
    // repairs: drop the (stub-stage → stub-step) stage_step. Normal refresh
    // leaves it dropped; ForceRepair re-links it against the SAME playbook
    // version (corrupt-then-verify, per "Testing Repair Operations").
    async fn stage_step_count(pool: &PgPool, company_id: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*)::BIGINT
               FROM rev.stage_steps ss
               JOIN rev.stages s ON s.id = ss.stage_id
               JOIN rev.versioned_objects vo ON vo.id = s.versioned_object_id
               JOIN rev.object_paths op ON op.id = vo.path_id
               JOIN rev.stage_path_details spd ON spd.path_id = op.id
               WHERE op.namespace_company_id = $1 AND spd.slug = 'stub-stage'"#,
        )
        .bind(company_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    assert_eq!(
        stage_step_count(&pool, customer.company_id).await,
        1,
        "baseline must carry the stub-stage → stub-step link",
    );

    // Corrupt: drop the stage_step.
    sqlx::query(
        r#"DELETE FROM rev.stage_steps ss
           USING rev.stages s, rev.versioned_objects vo, rev.object_paths op,
                 rev.stage_path_details spd
           WHERE ss.stage_id = s.id AND vo.id = s.versioned_object_id
             AND op.id = vo.path_id AND spd.path_id = op.id
             AND op.namespace_company_id = $1 AND spd.slug = 'stub-stage'"#,
    )
    .bind(customer.company_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        stage_step_count(&pool, customer.company_id).await,
        0,
        "corruption must drop the stage_step",
    );

    // 2. backfill=false (Normal) must NOT repair the structural drop.
    let normal = sdk
        .guides()
        .ingest_version_from_dir(path_id, &fixture_src, false)
        .await
        .expect("SDK ingest_version (normal refresh)");
    assert_eq!(normal.extra["outcome"], "refreshed");
    assert_eq!(
        stage_step_count(&pool, customer.company_id).await,
        0,
        "backfill=false (Normal) must leave the dropped stage_step unrepaired",
    );

    // 3. backfill=true (ForceRepair) re-links the stage_step IN PLACE.
    let repaired = sdk
        .guides()
        .ingest_version_from_dir(path_id, &fixture_src, true)
        .await
        .expect("SDK ingest_version (backfill)");
    assert_eq!(
        repaired.extra["outcome"], "refreshed",
        "guide content is unchanged, so the guide-level outcome stays refreshed",
    );
    assert_eq!(
        stage_step_count(&pool, customer.company_id).await,
        1,
        "backfill=true must re-link the dropped stage_step (Model B repair-in-place)",
    );
    assert_eq!(
        task_version_count(&pool, customer.company_id).await,
        1,
        "Model B backfill must NOT mint a new task version — repair in place",
    );

    // Safety: ForceRepair must not leak derived task paths into dapple's
    // namespace — same invariant the refresh-branch test guards.
    let dapple_task_leak: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.versioned_objects vo
           JOIN rev.object_paths op ON op.id = vo.path_id
           JOIN rev.task_path_details tpd ON tpd.path_id = op.id
           WHERE op.namespace_company_id = $1::UUID"#,
        dapple_data::DAPPLE_COMPANY_UUID,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dapple_task_leak, 0,
        "ForceRepair must not leak derived task paths into dapple's namespace",
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_upsert_from_dir_creates_when_missing(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    let fixture_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");

    let outcome = sdk
        .guides()
        .upsert_from_dir(&fixture_dir, false)
        .await
        .expect("upsert_from_dir on empty namespace");

    match outcome {
        dapple_sdk::resource::external_guides::UpsertOutcome::Created(_) => {}
        dapple_sdk::resource::external_guides::UpsertOutcome::Updated(_) => {
            panic!("fresh namespace should hit Created branch")
        }
    }

    let count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.guides g
           JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
           JOIN rev.object_paths op ON op.id = vo.path_id
           WHERE op.namespace_company_id = $1"#,
        customer.company_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "upsert should have created exactly one guide");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_upsert_from_dir_updates_when_present(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let fixture_src =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/guides/stub-flow");
    let tmp = clone_guide_fixture(&fixture_src);
    let guide_dir = tmp.path().join("stub-flow");

    // 1. POST a baseline guide via real HTTP so the slug lookup has
    //    something to match against on the second call.
    let _ = post_guide_ingest(&http, &base_url, &jwt, &guide_dir).await;

    let path_id_before: Uuid = sqlx::query_scalar!(
        "SELECT vo.path_id FROM rev.guides g \
         JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id \
         JOIN rev.object_paths op ON op.id = vo.path_id \
         WHERE op.namespace_company_id = $1",
        customer.company_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // 2. Mutate the cloned fixture so the upsert lands on the PATCH branch
    //    with a fresh content hash.
    let intro_path = guide_dir.join("introduction.md");
    let mut content = std::fs::read_to_string(&intro_path).expect("read intro");
    content.push_str("\n\n<!-- upsert update mutation -->\n");
    std::fs::write(&intro_path, content).expect("write intro");

    // 3. Run upsert.
    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );
    let outcome = sdk
        .guides()
        .upsert_from_dir(&guide_dir, false)
        .await
        .expect("upsert_from_dir on populated namespace");
    match outcome {
        dapple_sdk::resource::external_guides::UpsertOutcome::Updated(_) => {}
        dapple_sdk::resource::external_guides::UpsertOutcome::Created(_) => {
            panic!("populated namespace with matching slug should hit Updated branch")
        }
    }

    // Same path, new version.
    let version_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!" FROM rev.versioned_objects WHERE path_id = $1"#,
        path_id_before,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        version_count >= 2,
        "upsert update should have produced a second versioned_object under path {} (got {})",
        path_id_before,
        version_count,
    );
}

// =============================================================================
// Multi-tenant collision tests — migration 41 namespace-scoped UNIQUE
// =============================================================================
//
// Before migration 41 every `*_path_details.name`/`.slug` UNIQUE was global.
// Two customers shipping a guide whose authors picked the same playbook /
// task / stage / step name would 500 on the second POST with a
// `*_path_details_*_key` violation. These tests assert that scenario now
// works: each customer keeps their own derived-object paths in their own
// namespace, and PATCH-ing one customer's guide does NOT touch the other
// customer's same-named derived objects.

/// Two customers POST guides whose titles slug to the same playbook name.
/// Today (pre-migration 41 + namespace-aware materializer lookups) the
/// second POST 500s on `playbook_path_details.name_key`. After: each
/// customer holds their own playbook path under their own namespace.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_two_customers_same_playbook_name_isolated(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;

    let alpha_signed = sign_test_token(&alpha, 60, 0);
    let (_, abody) = exchange_token(&http, &base_url, &alpha_signed, &alpha.service_id).await;
    let alpha_jwt = abody["jwt"].as_str().unwrap().to_string();

    let beta_signed = sign_test_token(&beta, 60, 0);
    let (_, bbody) = exchange_token(&http, &base_url, &beta_signed, &beta.service_id).await;
    let beta_jwt = bbody["jwt"].as_str().unwrap().to_string();

    // Both guides have the SAME title — therefore the same playbook name
    // ("Shared Playbook Name" → playbook with that exact name from
    // `dapple-parser::transform::playbook::extract_playbooks` line 56).
    // Pre-fix, the global UNIQUE on `playbook_path_details.name` blocks
    // beta's POST.
    let title = "Shared Playbook Name";
    let (_alpha_tmp, alpha_dir) = clone_stub_flow_with_title(title);
    let (_beta_tmp, beta_dir) = clone_stub_flow_with_title(title);

    let (status_a, body_a) = post_guide_ingest_raw(&http, &base_url, &alpha_jwt, &alpha_dir).await;
    assert_eq!(
        status_a,
        StatusCode::CREATED,
        "alpha POST must succeed; body={}",
        body_a,
    );

    let (status_b, body_b) = post_guide_ingest_raw(&http, &base_url, &beta_jwt, &beta_dir).await;
    assert_eq!(
        status_b,
        StatusCode::CREATED,
        "beta POST with the same playbook name must succeed (namespace-scoped UNIQUE); body={}",
        body_b,
    );

    // Each customer should own a distinct playbook path in their own
    // namespace. The names match (the collision case), but the path_id
    // and namespace_company_id differ.
    let rows = sqlx::query!(
        "SELECT ppd.path_id, ppd.name, ppd.namespace_company_id \
         FROM rev.playbook_path_details ppd \
         WHERE ppd.name = $1 \
         ORDER BY ppd.namespace_company_id",
        title,
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        rows.len(),
        2,
        "exactly two playbook paths with this name should exist (one per namespace)",
    );
    assert_ne!(rows[0].path_id, rows[1].path_id, "path_ids must differ");
    let namespaces: std::collections::HashSet<_> =
        rows.iter().map(|r| r.namespace_company_id).collect();
    assert!(namespaces.contains(&alpha.company_id));
    assert!(namespaces.contains(&beta.company_id));
}

/// alpha POSTs a guide with playbook "Foo". beta POSTs a guide with
/// playbook "Foo" (now possible per the previous test). alpha PATCHes
/// their guide. beta's playbook MUST remain untouched — no new
/// versioned_object created under beta's playbook path.
///
/// This proves the materializer's namespace-filtered slug lookup is
/// doing its job: without it, alpha's PATCH would find beta's playbook
/// via the global slug lookup and PATCH that one instead.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_patch_does_not_cross_namespace(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;

    let alpha_signed = sign_test_token(&alpha, 60, 0);
    let (_, abody) = exchange_token(&http, &base_url, &alpha_signed, &alpha.service_id).await;
    let alpha_jwt = abody["jwt"].as_str().unwrap().to_string();

    let beta_signed = sign_test_token(&beta, 60, 0);
    let (_, bbody) = exchange_token(&http, &base_url, &beta_signed, &beta.service_id).await;
    let beta_jwt = bbody["jwt"].as_str().unwrap().to_string();

    let title = "Cross Tenant Test";
    let (_alpha_tmp, alpha_dir) = clone_stub_flow_with_title(title);
    let (_beta_tmp, beta_dir) = clone_stub_flow_with_title(title);

    let _ = post_guide_ingest(&http, &base_url, &alpha_jwt, &alpha_dir).await;
    let _ = post_guide_ingest(&http, &base_url, &beta_jwt, &beta_dir).await;

    // Snapshot beta's playbook versioned_object count before alpha touches anything.
    let beta_versions_before: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.versioned_objects vo
           JOIN rev.object_paths op ON op.id = vo.path_id
           JOIN rev.playbook_path_details ppd ON ppd.path_id = op.id
           WHERE ppd.namespace_company_id = $1"#,
        beta.company_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // alpha mutates and PATCHes via the SDK (namespace-stamped, will hit the
    // materializer's namespace-filtered lookup).
    let intro = alpha_dir.join("introduction.md");
    let mut content = std::fs::read_to_string(&intro).unwrap();
    content.push_str("\n\n<!-- alpha mutation -->\n");
    std::fs::write(&intro, content).unwrap();

    let alpha_signing_key_bytes = alpha.signing_key.to_bytes();
    let alpha_sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &alpha.service_id,
        &alpha_signing_key_bytes,
    );
    let _outcome = alpha_sdk
        .guides()
        .upsert_from_dir(&alpha_dir, false)
        .await
        .expect("alpha upsert must succeed");

    // beta's playbook must have the SAME number of versioned_objects after
    // alpha's PATCH — alpha did not (and structurally cannot) PATCH beta's
    // playbook.
    let beta_versions_after: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.versioned_objects vo
           JOIN rev.object_paths op ON op.id = vo.path_id
           JOIN rev.playbook_path_details ppd ON ppd.path_id = op.id
           WHERE ppd.namespace_company_id = $1"#,
        beta.company_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        beta_versions_after, beta_versions_before,
        "beta's playbook versions count must be unchanged after alpha's PATCH",
    );
}

// =============================================================================
// Unicode identifier round-trip
// =============================================================================
//
// Migration 42 made `rev.slugify()` Unicode-preserving and the in-tree
// `fluent-syntax-unicode` fork loosens the Fluent identifier grammar to accept
// Latin-script diacritics in message keys. The two tests below assert both
// layers ingest, slug-derive, and round-trip an authored guide whose
// identifiers carry ç / ã / á through the external surface, AND that namespace
// isolation still holds for Unicode-named content.

/// Assemble an in-memory tarball whose chapter titles, directive `name=`
/// attributes, and FTL message keys all carry Latin-script diacritics. Mirrors
/// the acme `esboçar-o-restaurante` failure mode that originally
/// surfaced this work.
fn unicode_guide_tarball_bytes(suffix: &str) -> Vec<u8> {
    fn append_file(
        archive: &mut tar::Builder<flate2::write::GzEncoder<&mut Vec<u8>>>,
        path: &str,
        content: &[u8],
    ) {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content).unwrap();
    }

    let mut tarball = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let guide_toml = format!(
        "title = \"Guía Esboçar {suffix}\"\n\
         description = \"Esboçar identificadores Unicode no pipeline\"\n\
         author = \"Test Runner\"\n\
         classification = \"test-{suffix}\"\n"
    );
    append_file(&mut archive, "guide/guide.toml", guide_toml.as_bytes());
    append_file(
        &mut archive,
        "guide/SUMMARY.md",
        b"# Summary\n\n- [Introducao](ch1.md)\n- [Estagios](ch2.md)\n",
    );
    append_file(
        &mut archive,
        "guide/CONDUITS.md",
        b"# Conduit Context\n\nTest guide frame.\n",
    );
    append_file(
        &mut archive,
        "guide/ch1.md",
        b"# Introducao\n\nConteudo introdutorio.\n",
    );
    // ASCII chapter H1s here because the chapter title slugs would clash if
    // acme's full Unicode chapter slugs aren't yet wired through the
    // SUMMARY anchor parser. The Unicode pressure is on directive names + FTL
    // identifiers, which is what the parser fork addresses. The `:::nushell`
    // blocks below carry Unicode `name=` attributes too (matched to each step's
    // `task:` ref) — that path lands in `rev.task_path_details.name` via the
    // shared `insert_object_paths` CTE, exercising the slug + UNIQUE constraint
    // through the same Unicode flow.
    let ch2 = b"# Estagios\n\n\
:::stage { name: \"esbo\xc3\xa7ar-est\xc3\xa1gio\" }\n\
\n\
:::step { name: \"padr\xc3\xa3o-passo\", task: \"padr\xc3\xa3o-task\" }\n\
:::\n\
\n\
:::step { name: \"o-tr\xc3\xa1fego-passo\", task: \"o-tr\xc3\xa1fego-task\" }\n\
:::\n\
\n\
:::\n\
\n\
:::nushell { name: \"padr\xc3\xa3o-task\", tags: [coder] }\n\
{ payload: { ok: true }, signals: { done: true } }\n\
:::\n\
\n\
:::nushell { name: \"o-tr\xc3\xa1fego-task\", tags: [coder] }\n\
{ payload: { ok: true }, signals: { done: true } }\n\
:::\n";
    append_file(&mut archive, "guide/ch2.md", ch2);

    let fluent = format!(
        "\
esboçar-estágio = Estágio Esboçar.
    .descrição = Descrição estendida do estágio em pt-PT.
padrão-passo = Passo padrão.
o-tráfego-passo = Passo sobre o tráfego.
as-horas-de-operação-intro = Texto introdutório com ç e ã.
unicode-msg-{suffix} = Mensagem única do teste lifecycle.
"
    );
    append_file(&mut archive, "guide/locales/en.ftl", fluent.as_bytes());

    archive.finish().unwrap();
    drop(archive);
    tarball
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_unicode_ingest_preserves_keys(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // 1. Onboard customer + JWT exchange.
    let customer = setup_customer(&pool, "unicorp", "unicorp_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 2. POST a Unicode-bearing guide.
    let suffix = uuid::Uuid::now_v7().to_string();
    let tar_bytes = unicode_guide_tarball_bytes(&suffix);
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tar_bytes)
            .file_name("unicode.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let resp = http
        .post(format!("{}/api/guides/ingest", base_url))
        .bearer_auth(&jwt)
        .multipart(form)
        .send()
        .await
        .expect("ingest");
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "Unicode ingest should return 201; body: {body_text}"
    );
    let ingest_body: Value = serde_json::from_str(&body_text).expect("ingest json");
    let guide_id: Uuid =
        serde_json::from_value(ingest_body["guide"]["id"].clone()).expect("guide id");
    let slug = ingest_body["guide"]["slug"].as_str().expect("slug present");
    assert!(
        slug.contains('ç'),
        "external ingest slug should preserve Unicode (got {slug:?})"
    );

    // 3. Guide path lands in the customer's namespace at scope='external'.
    let guide_path_row = sqlx::query!(
        r#"SELECT op.namespace_company_id, op.scope
           FROM rev.guides g
           JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
           JOIN rev.object_paths op ON op.id = vo.path_id
           WHERE g.id = $1"#,
        guide_id,
    )
    .fetch_one(&pool)
    .await
    .expect("guide path lookup");
    assert_eq!(
        guide_path_row.namespace_company_id, customer.company_id,
        "Unicode guide must live in the customer's namespace"
    );
    assert_eq!(guide_path_row.scope, "external");

    // 4. FTL identifiers landed in `fluent_message_path_details.name`
    //    byte-identical with their authored Unicode.
    let unicode_keys: Vec<String> = sqlx::query_scalar!(
        r#"SELECT fmpd.name
           FROM rev.fluent_message_path_details fmpd
           JOIN rev.object_paths op ON op.id = fmpd.path_id
           WHERE op.namespace_company_id = $1
             AND op.scope = 'external'
             AND fmpd.name IN (
                 'esboçar-estágio',
                 'padrão-passo',
                 'o-tráfego-passo',
                 'as-horas-de-operação-intro'
             )"#,
        customer.company_id,
    )
    .fetch_all(&pool)
    .await
    .expect("fluent keys lookup");
    let key_set: std::collections::HashSet<&str> =
        unicode_keys.iter().map(|s| s.as_str()).collect();
    for expected in [
        "esboçar-estágio",
        "padrão-passo",
        "o-tráfego-passo",
        "as-horas-de-operação-intro",
    ] {
        assert!(
            key_set.contains(expected),
            "expected Unicode FTL key {expected:?} to round-trip; got {:?}",
            key_set
        );
    }
}

// =============================================================================
// brief-form / bindings test helpers
// =============================================================================
//
// These seed a tiny journey wired up to a brief_pack containing one brief
// with three units (one input, one notice, one markdown) plus a select-input
// brief_unit with two choices. Direct INSERTs, not the ingest pipeline — the
// ingest pipeline has no asset-free brief_pack fixture in the repo and the
// goal here is to exercise the brief-form / bindings code paths, not the
// ingest pipeline.

/// Bundle of UUIDs produced by `seed_brief_form_fixture` so the test asserts
/// against known ids.
#[allow(dead_code)]
struct BriefFormFixture {
    journey_id: Uuid,
    dispatch_id: Uuid,
    brief_id: Uuid,
    text_input_unit_id: Uuid,
    text_input_slot_id: Uuid,
    select_input_unit_id: Uuid,
    select_input_slot_id: Uuid,
    notice_unit_id: Uuid,
    markdown_unit_id: Uuid,
}

/// Seed a brief_pack attached to a journey + dispatch in `customer`'s
/// namespace. The brief has:
///   - one text input bound to a fresh value_slot (`name_slot`)
///   - one select input bound to a fresh value_slot (`color_slot`), with two
///     choices ("red"/"Red", "blue"/"Blue")
///   - one notice unit
///   - one markdown unit
async fn seed_brief_form_fixture(pool: &PgPool, customer: &TestCustomer) -> BriefFormFixture {
    // playbook + journey + dispatch via existing helpers
    let playbook_id = seed_external_playbook(pool, customer).await;
    let journey_id = seed_external_journey(pool, customer, playbook_id).await;
    let dispatch_id = seed_external_dispatch(pool, customer, journey_id).await;

    // -- brief ----------------------------------------------------------------
    let brief_entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'brief'")
            .fetch_one(pool)
            .await
            .unwrap();
    let brief_path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        brief_entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let brief_vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        brief_path_id,
        fake_content_hash(),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_path_details (path_id, name, namespace_company_id) \
         VALUES ($1, $2, $3)",
        brief_path_id,
        format!("brief-{}", Uuid::now_v7()),
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    let brief_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.briefs (versioned_object_id, title, description) \
         VALUES ($1, $2, $3) RETURNING id",
        brief_vo_id,
        "Test Brief",
        Some("Brief used by brief-form tests"),
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // -- two value_slots (one text, one for select) ---------------------------
    let text_input_slot_id = seed_external_value_slot(pool, customer, "name_slot").await;
    let select_input_slot_id = seed_external_value_slot(pool, customer, "color_slot").await;

    // -- brief_units (text input, select input, notice, markdown) ------------
    let text_control_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.brief_input_control_types WHERE key = 'text'")
            .fetch_one(pool)
            .await
            .unwrap();
    let select_control_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.brief_input_control_types WHERE key = 'select'")
            .fetch_one(pool)
            .await
            .unwrap();

    // Text input unit
    let text_input_unit_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.brief_units (brief_id, unit_key) VALUES ($1, $2) RETURNING id",
        brief_id,
        "name_input",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_unit_input \
         (brief_unit_id, control_type_id, label_text, help_text, placeholder_text, control_spec) \
         VALUES ($1, $2, $3, $4, $5, $6)",
        text_input_unit_id,
        text_control_type_id,
        Some("Your name"),
        Some("Just your first name"),
        Some("Type here"),
        serde_json::json!({ "value_slot_id": text_input_slot_id.to_string() }),
    )
    .execute(pool)
    .await
    .unwrap();

    // Select input unit (with 2 choices)
    let select_input_unit_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.brief_units (brief_id, unit_key) VALUES ($1, $2) RETURNING id",
        brief_id,
        "color_input",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_unit_input \
         (brief_unit_id, control_type_id, label_text, control_spec) \
         VALUES ($1, $2, $3, $4)",
        select_input_unit_id,
        select_control_type_id,
        Some("Pick a color"),
        serde_json::json!({ "value_slot_id": select_input_slot_id.to_string() }),
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_unit_input_choices (brief_unit_id, label_text, value_text) \
         VALUES ($1, $2, $3), ($1, $4, $5)",
        select_input_unit_id,
        Some("Red"),
        "red",
        Some("Blue"),
        "blue",
    )
    .execute(pool)
    .await
    .unwrap();

    // Notice unit
    let notice_unit_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.brief_units (brief_id, unit_key) VALUES ($1, $2) RETURNING id",
        brief_id,
        "operator_notice",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_unit_notice (brief_unit_id, level, title_text, detail_text) \
         VALUES ($1, $2, $3, $4)",
        notice_unit_id,
        "info",
        Some("Heads up"),
        Some("Pay attention to this"),
    )
    .execute(pool)
    .await
    .unwrap();

    // Markdown unit
    let markdown_unit_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.brief_units (brief_id, unit_key) VALUES ($1, $2) RETURNING id",
        brief_id,
        "intro_markdown",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_unit_markdown (brief_unit_id, markdown_text) \
         VALUES ($1, $2)",
        markdown_unit_id,
        "# Welcome\nFill in the form below.",
    )
    .execute(pool)
    .await
    .unwrap();

    // -- brief_group -> brief_group_briefs -----------------------------------
    let brief_group_entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'brief_group'")
            .fetch_one(pool)
            .await
            .unwrap();
    let brief_group_path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        brief_group_entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let brief_group_vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        brief_group_path_id,
        fake_content_hash(),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_group_path_details (path_id, name, namespace_company_id) \
         VALUES ($1, $2, $3)",
        brief_group_path_id,
        format!("brief-group-{}", Uuid::now_v7()),
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    let brief_group_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.brief_groups (versioned_object_id, title) \
         VALUES ($1, $2) RETURNING id",
        brief_group_vo_id,
        "Test Group",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_group_briefs (brief_group_id, brief_id) VALUES ($1, $2)",
        brief_group_id,
        brief_id,
    )
    .execute(pool)
    .await
    .unwrap();

    // -- brief_pack -> brief_pack_groups -> journey_brief_packs --------------
    let brief_pack_entity_type_id: i16 =
        sqlx::query_scalar!("SELECT id FROM rev.entity_types WHERE key = 'brief_pack'")
            .fetch_one(pool)
            .await
            .unwrap();
    let brief_pack_path_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.object_paths (entity_type_id, namespace_company_id, scope) \
         VALUES ($1, $2, 'external') RETURNING id",
        brief_pack_entity_type_id,
        customer.company_id,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let brief_pack_vo_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.versioned_objects (path_id, content_hash) \
         VALUES ($1, $2) RETURNING id",
        brief_pack_path_id,
        fake_content_hash(),
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_pack_path_details (path_id, name, namespace_company_id) \
         VALUES ($1, $2, $3)",
        brief_pack_path_id,
        format!("brief-pack-{}", Uuid::now_v7()),
        customer.company_id,
    )
    .execute(pool)
    .await
    .unwrap();
    let brief_pack_id: Uuid = sqlx::query_scalar!(
        "INSERT INTO rev.brief_packs (versioned_object_id, title) \
         VALUES ($1, $2) RETURNING id",
        brief_pack_vo_id,
        "Test Pack",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.brief_pack_groups (brief_pack_id, brief_group_id) VALUES ($1, $2)",
        brief_pack_id,
        brief_group_id,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO rev.journey_brief_packs (journey_id, brief_pack_id) VALUES ($1, $2)",
        journey_id,
        brief_pack_id,
    )
    .execute(pool)
    .await
    .unwrap();

    BriefFormFixture {
        journey_id,
        dispatch_id,
        brief_id,
        text_input_unit_id,
        text_input_slot_id,
        select_input_unit_id,
        select_input_slot_id,
        notice_unit_id,
        markdown_unit_id,
    }
}

/// `GET /api/journeys/{id}/brief-form` happy path — all unit subtypes
/// (input/notice/markdown) come back; the select input carries its choices;
/// `value_slot_id` is populated on each input.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_brief_form_happy_path(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let fixture = seed_brief_form_fixture(&pool, &customer).await;
    let signed = sign_test_token(&customer, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    let jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/brief-form", fixture.journey_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {}", body);
    assert_eq!(body["journey_id"], serde_json::json!(fixture.journey_id));

    let briefs = body["briefs"].as_array().expect("briefs array");
    assert_eq!(briefs.len(), 1, "expected one brief; got {}", body);
    let brief = &briefs[0];
    assert_eq!(brief["id"], serde_json::json!(fixture.brief_id));
    assert_eq!(brief["title"], "Test Brief");

    // Three units total: text-input, select-input, notice, markdown == 4.
    let units = brief["units"].as_array().expect("units array");
    assert_eq!(units.len(), 4, "expected 4 units; got {}", brief);

    // Find each by id.
    let find_unit = |id: Uuid| {
        units
            .iter()
            .find(|u| u["id"] == serde_json::json!(id))
            .unwrap_or_else(|| panic!("unit {} missing", id))
    };

    let text_unit = find_unit(fixture.text_input_unit_id);
    assert!(
        text_unit["notice"].is_null(),
        "text unit should have no notice"
    );
    assert!(
        text_unit["markdown"].is_null(),
        "text unit should have no markdown"
    );
    let text_input = text_unit["input"].as_object().expect("input populated");
    assert_eq!(text_input["control_type"], "text");
    assert_eq!(text_input["label_text"], "Your name");
    assert_eq!(
        text_input["value_slot_id"],
        serde_json::json!(fixture.text_input_slot_id),
    );
    assert!(
        text_input["choices"].as_array().unwrap().is_empty(),
        "text input has no choices",
    );

    let select_unit = find_unit(fixture.select_input_unit_id);
    let select_input = select_unit["input"].as_object().expect("select input");
    assert_eq!(select_input["control_type"], "select");
    assert_eq!(
        select_input["value_slot_id"],
        serde_json::json!(fixture.select_input_slot_id),
    );
    let choices = select_input["choices"].as_array().expect("choices array");
    assert_eq!(choices.len(), 2, "expected 2 choices; got {:?}", choices);
    let choice_values: Vec<String> = choices
        .iter()
        .map(|c| c["value"].as_str().unwrap().to_string())
        .collect();
    assert!(choice_values.contains(&"red".to_string()));
    assert!(choice_values.contains(&"blue".to_string()));

    let notice_unit = find_unit(fixture.notice_unit_id);
    assert!(notice_unit["input"].is_null());
    assert!(notice_unit["markdown"].is_null());
    let notice = notice_unit["notice"].as_object().expect("notice block");
    assert_eq!(notice["level"], "info");
    assert_eq!(notice["title_text"], "Heads up");
    assert_eq!(notice["detail_text"], "Pay attention to this");

    let markdown_unit = find_unit(fixture.markdown_unit_id);
    assert!(markdown_unit["input"].is_null());
    assert!(markdown_unit["notice"].is_null());
    let md = markdown_unit["markdown"]
        .as_object()
        .expect("markdown block");
    assert_eq!(md["text"], "# Welcome\nFill in the form below.");
}

/// `GET /api/journeys/{id}/brief-form` honors `If-None-Match` and returns 304
/// with the same ETag header echoed back.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_brief_form_etag_returns_304(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let fixture = seed_brief_form_fixture(&pool, &customer).await;
    let signed = sign_test_token(&customer, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 1. First call returns 200 + ETag.
    let first = http
        .get(format!(
            "{}/api/journeys/{}/brief-form",
            base_url, fixture.journey_id
        ))
        .bearer_auth(&jwt)
        .send()
        .await
        .expect("first GET");
    assert_eq!(first.status(), StatusCode::OK);
    let etag = first
        .headers()
        .get(reqwest::header::ETAG)
        .expect("ETag header present")
        .to_str()
        .expect("ETag is utf8")
        .to_string();
    assert!(
        etag.starts_with('"') && etag.ends_with('"'),
        "ETag must be quoted per RFC 7232: {}",
        etag
    );

    // 2. Second call with If-None-Match returns 304 + same ETag.
    let second = http
        .get(format!(
            "{}/api/journeys/{}/brief-form",
            base_url, fixture.journey_id
        ))
        .bearer_auth(&jwt)
        .header(reqwest::header::IF_NONE_MATCH, &etag)
        .send()
        .await
        .expect("second GET");
    assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    let echoed_etag = second
        .headers()
        .get(reqwest::header::ETAG)
        .expect("ETag echoed on 304")
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(echoed_etag, etag, "304 must echo the same ETag");

    // 3. Body MUST be empty on 304.
    let body_bytes = second.bytes().await.unwrap_or_default();
    assert!(
        body_bytes.is_empty(),
        "304 body should be empty; got: {:?}",
        body_bytes
    );
}

/// `GET /api/journeys/{id}/bindings` happy path. POST a binding via the
/// existing dispatch_intent_bindings handler, then assert the bindings
/// endpoint surfaces it with the typed value + `bound_at`.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_journey_bindings_happy_path(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "alpha", "alpha_prod").await;
    let fixture = seed_brief_form_fixture(&pool, &customer).await;
    let signed = sign_test_token(&customer, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 1. Initially no bindings.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/bindings", fixture.journey_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["journey_id"], serde_json::json!(fixture.journey_id));
    assert!(
        body["bindings"].as_array().unwrap().is_empty(),
        "expected empty bindings list; got {}",
        body
    );

    // 2. POST a binding for the text_input slot.
    let (status, _body) = post_json(
        &http,
        &base_url,
        "/api/dispatch-intent-bindings",
        &jwt,
        serde_json::json!({
            "objs": [{
                "dispatch_id": fixture.dispatch_id,
                "value_slot_id": fixture.text_input_slot_id,
                "text_items": [{ "value": "Alice" }],
                "int_items": [],
                "number_items": [],
                "bool_items": [],
                "json_items": [],
                "asset_items": [],
                "value_meta": [],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 3. Bindings now shows the text binding.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/bindings", fixture.journey_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let bindings = body["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0]["value_slot_id"],
        serde_json::json!(fixture.text_input_slot_id),
    );
    assert_eq!(bindings[0]["value_kind"], "text");
    assert_eq!(bindings[0]["text_value"], "Alice");
    assert!(
        bindings[0]["bound_at"].is_string(),
        "bound_at must be an RFC3339 string; got {}",
        bindings[0]
    );

    // 4. POST the second binding (for the select_input slot) so the journey
    //    becomes fully bound.
    let (status, _body) = post_json(
        &http,
        &base_url,
        "/api/dispatch-intent-bindings",
        &jwt,
        serde_json::json!({
            "objs": [{
                "dispatch_id": fixture.dispatch_id,
                "value_slot_id": fixture.select_input_slot_id,
                "text_items": [{ "value": "red" }],
                "int_items": [],
                "number_items": [],
                "bool_items": [],
                "json_items": [],
                "asset_items": [],
                "value_meta": [],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 5. Both bindings visible now.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/bindings", fixture.journey_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let bindings = body["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 2, "expected 2 bindings; got {}", body);
}

/// Cross-namespace probe: alpha asks for a journey owned by beta and gets
/// 404 RESOURCE_NOT_FOUND. The error body must NOT distinguish between
/// "doesn't exist" and "belongs to another namespace."
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_brief_form_foreign_journey_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;

    // Beta has a real journey with brief-form.
    let beta_fixture = seed_brief_form_fixture(&pool, &beta).await;

    // Alpha calls the endpoint with beta's journey_id.
    let signed = sign_test_token(&alpha, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &alpha.service_id).await;
    let alpha_jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/brief-form", beta_fixture.journey_id),
        &alpha_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");

    // Same response for a brand-new UUID alpha invented: structurally
    // identical so cross-namespace probes can't distinguish.
    let invented = Uuid::now_v7();
    let (status2, body2) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/brief-form", invented),
        &alpha_jwt,
    )
    .await;
    assert_eq!(status2, StatusCode::NOT_FOUND);
    assert_eq!(body2["error"], "RESOURCE_NOT_FOUND");
}

/// Same cross-namespace test for the bindings endpoint.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_journey_bindings_foreign_journey_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let beta = setup_customer(&pool, "beta", "beta_prod").await;

    let beta_fixture = seed_brief_form_fixture(&pool, &beta).await;

    let signed = sign_test_token(&alpha, 60, 0);
    let (_, body) = exchange_token(&http, &base_url, &signed, &alpha.service_id).await;
    let alpha_jwt = body["jwt"].as_str().unwrap().to_string();

    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/bindings", beta_fixture.journey_id),
        &alpha_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {}", body);
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");
}

/// Defense-in-depth for the namespace filter on Unicode-keyed content: alpha
/// ingests a Unicode guide, beta cannot see it via the external `GET /guides`.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_unicode_guide_isolated_across_companies(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let alpha = setup_customer(&pool, "alpha-uni", "alpha_uni_prod").await;
    let beta = setup_customer(&pool, "beta-uni", "beta_uni_prod").await;

    let exchange_for = |c: &TestCustomer| {
        let app_url = base_url.clone();
        let signed = sign_test_token(c, 60, 0);
        let sid = c.service_id.clone();
        let http = http.clone();
        async move {
            let (status, body) = exchange_token(&http, &app_url, &signed, &sid).await;
            assert_eq!(status, StatusCode::OK);
            body["jwt"].as_str().unwrap().to_string()
        }
    };
    let alpha_jwt = exchange_for(&alpha).await;
    let beta_jwt = exchange_for(&beta).await;

    // Alpha ingests the Unicode guide.
    let suffix = uuid::Uuid::now_v7().to_string();
    let tar_bytes = unicode_guide_tarball_bytes(&suffix);
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(tar_bytes)
            .file_name("unicode.tar.gz")
            .mime_str("application/gzip")
            .unwrap(),
    );
    let resp = http
        .post(format!("{}/api/guides/ingest", base_url))
        .bearer_auth(&alpha_jwt)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Beta lists — must see zero guides even though Unicode keys are in the DB.
    let (status, body) = list_guides(&http, &base_url, &beta_jwt).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["objs"].as_array().unwrap().len(),
        0,
        "beta must not see alpha's Unicode-keyed guide"
    );
}

// =============================================================================
// Slice 3/4 (Migration 43): auto-derive journey_brief_packs from the playbook
// =============================================================================
//
// Background: pre-Migration-43, journey-creation only attached brief_packs
// the caller passed in `brief_pack_ids`. Clients that only know `playbook_id`
// (dapple-ui-demo, dapple-suite, acme) had no way to discover the
// brief_packs the source guide declares, so journeys were created with zero
// `journey_brief_packs` rows and `/api/journeys/{id}/brief-form` returned
// no input units. Slice 3 makes the server auto-derive when the caller
// omits brief_pack_ids; Slice 4 repairs pre-existing broken journeys under
// `?backfill=true`.
//
// These three tests pin the forward path, the explicit-override escape
// hatch, and the corrupt-then-repair invariant codified in
// `crates/dapple-prompt-api/AGENTS.md` (the "Testing Repair Operations"
// rule that response-shape-only assertions are insufficient — we must
// SQL-verify the underlying state changed).

/// Build a guide tarball that wires a brief-pack with one text-input
/// referencing a real value-slot. Used by the auto-derive tests to seed a
/// playbook whose source guide carries an enumerable brief_pack. Each call
/// receives a unique suffix so multiple tests on the same DB don't collide
/// on the `(name, namespace_company_id)` UNIQUEs.
fn make_brief_pack_guide_dir(suffix: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let guide_dir = tmp.path().join(format!("brief-pack-guide-{}", suffix));
    std::fs::create_dir_all(guide_dir.join("locales")).expect("mkdir locales");

    std::fs::write(
        guide_dir.join("guide.toml"),
        format!(
            r#"title = "Brief Pack Guide {suffix}"
description = "Fixture used by journey-brief-pack auto-derive tests."
author = ""
classification = "workflow"
source_path = "brief-pack-guide-{suffix}"

[chapters."intro.md"]
subtitle = "Wires brief-input → brief → brief-group → brief-pack with a real value-slot."
"#,
            suffix = suffix
        ),
    )
    .expect("write guide.toml");

    std::fs::write(
        guide_dir.join("SUMMARY.md"),
        "# Summary\n\n- [Introduction](intro.md)\n",
    )
    .expect("write SUMMARY.md");
    std::fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

    std::fs::write(
        guide_dir.join("intro.md"),
        format!(
            r#"# Introduction

A guide carrying one brief_pack so journey-create auto-derive has a real
walk: `playbook → guide_playbooks → guide_brief_packs → brief_pack_ids`.

:::value-slot {{ name: "topic-slot-{suffix}", kind: "text" }}
:::

:::brief-input {{ name: "topic-input-{suffix}", control: "text", slot: "topic-slot-{suffix}" }}
:::

:::brief {{ name: "topic-brief-{suffix}", units: ["topic-input-{suffix}"] }}
:::

:::brief-group {{ name: "topic-group-{suffix}", briefs: ["topic-brief-{suffix}"] }}
:::

:::brief-pack {{ name: "topic-pack-{suffix}", groups: ["topic-group-{suffix}"] }}
:::

:::stage {{ name: "intro-stage-{suffix}" }}

:::step {{ name: "intro-step-{suffix}", task: "intro-task-{suffix}" }}
:::

:::

:::nushell {{ name: "intro-task-{suffix}", tags: [coder] }}
{{ payload: {{ ok: true }}, signals: {{ done: true }} }}
:::
"#,
            suffix = suffix
        ),
    )
    .expect("write intro.md");

    std::fs::write(
        guide_dir.join("locales/en.ftl"),
        format!(
            r#"topic-input-{suffix} = Topic input label
topic-brief-{suffix} = Topic brief title
topic-group-{suffix} = Topic group title
topic-pack-{suffix} = Topic pack title
intro-stage-{suffix} = Intro stage description
intro-step-{suffix} = Intro step description
"#,
            suffix = suffix
        ),
    )
    .expect("write en.ftl");

    // Move guide_dir contents into a TempDir-rooted directory whose name
    // matches what build_tarball expects (`build_tarball` uses
    // file_name() of the dir, so the inner directory must already exist).
    // We return the outer TempDir; the caller composes
    // `tmp.path().join("brief-pack-guide-{suffix}")`.
    tmp
}

/// A guide whose single-step stage exposes ONLY a computed `:::brief-output`
/// (`synth`); `slot-a` is an UNEXPOSED brief input so the brief is valid yet
/// nothing gates the dispatch start (computed outputs are excluded from the
/// start gate). Caller composes `tmp.path().join("computed-output-guide-{suffix}")`.
/// Used by `computed_output_reaches_done_after_dispatch`.
fn make_computed_output_guide_dir(suffix: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let guide_dir = tmp.path().join(format!("computed-output-guide-{}", suffix));
    std::fs::create_dir_all(guide_dir.join("locales")).expect("mkdir locales");

    std::fs::write(
        guide_dir.join("guide.toml"),
        format!(
            r#"title = "Computed Output Guide {suffix}"
description = "Single-step stage exposing one computed output."
author = ""
classification = "workflow"
source_path = "computed-output-guide-{suffix}"
"#,
            suffix = suffix
        ),
    )
    .expect("write guide.toml");

    std::fs::write(
        guide_dir.join("SUMMARY.md"),
        "# Summary\n\n- [Introduction](intro.md)\n",
    )
    .expect("write SUMMARY.md");
    std::fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

    std::fs::write(
        guide_dir.join("intro.md"),
        format!(
            r#"# Introduction

:::value-slot {{ name: "slot-a-{suffix}", kind: "text" }}
:::

:::value-slot {{ name: "synth-{suffix}", kind: "json" }}
:::

:::brief-input {{ name: "input-a-{suffix}", control: "text", slot: "slot-a-{suffix}" }}
:::

:::brief-output {{ name: "synth-out-{suffix}", slot: "synth-{suffix}", source: "computed", display: "markdown", reveal: true, title: "Synth Title" }}
:::

:::brief {{ name: "brief-{suffix}", units: ["input-a-{suffix}", "synth-out-{suffix}"] }}
:::

:::brief-group {{ name: "group-{suffix}", briefs: ["brief-{suffix}"] }}
:::

:::brief-pack {{ name: "pack-{suffix}", groups: ["group-{suffix}"] }}
:::

:::stage {{ name: "stage-{suffix}", expose: ["synth-{suffix}"] }}

:::step {{ name: "step-{suffix}", task: "task-{suffix}" }}
:::

:::

:::nushell {{ name: "task-{suffix}", tags: [coder] }}
{{ payload: {{ result: "done" }}, signals: {{ done: true }} }}
:::
"#,
            suffix = suffix
        ),
    )
    .expect("write intro.md");

    std::fs::write(
        guide_dir.join("locales/en.ftl"),
        format!(
            r#"input-a-{suffix} = Topic A input
synth-out-{suffix} = Synthesis output
brief-{suffix} = The brief
group-{suffix} = The group
pack-{suffix} = The pack
stage-{suffix} = The stage
step-{suffix} = The step
"#,
            suffix = suffix
        ),
    )
    .expect("write en.ftl");

    tmp
}

/// A producer→consumer guide for the consumed-computed-dependency gate test.
/// `produce-stage` binds a computed slot (`synth`) via `source_path: /synthkey`;
/// `consume-stage` is a LATER stage whose step also exposes `synth` (the consumer
/// reads it). Neither stage has an operator input, so the only thing that can
/// gate the consumer dispatch is the computed dependency. The consumer task's
/// payload has no `synthkey`, so it does not re-produce `synth` (source_path
/// misses) — matching the wimmelbook render step's relationship to the shotlist.
fn make_consumed_computed_guide_dir(suffix: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let guide_dir = tmp
        .path()
        .join(format!("consumed-computed-guide-{}", suffix));
    std::fs::create_dir_all(guide_dir.join("locales")).expect("mkdir locales");

    std::fs::write(
        guide_dir.join("guide.toml"),
        format!(
            r#"title = "Consumed Computed Guide {suffix}"
description = "Producer stage binds a computed slot; a later consumer stage exposes it."
author = ""
classification = "workflow"
source_path = "consumed-computed-guide-{suffix}"
"#,
            suffix = suffix
        ),
    )
    .expect("write guide.toml");

    std::fs::write(
        guide_dir.join("SUMMARY.md"),
        "# Summary\n\n- [Introduction](intro.md)\n",
    )
    .expect("write SUMMARY.md");
    std::fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

    std::fs::write(
        guide_dir.join("intro.md"),
        format!(
            r#"# Introduction

:::value-slot {{ name: "synth-{suffix}", kind: "json" }}
:::

:::brief-output {{ name: "synth-out-{suffix}", slot: "synth-{suffix}", source: "computed", source_path: "/synthkey", display: "hidden" }}
:::

:::brief {{ name: "brief-{suffix}", units: ["synth-out-{suffix}"] }}
:::

:::brief-group {{ name: "group-{suffix}", briefs: ["brief-{suffix}"] }}
:::

:::brief-pack {{ name: "pack-{suffix}", groups: ["group-{suffix}"] }}
:::

:::stage {{ name: "produce-stage-{suffix}", expose: ["synth-{suffix}"] }}

:::step {{ name: "produce-step-{suffix}", task: "produce-task-{suffix}" }}
:::

:::

:::stage {{ name: "consume-stage-{suffix}", expose: ["synth-{suffix}"] }}

:::step {{ name: "consume-step-{suffix}", task: "consume-task-{suffix}" }}
:::

:::

:::nushell {{ name: "produce-task-{suffix}", tags: [coder] }}
{{ payload: {{ synthkey: "produced" }}, signals: {{ done: true }} }}
:::

:::nushell {{ name: "consume-task-{suffix}", tags: [coder] }}
{{ payload: {{ other: "x" }}, signals: {{ done: true }} }}
:::
"#,
            suffix = suffix
        ),
    )
    .expect("write intro.md");

    std::fs::write(
        guide_dir.join("locales/en.ftl"),
        format!(
            r#"synth-out-{suffix} = Synthesis output
brief-{suffix} = The brief
group-{suffix} = The group
pack-{suffix} = The pack
produce-stage-{suffix} = Produce stage
produce-step-{suffix} = Produce step
consume-stage-{suffix} = Consume stage
consume-step-{suffix} = Consume step
"#,
            suffix = suffix
        ),
    )
    .expect("write en.ftl");

    tmp
}

/// Two no-operator-input compute stages, each producing its OWN distinct computed
/// slot (`a` from `/akey`, `b` from `/bkey`). Unlike `make_consumed_computed_guide_dir`
/// (where both stages expose the SAME slot, so the second is trivially `done` once the
/// first binds it), here stage B is `done` ONLY after its own dispatch runs — so a
/// reconcile-driven cascade must create AND run a dispatch for EACH stage. Used by the
/// reconcile-on-poll recovery test to prove the count grows one-per-stage.
fn make_two_stage_distinct_slots_guide_dir(suffix: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let guide_dir = tmp
        .path()
        .join(format!("two-stage-distinct-guide-{}", suffix));
    std::fs::create_dir_all(guide_dir.join("locales")).expect("mkdir locales");

    std::fs::write(
        guide_dir.join("guide.toml"),
        format!(
            r#"title = "Two Stage Distinct Guide {suffix}"
description = "Two no-input compute stages, each producing its own computed slot."
author = ""
classification = "workflow"
source_path = "two-stage-distinct-guide-{suffix}"
"#,
            suffix = suffix
        ),
    )
    .expect("write guide.toml");

    std::fs::write(
        guide_dir.join("SUMMARY.md"),
        "# Summary\n\n- [Introduction](intro.md)\n",
    )
    .expect("write SUMMARY.md");
    std::fs::write(guide_dir.join("CONDUITS.md"), "# Conduit Context\n").unwrap();

    std::fs::write(
        guide_dir.join("intro.md"),
        format!(
            r#"# Introduction

:::value-slot {{ name: "a-{suffix}", kind: "json" }}
:::

:::value-slot {{ name: "b-{suffix}", kind: "json" }}
:::

:::brief-output {{ name: "a-out-{suffix}", slot: "a-{suffix}", source: "computed", source_path: "/akey", display: "hidden" }}
:::

:::brief-output {{ name: "b-out-{suffix}", slot: "b-{suffix}", source: "computed", source_path: "/bkey", display: "hidden" }}
:::

:::brief {{ name: "brief-{suffix}", units: ["a-out-{suffix}", "b-out-{suffix}"] }}
:::

:::brief-group {{ name: "group-{suffix}", briefs: ["brief-{suffix}"] }}
:::

:::brief-pack {{ name: "pack-{suffix}", groups: ["group-{suffix}"] }}
:::

:::stage {{ name: "stage-a-{suffix}", expose: ["a-{suffix}"] }}

:::step {{ name: "step-a-{suffix}", task: "task-a-{suffix}" }}
:::

:::

:::stage {{ name: "stage-b-{suffix}", expose: ["b-{suffix}"] }}

:::step {{ name: "step-b-{suffix}", task: "task-b-{suffix}" }}
:::

:::

:::nushell {{ name: "task-a-{suffix}", tags: [coder] }}
{{ payload: {{ akey: "a-done" }}, signals: {{ done: true }} }}
:::

:::nushell {{ name: "task-b-{suffix}", tags: [coder] }}
{{ payload: {{ bkey: "b-done" }}, signals: {{ done: true }} }}
:::
"#,
            suffix = suffix
        ),
    )
    .expect("write intro.md");

    std::fs::write(
        guide_dir.join("locales/en.ftl"),
        format!(
            r#"a-out-{suffix} = A output
b-out-{suffix} = B output
brief-{suffix} = The brief
group-{suffix} = The group
pack-{suffix} = The pack
stage-a-{suffix} = Stage A
step-a-{suffix} = Step A
stage-b-{suffix} = Stage B
step-b-{suffix} = Step B
"#,
            suffix = suffix
        ),
    )
    .expect("write en.ftl");

    tmp
}

/// All (stage_step_id, step_task_id) of a playbook in playbook order
/// (stage ord, step ord, task ord). Lets a test dispatch a specific step.
async fn resolve_playbook_steps_ordered(pool: &PgPool, playbook_id: Uuid) -> Vec<(Uuid, Uuid)> {
    let rows = sqlx::query!(
        r#"SELECT ss.id AS stage_step_id, st.id AS step_task_id
           FROM rev.playbooks p
           JOIN rev.playbook_stages ps ON ps.playbook_id = p.id
           JOIN rev.stage_steps ss ON ss.stage_id = ps.stage_id
           JOIN rev.step_tasks st ON st.step_id = ss.step_id
           WHERE p.id = $1
           ORDER BY ps.ord, ss.ord, st.ord"#,
        playbook_id,
    )
    .fetch_all(pool)
    .await
    .expect("resolve ordered steps");
    rows.into_iter()
        .map(|r| (r.stage_step_id, r.step_task_id))
        .collect()
}

/// Count task attempts a dispatch has produced (across its stages/steps). Zero
/// means the dispatch never executed — the DB-level signal the gate test asserts
/// on, rather than a response flag.
async fn dispatch_attempt_count(pool: &PgPool, dispatch_id: Uuid) -> i64 {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!"
           FROM rev.dispatch_task_attempts a
           JOIN rev.dispatch_steps ds ON ds.id = a.dispatch_step_id
           JOIN rev.dispatch_stages dst ON dst.id = ds.dispatch_stage_id
           WHERE dst.dispatch_id = $1"#,
        dispatch_id,
    )
    .fetch_one(pool)
    .await
    .expect("attempt count")
}

/// Count how many dispatches a journey has. Zero on a fresh all-no-input journey is
/// the stalled state reconcile-on-poll recovers; the count growing to one-per-stage
/// is the DB-level proof the reconcile drove the cascade. Runtime query (not the
/// `sqlx::query!` macro) so it needs no offline-cache entry.
async fn journey_dispatch_count(pool: &PgPool, journey_id: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM rev.dispatches WHERE journey_id = $1")
        .bind(journey_id)
        .fetch_one(pool)
        .await
        .expect("journey dispatch count")
}

/// Resolve the latest brief_pack_id from a guide's source markdown name
/// suffix. Used by tests that need to compare auto-derived vs explicit
/// brief_pack_ids — the ingest response only carries derived_objects, not
/// the per-pack ids.
async fn brief_pack_id_by_suffix(pool: &PgPool, suffix: &str) -> Uuid {
    let name = format!("topic-pack-{suffix}");
    sqlx::query_scalar!(
        "SELECT bp.id
         FROM rev.brief_packs bp
         JOIN rev.versioned_objects vo ON vo.id = bp.versioned_object_id
         JOIN rev.brief_pack_path_details bpd ON bpd.path_id = vo.path_id
         WHERE bpd.name = $1
         ORDER BY vo.created_at DESC
         LIMIT 1",
        name,
    )
    .fetch_one(pool)
    .await
    .unwrap_or_else(|_| panic!("brief_pack {} not found after ingest", name))
}

/// Resolve the playbook_id derived from a brief-pack guide ingest. The
/// ingest response carries `derived_objects.playbook_ids`; we just unwrap
/// the first one.
fn playbook_id_from_ingest(ingest_body: &Value) -> Uuid {
    let ids = ingest_body["derived_objects"]["playbook_ids"]
        .as_array()
        .expect("derived_objects.playbook_ids must be present");
    assert!(
        !ids.is_empty(),
        "ingest produced no derived playbooks; body: {}",
        ingest_body,
    );
    serde_json::from_value(ids[0].clone()).expect("playbook_id deserializes")
}

/// Resolve the guide path_id from an ingest response — needed for the
/// PATCH backfill scenario.
async fn guide_path_id_from_ingest(pool: &PgPool, ingest_body: &Value) -> Uuid {
    let guide_id: Uuid =
        serde_json::from_value(ingest_body["guide"]["id"].clone()).expect("guide id");
    sqlx::query_scalar!(
        "SELECT vo.path_id
         FROM rev.guides g
         JOIN rev.versioned_objects vo ON vo.id = g.versioned_object_id
         WHERE g.id = $1",
        guide_id,
    )
    .fetch_one(pool)
    .await
    .expect("path_id lookup")
}

/// Auto-derive: POST a journey without `brief_pack_ids` and confirm the
/// server walks the guide_playbooks → guide_brief_packs chain to populate
/// `rev.journey_brief_packs`. Brief-form must then return at least one
/// input unit with a populated `value_slot_id` (the load-bearing
/// downstream property — the auto-derive only matters if it makes
/// `/brief-form` work).
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn test_journey_creation_auto_derives_brief_packs(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // 1. Onboard + JWT exchange.
    let customer = setup_customer(&pool, "autoderiveco", "autoderive_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 2. Ingest a guide that declares a brief-pack.
    let suffix = "autoderive";
    let tmp = make_brief_pack_guide_dir(suffix);
    let guide_dir = tmp.path().join(format!("brief-pack-guide-{}", suffix));
    let ingest_body = post_guide_ingest(&http, &base_url, &jwt, &guide_dir).await;

    let playbook_id = playbook_id_from_ingest(&ingest_body);
    let expected_pack_id = brief_pack_id_by_suffix(&pool, suffix).await;
    let guide_id = guide_for_playbook(&pool, playbook_id).await;

    // 3. POST a journey with NO brief_pack_ids — the server must auto-derive.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "auto-derived journey",
                "playbook_id": playbook_id,
                "guide_id": guide_id,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {}", body);
    let journey_id: Uuid =
        serde_json::from_value(body["objs"][0]["id"].clone()).expect("journey id");

    // Response DTO surfaces the auto-derived ids on `brief_packs` so the
    // client can see what got attached without a separate query.
    let response_bp_ids: Vec<Uuid> = body["objs"][0]["brief_packs"]
        .as_array()
        .expect("brief_packs array")
        .iter()
        .map(|v| serde_json::from_value(v["id"].clone()).expect("bp id"))
        .collect();
    assert_eq!(
        response_bp_ids,
        vec![expected_pack_id],
        "response brief_packs must match the auto-derived set"
    );

    // 4. SQL truth-check: `rev.journey_brief_packs` must carry the
    //    derived pack. The forward path matters only because the
    //    table population works.
    let bp_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*)::BIGINT AS "count!"
           FROM rev.journey_brief_packs
           WHERE journey_id = $1"#,
        journey_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        bp_count >= 1,
        "expected at least one journey_brief_packs row for auto-derived journey; got {}",
        bp_count,
    );
    let pack_ids: Vec<Uuid> = sqlx::query_scalar!(
        r#"SELECT brief_pack_id FROM rev.journey_brief_packs WHERE journey_id = $1"#,
        journey_id,
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        pack_ids.contains(&expected_pack_id),
        "expected brief_pack {} in the auto-derived set; got {:?}",
        expected_pack_id,
        pack_ids,
    );

    // 5. Brief-form must return at least one input with a populated
    //    value_slot_id. This is the load-bearing downstream property —
    //    the auto-derive only matters if it makes the brief-form work.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{}/brief-form", journey_id),
        &jwt,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "brief-form body: {}", body);
    let briefs = body["briefs"].as_array().expect("briefs array");
    assert!(
        !briefs.is_empty(),
        "expected non-empty briefs after auto-derive; body: {}",
        body
    );
    let units = briefs[0]["units"].as_array().expect("units array");
    // brief-form serializes each unit with optional `input`/`notice`/`markdown`
    // objects; the populated one identifies the subtype. We need an input
    // unit whose `value_slot_id` resolves (proves the brief→input→slot chain
    // is wired through to the response).
    let input_unit = units
        .iter()
        .find(|u| u.get("input").map(|i| !i.is_null()).unwrap_or(false))
        .unwrap_or_else(|| panic!("no input unit found; units: {:?}", units));
    let value_slot_id = input_unit["input"]["value_slot_id"].as_str();
    assert!(
        value_slot_id.is_some() && !value_slot_id.unwrap().is_empty(),
        "input unit must carry populated value_slot_id; got: {:?}",
        input_unit
    );
}

/// Caller-supplied IDs take precedence. The explicit-override escape hatch
/// is a contract for advanced clients and tests; if the server starts
/// merging caller-supplied with auto-derived, existing tests + dapple-suite
/// would silently change behavior.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn test_journey_creation_caller_supplied_brief_packs_take_precedence(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let customer = setup_customer(&pool, "explicitco", "explicit_prod").await;
    let signed = sign_test_token(&customer, 60, 0);
    let (status, body) = exchange_token(&http, &base_url, &signed, &customer.service_id).await;
    assert_eq!(status, StatusCode::OK);
    let jwt = body["jwt"].as_str().unwrap().to_string();

    // 1. Ingest two brief-pack guides in this namespace.
    let suffix_a = "explicit-a";
    let suffix_b = "explicit-b";
    let tmp_a = make_brief_pack_guide_dir(suffix_a);
    let tmp_b = make_brief_pack_guide_dir(suffix_b);
    let dir_a = tmp_a.path().join(format!("brief-pack-guide-{}", suffix_a));
    let dir_b = tmp_b.path().join(format!("brief-pack-guide-{}", suffix_b));
    let ingest_a = post_guide_ingest(&http, &base_url, &jwt, &dir_a).await;
    let _ingest_b = post_guide_ingest(&http, &base_url, &jwt, &dir_b).await;

    let playbook_a = playbook_id_from_ingest(&ingest_a);
    // Use guide B's pack as the explicit override — proves the server doesn't
    // silently swap to the auto-derived set.
    let explicit_pack = brief_pack_id_by_suffix(&pool, suffix_b).await;
    let auto_pack = brief_pack_id_by_suffix(&pool, suffix_a).await;
    let guide_a = guide_for_playbook(&pool, playbook_a).await;

    // 2. POST a journey on playbook A with explicit brief_pack_ids = [pack B].
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "explicit override journey",
                "playbook_id": playbook_a,
                "guide_id": guide_a,
                "brief_pack_ids": [explicit_pack],
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {}", body);
    let journey_id: Uuid =
        serde_json::from_value(body["objs"][0]["id"].clone()).expect("journey id");

    // 3. SQL: exactly the explicit pack must be attached; auto-derived pack
    //    must NOT have leaked in.
    let pack_ids: Vec<Uuid> = sqlx::query_scalar!(
        r#"SELECT brief_pack_id FROM rev.journey_brief_packs
           WHERE journey_id = $1 ORDER BY brief_pack_id"#,
        journey_id,
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        pack_ids,
        vec![explicit_pack],
        "explicit brief_pack_ids must be exactly what was passed; auto-derive must NOT \
         have merged playbook A's pack in. caller_supplied={}, auto_derived={}, got={:?}",
        explicit_pack,
        auto_pack,
        pack_ids,
    );
}

// Removed: `test_backfill_repairs_journey_brief_packs_for_existing_journeys`
// and its `guide_id_from_path` helper.
//
// The test exercised the `insert_missing_journey_brief_packs_for_guide`
// ForceRepair backfill, which is deleted alongside the journey↔guide
// version pinning work. New journeys auto-pin their guide_id at create
// time (handler Step 0.5) and existing journeys were pinned by Migration
// 45, so the per-ingest repair is no longer required. Stable-attachment
// behavior is now covered by a pinning regression test in
// `crates/dapple-prompt-api/tests/`.

// =============================================================================
// Journey-chain primitive (Migration 44) — external surface
// =============================================================================
//
// Three mirror tests:
//
//   1. `external_chain_journey_records_predecessor_and_guide` — happy path,
//      C1 creates J1 (root) then J2 chained from J1; both columns land in
//      the DB.
//   2. `external_predecessor_in_foreign_namespace_returns_404` — C2 tries
//      to chain from C1's journey; response is 404 with the canonical
//      RESOURCE_NOT_FOUND body (no existence leak across customers).
//   3. `external_chain_guide_mismatch_returns_400` — same caller, same
//      namespace, but `guide_id` differs from the predecessor's guide_id;
//      handler-level pre-check rejects with 400 before the DB trigger
//      fires.

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_chain_journey_records_predecessor_and_guide(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let (customer, jwt, playbook_id) =
        onboard_with_playbook(&pool, &http, &base_url, "chain1", "chain1_prod").await;
    let guide_id = seed_external_guide(&pool, &customer, "chain1-guide").await;
    // The journey-POST handler now validates that guide_id exposes playbook_id;
    // link the freshly-seeded chain guide to the playbook so both journeys pass.
    link_guide_playbook(&pool, guide_id, playbook_id).await;

    // 1. Root journey (no predecessor, but pin to guide_id so the chain
    //    coherence trigger has a value to match against later).
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "chapter 1",
                "playbook_id": playbook_id,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "guide_id": guide_id,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "root create failed: {}", body);
    let j1: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // 2. Chained journey (chapter 2) referencing j1.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "chapter 2",
                "playbook_id": playbook_id,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "predecessor_journey_id": j1,
                "guide_id": guide_id,
            }],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "chained create failed: {}",
        body
    );
    let j2: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // 3. Verify both columns landed on the DB row for j2.
    let (db_predecessor, db_guide): (Option<Uuid>, Option<Uuid>) =
        sqlx::query_as("SELECT predecessor_journey_id, guide_id FROM rev.journeys WHERE id = $1")
            .bind(j2)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(db_predecessor, Some(j1), "chain link must be persisted");
    assert_eq!(db_guide, Some(guide_id), "guide pin must be persisted");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_predecessor_in_foreign_namespace_returns_404(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // Customer A: seed playbook + guide + a journey J1 owned by A.
    let (customer_a, jwt_a, playbook_a) =
        onboard_with_playbook(&pool, &http, &base_url, "alpha", "alpha_prod").await;
    let guide_a = seed_external_guide(&pool, &customer_a, "alpha-guide").await;
    link_guide_playbook(&pool, guide_a, playbook_a).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt_a,
        serde_json::json!({
            "objs": [{
                "name": "alpha root",
                "playbook_id": playbook_a,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "guide_id": guide_a,
            }],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "A root create failed: {}",
        body
    );
    let j_alpha: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // Customer B: seed independently, attempt to chain from A's journey.
    // Must return 404 with the same body shape as an unknown UUID — no
    // existence leak.
    let (customer_b, jwt_b, playbook_b) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;
    // Give B a valid guide that exposes its own playbook so the journey passes
    // the namespace + guide_exposes_playbook checks; the 404 must then fire on
    // the *foreign predecessor*, which is the property under test.
    let guide_b = seed_external_guide(&pool, &customer_b, "beta-guide").await;
    link_guide_playbook(&pool, guide_b, playbook_b).await;

    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt_b,
        serde_json::json!({
            "objs": [{
                "name": "beta chained-from-alpha",
                "playbook_id": playbook_b,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "predecessor_journey_id": j_alpha,
                "guide_id": guide_b,
            }],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "foreign predecessor must be 404, got {status}: {body}"
    );
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");

    // Sanity: try a totally bogus predecessor id from beta — should produce
    // the SAME body shape (no existence-leak distinction).
    let bogus = Uuid::now_v7();
    let (status_bogus, body_bogus) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt_b,
        serde_json::json!({
            "objs": [{
                "name": "beta chained-from-bogus",
                "playbook_id": playbook_b,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "predecessor_journey_id": bogus,
                "guide_id": guide_b,
            }],
        }),
    )
    .await;
    assert_eq!(status_bogus, StatusCode::NOT_FOUND);
    assert_eq!(body_bogus["error"], "RESOURCE_NOT_FOUND");
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_chain_guide_mismatch_returns_400(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    let (customer, jwt, playbook_id) =
        onboard_with_playbook(&pool, &http, &base_url, "coh", "coh_prod").await;
    let guide_a = seed_external_guide(&pool, &customer, "coh-guide-a").await;
    let guide_b = seed_external_guide(&pool, &customer, "coh-guide-b").await;
    // The root must pass guide_exposes_playbook (guide_a→playbook); the child's
    // mismatch is caught at the predecessor pre-check (guide_b != predecessor's
    // guide_a) before guide_exposes_playbook is reached, so guide_b stays
    // intentionally unlinked.
    link_guide_playbook(&pool, guide_a, playbook_id).await;

    // Root journey pinned to guide_a.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "root in guide_a",
                "playbook_id": playbook_id,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "guide_id": guide_a,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "root create failed: {}", body);
    let root: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // Try to chain a child with guide_id = guide_b. Handler pre-check
    // returns 400 with the canonical JOURNEY_CHAIN_GUIDE_MISMATCH body.
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt,
        serde_json::json!({
            "objs": [{
                "name": "mismatched chain",
                "playbook_id": playbook_id,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "predecessor_journey_id": root,
                "guide_id": guide_b,
            }],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "guide mismatch must be 400, got {status}: {body}"
    );
    assert_eq!(body["error"], "JOURNEY_CHAIN_GUIDE_MISMATCH");

    // And as a defense-in-depth check: no row was created with the bad
    // chain — the post failure must not leave a partial DB state.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rev.journeys WHERE name = $1 AND predecessor_journey_id = $2",
    )
    .bind("mismatched chain")
    .bind(root)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "no journey should be persisted after a 400");
}

// =============================================================================
// Phase 2 — chain enumeration (external surface)
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_list_chains_scoped_to_caller_company(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // Two customers, each with their own guide + a chain.
    let (customer_a, jwt_a, playbook_a) =
        onboard_with_playbook(&pool, &http, &base_url, "alpha", "alpha_prod").await;
    let guide_a = seed_external_guide(&pool, &customer_a, "alpha-chain-guide").await;
    link_guide_playbook(&pool, guide_a, playbook_a).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt_a,
        serde_json::json!({
            "objs": [{
                "name": "alpha root",
                "playbook_id": playbook_a,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "guide_id": guide_a,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "alpha root: {body}");
    let j_alpha: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    let (customer_b, jwt_b, playbook_b) =
        onboard_with_playbook(&pool, &http, &base_url, "beta", "beta_prod").await;
    let guide_b = seed_external_guide(&pool, &customer_b, "beta-chain-guide").await;
    link_guide_playbook(&pool, guide_b, playbook_b).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt_b,
        serde_json::json!({
            "objs": [{
                "name": "beta root",
                "playbook_id": playbook_b,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "guide_id": guide_b,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "beta root: {body}");
    let j_beta: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // Alpha listing on guide_a returns alpha's chain only.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/chains?guide_id={guide_a}"),
        &jwt_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "alpha list chains: {body}");
    let alpha_roots: Vec<Uuid> = body["objs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| serde_json::from_value(o["root_id"].clone()).unwrap())
        .collect();
    assert_eq!(alpha_roots, vec![j_alpha]);

    // Alpha listing on guide_b (alpha doesn't own guide_b) returns empty.
    // Note: this isn't an authorization check — guide_b just happens to
    // have no chains for principal A.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/chains?guide_id={guide_b}"),
        &jwt_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["objs"].as_array().unwrap().len(),
        0,
        "alpha must not see beta's chains via guide_b filter"
    );

    // Beta listing on guide_b returns beta's chain only.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/chains?guide_id={guide_b}"),
        &jwt_b,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let beta_roots: Vec<Uuid> = body["objs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| serde_json::from_value(o["root_id"].clone()).unwrap())
        .collect();
    assert_eq!(beta_roots, vec![j_beta]);
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_get_chain_404_on_foreign_journey(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;
    let http = reqwest::Client::new();

    // Customer A has a journey J. Customer B asks for its chain → 404.
    let (customer_a, jwt_a, playbook_a) =
        onboard_with_playbook(&pool, &http, &base_url, "alpha-gc", "alpha_gc_prod").await;
    let guide_a = seed_external_guide(&pool, &customer_a, "alpha-gc-guide").await;
    link_guide_playbook(&pool, guide_a, playbook_a).await;
    let (status, body) = post_json(
        &http,
        &base_url,
        "/api/journeys",
        &jwt_a,
        serde_json::json!({
            "objs": [{
                "name": "alpha root",
                "playbook_id": playbook_a,
                "brief_pack_ids": [],
                "inference_profile_ids": [],
                "guide_id": guide_a,
            }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "alpha create: {body}");
    let j_alpha: Uuid = serde_json::from_value(body["objs"][0]["id"].clone()).unwrap();

    // Sanity: A can see its own chain.
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{j_alpha}/chain"),
        &jwt_a,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "alpha own chain: {body}");
    assert_eq!(body["objs"].as_array().unwrap().len(), 1);

    // Beta (different customer) requests A's chain → 404 with the
    // canonical RESOURCE_NOT_FOUND body.
    let (_customer_b, jwt_b, _playbook_b) =
        onboard_with_playbook(&pool, &http, &base_url, "beta-gc", "beta_gc_prod").await;
    let (status, body) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{j_alpha}/chain"),
        &jwt_b,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "foreign chain must be 404, got {status}: {body}"
    );
    assert_eq!(body["error"], "RESOURCE_NOT_FOUND");

    // And a bogus UUID returns the same body shape (no existence leak).
    let bogus = Uuid::now_v7();
    let (status_bogus, body_bogus) = get_authed(
        &http,
        &base_url,
        &format!("/api/journeys/{bogus}/chain"),
        &jwt_b,
    )
    .await;
    assert_eq!(status_bogus, StatusCode::NOT_FOUND);
    assert_eq!(body_bogus["error"], "RESOURCE_NOT_FOUND");
}

// =============================================================================
// SDK-driven update / hide round-trips
//
// These two tests exercise `ExternalJourneysClient::update` and `::hide` via
// the real SDK client (not raw reqwest), which is the only way to catch
// envelope-shape drift between server response and `GetEnvelope<T>`
// deserialization. A raw-HTTP test would pass against a server that omits
// `results` because nothing on that path deserializes through the SDK's
// envelope type.
// =============================================================================

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_journey_update_returns_renamed(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let playbook_id = seed_external_playbook(&pool, &alpha).await;
    let journey_id = seed_external_journey(&pool, &alpha, playbook_id).await;

    let signing_key_bytes = alpha.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &alpha.service_id,
        &signing_key_bytes,
    );

    let journey = sdk
        .journeys()
        .update(journey_id, "renamed-by-sdk-test")
        .await
        .expect("update should deserialize through GetEnvelope<Journey>");

    assert_eq!(
        journey.name, "renamed-by-sdk-test",
        "rename should round-trip through the SDK"
    );
}

#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn external_sdk_journey_hide_returns_hidden_at(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let alpha = setup_customer(&pool, "alpha", "alpha_prod").await;
    let playbook_id = seed_external_playbook(&pool, &alpha).await;
    let journey_id = seed_external_journey(&pool, &alpha, playbook_id).await;

    let signing_key_bytes = alpha.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &alpha.service_id,
        &signing_key_bytes,
    );

    let journey = sdk
        .journeys()
        .hide(journey_id)
        .await
        .expect("hide should deserialize through GetEnvelope<Journey>");

    assert!(
        journey.hidden_at.is_some(),
        "hide should populate hidden_at; got {:?}",
        journey.hidden_at
    );
}

// =============================================================================
// Reactive JWT refresh: the SDK transparently re-exchanges across expiry
// =============================================================================

/// Regression for the reactive-refresh outage. A cached service-token JWT
/// that crosses its TTL must be transparently re-exchanged by the SDK, not
/// surface a permanent 401.
///
/// Root cause being guarded against: `on_unauthorized` (the only path that
/// clears the SDK's cached JWT) used to be gated behind a successful
/// `ErrorEnvelope` parse. The external surface returns
/// `{ "error": ..., "message": ... }`, which lacks the `status` field the
/// SDK's envelope required, so the parse failed, the cache was never
/// cleared, and every call past the JWT's expiry returned `401 INVALID_JWT`
/// forever (recoverable only by a process restart).
///
/// We drive real expiry by setting `EXTERNAL_JWT_EXPIRY_SECS=1` on the
/// spawned server, then sleeping past it. The first post-expiry SDK call
/// must succeed because `ClientCore::send` now clears the stale JWT and
/// re-exchanges a fresh one transparently. The `EnvVarGuard` removes the
/// short-TTL override on drop so it can't leak into other serial tests.
#[sqlx::test(migrations = "./../dapple-data/migrations")]
#[serial_test::serial(external_signing_key)]
async fn sdk_reactive_jwt_refresh_across_expiry(pool: PgPool) {
    unsafe {
        std::env::set_var(EXTERNAL_JWT_SIGNING_KEY_ENV, EXTERNAL_TEST_KEY);
    }
    // 1-second JWT TTL so we can cross expiry within the test; auto-removed
    // on drop (incl. panic-unwind) so the 20s dispatch poll test isn't
    // poisoned by a leaked short TTL.
    let _ttl_guard = EnvVarGuard::set("EXTERNAL_JWT_EXPIRY_SECS", "1");

    let (base_url, _server) = spawn_external_server(pool.clone()).await;

    let customer = setup_customer(&pool, "refreshcorp", "refreshcorp_prod").await;
    seed_external_guide(&pool, &customer, "refresh-guide").await;

    let signing_key_bytes = customer.signing_key.to_bytes();
    let sdk = dapple_sdk::ExternalDappleClient::external_at(
        &base_url,
        &customer.service_id,
        &signing_key_bytes,
    );

    // First call: exchanges a token and caches the (1s-TTL) JWT.
    let first = sdk.guides().list(None).await.expect("first list");
    assert_eq!(first.len(), 1, "should see the seeded guide");

    // Cross the JWT's expiry — the cached JWT is now dead.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // Second call: the server rejects the expired cached JWT with 401; the
    // fix makes `send` clear the cache and re-exchange transparently, so this
    // call still succeeds. Before the fix it looped 401 forever.
    let second = sdk
        .guides()
        .list(None)
        .await
        .expect("list after JWT expiry should transparently re-exchange");
    assert_eq!(
        second.len(),
        1,
        "should still see the seeded guide after transparent refresh"
    );
}
