use crate::consumer::{Endpoint, ScenarioContext};
use dapple_data::model::steps::Step;
use dapple_prompt_api::schema::steps::{
    CreateStepSchema, CreateStepSetSchema, CreateStepTaskSchema, GetStepSchema, GetStepSetSchema,
};
use fake::Fake;
use reqwest::Url;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::consumer::error::ConsumerError;
use log::info;

pub struct StepEndpoint {
    pub base_url: Url,
    pub context: Arc<Mutex<ScenarioContext>>,
}

impl StepEndpoint {
    pub fn fake_create_schema(&self) -> <StepEndpoint as Endpoint>::CreateSchema {
        self.fake_create_schema_with_child_count(1)
    }

    fn fake_create_schema_with_child_count(&self, count: usize) -> CreateStepSchema {
        let guard = self.context.lock().unwrap_or_else(|p| p.into_inner());
        let mut seen = std::collections::HashSet::new();
        let mut tasks: Vec<CreateStepTaskSchema> = Vec::new();
        // Try enough times to collect `count` distinct tasks
        for _ in 0..count * 10 {
            if tasks.len() >= count {
                break;
            }
            if let Some(task) = guard.random_task_from_any_batch()
                && seen.insert(task.id)
            {
                tasks.push(CreateStepTaskSchema { task_id: task.id });
            }
        }

        // Fail loud if the task batch is empty — silently fabricating a
        // fake UUID here used to push the failure downstream into
        // `tasks_count_mismatch` assertions, which is harder to debug than
        // a panic at the trigger site. If you hit this, register a task
        // batch first (run the `create_tasks` trial earlier in the order).
        assert!(
            !tasks.is_empty(),
            "fake_create_schema_with_child_count: no tasks available in any registered batch — register tasks before creating steps",
        );

        CreateStepSchema {
            name: Self::fake_name(),
            description: Self::fake_description(),
            tasks,
        }
    }

    pub fn fake_create_set_schema(
        &self,
        size: Range<usize>,
    ) -> <StepEndpoint as Endpoint>::CreateSetSchema {
        let fakes = (size).fake::<usize>();

        CreateStepSetSchema {
            objs: (0..fakes)
                .map(|i| {
                    let count = if i == 0 { 1 } else { 3 };
                    self.fake_create_schema_with_child_count(count)
                })
                .collect(),
        }
    }
}

impl Endpoint for StepEndpoint {
    type Object = Step;
    type GetSchema = GetStepSchema;
    type GetSetSchema = GetStepSetSchema;
    type CreateSchema = CreateStepSchema;
    type CreateSetSchema = CreateStepSetSchema;

    fn path(&self) -> &str {
        "steps"
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
                .expect("response missing step with matching name");
            assert_eq!(s.description, o.description, "description");
            assert_eq!(
                s.tasks.len(),
                o.tasks.len(),
                "tasks count mismatch for step {}",
                o.id
            );
        }

        info!("requested count: {}", schema.objs.len());
        info!("POSTed set of {} steps successfully verified", objs.len());
        info!("response results size: {}", objs.len());
        info!(
            "Created steps IDs: {:?}",
            objs.iter().map(|o| o.id).collect::<Vec<Uuid>>()
        );

        Ok(())
    }

    fn fake_create_schema(&self) -> Self::CreateSchema {
        self.fake_create_schema()
    }

    fn fake_create_set_schema(&self, size: Range<usize>) -> Self::CreateSetSchema {
        self.fake_create_set_schema(size)
    }
}
