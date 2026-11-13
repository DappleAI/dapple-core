use async_trait::async_trait;
use sqlx::types::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum PublishError {
    #[error("redis error: {0}")]
    Other(String),
}

#[async_trait]
pub trait EventPublisher: Send + Sync + 'static {
    async fn principal_changed(&self, ty: &str, id: &Uuid, op: &str) -> Result<(), PublishError>;

    async fn dependent_changed(
        &self,
        stream: &str,
        parent_id: &Uuid,
        dependent_id: &Uuid,
        op: &str,
    ) -> Result<(), PublishError> {
        let _ = (stream, parent_id, dependent_id, op);
        Ok(())
    }
}

/// No-op publisher used when Redis is not configured.
pub struct NoopPublisher;

#[async_trait]
impl EventPublisher for NoopPublisher {
    async fn principal_changed(
        &self,
        _ty: &str,
        _id: &Uuid,
        _op: &str,
    ) -> Result<(), PublishError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::types::Uuid;

    #[tokio::test]
    async fn noop_publisher_principal_changed_ok() {
        let p = NoopPublisher;
        let id = Uuid::now_v7();
        let res = p.principal_changed("dispatch", &id, "insert").await;
        assert!(res.is_ok());
    }
}
