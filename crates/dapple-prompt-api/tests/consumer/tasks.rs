use crate::consumer::Endpoint;
use dapple_data::model::tasks::Task;
use dapple_prompt_api::schema::tasks::{
    CreateTaskSchema, CreateTaskSetSchema, GetTaskSchema, GetTaskSetSchema,
};
use reqwest::Url;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use fake::Fake;

use crate::consumer::ScenarioContext;
use crate::consumer::error::ConsumerError;
use log::info;

pub struct TaskEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl TaskEndpoint {
    pub fn fake_function_name() -> String {
        Uuid::now_v7().to_string()
    }

    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateTaskSchema {
        let prompt_ids = {
            let ctx = self.context.lock().unwrap_or_else(|p| p.into_inner());
            ctx.prompt_batches
                .iter()
                .flatten()
                .take(count)
                .map(|p| p.id)
                .collect::<Vec<_>>()
        };

        CreateTaskSchema {
            name: Self::fake_name(),
            description: Self::fake_description(),
            function: Self::fake_function_name(),
            function_input: Default::default(),
            prompt_ids,
        }
    }
}

impl Endpoint for TaskEndpoint {
    type Object = Task;
    type GetSchema = GetTaskSchema;
    type GetSetSchema = GetTaskSetSchema;
    type CreateSchema = CreateTaskSchema;
    type CreateSetSchema = CreateTaskSetSchema;

    fn path(&self) -> &str {
        "tasks"
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    async fn test_fetched_object_id(
        &self,
        object_id: Uuid,
        obj: &Self::Object,
    ) -> Result<(), ConsumerError> {
        assert_eq!(obj.id, object_id, "fetched object id");

        Ok(())
    }

    async fn test_returned_set(&self, objs: &Vec<Self::Object>) -> Result<(), ConsumerError> {
        for obj in objs {
            let fetched = self.get_object(obj.id).await?;

            assert_eq!(obj, &fetched, "fetched object does not match original");
        }

        Ok(())
    }

    async fn test_returned_post_set(
        &self,
        schema: &Self::CreateSetSchema,
        objs: &Vec<Self::Object>,
    ) -> Result<(), ConsumerError> {
        assert_eq!(schema.objs.len(), objs.len());

        for s in schema.objs.iter() {
            let o = objs
                .iter()
                .find(|o| o.name == s.name)
                .expect("response missing task with matching name");
            assert_eq!(s.description, o.description, "description");
            assert_eq!(
                s.prompt_ids.len(),
                o.prompts.len(),
                "prompts count mismatch for task {}",
                o.id
            );
        }

        info!("requested count: {}", schema.objs.len());
        info!("POSTed set of {} tasks successfully verified", objs.len());
        info!("response results size: {}", objs.len());
        info!(
            "Created tasks IDs: {:?}",
            objs.iter().map(|o| o.id).collect::<Vec<Uuid>>()
        );

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema_with_child_count(1)
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        Self::CreateSetSchema {
            objs: (0..fakes)
                .map(|i| {
                    let count = if i == 0 { 1 } else { 3 };
                    self.fake_create_schema_with_child_count(count)
                })
                .collect(),
        }
    }
}
