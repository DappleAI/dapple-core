use once_cell::sync::OnceCell;
use sqlx::PgPool;
use std::sync::Arc;

use crate::cache::{NoopCache, PrincipalCache};
use crate::publisher::{EventPublisher, NoopPublisher};
use crate::redis_cache::RedisCache;
use crate::redis_publisher::RedisPublisher;

static CACHE: OnceCell<Arc<dyn PrincipalCache>> = OnceCell::new();
static PUBLISHER: OnceCell<Arc<dyn EventPublisher>> = OnceCell::new();
static REDIS_URL: OnceCell<String> = OnceCell::new();
static PUBLISHER_PG_POOL: OnceCell<PgPool> = OnceCell::new();

fn mask_redis_url(url: &str) -> String {
    // replace :password@ with :****@ for logging
    // also handle redis://user:pass@host and redis://:pass@host
    if let Some(at_pos) = url.find('@') {
        // find the preceding ':' that starts the password (after scheme)
        if let Some(scheme_pos) = url.find("//") {
            let userinfo_start = scheme_pos + 2;
            if userinfo_start < at_pos {
                // mask between last ':' before '@' and '@'
                if let Some(colon_pos) = url[..at_pos].rfind(':') {
                    let mut masked = String::new();
                    masked.push_str(&url[..colon_pos + 1]);
                    masked.push_str("****");
                    masked.push_str(&url[at_pos..]);
                    return masked;
                }
            }
        }
    }
    url.to_string()
}

/// Initialize integrations with provided implementations. Call once at startup.
pub fn init<C: PrincipalCache + 'static>(cache: C) -> Result<(), &'static str> {
    let _ = CACHE.set(Arc::new(cache));
    Ok(())
}

/// Initialize publisher separately (optional).
pub fn init_publisher<P: EventPublisher + 'static>(publisher: P) -> Result<(), &'static str> {
    let _ = PUBLISHER.set(Arc::new(publisher));
    Ok(())
}

/// Initialize default no-op integrations (safe fallback).
pub fn init_noop() {
    let _ = CACHE.set(Arc::new(NoopCache));
    let _ = PUBLISHER.set(Arc::new(NoopPublisher));
}

/// Initialize Redis-backed cache and publisher using `DAPPLE_REDIS_HOST` env var.
/// Defaults to the cluster host `redis://dapple-redis.kube.dappleai.dev:6379` when unset.
pub fn init_redis_from_env() -> Result<(), String> {
    let mut url = std::env::var("DAPPLE_REDIS_HOST")
        .unwrap_or_else(|_| "redis://dapple-redis.kube.dappleai.dev:6379".to_string());

    // If a password env var is set and the URL has no userinfo, inject it.
    if std::env::var("DAPPLE_REDIS_PASSWORD").is_ok()
        && !url.contains('@')
        && let Ok(pw) = std::env::var("DAPPLE_REDIS_PASSWORD")
    {
        if let Some(rest) = url.strip_prefix("redis://") {
            url = format!("redis://:{}@{}", pw, rest);
        } else {
            url = format!("redis://:{}@{}", pw, url);
        }
    }

    init_redis_with_url(&url)
}

/// Initialize Redis-backed cache and publisher with an explicit URL.
/// Initialize Redis-backed cache and publisher with an explicit URL.
///
/// Note: this function records the canonical URL and installs the Redis-backed
/// cache/publisher, but it does not validate network connectivity or perform a
/// PING. Call `try_init_redis_with_url_async` (async) or `try_init_redis_with_url`
/// (blocking wrapper) to perform a connectivity check and receive a failure
/// `Err` when Redis is unreachable. This keeps `init_redis_with_url` idempotent
/// and focused on installation semantics.
pub fn init_redis_with_url(url: &str) -> Result<(), String> {
    // Check canonical URL recorded for idempotency / conflict detection
    if let Some(existing) = REDIS_URL.get() {
        if existing == url {
            // already initialized with the same URL — idempotent
            return Ok(());
        } else {
            return Err(format!(
                "redis init conflict: existing={} attempted={}",
                mask_redis_url(existing),
                mask_redis_url(url)
            ));
        }
    }

    // record the canonical URL
    REDIS_URL
        .set(url.to_string())
        .map_err(|_| "failed to set canonical redis url".to_string())?;

    let cache = RedisCache::new(url.to_string());
    let publisher = RedisPublisher::new(url.to_string());

    // Attempt to set cache/publisher once; if already set, keep existing (no overwrite).
    let _ = CACHE.set(Arc::new(cache));
    let _ = PUBLISHER.set(Arc::new(publisher));

    log::info!("redis integrations initialized: {}", mask_redis_url(url));

    Ok(())
}

/// Try to validate connectivity to Redis at `url` before installing the
/// Redis-backed integrations. This async variant performs a PING over an
/// async multiplexed connection and on success installs the integrations and
/// returns `Ok(())`. On error the integrations are left unchanged and an
/// explanatory `Err(String)` is returned.
pub async fn try_init_redis_with_url_async(url: &str) -> Result<(), String> {
    // Respect canonical URL idempotency/conflict semantics first.
    if let Some(existing) = REDIS_URL.get() {
        if existing == url {
            return Ok(());
        } else {
            return Err(format!(
                "redis init conflict: existing={} attempted={}",
                mask_redis_url(existing),
                mask_redis_url(url)
            ));
        }
    }

    // Attempt to establish an async multiplexed connection and PING.
    let client = redis::Client::open(url).map_err(|e| format!("invalid redis url: {}", e))?;
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| format!("failed to connect to redis: {}", e))?;

    let pong: Result<String, _> = redis::cmd("PING").query_async(&mut conn).await;
    match pong {
        Ok(s) if s == "PONG" => {
            // connectivity OK — install integrations using existing installer
            init_redis_with_url(url)
        }
        Ok(s) => Err(format!("unexpected PING response: {}", s)),
        Err(e) => Err(format!("redis ping error: {}", e)),
    }
}

/// Blocking convenience wrapper around `try_init_redis_with_url_async`.
/// Creates a small current-thread runtime to perform the async check.
pub fn try_init_redis_with_url(url: &str) -> Result<(), String> {
    // Build a current-thread runtime to avoid interfering with other runtimes.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to create runtime for redis check: {}", e))?;
    rt.block_on(try_init_redis_with_url_async(url))
}

/// Try to init Redis with connectivity check; if it fails, install noop
/// integrations and log a warning. This is a convenience used by services that
/// prefer to continue operating without Redis rather than fail hard.
pub fn init_redis_with_url_fallback(url: &str) {
    match try_init_redis_with_url(url) {
        Ok(_) => (),
        Err(e) => {
            log::warn!(
                "redis init failed, falling back to noop integrations: {}",
                e
            );
            init_noop();
        }
    }
}

pub fn get_cache() -> Option<&'static Arc<dyn PrincipalCache>> {
    CACHE.get()
}

pub fn get_publisher() -> Option<&'static Arc<dyn EventPublisher>> {
    PUBLISHER.get()
}

/// Initialize a shared PgPool for background publisher/cache tasks.
/// Safe to call once at startup. Subsequent calls are ignored.
pub fn init_publisher_pool(pool: PgPool) -> Result<(), String> {
    PUBLISHER_PG_POOL
        .set(pool)
        .map_err(|_| "failed to set publisher pg pool".to_string())
}

/// Get the shared publisher PgPool if initialized.
pub fn get_publisher_pool() -> Option<&'static PgPool> {
    PUBLISHER_PG_POOL.get()
}
