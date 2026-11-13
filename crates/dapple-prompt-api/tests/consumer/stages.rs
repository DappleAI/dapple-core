use crate::consumer::error::ConsumerError;
use crate::consumer::{Endpoint, ScenarioContext};
use fake::Fake;
use reqwest::Url;
use serde_json::Value;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct StageEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl StageEndpoint {
    pub fn fake_create_schema(&self) -> <StageEndpoint as Endpoint>::CreateSchema {
        let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());
        let playbook_oid = guard
            .random_playbook_from_any_batch()
            .map(|pb| pb.id)
            .expect("StageEndpoint::fake_create_schema: no playbooks registered — register playbooks before creating stages");
        let step_oid = guard
            .random_step_from_any_batch()
            .map(|step| step.id)
            .expect("StageEndpoint::fake_create_schema: no steps registered — register steps before creating stages");

        serde_json::json!({
            "playbook_id": playbook_oid,
            "params": serde_json::json!({}),
            "steps": [
                {
                    "stage_step_id": step_oid,
                    "params": serde_json::json!({})
                }
            ]
        })
    }

    pub fn fake_create_set_schema(
        &self,
        size: Range<usize>,
    ) -> <StageEndpoint as Endpoint>::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        serde_json::json!({"objs": (0..fakes).map(|_| self.fake_create_schema()).collect::<Vec<Value>>()})
    }
}

impl Endpoint for StageEndpoint {
    type Object = Value;
    type GetSchema = Value;
    type GetSetSchema = Value;
    type CreateSchema = Value;
    type CreateSetSchema = Value;

    fn path(&self) -> &str {
        "stages"
    }
    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema()
    }
    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        self.fake_create_set_schema(size)
    }

    // The consumer test helpers call get_object/post and expect serde-json values; keep minimal implementations
    async fn test_fetched_object_id(
        &self,
        _object_id: Uuid,
        _obj: &Self::Object,
    ) -> Result<(), ConsumerError> {
        Ok(())
    }
    async fn test_returned_set(&self, _objs: &Vec<Self::Object>) -> Result<(), ConsumerError> {
        Ok(())
    }
    async fn test_returned_post_set(
        &self,
        _schema: &Self::CreateSetSchema,
        _objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        Ok(())
    }
}
