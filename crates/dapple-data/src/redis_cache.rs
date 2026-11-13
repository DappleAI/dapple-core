use crate::cache::{CacheError, PrincipalCache};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::types::Uuid;

/// Simple Redis-backed cache. Opens a fresh async connection per operation.
pub struct RedisCache {
    url: String,
}

impl RedisCache {
    pub fn new(url: impl Into<String>) -> Self {
        RedisCache { url: url.into() }
    }
}

#[async_trait]
impl PrincipalCache for RedisCache {
    async fn set(&self, ty: &str, id: &Uuid, dto: Value) -> Result<(), CacheError> {
        // key pattern: dto:{type}:{id}
        let key = format!("dto:{}:{}", ty, id);

        let s = serde_json::to_string(&dto)?;

        let client =
            redis::Client::open(self.url.as_str()).map_err(|e| CacheError::Other(e.to_string()))?;
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| CacheError::Other(e.to_string()))?;

        // Use RedisJSON command JSON.SET <key> $ '<json>'
        // If RedisJSON is not available, fall back to SET for plain JSON
        let res = redis::cmd("JSON.SET")
            .arg(&key)
            .arg("$")
            .arg(&s)
            .query_async::<()>(&mut conn)
            .await;

        // fallback to SET when JSON.SET fails
        let res = match res {
            Ok(v) => Ok(v),
            Err(_) => {
                redis::cmd("SET")
                    .arg(&key)
                    .arg(&s)
                    .query_async::<()>(&mut conn)
                    .await
            }
        };

        res.map_err(|e| CacheError::Other(e.to_string()))?;
        Ok(())
    }
}
