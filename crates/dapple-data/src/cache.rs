use async_trait::async_trait;
use serde_json::Value;
use sqlx::types::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum CacheError {
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("other: {0}")]
    Other(String),
}

#[async_trait]
pub trait PrincipalCache: Send + Sync + 'static {
    async fn set(&self, ty: &str, id: &Uuid, dto: Value) -> Result<(), CacheError>;

    async fn get(&self, ty: &str, id: &Uuid) -> Result<Option<Value>, CacheError> {
        let _ = (ty, id);
        Ok(None)
    }
}

/// A no-op cache implementation used when Redis is not initialized.
pub struct NoopCache;

#[async_trait]
impl PrincipalCache for NoopCache {
    async fn set(&self, _ty: &str, _id: &Uuid, _dto: Value) -> Result<(), CacheError> {
        let _ = _dto;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::types::Uuid;

    #[tokio::test]
    async fn noop_cache_set_ok() {
        let c = NoopCache;
        let id = Uuid::now_v7();
        let v = json!({ "foo": "bar" });
        let res = c.set("test", &id, v).await;
        assert!(res.is_ok());
    }
}
