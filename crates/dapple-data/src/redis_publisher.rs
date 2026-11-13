use crate::publisher::{EventPublisher, PublishError};
use async_trait::async_trait;
use sqlx::types::Uuid;

pub struct RedisPublisher {
    url: String,
}

impl RedisPublisher {
    pub fn new(url: impl Into<String>) -> Self {
        RedisPublisher { url: url.into() }
    }
}

#[async_trait]
impl EventPublisher for RedisPublisher {
    async fn principal_changed(&self, ty: &str, id: &Uuid, op: &str) -> Result<(), PublishError> {
        let client = redis::Client::open(self.url.as_str())
            .map_err(|e| PublishError::Other(e.to_string()))?;
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PublishError::Other(e.to_string()))?;

        // XADD stream:principal_changes * type {type} id {id} op {insert|update|delete}
        let res: redis::RedisResult<String> = redis::cmd("XADD")
            .arg("stream:principal_changes")
            .arg("*")
            .arg("type")
            .arg(ty)
            .arg("id")
            .arg(id.to_string())
            .arg("op")
            .arg(op)
            .query_async(&mut conn)
            .await;

        res.map_err(|e| PublishError::Other(e.to_string()))?;
        Ok(())
    }

    async fn dependent_changed(
        &self,
        stream: &str,
        parent_id: &Uuid,
        dependent_id: &Uuid,
        op: &str,
    ) -> Result<(), PublishError> {
        let client = redis::Client::open(self.url.as_str())
            .map_err(|e| PublishError::Other(e.to_string()))?;
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PublishError::Other(e.to_string()))?;

        // XADD <stream> * parent_id {parent_id} dependent_id {dependent_id} op {op}
        let res: redis::RedisResult<String> = redis::cmd("XADD")
            .arg(stream)
            .arg("*")
            .arg("parent_id")
            .arg(parent_id.to_string())
            .arg("dependent_id")
            .arg(dependent_id.to_string())
            .arg("op")
            .arg(op)
            .query_async(&mut conn)
            .await;

        res.map_err(|e| PublishError::Other(e.to_string()))?;
        Ok(())
    }
}
